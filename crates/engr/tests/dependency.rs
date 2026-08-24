//! What a Ref depends on, and what counts as that dependency moving.
//!
//! The question these ask is not the one `tests/integrity.rs` asks. Integrity
//! is *was this stored state changed outside the supported transition*;
//! dependency is *did the facts my source relies on change*. #35 §13 is where
//! the two visibly come apart, and it has its own test here.

use engr::dependency::{
    canonical_fields, check_not_stale_at_birth, compare, ref_snapshot, semantic_projection,
    semantic_value, Dependency, SemanticField,
};
use engr::model::{Ref, Section};
use engr::proof::section_target;
use engr::semantics::{Admission, Relation, Role, Supplement};

fn object_id() -> String {
    "0192f0c8-1a2b-7c3d-8e4f-5a6b7c8d9e0f".to_owned()
}

fn target() -> String {
    section_target(&object_id(), 1)
}

fn commit() -> String {
    "d".repeat(40)
}

fn section() -> Section {
    Section {
        id: 1,
        admission: Admission::Agent,
        role: Some(Role::Decision),
        text: "the store appends under a lock".to_owned(),
        content: Vec::new(),
        based_on: None,
        refs: Vec::new(),
        relations: Vec::new(),
        sha256: String::new(),
        admitted_at: "2026-08-24T00:00:00Z".to_owned(),
    }
}

fn snapshot(section: &Section, fields: &[SemanticField]) -> engr::dependency::RefSnapshot {
    ref_snapshot(target(), fields, section, commit()).expect("snapshot")
}

fn digest_of(snapshot: &engr::dependency::RefSnapshot) -> String {
    snapshot.digest().expect("digest").to_string()
}

/// #35 §13, the consequence that justifies the whole separation.
///
/// Promote a Section from Agent to Human without touching a word of it. Both
/// seals move, because `admission` is stable persisted integrity-protected
/// state. Whether the *dependency* moved depends entirely on what the Ref said
/// it depended on.
#[test]
fn promotion_drifts_a_reference_that_selected_admission_and_not_one_that_did_not() {
    let before = section();
    let mut after = before.clone();
    after.admission = Admission::Human;
    assert_eq!(before.text, after.text, "not a word changed");

    let text_only = snapshot(&before, &[SemanticField::Text]);
    let attested = digest_of(&text_only);
    assert_eq!(
        compare(&text_only, &attested, &after).expect("compare"),
        Dependency::Unchanged,
        "fields=[text] does not drift merely because admission changed"
    );

    let with_admission = snapshot(&before, &[SemanticField::Admission, SemanticField::Text]);
    let attested = digest_of(&with_admission);
    assert_eq!(
        compare(&with_admission, &attested, &after).expect("compare"),
        Dependency::Drifted {
            fields: vec![SemanticField::Admission]
        },
        "fields=[admission,text] does"
    );
}

/// Staleness is relative to the dependency each Ref declares, so two Refs to
/// one target admitted in the same moment may legitimately disagree.
#[test]
fn two_references_to_one_target_can_disagree_about_staleness() {
    let historical = section();
    let mut current = historical.clone();
    current.role = Some(Role::Risk);

    check_not_stale_at_birth(&historical, &current, &[SemanticField::Text])
        .expect("text is unchanged, so a text dependency is not stale");
    let refused = check_not_stale_at_birth(&historical, &current, &[SemanticField::Role])
        .expect_err("role moved, so a role dependency is");
    assert!(refused.to_string().contains("stale at birth"), "{refused}");
}

/// A freshly admitted Ref is non-drifting by construction, because birth and
/// read use the same projection. If these two ever used different code, a Ref
/// could be born already drifting and nobody would notice until read time.
#[test]
fn birth_and_read_agree_by_construction() {
    let section = section();
    let snapshot = snapshot(&section, &[SemanticField::Text, SemanticField::Role]);
    check_not_stale_at_birth(&section, &section, &snapshot.fields).expect("not stale");
    assert_eq!(
        compare(&snapshot, &digest_of(&snapshot), &section).expect("compare"),
        Dependency::Unchanged
    );
}

/// The exact hash input of #35 §6: four members, `values` keyed exactly by
/// `fields`, and the bytes pinned.
#[test]
fn the_snapshot_is_the_four_member_object_the_contract_writes_out() {
    let snapshot = snapshot(&section(), &[SemanticField::Text, SemanticField::Admission]);
    let value = serde_json::to_value(&snapshot).expect("value");
    let members = value.as_object().expect("object");
    assert_eq!(
        members.keys().collect::<Vec<_>>(),
        vec!["commit", "fields", "target", "values"],
        "four members and no others"
    );

    let values = members["values"].as_object().expect("values");
    let selected: Vec<&str> = snapshot.fields.iter().map(|f| f.as_str()).collect();
    assert_eq!(
        values.keys().map(String::as_str).collect::<Vec<_>>(),
        selected,
        "values keys are exactly the members of fields"
    );

    assert_eq!(
        engr::proof::canonical_bytes(&snapshot, "snapshot").expect("canonical"),
        format!(
            r#"{{"commit":"{}","fields":["admission","text"],"target":"{}","values":{{"admission":"agent","text":"the store appends under a lock"}}}}"#,
            commit(),
            target()
        )
    );
    assert_eq!(
        digest_of(&snapshot),
        "1:9d1e70bb095529df7ff0229e2bd76d45d2eb448a3bdd48aa628375279f4b8e5f"
    );
}

