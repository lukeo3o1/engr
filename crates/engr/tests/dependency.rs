//! What a Ref depends on, and what counts as that dependency moving.
//!
//! The question these ask is not the one `tests/integrity.rs` asks. Integrity
//! is *was this stored state changed outside the supported transition*;
//! dependency is *did the facts my source relies on change*. #35 §13 is where
//! the two visibly come apart, and it has its own test here.

use engr::dependency::{
    canonical_fields, check_not_stale_at_birth, compare, parse_target, ref_snapshot,
    semantic_projection, semantic_value, Dependency, SemanticField,
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
    check_not_stale_at_birth(&section, &section, snapshot.fields()).expect("not stale");
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
    let selected: Vec<&str> = snapshot.fields().iter().map(|f| f.as_str()).collect();
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

/// Every Contract-1 rule is enforced where a snapshot is built, because that is
/// the only place a snapshot can come from.
///
/// The defect this replaces: `RefSnapshot` once had public members, so every
/// rule in the module was advisory. An empty selection, a target naming
/// nothing and a five-character commit produced a perfectly well-formed
/// `1:<sha256>` — a digest that verifies against itself forever and against
/// nothing any other implementation would compute.
///
/// Missing and extra `values` keys are absent from this list on purpose: they
/// are no longer reachable to test. `values` is derived from `fields` inside
/// the constructor, so there is no argument through which a mismatched map
/// could arrive. The nearest check is
/// `the_snapshot_is_the_four_member_object_the_contract_writes_out`, which
/// asserts the keys are exactly the selection.
#[test]
fn an_illegal_snapshot_cannot_be_built_and_therefore_cannot_be_hashed() {
    let section = section();

    // A target that is not a canonical Section identity.
    for target in [
        "not a target",
        "obj:0192f0c8-1a2b-7c3d-8e4f-5a6b7c8d9e0f",
        "obj:not-a-uuid:1",
        "obj:0192f0c8-1a2b-7c3d-8e4f-5a6b7c8d9e0f:0",
        "",
    ] {
        ref_snapshot(target, &[SemanticField::Text], &section, commit()).expect_err(target);
    }

    // A commit that is not a full native Git object id.
    for oid in ["short", "", &"d".repeat(39), &"g".repeat(40), "HEAD"] {
        let refused = ref_snapshot(target(), &[SemanticField::Text], &section, oid).expect_err(oid);
        assert!(
            refused.to_string().contains("full resolved Git object id"),
            "{oid}: {refused}"
        );
    }

    // An empty selection, and one that names a fact twice.
    ref_snapshot(target(), &[], &section, commit()).expect_err("no implicit full reference");
    ref_snapshot(
        target(),
        &[SemanticField::Text, SemanticField::Text],
        &section,
        commit(),
    )
    .expect_err("one fact selected twice is not two facts");

    // The legal case still works, and abbreviating its commit does not.
    ref_snapshot(target(), &[SemanticField::Text], &section, commit()).expect("legal");
    ref_snapshot(target(), &[SemanticField::Text], &section, &commit()[..8])
        .expect_err("an abbreviation is not the object id");
}

/// A 64-character (SHA-256) Git object id is as legal as a 40-character one:
/// #35 says commits are persisted in their native full repository form, so a
/// check that assumed SHA-1 would refuse every Ref in a SHA-256 repository.
#[test]
fn a_sha256_repository_object_id_is_a_full_object_id() {
    ref_snapshot(target(), &[SemanticField::Text], &section(), "a".repeat(64))
        .expect("native full form, whichever algorithm the repository uses");
}

/// Two Sections differing only by the stored order of a set are one Section, to
/// every projection that hashes them.
///
/// The two refs below are chosen so the orders actually disagree. `derive(Ord)`
/// on `Ref` compares `object`, then `section`, then `sha256`, then `commit`;
/// the protocol set rule compares canonical JCS bytes, which begin with
/// `commit`. Before this was one shared projection, the candidate proof copied
/// the array verbatim and produced different bytes for these two orderings,
/// while dependency semantics produced the same — one Section with two answers
/// depending on which contract asked.
#[test]
fn incidental_set_order_reaches_no_projection_that_hashes_it() {
    let low_section_late_commit = Ref {
        object: object_id(),
        section: 1,
        sha256: "a".repeat(64),
        commit: "f".repeat(40),
    };
    let high_section_early_commit = Ref {
        object: object_id(),
        section: 2,
        sha256: "a".repeat(64),
        commit: "0".repeat(40),
    };
    assert!(
        low_section_late_commit < high_section_early_commit,
        "derive(Ord) puts these one way round"
    );

    let mut one = section();
    one.refs = vec![
        low_section_late_commit.clone(),
        high_section_early_commit.clone(),
    ];
    let mut other = section();
    other.refs = vec![high_section_early_commit, low_section_late_commit];

    // The dependency view.
    assert_eq!(
        semantic_value(&one, SemanticField::Refs).expect("one"),
        semantic_value(&other, SemanticField::Refs).expect("other")
    );

    // The proof projection, which is now the same projection.
    let projected = |section: &Section| {
        engr::proof::canonical_bytes(
            &engr::proof::SectionSemantic::of(section).expect("project"),
            "section",
        )
        .expect("canonical")
    };
    assert_eq!(projected(&one), projected(&other), "one canonical form");

    // And the candidate subject built over them, which is what actually gets
    // hashed into a proof somebody keeps.
    let subject = |section: &Section| {
        let mut before =
            engr::model::Object::new(object_id(), "ordering".to_owned()).expect("object");
        before.rev = 1;
        before.next_section_id = 2;
        before.sections = vec![section.clone()];
        let mut after = before.clone();
        after.rev = 2;
        after.sections[0].text = "revised".to_owned();
        let payload = engr::model::Payload {
            action: engr::model::Action::SectionRevised { section: 1 },
            object: object_id(),
            becomes: None,
            content: engr::model::Content {
                text: "revised".to_owned(),
                ..engr::model::Content::default()
            },
        };
        engr::proof::candidate_subject(&before, &after, &payload, None)
            .expect("subject")
            .digest()
            .expect("digest")
    };
    assert_eq!(
        subject(&one),
        subject(&other),
        "a candidate proof cannot depend on which order the array was stored in"
    );
}

/// The selectable vocabulary and the canonical projection are the same set of
/// names, checked rather than asserted.
///
/// They are two views of one thing now, but they are still declared in two
/// places — an enum and a struct. This is what makes adding a field to one and
/// forgetting the other a failure instead of a silent divergence.
#[test]
fn the_vocabulary_is_exactly_the_canonical_projection() {
    let projected = semantic_projection(&section()).expect("project");
    let mut names: Vec<&str> = projected.keys().map(String::as_str).collect();
    names.sort_unstable();

    let mut vocabulary: Vec<&str> = engr::dependency::ALL
        .iter()
        .map(|field| field.as_str())
        .collect();
    vocabulary.sort_unstable();

    assert_eq!(names, vocabulary);
}

/// A legacy Ref whose `section` predates the Phase-3 numeric domain. Valid v2
/// history: v2's `Ref::validate` bounds the object id and the commit, never
/// this number.
fn out_of_domain_legacy_ref() -> Ref {
    Ref {
        object: object_id(),
        section: 1u64 << 53,
        sha256: "c".repeat(64),
        commit: commit(),
    }
}

/// An unselected field cannot break a dependency that never declared it.
///
/// #35 keeps historical representations under their own contracts, and v2 never
/// bounded a Ref's `section` by the Phase-3 safe-integer domain. So a retained
/// v2 Section can hold such a Ref and still be legitimate history. A `[text]`
/// dependency has nothing to do with it.
///
/// This regressed once, from projecting all seven fields to answer a question
/// about one: the walk reached the unselected Ref, applied the P3 bound, and
/// failed the selection. Field-relative selection exists precisely to stop a
/// Ref from depending on what it did not declare.
#[test]
fn an_unselected_field_outside_the_phase_three_domain_does_not_break_a_selection() {
    let mut legacy = section();
    legacy.refs = vec![out_of_domain_legacy_ref()];

    // The unselected field is genuinely uninterpretable under P3 rules.
    semantic_value(&legacy, SemanticField::Refs)
        .expect_err("the P3 domain does not reach that far");

    // And every other field still projects, because none of them looks at it.
    for field in [
        SemanticField::Text,
        SemanticField::Admission,
        SemanticField::Role,
        SemanticField::BasedOn,
        SemanticField::Content,
        SemanticField::Relations,
    ] {
        semantic_value(&legacy, field)
            .unwrap_or_else(|error| panic!("{} must not depend on refs: {error}", field.as_str()));
    }

    // A whole-vocabulary projection is a different request and may fail; the
    // point is that nothing on the selective-Ref path makes it.
    semantic_projection(&legacy).expect_err("asking for everything asks for that too");

    // The snapshot a `[text]` reference hashes is unaffected.
    ref_snapshot(target(), &[SemanticField::Text], &legacy, commit()).expect("text alone");
    check_not_stale_at_birth(&legacy, &legacy, &[SemanticField::Text]).expect("unchanged");
}

/// The two projections give the same answer for every field in the vocabulary.
///
/// This is what makes "one canonical projection" true without materializing all
/// seven for a one-field question. They are separate code paths sharing a rule,
/// so the guarantee has to be checked rather than asserted — and checked over
/// the whole vocabulary, since a divergence in one field is exactly the shape
/// of the defect this replaced.
#[test]
fn the_two_projections_agree_field_by_field() {
    let mut section = section();
    section.refs = vec![reference(4), reference(2)];
    section.relations = vec![Relation::superseded_by(object_id())];
    section.content = vec![Supplement::new("data.note", "a note")];
    section.based_on = Some("a".repeat(40));

    let proof = serde_json::to_value(engr::proof::SectionSemantic::of(&section).expect("project"))
        .expect("value");
    let proof = proof.as_object().expect("object");

    assert_eq!(proof.len(), engr::dependency::ALL.len());
    for field in engr::dependency::ALL {
        assert_eq!(
            proof.get(field.as_str()),
            Some(&semantic_value(&section, *field).expect("value")),
            "{} differs between the two projections",
            field.as_str()
        );
    }
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

        assert_eq!(reference.target(), target());
        assert_eq!(reference.commit(), commit);
        assert_eq!(
            reference.fields(),
            [SemanticField::Admission, SemanticField::Text]
        );
        assert!(reference.digest().starts_with("1:"));

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

        let reference = SelectiveRef::stored(
            target(),
            vec![SemanticField::Role],
            commit,
            format!("1:{}", "a".repeat(64)),
        )
        .expect("canonical");
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

        let unknown = SelectiveRef::stored(
            target(),
            vec![SemanticField::Text],
            "f".repeat(40),
            format!("1:{}", "a".repeat(64)),
        )
        .expect("canonical");
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

    /// History that was changed outside the gate cannot become the authority
    /// for a new reference.
    ///
    /// The laundering this prevents is specific and quiet. Commit a Section
    /// whose wording was hand-edited while its stored seal was left alone: it
    /// is schema-valid, so `git::object_at` returns it happily. Then point a
    /// new Ref at that commit, selecting a field whose current value happens to
    /// match the edit. Before this check, admission succeeded and produced
    /// `1:5a21bf13…` — a fresh, valid, permanently verifiable proof over an
    /// out-of-band change.
    ///
    /// Structural validation does not close it. `Object::validate` checks ids,
    /// states and relations; whether the bytes are the ones that were admitted
    /// is a different question, and only the seal answers it.
    #[test]
    fn history_changed_outside_the_gate_cannot_authorize_a_reference() {
        let (_dir, root, object, honest) = workspace();

        let path = root
            .join(".engr")
            .join("objects")
            .join(format!("{}.json", object.id));
        let stored = std::fs::read_to_string(&path).expect("read");
        let edited = stored.replace("the store appends under a lock", "hand edited in history");
        assert_ne!(stored, edited, "the fixture wording must be present");
        std::fs::write(&path, edited).expect("write");
        let tampered = commit_all(&root, "tampered");

        // Current state agrees with what the tampered history says, which is
        // exactly the case that used to slip through: nothing looks stale.
        let mut current = object.clone();
        current.sections[0].text = "hand edited in history".to_owned();
        current.sections[0].sha256 = current.sections[0].recomputed_sha256().expect("v2 seal");
        let (current, seal) = phase_three_seals(&current);

        let refused = admit(&root, &current, &seal, 1, &[SemanticField::Text], &tampered)
            .expect_err("the pinned commit holds a section that fails its own seal");
        assert!(
            refused.to_string().contains("outside the gate"),
            "{refused}"
        );

        // Reading a Ref that already points at such a commit reports the
        // integrity failure, not drift — the dependency did not move, the
        // record it was pinned to was rewritten.
        let reference = SelectiveRef::stored(
            target(),
            vec![SemanticField::Text],
            tampered,
            format!("1:{}", "a".repeat(64)),
        )
        .expect("canonical");
        assert_eq!(
            evaluate(&root, &current, &seal, &reference).expect("evaluate"),
            Dependency::TargetIntegrityFailure
        );

        // The honest commit is still usable, so the check refuses tampering
        // rather than refusing history.
        let mut original = object.clone();
        original.sections[0].sha256 = original.sections[0].recomputed_sha256().expect("v2 seal");
        let (original, original_seal) = phase_three_seals(&original);
        admit(
            &root,
            &original,
            &original_seal,
            1,
            &[SemanticField::Text],
            &honest,
        )
        .expect("untouched history is still authority");
    }

    fn rev_parse(root: &Path, revision: &str) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", revision])
            .output()
            .expect("rev-parse");
        assert!(out.status.success(), "rev-parse {revision}");
        String::from_utf8(out.stdout)
            .expect("utf8")
            .trim()
            .to_owned()
    }

    fn object_type(root: &Path, oid: &str) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["cat-file", "-t", oid])
            .output()
            .expect("cat-file");
        String::from_utf8(out.stdout)
            .expect("utf8")
            .trim()
            .to_owned()
    }

    /// A reference pins the commit itself, not something that resolves to one.
    ///
    /// An annotated tag has its own object id, full-length hex like any other,
    /// and every path that reads history peels it silently — `git show
    /// <tag-oid>:<path>` returns the blob from the commit behind it. So before
    /// this check, admitting against a tag id succeeded and stored the *tag's*
    /// id as `commit` while the `values` came from the commit it pointed at:
    /// a stable digest attesting the wrong kind of identity.
    ///
    /// Well-formedness cannot catch it. `is_canonical_git_oid` says the id is
    /// 40 or 64 lowercase hex characters, which a tag id is. Only asking the
    /// repository what kind of object it is separates them.
    #[test]
    fn a_reference_pins_a_commit_and_not_a_tag_that_points_at_one() {
        let (_dir, root, object, commit) = workspace();
        git(
            &root,
            &[
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.com",
                "tag",
                "-a",
                "v1",
                "-m",
                "annotated",
            ],
        );
        let tag = rev_parse(&root, "v1");
        assert_eq!(object_type(&root, &tag), "tag", "an annotated tag object");
        assert_ne!(tag, commit, "with an id of its own");
        assert_eq!(
            rev_parse(&root, "v1^{commit}"),
            commit,
            "that peels to the commit"
        );

        let (current, seal) = phase_three_seals(&object);
        let refused = admit(&root, &current, &seal, 1, &[SemanticField::Text], &tag)
            .expect_err("the tag is not the commit");
        assert!(
            refused.to_string().contains("names a tag object"),
            "{refused}"
        );

        // The peeled commit id is accepted, so this refuses the wrong identity
        // rather than refusing tagged history.
        let reference = admit(&root, &current, &seal, 1, &[SemanticField::Text], &commit)
            .expect("the commit itself");
        assert_eq!(reference.commit(), commit);

        // And the read path fails closed on a stored Ref that names the tag.
        let stored = SelectiveRef::stored(
            reference.target(),
            reference.fields().to_vec(),
            tag,
            reference.digest(),
        )
        .expect("canonical spelling; the commit kind is evaluate's question");
        evaluate(&root, &current, &seal, &stored).expect_err("not a commit id");
    }

    /// A tree id is full-length hex too, and `git show <tree>:<path>` reads
    /// straight through it — so the same check has to refuse it.
    #[test]
    fn a_tree_id_is_not_a_commit_either() {
        let (_dir, root, object, commit) = workspace();
        let tree = rev_parse(&root, &format!("{commit}^{{tree}}"));
        assert_eq!(object_type(&root, &tree), "tree");

        let (current, seal) = phase_three_seals(&object);
        let refused = admit(&root, &current, &seal, 1, &[SemanticField::Text], &tree)
            .expect_err("a tree is not a commit");
        assert!(
            refused.to_string().contains("names a tree object"),
            "{refused}"
        );
    }

    /// Selecting a field that cannot be interpreted under the applicable
    /// contract is `SchemaMismatch`, a state #35 §9 defines — not a raw error.
    ///
    /// The material sits in history, which is where it belongs: v2 never
    /// bounded a Ref's `section` by the Phase-3 domain, so a committed Section
    /// holding one is legitimate history under its own contract, and #35 says
    /// history is not reinterpreted under the newer one. Its v2 seal still
    /// verifies, because that seal is taken over serde bytes and never applied
    /// the P3 numeric walk.
    ///
    /// The difference matters to a caller: a classified state is an answer it
    /// was told to expect and can act on, a raw error is a failure it has no
    /// contract for. This escaped as an error until the classification landed.
    #[test]
    fn a_selected_field_the_contract_cannot_interpret_is_a_schema_mismatch() {
        let (_dir, root, object, _first) = workspace();

        // Commit a Section carrying a legacy out-of-domain Ref.
        let path = root
            .join(".engr")
            .join("objects")
            .join(format!("{}.json", object.id));
        let mut legacy = object.clone();
        legacy.sections[0].refs = vec![Ref {
            object: object_id(),
            section: 1u64 << 53,
            sha256: "c".repeat(64),
            commit: "d".repeat(40),
        }];
        legacy.sections[0].sha256 = legacy.sections[0].recomputed_sha256().expect("v2 seal");
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&legacy).expect("serialize"),
        )
        .expect("write");
        let historical = commit_all(&root, "legacy reference");

        // The current target is clean, and carries a Phase-3 seal.
        let (current, seal) = phase_three_seals(&object);

        let selecting_refs = SelectiveRef::stored(
            target(),
            vec![SemanticField::Refs],
            historical.clone(),
            format!("1:{}", "a".repeat(64)),
        )
        .expect("canonical");
        assert_eq!(
            evaluate(&root, &current, &seal, &selecting_refs).expect("classified, not errored"),
            Dependency::SchemaMismatch
        );

        // A reference selecting something else is answered normally: the
        // uninterpretable field is not part of its dependency.
        let selecting_text = SelectiveRef::stored(
            selecting_refs.target(),
            vec![SemanticField::Text],
            selecting_refs.commit(),
            selecting_refs.digest(),
        )
        .expect("canonical");
        let answer = evaluate(&root, &current, &seal, &selecting_text).expect("answered");
        assert_ne!(
            answer,
            Dependency::SchemaMismatch,
            "text is interpretable regardless of what refs holds"
        );
    }

    /// A stored Ref has one spelling; authoring may have any.
    ///
    /// #35 and #13 give current resources exactly one schema-canonical
    /// representation, and `fields[]` is a protocol set whose persisted array
    /// is already in canonical order. So the two boundaries are genuinely
    /// different: `admit` takes whatever selection the author names and
    /// *produces* the canonical order, while a value read as already-stored
    /// must already be in it.
    ///
    /// Before the distinction was in the type, a Ref stored as
    /// `[text, admission]` evaluated to `Unchanged` against the digest of
    /// `[admission, text]` — the read path canonicalized before hashing, and so
    /// never noticed it had been handed a second encoding of one Ref.
    ///
    /// Normalizing on read would have been the easy fix and the wrong one: it
    /// accepts the second spelling instead of refusing it, which is what having
    /// one canonical representation is supposed to prevent.
    #[test]
    fn a_stored_reference_carries_the_canonical_field_order_and_only_that() {
        let (_dir, root, object, commit) = workspace();
        let (current, seal) = phase_three_seals(&object);

        // Authoring: either order is accepted, and one order comes back.
        let canonical = admit(
            &root,
            &current,
            &seal,
            1,
            &[SemanticField::Text, SemanticField::Admission],
            &commit,
        )
        .expect("admitted");
        assert_eq!(
            canonical.fields(),
            [SemanticField::Admission, SemanticField::Text],
            "admission emits the canonical order whichever way it was asked"
        );
        let other_way = admit(
            &root,
            &current,
            &seal,
            1,
            &[SemanticField::Admission, SemanticField::Text],
            &commit,
        )
        .expect("admitted");
        assert_eq!(canonical.digest(), other_way.digest());

        // Reading: the non-canonical spelling is refused, not repaired.
        let refused = SelectiveRef::stored(
            canonical.target(),
            vec![SemanticField::Text, SemanticField::Admission],
            canonical.commit(),
            canonical.digest(),
        )
        .expect_err("a stored reference is already canonical");
        assert!(refused.to_string().contains("canonical order"), "{refused}");

        // The canonical one reads back and evaluates.
        let stored = SelectiveRef::stored(
            canonical.target(),
            canonical.fields().to_vec(),
            canonical.commit(),
            canonical.digest(),
        )
        .expect("canonical");
        assert_eq!(
            evaluate(&root, &current, &seal, &stored).expect("evaluate"),
            Dependency::Unchanged
        );

        // The other members are checked as written too.
        SelectiveRef::stored(
            "obj:0192f0c8-1a2b-7c3d-8e4f-5a6b7c8d9e0f:01",
            vec![SemanticField::Text],
            commit.clone(),
            canonical.digest(),
        )
        .expect_err("non-canonical target spelling");
        SelectiveRef::stored(
            canonical.target(),
            vec![SemanticField::Text],
            &commit[..8],
            canonical.digest(),
        )
        .expect_err("abbreviated commit");
        SelectiveRef::stored(
            canonical.target(),
            vec![SemanticField::Text],
            commit.clone(),
            "a".repeat(64),
        )
        .expect_err("unversioned digest scalar");
        SelectiveRef::stored(canonical.target(), Vec::new(), commit, canonical.digest())
            .expect_err("no implicit full reference");
    }

    /// A Ref whose proof is still good, pointing at a target that is gone.
    ///
    /// Ruled on #35 (`5395844059`): current absence is its own state. Not
    /// integrity failure — the Object still verifies, because the removal went
    /// through a supported transition and resealed. Not drift — nothing was
    /// compared. Not provenance unavailable — the recorded commit is right
    /// there and still holds the target. And not a raw NOT_FOUND escaping the
    /// classification, which is what it used to be.
    #[test]
    fn a_target_removed_through_a_supported_transition_is_missing_not_broken() {
        let (_dir, root, object, commit) = workspace();
        let (current, seal) = phase_three_seals(&object);
        let reference =
            admit(&root, &current, &seal, 1, &[SemanticField::Text], &commit).expect("admitted");

        // The history the Ref pins is untouched and still verifies.
        assert_eq!(
            evaluate(&root, &current, &seal, &reference).expect("evaluate"),
            Dependency::Unchanged
        );

        // Remove the Section and reseal, the way a supported transition would.
        let mut without = current.clone();
        without.sections.clear();
        without.rev += 1;
        let (without, resealed) = phase_three_seals(&without);
        engr::integrity::check_object_integrity(&without, &resealed)
            .expect("the object still vouches for itself");

        assert_eq!(
            evaluate(&root, &without, &resealed, &reference).expect("evaluate"),
            Dependency::TargetMissing
        );
    }

    /// A Section deleted by hand is not a removal, and must not be able to
    /// present itself as one.
    ///
    /// The aggregate seal covers `sections[]`, so deleting one breaks it. That
    /// is why integrity is asked before absence: reversing the order would let
    /// a hand-edited Object report `TargetMissing`, which reads as a legitimate
    /// removal and is the one answer tampering must not be able to produce.
    #[test]
    fn a_section_deleted_by_hand_is_not_reported_as_a_removal() {
        let (_dir, root, object, commit) = workspace();
        let (current, seal) = phase_three_seals(&object);
        let reference =
            admit(&root, &current, &seal, 1, &[SemanticField::Text], &commit).expect("admitted");

        // Same deletion, but the old seal is kept rather than recomputed.
        let mut tampered = current.clone();
        tampered.sections.clear();

        assert_eq!(
            evaluate(&root, &tampered, &seal, &reference).expect("evaluate"),
            Dependency::TargetIntegrityFailure,
            "the aggregate no longer follows from what the object holds"
        );
    }

    /// Admission answers the same question in the same order that reading does.
    ///
    /// The mirror of `a_section_deleted_by_hand_is_not_reported_as_a_removal`,
    /// through the authoring API. A Section deleted out of band while the old
    /// aggregate seal is retained used to be refused as NOT_FOUND, because the
    /// existence lookup came first and returned before the aggregate was ever
    /// checked. #13 keeps invalid authority distinct from absent authority, and
    /// a trust-sensitive path that reports the first as the second has told the
    /// caller the wrong thing about why it failed.
    ///
    /// The existing integrity test does not cover this: it edits a Section's
    /// text, so the Section is still there and the lookup succeeds.
    #[test]
    fn admission_reports_tampering_as_tampering_and_not_as_a_missing_target() {
        let (_dir, root, object, commit) = workspace();
        let (current, seal) = phase_three_seals(&object);

        // Deleted out of band: the old aggregate seal is kept.
        let mut tampered = current.clone();
        tampered.sections.clear();
        let refused = admit(&root, &tampered, &seal, 1, &[SemanticField::Text], &commit)
            .expect_err("refused");
        assert_eq!(
            refused.code,
            engr::EXIT_INVARIANT,
            "integrity, not NOT_FOUND: {}",
            refused.message
        );

        // The same removal done properly is still refused, but as what it is:
        // there is genuinely no such Section to reference.
        let mut removed = current.clone();
        removed.sections.clear();
        removed.rev += 1;
        let (removed, resealed) = phase_three_seals(&removed);
        let refused = admit(
            &root,
            &removed,
            &resealed,
            1,
            &[SemanticField::Text],
            &commit,
        )
        .expect_err("nothing to reference");
        assert_eq!(
            refused.code,
            engr::EXIT_NOT_FOUND,
            "absent, not tampered: {}",
            refused.message
        );
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

/// A Section id inside target text is still a Section id, and still bound by
/// the shared numeric domain.
///
/// #35 §7 says string embedding is not an escape from it. It has to be checked
/// where the string is parsed, because the safe-integer walk that guards every
/// other protocol integer runs over JSON *numbers* — and this one is inside a
/// string, where that walk cannot reach it. Without this, `2^53` produced a
/// perfectly valid `1:` digest for a target the schema says cannot denote a
/// Section.
#[test]
fn a_section_id_in_target_text_cannot_escape_the_shared_domain() {
    let ceiling = (1u64 << 53) - 1;
    let at_the_bound = format!("obj:{}:{ceiling}", object_id());
    assert_eq!(
        parse_target(&at_the_bound)
            .expect("the bound itself is inside")
            .1,
        ceiling
    );

    for outside in [1u64 << 53, (1u64 << 53) + 1, u64::MAX] {
        let target = format!("obj:{}:{outside}", object_id());
        let refused = parse_target(&target).expect_err("past the ceiling");
        assert!(
            refused.to_string().contains("safe-integer domain"),
            "{outside}: {refused}"
        );
        ref_snapshot(&target, &[SemanticField::Text], &section(), commit())
            .expect_err("and it cannot be hashed either");
    }
}

/// One Section identity has one spelling.
///
/// `:01` and `:1` name the same Section and hash differently, so accepting both
/// would give one identity two digests — the ambiguity a canonical form exists
/// to remove. The check rebuilds the target from what it parsed, so the reader
/// accepts exactly what the emitter writes and the two cannot drift apart.
#[test]
fn a_target_has_exactly_one_canonical_spelling() {
    let id = object_id();
    parse_target(&format!("obj:{id}:1")).expect("canonical");

    for padded in [
        format!("obj:{id}:01"),
        format!("obj:{id}:001"),
        format!("obj:{id}:+1"),
        format!("obj:{id}:1 "),
    ] {
        let refused = parse_target(&padded).expect_err(&padded);
        assert!(
            refused.to_string().contains("canonical"),
            "{padded}: {refused}"
        );
        ref_snapshot(&padded, &[SemanticField::Text], &section(), commit())
            .expect_err("and it cannot be hashed either");
    }

    // Whatever the emitter writes is what the reader accepts, by construction.
    for section in [1u64, 9, 10, 4095, (1u64 << 53) - 1] {
        let emitted = engr::proof::section_target(&id, section);
        assert_eq!(parse_target(&emitted).expect("round trip").1, section);
    }
}