/// `fields[]` is a protocol set: order is not meaning, and a repeat is not a
/// selection made twice.
#[test]
fn the_selection_is_a_set() {
    let one = snapshot(&section(), &[SemanticField::Text, SemanticField::Role]);
    let other = snapshot(&section(), &[SemanticField::Role, SemanticField::Text]);
    assert_eq!(
        digest_of(&one),
        digest_of(&other),
        "the same two facts in another order are the same dependency"
    );

    canonical_fields(&[SemanticField::Text, SemanticField::Text])
        .expect_err("one fact selected twice is not two facts");
    canonical_fields(&[]).expect_err("there is no implicit full reference");
}

/// A name outside the vocabulary is an error, not a `null`.
///
/// The failure this prevents is quiet: `fields: ["admited_at"]` would otherwise
/// hash a `null`, verify forever, and never drift — a dependency on nothing,
/// indistinguishable from a satisfied one.
#[test]
fn a_selector_outside_the_vocabulary_is_refused_by_name() {
    for outside in ["admited_at", "admitted_at", "sha256", "id", "text.body", ""] {
        let refused = SemanticField::parse(outside).expect_err("not selectable");
        assert!(
            refused.to_string().contains("vocabulary is"),
            "{outside}: {refused}"
        );
    }
    for inside in [
        "admission",
        "based_on",
        "content",
        "refs",
        "relations",
        "role",
        "text",
    ] {
        SemanticField::parse(inside).expect("selectable");
    }
}

/// The projection carries the semantic vocabulary and nothing else — identity,
/// provenance and integrity are not facts a source depends on.
#[test]
fn the_projection_excludes_identity_provenance_and_the_seal() {
    let projected = semantic_projection(&section()).expect("project");
    assert_eq!(projected.len(), 7);
    for absent in ["id", "admitted_at", "sha256"] {
        assert!(
            !projected.contains_key(absent),
            "{absent} is not a selectable semantic fact"
        );
    }
}

/// Selected absent optionals project as `null`, and set-like values project
/// canonically.
#[test]
fn absent_optionals_project_as_null_and_sets_project_canonically() {
    let mut section = section();
    assert_eq!(
        semantic_value(&section, SemanticField::BasedOn).expect("value"),
        serde_json::Value::Null
    );

    section.role = None;
    assert_eq!(
        semantic_value(&section, SemanticField::Role).expect("value"),
        serde_json::Value::Null
    );

    section.content = vec![Supplement::new("data.note", "a note")];
    assert_eq!(
        semantic_value(&section, SemanticField::Content).expect("value"),
        serde_json::json!([{"type": "data.note", "body": "a note"}])
    );

    // A set projects in canonical order whichever way it was stored.
    let one = reference(4);
    let other = reference(2);
    section.refs = vec![one.clone(), other.clone()];
    let forward = semantic_value(&section, SemanticField::Refs).expect("value");
    section.refs = vec![other, one];
    let backward = semantic_value(&section, SemanticField::Refs).expect("value");
    assert_eq!(forward, backward, "refs is a set, not a sequence");
}

fn reference(section: u64) -> Ref {
    Ref {
        object: object_id(),
        section,
        sha256: "c".repeat(64),
        commit: commit(),
    }
}

/// Ordered values keep their order — `content` is a sequence a reader goes
/// through, so moving one entry is a change to the assertion.
#[test]
fn ordered_values_keep_their_order() {
    let mut section = section();
    section.content = vec![
        Supplement::new("data.note", "first"),
        Supplement::new("data.note", "second"),
    ];
    let forward = snapshot(&section, &[SemanticField::Content]);
    section.content.reverse();
    let backward = snapshot(&section, &[SemanticField::Content]);
    assert_ne!(
        digest_of(&forward),
        digest_of(&backward),
        "content is ordered, unlike refs and relations"
    );
}

/// A stored digest that its own recorded past does not reproduce is reported as
/// invalid, not as drift.
///
/// The order matters: if the record of the dependency is unusable, saying "your
/// dependency moved" tells a reader something that is not known to be true.
#[test]
fn an_unreproducible_digest_is_invalid_rather_than_drifted() {
    let section = section();
    let snapshot = snapshot(&section, &[SemanticField::Text]);
    let wrong = format!("1:{}", "a".repeat(64));

    let mut moved = section.clone();
    moved.text = "and the facts moved too".to_owned();
    assert_eq!(
        compare(&snapshot, &wrong, &moved).expect("compare"),
        Dependency::DigestInvalid,
        "the unusable record is reported first"
    );

    // A scalar naming a contract this build cannot verify is a third answer
    // again, and not silently treated as a mismatch.
    compare(&snapshot, &format!("2:{}", "a".repeat(64)), &moved)
        .expect_err("no contract for version 2");
    compare(&snapshot, &"a".repeat(64), &moved).expect_err("an unversioned scalar is malformed");
}

/// Every selected field is compared, and every one that moved is named — a
/// report that stopped at the first would leave a reader repairing one
/// dependency at a time.
#[test]
fn every_moved_field_is_named() {
    let before = section();
    let mut after = before.clone();
    after.admission = Admission::Human;
    after.text = "revised".to_owned();
    after.relations = vec![Relation::superseded_by(object_id())];

    let snapshot = snapshot(
        &before,
        &[
            SemanticField::Admission,
            SemanticField::Text,
            SemanticField::Relations,
            SemanticField::Role,
        ],
    );
    assert_eq!(
        compare(&snapshot, &digest_of(&snapshot), &after).expect("compare"),
        Dependency::Drifted {
            fields: vec![
                SemanticField::Admission,
                SemanticField::Relations,
                SemanticField::Text
            ]
        },
        "role did not move and is not listed"
    );
}

mod against_a_workspace {
    use super::*;
    use engr::dependency::{admit, evaluate, parse_target, SelectiveRef};
    use engr::integrity::{sealed_object, sealed_section};
    use engr::model::Object;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    fn git(root: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .expect("git");
        assert!(status.success(), "git {args:?}");
    }

    fn commit_all(root: &Path, message: &str) -> String {
        git(root, &["add", "-A"]);
        git(
            root,
            &[
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-qm",
                message,
            ],
        );
        engr::git::head(root).expect("HEAD")
    }

    /// A workspace holding one Object with one Section, committed.
    fn workspace() -> (TempDir, PathBuf, Object, String) {
        let dir = TempDir::new().expect("temp dir");
        let root = dir.path().to_path_buf();
        git(&root, &["init", "-q"]);
        engr::store::init(&root).expect("init");

        let mut object =
            Object::new(object_id(), "the append boundary".to_owned()).expect("object");
        object.rev = 1;
        object.next_section_id = 2;
        let mut section = section();
        section.admission = Admission::Human;
        section.sha256 = section.recomputed_sha256().expect("v2 seal");
        object.sections = vec![section];
        engr::store::save_object(&root, &object).expect("save");

        let commit = commit_all(&root, "record");
        (dir, root, object, commit)
    }

    /// The v3 seals the Phase-3 verifier expects, computed rather than stored —
    /// no workspace carries them yet, which is the write boundary, not a gap.
    fn phase_three_seals(object: &Object) -> (Object, String) {
        let mut sealed = object.clone();
        for section in &mut sealed.sections {
            section.sha256 = sealed_section(section)
                .expect("project")
                .seal()
                .expect("seal");
        }
        let seal = sealed_object(&sealed)
            .expect("project")
            .seal()
            .expect("seal");
        (sealed, seal)
    }

    #[test]
    fn a_reference_is_admitted_against_the_commit_it_pins() {
        let (_dir, root, object, commit) = workspace();
        let (object, seal) = phase_three_seals(&object);

        let reference = admit(
            &root,
            &object,
            &seal,
            1,
            &[SemanticField::Text, SemanticField::Admission],
            &commit,
        )
        .expect("nothing has moved since the commit");

        assert_eq!(reference.target, target());
        assert_eq!(reference.commit, commit);
        assert_eq!(
            reference.fields,
            vec![SemanticField::Admission, SemanticField::Text]
        );
        assert!(reference.digest.starts_with("1:"));

        assert_eq!(
            evaluate(&root, &object, &seal, &reference).expect("evaluate"),
            Dependency::Unchanged,
            "a fresh reference is non-drifting by construction"
        );
    }

    /// The order of #35 §8 is load, verify integrity, *then* look at fields. A
    /// target whose own integrity fails cannot be legitimized by referencing a
    /// still-hashable subset of it.
    #[test]
    fn a_target_that_fails_its_own_integrity_cannot_be_referenced() {
        let (_dir, root, object, commit) = workspace();
        let (mut object, seal) = phase_three_seals(&object);
        object.sections[0].text = "rewritten out of band".to_owned();

        admit(&root, &object, &seal, 1, &[SemanticField::Role], &commit)
            .expect_err("even selecting a field the edit did not touch");

        // And when the tampering also makes the selection stale, the refusal
        // still names the integrity failure. Order is why: reporting staleness
        // here would tell the author to re-pin the commit, when what actually
        // happened is that the target was rewritten out of band.
        let refused = admit(&root, &object, &seal, 1, &[SemanticField::Text], &commit)
            .expect_err("both wrong at once");
        assert!(
            !refused.to_string().contains("stale at birth"),
            "the integrity failure is the one worth reporting: {refused}"
        );

        let reference = SelectiveRef {
            target: target(),
            fields: vec![SemanticField::Role],
            commit,
            digest: format!("1:{}", "a".repeat(64)),
        };
        assert_eq!(
            evaluate(&root, &object, &seal, &reference).expect("evaluate"),
            Dependency::TargetIntegrityFailure,
            "and reading one reports the tampering rather than drift"
        );
    }

    /// A commit that no longer resolves, and a target absent at one that does,
    /// are the same answer: the provenance is gone, which is not drift.
    #[test]
    fn missing_provenance_is_not_drift() {
        let (_dir, root, object, _commit) = workspace();
        let (object, seal) = phase_three_seals(&object);

        let unknown = SelectiveRef {
            target: target(),
            fields: vec![SemanticField::Text],
            commit: "f".repeat(40),
            digest: format!("1:{}", "a".repeat(64)),
        };
        assert_eq!(
            evaluate(&root, &object, &seal, &unknown).expect("evaluate"),
            Dependency::ProvenanceUnavailable
        );
    }

    /// Drift, end to end: the selected fact really moved between the pinned
    /// commit and now.
    #[test]
    fn a_selected_fact_that_moved_is_reported_as_drift() {
        let (_dir, root, object, commit) = workspace();
        let (object, seal) = phase_three_seals(&object);
        let reference =
            admit(&root, &object, &seal, 1, &[SemanticField::Text], &commit).expect("admitted");

        let mut moved = object.clone();
        moved.sections[0].text = "revised through the gate".to_owned();
        let (moved, moved_seal) = phase_three_seals(&moved);

        assert_eq!(
            evaluate(&root, &moved, &moved_seal, &reference).expect("evaluate"),
            Dependency::Drifted {
                fields: vec![SemanticField::Text]
            }
        );

        // The same movement, for a reference that never selected `text`.
        let other =
            admit(&root, &object, &seal, 1, &[SemanticField::Role], &commit).expect("admitted");
        assert_eq!(
            evaluate(&root, &moved, &moved_seal, &other).expect("evaluate"),
            Dependency::Unchanged,
            "drift is relative to the dependency actually declared"
        );
    }

    /// A reference cannot be born already stale.
    #[test]
    fn a_reference_stale_at_birth_is_refused() {
        let (_dir, root, object, commit) = workspace();
        let (mut object, _) = phase_three_seals(&object);
        object.sections[0].text = "moved since the commit".to_owned();
        let (object, seal) = phase_three_seals(&object);

        let refused = admit(&root, &object, &seal, 1, &[SemanticField::Text], &commit)
            .expect_err("text already differs from the pinned commit");
        assert!(refused.to_string().contains("stale at birth"), "{refused}");

        // And the same moment admits a reference that selects something else.
        admit(&root, &object, &seal, 1, &[SemanticField::Role], &commit)
            .expect("role has not moved");
    }

    #[test]
    fn a_target_must_be_a_canonical_section_identity() {
        parse_target(&target()).expect("canonical");
        for malformed in [
            "obj:0192f0c8-1a2b-7c3d-8e4f-5a6b7c8d9e0f",
            "0192f0c8-1a2b-7c3d-8e4f-5a6b7c8d9e0f:1",
            "obj:not-a-uuid:1",
            "obj:0192f0c8-1a2b-7c3d-8e4f-5a6b7c8d9e0f:0",
            "obj:0192f0c8-1a2b-7c3d-8e4f-5a6b7c8d9e0f:x",
            "",
        ] {
            parse_target(malformed).expect_err(malformed);
        }
    }

    /// A reference evaluated against the wrong Object is refused rather than
    /// answered — comparing section 1 of one Object with section 1 of another
    /// would produce a confident, meaningless verdict.
    #[test]
    fn a_reference_is_only_evaluated_against_the_object_it_names() {
        let (_dir, root, object, commit) = workspace();
        let (object, seal) = phase_three_seals(&object);
        let reference =
            admit(&root, &object, &seal, 1, &[SemanticField::Text], &commit).expect("admitted");

        let mut stranger = object.clone();
        stranger.id = "0192f0c8-1a2b-7c3d-8e4f-5a6b7c8d9e11".to_owned();
        let (stranger, stranger_seal) = phase_three_seals(&stranger);
        evaluate(&root, &stranger, &stranger_seal, &reference)
            .expect_err("that is a different object");
    }
}
