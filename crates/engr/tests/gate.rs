//! Human Gate and Agent Rule Review are the only ways in. These tests pin both.

use engr::model::{
    Action, Content, HumanConfirmation, Merge, Payload, Provenance, Ref, TaggedAdmission,
};
use engr::semantics::{Relation, Role, State, Supplement};
use engr::{gate, ops, store};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn workspace() -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().to_path_buf();
    store::init(&root).expect("init");
    (dir, root)
}

fn commit_all(root: &Path, message: &str) -> String {
    for args in [
        vec!["init", "-q"],
        vec!["add", "."],
        vec![
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            message,
        ],
    ] {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .expect("git");
        assert!(status.success());
    }
    engr::git::head(root).expect("HEAD")
}

fn content(text: &str) -> Content {
    Content {
        text: text.to_owned(),
        based_on: None,
        refs: Vec::new(),
        ..Content::default()
    }
}

fn candidate_event(candidate: &gate::Candidate) -> engr::model::Event {
    engr::model::Event {
        format: engr::model::EVENT_FORMAT.to_owned(),
        version: engr::EVENT_ENVELOPE_VERSION,
        event_id: engr::model::new_id(),
        rev: candidate.binding.expected_rev + 1,
        time: "2026-08-13T00:00:00Z".to_owned(),
        payload: candidate.payload.clone(),
        provenance: Provenance::Tagged {
            admission: TaggedAdmission {
                kind: engr::semantics::Admission::Human,
                confirmation: Some(HumanConfirmation {
                    challenge: candidate.challenge.clone(),
                    candidate_digest: candidate.candidate_digest.clone(),
                }),
                rule_review: None,
            },
        },
    }
}
/// A Human Event for a change that really was prepared.
///
/// The durable boundary admits one only against the candidate it was prepared
/// as, which is what makes the challenge unforgeable — so a direct caller that
/// wants to reach that boundary goes through `prepare`, exactly as `confirm`
/// does. [`direct_human_event`] stays for records that must be refused before
/// the admission proof is ever reached.
fn admissible_human_event(root: &Path, payload: Payload) -> engr::model::Event {
    let prepared = gate::prepare(root, payload).expect("prepare");
    candidate_event(&prepared.candidate)
}

fn direct_human_event(root: &Path, id: &str, payload: Payload, rev: u64) -> engr::model::Event {
    let confirmation = store::load_events(root, id)
        .expect("existing history")
        .into_iter()
        .find_map(|event| event.human_confirmation().cloned())
        .expect("human confirmation");
    engr::model::Event {
        format: engr::model::EVENT_FORMAT.to_owned(),
        version: engr::EVENT_ENVELOPE_VERSION,
        event_id: engr::model::new_id(),
        rev,
        time: "2026-08-23T00:00:00Z".to_owned(),
        provenance: Provenance::Tagged {
            admission: TaggedAdmission {
                kind: engr::semantics::Admission::Human,
                confirmation: Some(confirmation),
                rule_review: None,
            },
        },
        payload,
    }
}

fn payload(action: Action, object: &str, text: &str) -> Payload {
    Payload {
        action,
        object: object.to_owned(),
        becomes: None,
        content: content(text),
    }
}

fn empty(action: Action, object: &str) -> Payload {
    Payload {
        action,
        object: object.to_owned(),
        becomes: None,
        content: Content {
            text: String::new(),
            based_on: None,
            refs: Vec::new(),
            ..Content::default()
        },
    }
}

fn text_ref(root: &Path, object: &str, section: u64, commit: &str) -> Ref {
    let target = ops::effective(root, object).expect("reference target");
    let seal = target.sha256.as_deref().expect("aggregate seal");
    Ref::selective(
        engr::dependency::admit(
            root,
            &target,
            seal,
            section,
            &[engr::dependency::SemanticField::Text],
            commit,
        )
        .expect("admit selective reference"),
    )
}

fn stored_text_ref(object: &str, section: u64, commit: &str, digest: &str) -> Ref {
    Ref::selective(
        engr::dependency::SelectiveRef::stored(
            engr::proof::section_target(object, section),
            vec![engr::dependency::SemanticField::Text],
            commit,
            digest,
        )
        .expect("stored selective reference"),
    )
}

/// Prepare, then confirm with the exact phrase.
fn admit(root: &Path, payload: Payload) -> engr::model::Object {
    let prepared = gate::prepare(root, payload).expect("prepare");
    let response = format!("CONFIRM {}", prepared.candidate.challenge);
    gate::confirm(root, &response).expect("confirm").object
}

/// One way a stored candidate can be rewritten on disk, named so the matrix
/// below reads as a list of risks rather than a list of closure types.
type Tamper = (&'static str, Box<dyn Fn(&mut serde_json::Value)>);

fn new_object(root: &Path, title: &str) -> String {
    let id = engr::model::new_id();
    admit(root, payload(Action::ObjectCreated, &id, title));
    id
}

#[test]
fn a_candidate_is_only_admitted_by_the_exact_phrase() {
    let (_dir, root) = workspace();
    let id = engr::model::new_id();
    let prepared =
        gate::prepare(&root, payload(Action::ObjectCreated, &id, "a title")).expect("prepare");
    let code = prepared.candidate.challenge.clone();

    for response in [
        "yes",
        "confirm",
        &code,
        &format!("CONFIRM  {code}"),
        &format!("  CONFIRM {code}"),
    ] {
        assert!(
            gate::confirm(&root, response).is_err(),
            "{response:?} must not be assent"
        );
    }
    // None of those were exact, but none of them named the code in the shape
    // that discards it either, so the candidate survives.
    assert!(gate::find(&root, &code).is_ok());
    let object = gate::confirm(&root, &format!("CONFIRM {code}"))
        .expect("the exact phrase")
        .object;
    assert_eq!(object.title, "a title");
}

#[test]
fn hedged_assent_discards_the_candidate() {
    let (_dir, root) = workspace();
    let id = engr::model::new_id();
    let prepared =
        gate::prepare(&root, payload(Action::ObjectCreated, &id, "a title")).expect("prepare");
    let code = prepared.candidate.challenge.clone();

    let error = gate::confirm(&root, &format!("CONFIRM {code} but reword the second line"))
        .expect_err("hedged assent is not assent");
    assert_eq!(error.code, engr::EXIT_USAGE);
    assert!(
        gate::find(&root, &code).is_err(),
        "a hedged response discards the candidate rather than admitting it"
    );
}

#[test]
fn malformed_confirmation_codes_never_escape_candidates() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "confirmation paths");
    let prepared =
        gate::prepare(&root, payload(Action::SectionAdded, &id, "pending")).expect("prepare");
    let code = prepared.candidate.challenge.clone();
    let format_path = store::engr_dir(&root).join("format.json");
    let object_path = store::object_path(&root, &id);
    let outside_path = root.join("outside.json");
    std::fs::write(&outside_path, "outside candidate storage").expect("outside fixture");
    let candidate_path = store::candidate_path(&root, &code).expect("candidate path");
    let before: Vec<_> = [&format_path, &object_path, &outside_path, &candidate_path]
        .iter()
        .map(|path| ((*path).clone(), std::fs::read(path).expect("snapshot")))
        .collect();

    for malformed in [
        "../format".to_owned(),
        format!("../objects/{id}"),
        "../../outside".to_owned(),
    ] {
        let error = gate::confirm(&root, &format!("CONFIRM {malformed} commentary"))
            .expect_err("a path-shaped response is not a qualified assent");
        assert_eq!(error.code, engr::EXIT_USAGE);
        for (path, expected) in &before {
            assert_eq!(
                std::fs::read(path).expect("snapshot after refusal"),
                *expected
            );
        }
    }
    for malformed in ["../format", "../objects/not-an-object", "../../outside"] {
        assert_eq!(
            gate::find(&root, malformed)
                .expect_err("candidate lookup must reject malformed paths")
                .code,
            engr::EXIT_USAGE
        );
        assert_eq!(
            gate::discard(&root, malformed)
                .expect_err("candidate discard must reject malformed paths")
                .code,
            engr::EXIT_USAGE
        );
    }

    let error = gate::confirm(&root, &format!("CONFIRM {code} commentary"))
        .expect_err("a qualified valid code is discarded without nested locking");
    assert_eq!(error.code, engr::EXIT_USAGE);
    assert!(gate::find(&root, &code).is_err());
}

/// A refusal at the generation boundary writes nothing at all — not even a
/// prefix of what it was about to write.
///
/// The workspace goes back a generation with a pending candidate outstanding,
/// which is the awkward moment: `confirm` has an Event to append, a projection
/// to save and a candidate to clear, and none of them may happen. The trigger is
/// the workspace's own authority saying v2, because that is what an older
/// workspace *is* — a hand-edited Object cannot make one, and no longer
/// pretends to.
#[test]
fn direct_confirmation_refuses_an_older_workspace_without_partial_mutation() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "generation boundary");
    let prepared = gate::prepare(&root, empty(Action::ObjectClosed, &id)).expect("prepare");
    let code = prepared.candidate.challenge.clone();
    let object_path = store::object_path(&root, &id);
    let candidate_path = store::candidate_path(&root, &code).expect("candidate path");
    let events_path = store::events_path(&root, &id);

    std::fs::write(
        store::engr_dir(&root).join("format.json"),
        r#"{"format":"engr-workspace","version":2}"#,
    )
    .expect("put the workspace back a generation");

    let object_before = std::fs::read(&object_path).expect("object snapshot");
    let events_before = std::fs::read(&events_path).expect("events snapshot");
    let candidate_before = std::fs::read(&candidate_path).expect("candidate snapshot");
    for response in [
        format!("CONFIRM {code} but leave it open"),
        format!("CONFIRM {code}"),
    ] {
        let error = gate::confirm(&root, &response).expect_err("an older workspace is read-only");
        assert_eq!(error.code, engr::EXIT_SCHEMA);
        assert!(error.message.contains("engr migrate"));
        assert_eq!(
            std::fs::read(&object_path).expect("object after refusal"),
            object_before
        );
        assert_eq!(
            std::fs::read(&events_path).expect("events after refusal"),
            events_before
        );
        assert_eq!(
            std::fs::read(&candidate_path).expect("candidate after refusal"),
            candidate_before
        );
    }
}

#[test]
fn section_ids_are_never_reused() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "ids");
    admit(&root, payload(Action::SectionAdded, &id, "first"));
    admit(&root, payload(Action::SectionAdded, &id, "second"));
    let object = admit(&root, empty(Action::SectionDeleted { section: 2 }, &id));
    assert_eq!(object.next_section_id, 3);

    // The counter, not max(existing) + 1, decides the next id — otherwise this
    // section would take §2 and every outside reference to the deleted one would
    // silently point at different content.
    let object = admit(&root, payload(Action::SectionAdded, &id, "third"));
    let ids: Vec<u64> = object.sections.iter().map(|section| section.id).collect();
    assert_eq!(ids, vec![1, 3]);
}

#[test]
fn merging_keeps_the_destination_id_and_removes_its_sources() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "merging");
    admit(&root, payload(Action::SectionAdded, &id, "one"));
    admit(&root, payload(Action::SectionAdded, &id, "two"));
    let object = admit(
        &root,
        payload(
            Action::SectionMerged {
                merge: Merge::Into {
                    destination: 1,
                    sources: vec![2],
                },
            },
            &id,
            "one and two together",
        ),
    );
    let ids: Vec<u64> = object.sections.iter().map(|section| section.id).collect();
    assert_eq!(ids, vec![1]);
    assert_eq!(object.sections[0].text, "one and two together");
}

/// The activated generation writes the merge that names its survivor.
#[test]
fn the_phase_three_merge_representation_is_admitted() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "durable boundary");
    admit(&root, payload(Action::SectionAdded, &id, "one"));
    admit(&root, payload(Action::SectionAdded, &id, "two"));

    let object = admit(
        &root,
        payload(
            Action::SectionMerged {
                merge: Merge::Into {
                    destination: 1,
                    sources: vec![2],
                },
            },
            &id,
            "together",
        ),
    );
    assert_eq!(object.sections.len(), 1);
    assert_eq!(object.sections[0].id, 1);
    assert_eq!(object.sections[0].text, "together");
}

/// A consumed Section id is never handed out again, so a reference to one is
/// pinned to wording that exists nowhere and points at an id that will never
/// exist. v1 has no redirect and no tombstone, so the merge is refused and
/// whoever holds the reference decides what it should say now.
#[test]
fn a_merge_cannot_consume_a_section_something_still_depends_on() {
    let (_dir, root) = workspace();
    let target = new_object(&root, "target");
    let source = new_object(&root, "source");
    admit(&root, payload(Action::SectionAdded, &target, "depended on"));
    admit(&root, payload(Action::SectionAdded, &target, "the other"));
    let commit = commit_all(&root, "record target wording");

    let mut dependent = payload(Action::SectionAdded, &source, "rests on §1");
    dependent.content.based_on = Some(commit.clone());
    dependent.content.refs = vec![text_ref(&root, &target, 1, &commit)];
    admit(&root, dependent);

    let error = gate::prepare(
        &root,
        payload(
            Action::SectionMerged {
                merge: Merge::Into {
                    destination: 2,
                    sources: vec![1],
                },
            },
            &target,
            "together",
        ),
    )
    .expect_err("§1 is still depended on");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
}

#[test]
fn a_closed_object_refuses_section_changes() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "closing");
    admit(&root, payload(Action::SectionAdded, &id, "one"));
    let object = admit(&root, empty(Action::ObjectClosed, &id));
    assert_eq!(object.state, State::Closed);

    let error = gate::prepare(&root, payload(Action::SectionAdded, &id, "two"))
        .expect_err("a closed object is sealed");
    assert_eq!(error.code, engr::EXIT_INVARIANT);

    admit(&root, empty(Action::ObjectReopened, &id));
    admit(&root, payload(Action::SectionAdded, &id, "two"));
}

#[test]
fn one_live_candidate_per_object() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "candidates");
    let first = gate::prepare(&root, payload(Action::SectionAdded, &id, "first draft"))
        .expect("first")
        .candidate
        .challenge
        .clone();
    let second =
        gate::prepare(&root, payload(Action::SectionAdded, &id, "second draft")).expect("second");
    assert_eq!(second.superseded, vec![first.clone()]);
    assert!(
        gate::find(&root, &first).is_err(),
        "a human should never hold two codes for the same object"
    );
}

#[test]
fn direct_gate_callers_cannot_persist_non_v7_object_identities() {
    let (_dir, root) = workspace();
    for id in ["not-a-uuid", "550e8400-e29b-41d4-a716-446655440000"] {
        let error = gate::prepare(
            &root,
            payload(Action::ObjectCreated, id, "invalid identity"),
        )
        .expect_err("a direct caller cannot bypass Object identity validation");
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{id}");
    }

    let source = new_object(&root, "source");
    let mut invalid_ref = payload(Action::SectionAdded, &source, "invalid dependency");
    invalid_ref.content.refs = vec![Ref::legacy("not-a-uuid", 1, "0".repeat(64), "HEAD")];
    let error =
        gate::prepare(&root, invalid_ref).expect_err("a Ref Object identity is persisted data too");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(
        gate::pending(&root).expect("pending candidates").is_empty(),
        "rejected direct inputs must not leave a candidate behind"
    );
}

#[test]
fn title_actions_cannot_carry_hidden_basis_or_references() {
    let (_dir, root) = workspace();
    let mut created = payload(Action::ObjectCreated, &engr::model::new_id(), "new title");
    created.content.based_on = Some("HEAD".to_owned());
    created.content.refs = vec![Ref::legacy(
        engr::model::new_id(),
        1,
        "0".repeat(64),
        "HEAD",
    )];
    let error = gate::prepare(&root, created).expect_err("a title has no hidden context");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(gate::pending(&root).expect("pending").is_empty());

    let id = new_object(&root, "old title");
    let mut renamed = payload(Action::ObjectRenamed, &id, "new title");
    renamed.content.based_on = Some("HEAD".to_owned());
    let error = gate::prepare(&root, renamed).expect_err("a renamed title has no basis");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert_eq!(
        store::load_events(&root, &id).expect("events").len(),
        1,
        "rejected title context must not enter Event history"
    );
}

#[test]
fn public_gate_mutations_serialize_direct_callers() {
    use std::sync::{Arc, Barrier};

    let (_dir, root) = workspace();
    let id = new_object(&root, "direct lock");
    let first = payload(Action::SectionAdded, &id, "first proposal");
    let second = payload(Action::SectionAdded, &id, "second proposal");
    let start = Arc::new(Barrier::new(2));
    let (first_prepare, second_prepare) = std::thread::scope(|scope| {
        let first_start = Arc::clone(&start);
        let second_start = Arc::clone(&start);
        let first_root = &root;
        let second_root = &root;
        let first = scope.spawn(move || {
            first_start.wait();
            gate::prepare(first_root, first)
        });
        let second = scope.spawn(move || {
            second_start.wait();
            gate::prepare(second_root, second)
        });
        (
            first.join().expect("first prepare"),
            second.join().expect("second prepare"),
        )
    });
    assert!(first_prepare.is_ok() && second_prepare.is_ok());
    let candidates = gate::pending(&root).expect("one live candidate");
    assert_eq!(
        candidates.len(),
        1,
        "direct prepares must supersede under one lock"
    );

    let response = format!("CONFIRM {}", candidates[0].challenge);
    let start = Arc::new(Barrier::new(2));
    let (first_confirm, second_confirm) = std::thread::scope(|scope| {
        let first_start = Arc::clone(&start);
        let second_start = Arc::clone(&start);
        let first_root = &root;
        let second_root = &root;
        let first_response = response.clone();
        let second_response = response.clone();
        let first = scope.spawn(move || {
            first_start.wait();
            gate::confirm(first_root, &first_response)
        });
        let second = scope.spawn(move || {
            second_start.wait();
            gate::confirm(second_root, &second_response)
        });
        (
            first.join().expect("first confirm"),
            second.join().expect("second confirm"),
        )
    });
    assert_eq!(
        [first_confirm, second_confirm]
            .iter()
            .filter(|outcome| outcome.is_ok())
            .count(),
        1,
        "one direct confirmation may admit the candidate"
    );
    assert_eq!(
        store::load_events(&root, &id).expect("events").len(),
        2,
        "concurrent confirmation must not append competing revision 2 events"
    );
}

#[test]
fn direct_gate_callers_canonicalize_git_anchors_before_confirmation() {
    let (_dir, root) = workspace();
    let target = new_object(&root, "target");
    let source = new_object(&root, "source");
    let target = admit(
        &root,
        payload(Action::SectionAdded, &target, "target wording"),
    );
    let target_id = target.id;
    let commit = commit_all(&root, "record target wording");
    let reference = text_ref(&root, &target_id, 1, &commit);

    let mut direct = payload(
        Action::SectionAdded,
        &source.to_ascii_uppercase(),
        "dependent wording",
    );
    direct.content.based_on = Some("HEAD".to_owned());
    direct.content.refs = vec![reference];

    let prepared = gate::prepare(&root, direct).expect("direct payload is canonicalized");
    assert_eq!(prepared.candidate.payload.object, source);
    assert_eq!(
        prepared.candidate.payload.content.based_on.as_deref(),
        Some(commit.as_str())
    );
    assert_eq!(
        prepared.candidate.payload.content.refs[0]
            .target_identity()
            .expect("target")
            .0,
        target_id
    );
    assert_eq!(prepared.candidate.payload.content.refs[0].commit(), commit);
    assert!(prepared.candidate.candidate_digest.starts_with("1:"));

    let response = format!("CONFIRM {}", prepared.candidate.challenge);
    let event = gate::confirm(&root, &response)
        .expect("confirm canonical candidate")
        .event;
    assert_eq!(
        event.payload.content.based_on.as_deref(),
        Some(commit.as_str())
    );
    assert_eq!(event.payload.content.refs[0].commit(), commit);
    assert!(
        !serde_json::to_string(&event)
            .expect("event JSON")
            .contains("HEAD"),
        "symbolic input must never reach the Event"
    );
}

#[test]
fn references_are_checked_at_the_gate() {
    let (_dir, root) = workspace();
    let target = new_object(&root, "the target");
    let source = new_object(&root, "the source");
    admit(
        &root,
        payload(Action::SectionAdded, &target, "depended upon"),
    );
    let commit = commit_all(&root, "record target");
    let good_ref = text_ref(&root, &target, 1, &commit);

    let target_path = store::object_path(&root, &target);
    let target_before = std::fs::read(&target_path).expect("target bytes");
    let mut tampered: serde_json::Value =
        serde_json::from_slice(&target_before).expect("target JSON");
    tampered["sections"][0]["text"] = serde_json::json!("edited outside the gate");
    // Written as this generation's canonical bytes, so the refusal that follows
    // is about the seal rather than about the spelling.
    std::fs::write(
        &target_path,
        engr::proof::canonical_bytes(&tampered, "target").expect("canonical"),
    )
    .expect("tamper target");
    let mut forged_current = payload(Action::SectionAdded, &source, "depends on forged wording");
    forged_current.content.refs = vec![good_ref.clone()];
    let error = gate::prepare(&root, forged_current)
        .expect_err("a reference cannot trust a stale stored target hash");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("TargetIntegrityFailure"), "{error}");
    std::fs::write(&target_path, &target_before).expect("restore target");

    admit(
        &root,
        payload(
            Action::SectionRevised { section: 1 },
            &target,
            "new uncommitted wording",
        ),
    );
    let mut uncommitted = payload(Action::SectionAdded, &source, "depends on new wording");
    uncommitted.content.refs = vec![good_ref.clone()];
    let error = gate::prepare(&root, uncommitted)
        .expect_err("a commit cannot be paired with newer uncommitted wording");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("Drifted"));

    admit(
        &root,
        payload(
            Action::SectionRevised { section: 1 },
            &target,
            "depended upon",
        ),
    );

    let mut with_missing_object = payload(Action::SectionAdded, &source, "depends");
    with_missing_object.content.refs = vec![stored_text_ref(
        &engr::model::new_id(),
        1,
        &commit,
        &format!("1:{}", "0".repeat(64)),
    )];
    assert_eq!(
        gate::prepare(&root, with_missing_object)
            .expect_err("a reference to a missing object is refused")
            .code,
        engr::EXIT_NOT_FOUND
    );

    let mut with_missing_section = payload(Action::SectionAdded, &source, "depends");
    with_missing_section.content.refs = vec![stored_text_ref(
        &target,
        99,
        &commit,
        &format!("1:{}", "0".repeat(64)),
    )];
    assert_eq!(
        gate::prepare(&root, with_missing_section)
            .expect_err("a reference to a missing section is refused")
            .code,
        engr::EXIT_NOT_FOUND
    );

    let mut with_wrong_hash = payload(Action::SectionAdded, &source, "depends");
    with_wrong_hash.content.refs = vec![stored_text_ref(
        &target,
        1,
        &commit,
        &format!("1:{}", "0".repeat(64)),
    )];
    assert_eq!(
        gate::prepare(&root, with_wrong_hash)
            .expect_err("a reference cannot pin something the target never said")
            .code,
        engr::EXIT_INVARIANT
    );

    let mut good = payload(Action::SectionAdded, &source, "depends");
    good.content.refs = vec![good_ref];
    gate::prepare(&root, good).expect("a well-formed reference is admitted");
}

#[test]
fn historical_references_decode_the_snapshot_workspace_format() {
    let (_dir, root) = workspace();
    let target = new_object(&root, "historical target");
    let source = new_object(&root, "historical source");
    admit(
        &root,
        payload(Action::SectionAdded, &target, "historical wording"),
    );
    let pinned = store::load_object(&root, &target).expect("target").sections[0]
        .sha256
        .clone();
    let current_commit = commit_all(&root, "current historical snapshot");
    assert_eq!(
        engr::git::object_at(&root, &current_commit, &target)
            .expect("current snapshot format")
            .expect("current target")
            .section(1)
            .expect("current section")
            .sha256,
        pinned
    );

    let format_path = store::engr_dir(&root).join("format.json");
    let format_before = std::fs::read(&format_path).expect("format");
    let objects_before: Vec<_> = store::object_ids(&root)
        .expect("object ids")
        .into_iter()
        .map(|id| {
            let path = store::object_path(&root, &id);
            (path.clone(), std::fs::read(path).expect("object"))
        })
        .collect();
    for (path, _) in &objects_before {
        let decoded: engr::model::Object =
            serde_json::from_slice(&std::fs::read(path).expect("object")).expect("object");
        let mut object: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).expect("object")).expect("json");
        object.as_object_mut().expect("object").remove("sha256");
        object["format"] = serde_json::Value::String("engr-object".to_owned());
        object["version"] = serde_json::Value::from(1);
        let state = object
            .as_object_mut()
            .expect("object")
            .remove("state")
            .expect("state");
        object["status"] = state;
        for stored in object["sections"].as_array_mut().expect("sections") {
            let stored = stored.as_object_mut().expect("section");
            let id = stored["id"].as_u64().expect("section id");
            let section = decoded.section(id).expect("decoded section");
            stored.remove("admission");
            let admitted_at = stored.remove("admitted_at").expect("admitted_at");
            stored.insert("confirmed_at".to_owned(), admitted_at);
            stored.insert(
                "sha256".to_owned(),
                serde_json::Value::String(
                    section.recomputed_sha256().expect("legacy Section seal"),
                ),
            );
        }
        std::fs::write(path, serde_json::to_vec_pretty(&object).expect("json"))
            .expect("legacy object");
    }
    std::fs::remove_file(&format_path).expect("remove workspace authority");
    let legacy_commit = commit_all(&root, "legacy v0 historical snapshot");
    let legacy = engr::git::object_at(&root, &legacy_commit, &target)
        .expect("recognized legacy snapshot")
        .expect("legacy target");
    let legacy = legacy.section(1).expect("legacy section");
    assert_eq!(
        legacy.recomputed_sha256().expect("legacy seal"),
        legacy.sha256
    );

    for (path, bytes) in &objects_before {
        std::fs::write(path, bytes).expect("restore object");
    }
    std::fs::write(&format_path, &format_before).expect("restore workspace authority");
    let mut legacy_reference = payload(Action::SectionAdded, &source, "uses old wording");
    legacy_reference.content.refs = vec![text_ref(&root, &target, 1, &legacy_commit)];
    let prepared = gate::prepare(&root, legacy_reference).expect("legacy ref is admitted");
    gate::discard(&root, &prepared.candidate.challenge).expect("discard test candidate");

    commit_all(&root, "restore canonical historical snapshot");
    std::fs::write(&format_path, r#"{"format":"engr-workspace","version":99}"#)
        .expect("unsupported workspace authority");
    let unsupported_commit = commit_all(&root, "unsupported historical snapshot");
    std::fs::write(&format_path, &format_before).expect("restore current workspace authority");
    let error = engr::git::object_at(&root, &unsupported_commit, &target)
        .expect_err("an unknown historical workspace version is refused");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("workspace version 99"));

    let mut unsupported_reference = payload(Action::SectionAdded, &source, "must not guess");
    unsupported_reference.content.refs = vec![stored_text_ref(
        &target,
        1,
        &unsupported_commit,
        &format!("1:{}", "0".repeat(64)),
    )];
    let error = gate::prepare(&root, unsupported_reference)
        .expect_err("a reference cannot decode an unsupported historical workspace");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("cannot be interpreted"));
}

#[test]
fn sibling_references_are_allowed_but_direct_self_reference_is_not() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "self reference");
    admit(&root, payload(Action::SectionAdded, &id, "the first"));
    let commit = commit_all(&root, "record sibling");

    let mut inward = payload(Action::SectionAdded, &id, "the second");
    inward.content.refs = vec![text_ref(&root, &id, 1, &commit)];
    admit(&root, inward);

    let commit = commit_all(&root, "record dependent sibling");
    let mut direct = payload(Action::SectionRevised { section: 2 }, &id, "self-dependent");
    direct.content.refs = vec![text_ref(&root, &id, 2, &commit)];
    let error = gate::prepare(&root, direct).expect_err("a direct self reference is refused");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("cannot directly reference itself"));
}

#[test]
fn a_legacy_revision_candidate_without_semantic_history_cannot_be_confirmed() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "legacy candidate");
    admit(&root, payload(Action::SectionAdded, &id, "old wording"));
    let prepared = gate::prepare(
        &root,
        payload(Action::SectionRevised { section: 1 }, &id, "new wording"),
    )
    .expect("prepare");
    let path = store::candidate_path(&root, &prepared.candidate.challenge).expect("path");
    let mut stored: serde_json::Value = store::read_json(&path).expect("candidate");
    stored
        .as_object_mut()
        .expect("candidate object")
        .remove("previous_semantics_recorded");
    write_raw(&path, &stored).expect("legacy candidate");

    let error = gate::confirm(&root, &format!("CONFIRM {}", prepared.candidate.challenge))
        .expect_err("semantic history is required");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("prepare it again"));
}

/// A candidate is not only a mutation. It is also the binding that decides
/// whether admission is fresh, and the previous wording the human is shown the
/// change against. Rewriting either on disk would let a candidate present or
/// bind a different confirmation context and still pass its own hash, so one
/// integrity value covers all of it.
///
/// Exhaustive by construction, not by sample. Every field of `PreparedContext`
/// has a case below, and the assertion after the loop fails when one does not —
/// because the way a field silently stops being hashed is by being moved out of
/// the context struct, and a sampled list would never notice.
#[test]
fn rewriting_a_candidates_binding_or_presentation_is_detected_before_admission() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "candidate integrity");
    admit(&root, payload(Action::SectionAdded, &id, "old wording"));

    let cases: Vec<Tamper> = vec![
        (
            "expected_rev",
            Box::new(|value: &mut serde_json::Value| value["expected_rev"] = serde_json::json!(0)),
        ),
        (
            "previous_text",
            Box::new(|value: &mut serde_json::Value| {
                value["previous_text"] = serde_json::json!("wording that was never confirmed")
            }),
        ),
        (
            "previous_based_on",
            Box::new(|value: &mut serde_json::Value| {
                value["previous_based_on"] = serde_json::json!("0".repeat(40))
            }),
        ),
        (
            "previous_refs",
            Box::new(|value: &mut serde_json::Value| {
                value["previous_refs"] = serde_json::json!([{
                    "object": engr::model::new_id(),
                    "section": 1,
                    "sha256": "0".repeat(64),
                    "commit": "0".repeat(40),
                }])
            }),
        ),
        (
            "previous_semantics_recorded",
            Box::new(|value: &mut serde_json::Value| {
                value["previous_semantics_recorded"] = serde_json::json!(false)
            }),
        ),
        // The four below arrived with Phase 3 and Phase 4. Each is skipped when
        // empty, so tampering means introducing one that was never prepared —
        // which is the shape the risk actually takes: an exception nobody
        // granted, a supplementary body nobody was shown, a Backlog source
        // nobody named.
        (
            "previous_role",
            Box::new(|value: &mut serde_json::Value| {
                value["previous_role"] = serde_json::to_value(Role::Decision).expect("role")
            }),
        ),
        (
            "previous_content",
            Box::new(|value: &mut serde_json::Value| {
                value["previous_content"] =
                    serde_json::to_value(vec![Supplement::new("code.rs", "fn main() {}")])
                        .expect("content")
            }),
        ),
        (
            "previous_relations",
            Box::new(|value: &mut serde_json::Value| {
                value["previous_relations"] = serde_json::to_value(vec![Relation::superseded_by(
                    format!("obj:{}", "0".repeat(26)),
                )])
                .expect("relations")
            }),
        ),
        (
            "oversize",
            Box::new(|value: &mut serde_json::Value| value["oversize"] = serde_json::json!(true)),
        ),
        (
            "object_title",
            Box::new(|value: &mut serde_json::Value| {
                value["object_title"] = serde_json::json!("a record this is not")
            }),
        ),
    ];

    // Every field of `PreparedContext` is exercised above. The comparison is
    // against the struct's own serialized keys rather than a list written out
    // here, so adding a field to the prepared context without proving it is
    // hashed fails this test instead of passing quietly.
    let populated = gate::PreparedContext {
        previous_text: Some("wording".to_owned()),
        previous_based_on: Some("0".repeat(40)),
        previous_refs: vec![Ref::legacy(
            engr::model::new_id(),
            1,
            "0".repeat(64),
            "0".repeat(40),
        )],
        previous_role: Some(Role::Decision),
        previous_content: vec![Supplement::new("code.rs", "fn main() {}")],
        previous_relations: vec![Relation::superseded_by(format!("obj:{}", "0".repeat(26)))],
        previous_semantics_recorded: true,
        oversize: true,
        object_title: Some("the record being changed".to_owned()),
        rule_review: None,
    };
    let declared: BTreeSet<String> = serde_json::to_value(&populated)
        .expect("context")
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect();
    let exercised: BTreeSet<String> = cases
        .iter()
        .map(|(name, _)| (*name).to_owned())
        .filter(|name| name != "expected_rev")
        .collect();
    assert_eq!(
        declared, exercised,
        "a prepared-context field with no case here is a field nothing proves is hashed"
    );

    for (name, tamper) in cases {
        let prepared = gate::prepare(
            &root,
            payload(Action::SectionRevised { section: 1 }, &id, "new wording"),
        )
        .expect("prepare");
        let code = prepared.candidate.challenge.clone();
        let path = store::candidate_path(&root, &code).expect("path");

        // Untouched, it confirms — and renders — exactly as prepared.
        gate::find(&root, &code).expect("an untouched candidate loads");

        let mut stored: serde_json::Value = store::read_json(&path).expect("candidate");
        tamper(&mut stored);
        write_raw(&path, &stored).expect("rewrite candidate");

        let error = gate::find(&root, &code).expect_err(name);
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{name}");
        assert!(error.message.contains("integrity"), "{name}");
        let error = gate::confirm(&root, &format!("CONFIRM {code}")).expect_err(name);
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{name}");
        assert_eq!(
            ops::effective(&root, &id).expect("object").sections[0].text,
            "old wording",
            "{name}: a rewritten candidate must not be admitted"
        );
        gate::discard(&root, &code).expect("clear the tampered candidate");
    }
}

/// The upgrade refuses the old envelope rather than treating missing integrity
/// data as if it were protected. A live candidate is local and short-lived, so
/// the cost of re-preparing is a moment; the cost of the other choice is a
/// guarantee that only looks like one.
#[test]
fn a_candidate_envelope_without_integrity_is_refused_rather_than_trusted() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "envelope version");
    let prepared =
        gate::prepare(&root, payload(Action::SectionAdded, &id, "pending")).expect("prepare");
    let code = prepared.candidate.challenge.clone();
    let path = store::candidate_path(&root, &code).expect("path");
    let mut stored: serde_json::Value = store::read_json(&path).expect("candidate");
    let stored_object = stored.as_object_mut().expect("candidate object");
    stored_object.insert("version".to_owned(), serde_json::json!(1));
    stored_object.remove("integrity_sha256");
    write_raw(&path, &stored).expect("legacy candidate");

    let error =
        gate::confirm(&root, &format!("CONFIRM {code}")).expect_err("no integrity, no admission");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("integrity"), "{error}");
    assert_eq!(ops::effective(&root, &id).expect("object").rev, 1);
}

/// The code a candidate names has to be the code that admits it.
///
/// Two live candidates, and A's stored challenge rewritten to B's code. Every
/// other check still passes — both files are internally consistent — so without
/// binding the challenge, `engr candidate A` renders A's change and tells the
/// human to type `CONFIRM B`, and B's change is what enters the record. That is
/// the one property the whole design exists for, inverted.
#[test]
fn a_candidate_cannot_redirect_a_human_to_another_candidates_code() {
    let (_dir, root) = workspace();
    let first = new_object(&root, "the change they read");
    let second = new_object(&root, "the change they did not");
    let a = gate::prepare(&root, payload(Action::SectionAdded, &first, "wording A"))
        .expect("prepare A");
    let b = gate::prepare(&root, payload(Action::SectionAdded, &second, "wording B"))
        .expect("prepare B");
    let (a_code, b_code) = (a.candidate.challenge.clone(), b.candidate.challenge.clone());
    assert_ne!(a_code, b_code);

    let a_path = store::candidate_path(&root, &a_code).expect("path");
    let mut stored: serde_json::Value = store::read_json(&a_path).expect("candidate A");
    stored["challenge"] = serde_json::json!(b_code);
    write_raw(&a_path, &stored).expect("redirect A at B");

    // A cannot be rendered, so no screen can ever pair A's change with B's code.
    let error = gate::find(&root, &a_code).expect_err("a redirect is not a candidate");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(
        gate::confirm(&root, &format!("CONFIRM {a_code}")).is_err(),
        "and its own code admits nothing"
    );
    // Both files are still there to be found and cleaned up; it is loading the
    // rewritten one that fails, not noticing it.
    assert_eq!(
        gate::pending_codes(&root).expect("codes").len(),
        2,
        "the rewritten file is not hidden, it is refused"
    );
    gate::find(&root, &b_code).expect("B still loads");

    assert_eq!(
        ops::effective(&root, &first).expect("first").sections.len(),
        0,
        "neither mutation may have been admitted"
    );
    assert_eq!(
        ops::effective(&root, &second)
            .expect("second")
            .sections
            .len(),
        0
    );

    // B is untouched and still admits exactly what B says.
    let admitted = gate::confirm(&root, &format!("CONFIRM {b_code}")).expect("B is unaffected");
    assert_eq!(admitted.object.id, second);
    assert_eq!(admitted.object.sections[0].text, "wording B");
}

/// A candidate this build refuses is one file, not a broken workspace. It must
/// not take the listing down with it, must not stop anything else being
/// prepared, and must still be superseded when its own object is proposed
/// again — leaving it beside its replacement is what would hand one object two
/// live codes.
#[test]
fn a_refused_candidate_does_not_block_the_rest_of_the_workspace() {
    let (_dir, root) = workspace();
    let stranded = new_object(&root, "left by an older build");
    let unrelated = new_object(&root, "prepared afterwards");
    let prepared = gate::prepare(
        &root,
        payload(Action::SectionAdded, &stranded, "old envelope"),
    )
    .expect("prepare");
    let code = prepared.candidate.challenge.clone();
    let path = store::candidate_path(&root, &code).expect("path");
    let mut stored: serde_json::Value = store::read_json(&path).expect("candidate");
    let stored_object = stored.as_object_mut().expect("candidate object");
    stored_object.insert("version".to_owned(), serde_json::json!(1));
    stored_object.remove("integrity_sha256");
    write_raw(&path, &stored).expect("legacy candidate");

    // Preparing something else works, and does not reuse the stranded code.
    let other = gate::prepare(
        &root,
        payload(Action::SectionAdded, &unrelated, "unaffected"),
    )
    .expect("an unrelated proposal is unaffected");
    assert_ne!(other.candidate.challenge, code);
    assert!(other.superseded.is_empty());
    assert!(
        gate::confirm(&root, &format!("CONFIRM {}", other.candidate.challenge)).is_ok(),
        "and confirms normally"
    );

    // Proposing the stranded candidate's own object supersedes it.
    let replacement = gate::prepare(
        &root,
        payload(Action::SectionAdded, &stranded, "prepared again"),
    )
    .expect("prepare again");
    assert_eq!(replacement.superseded, vec![code.clone()]);
    assert!(store::candidate_path(&root, &code)
        .map(|path| !path.exists())
        .unwrap_or(false));
    gate::confirm(
        &root,
        &format!("CONFIRM {}", replacement.candidate.challenge),
    )
    .expect("the replacement admits");
}

/// The already-applied retry still has to work, and integrity is checked on the
/// way through it: cleanup after a crash is the one path where a candidate is
/// deliberately re-read after its event is durable.
#[test]
fn candidate_integrity_does_not_break_the_idempotent_cleanup_retry() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "integrity retry");
    let prepared =
        gate::prepare(&root, payload(Action::SectionAdded, &id, "apply once")).expect("prepare");
    let code = prepared.candidate.challenge.clone();
    gate::confirm(&root, &format!("CONFIRM {code}")).expect("apply");
    write_raw(
        &store::candidate_path(&root, &code).expect("path"),
        &prepared.candidate,
    )
    .expect("restore the candidate a crash would have left");

    let object = gate::confirm(&root, &format!("CONFIRM {code}"))
        .expect("the retry is idempotent")
        .object;
    assert_eq!(object.rev, 2);
    assert_eq!(store::load_events(&root, &id).expect("events").len(), 2);
    assert!(gate::find(&root, &code).is_err());
}

/// The field is unforgiving — no rename, and the mistake only shows after
/// confirmation — so a body pasted in here has to be refused at the gate.
#[test]
fn a_title_that_is_really_a_body_is_refused() {
    let (_dir, root) = workspace();

    let body = "The audit failure reason code is not queryable. ".repeat(12);
    assert!(body.chars().count() > 120);
    let error = gate::prepare(
        &root,
        payload(Action::ObjectCreated, &engr::model::new_id(), &body),
    )
    .expect_err("a 500-character title is refused");
    assert_eq!(error.code, engr::EXIT_USAGE);
    assert!(
        error.message.contains("title") && error.message.contains("--add"),
        "the message has to teach what the field is and where the detail goes: {:?}",
        error.message
    );

    let error = gate::prepare(
        &root,
        payload(
            Action::ObjectCreated,
            &engr::model::new_id(),
            "a title\nwith a second line",
        ),
    )
    .expect_err("a title cannot span lines");
    assert_eq!(error.code, engr::EXIT_USAGE);

    gate::prepare(
        &root,
        payload(
            Action::ObjectCreated,
            &engr::model::new_id(),
            "audit failure reason codes",
        ),
    )
    .expect("an ordinary title is admitted");
}

/// Not blocked: two objects may legitimately share a title. But they cannot be
/// told apart in `ls`, and the moment to reconsider is while the human is
/// still holding the code.
#[test]
fn a_duplicate_title_is_flagged_but_not_blocked() {
    let (_dir, root) = workspace();
    let first = new_object(&root, "audit failure reason codes");

    let prepared = gate::prepare(
        &root,
        payload(
            Action::ObjectCreated,
            &engr::model::new_id(),
            "  Audit Failure Reason Codes  ",
        ),
    )
    .expect("a duplicate title is admitted");
    assert_eq!(prepared.notes.len(), 1, "trimmed and case-folded match");
    let gate::Note::DuplicateTitle { object } = &prepared.notes[0];
    assert_eq!(object, &first);

    let prepared = gate::prepare(
        &root,
        payload(
            Action::ObjectCreated,
            &engr::model::new_id(),
            "something else entirely",
        ),
    )
    .expect("prepare");
    assert!(prepared.notes.is_empty());
}

/// A title written correctly in January can be wrong by June without anyone
/// having touched it. One confirmation changes it; nothing else does.
#[test]
fn a_title_changes_through_one_confirmation() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "audit failure reason codes");

    let prepared = gate::prepare(
        &root,
        payload(Action::ObjectRenamed, &id, "audit failure reason codes v2"),
    )
    .expect("prepare");
    assert_eq!(
        prepared.candidate.context.previous_text.as_deref(),
        Some("audit failure reason codes"),
        "a rename shows the change, so the old title has to travel with it"
    );

    let object = gate::confirm(&root, &format!("CONFIRM {}", prepared.candidate.challenge))
        .expect("confirm")
        .object;
    assert_eq!(object.title, "audit failure reason codes v2");
    assert_eq!(object.rev, 2, "a rename is an action like any other");

    // It replays from the log, not just from the projection sitting on disk.
    let replayed = ops::reconcile(&root, &id).expect("reconcile");
    assert_eq!(replayed.title, "audit failure reason codes v2");
}

/// The guard on `--new` would be worth nothing if `--rename` were the way past
/// it, and the refusal has to name the flag that was actually typed.
#[test]
fn a_rename_is_held_to_the_same_shape_as_a_title() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "short enough");

    let body = "The audit failure reason code is not queryable. ".repeat(12);
    let error =
        gate::prepare(&root, payload(Action::ObjectRenamed, &id, &body)).expect_err("too long");
    assert_eq!(error.code, engr::EXIT_USAGE);
    assert!(
        error.message.contains("--rename") && !error.message.contains("--new"),
        "the refusal has to name the flag that was typed: {:?}",
        error.message
    );

    let error = gate::prepare(&root, payload(Action::ObjectRenamed, &id, "two\nlines"))
        .expect_err("a title cannot span lines");
    assert_eq!(error.code, engr::EXIT_USAGE);
}

/// Renaming onto a title someone else already holds is worth saying; renaming
/// an object onto its own title is not, and a note that fires on a non-problem
/// is how people learn to skip the notes.
#[test]
fn a_rename_reports_a_clash_with_another_object_but_not_with_itself() {
    let (_dir, root) = workspace();
    let first = new_object(&root, "audit failure reason codes");
    let second = new_object(&root, "retry policy");

    let prepared = gate::prepare(
        &root,
        payload(
            Action::ObjectRenamed,
            &second,
            "  Audit Failure Reason Codes  ",
        ),
    )
    .expect("a duplicate title is admitted, not refused");
    assert_eq!(prepared.notes.len(), 1);
    let gate::Note::DuplicateTitle { object } = &prepared.notes[0];
    assert_eq!(object, &first);
    // Stored as it will be listed. The duplicate check above already ignores the
    // padding, and a listing that prints what that check ignores puts one row
    // out of column underneath a note saying the two titles match.
    assert_eq!(
        prepared.candidate.payload.content.text,
        "Audit Failure Reason Codes"
    );

    let prepared = gate::prepare(
        &root,
        payload(Action::ObjectRenamed, &second, "Retry Policy"),
    )
    .expect("prepare");
    assert!(
        prepared.notes.is_empty(),
        "an object already holding this title is not a clash with itself"
    );
}

/// Closed has to mean the whole object settled, not just its sections.
#[test]
fn a_closed_object_refuses_a_rename() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "settled");
    admit(&root, empty(Action::ObjectClosed, &id));

    let error = gate::prepare(&root, payload(Action::ObjectRenamed, &id, "unsettled"))
        .expect_err("a closed object refuses a rename");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(
        error.message.contains("reopen"),
        "the refusal has to say the way through: {:?}",
        error.message
    );

    admit(&root, empty(Action::ObjectReopened, &id));
    let object = admit(&root, payload(Action::ObjectRenamed, &id, "unsettled"));
    assert_eq!(object.title, "unsettled");
}

#[test]
fn re_confirming_after_a_crash_does_not_apply_twice() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "recovery");
    let prepared =
        gate::prepare(&root, payload(Action::SectionAdded, &id, "once")).expect("prepare");
    let code = prepared.candidate.challenge.clone();
    let response = format!("CONFIRM {code}");
    gate::confirm(&root, &response).expect("confirm");

    // Reinstate the candidate to stand in for a crash between saving the
    // projection and clearing it.
    write_raw(
        &store::candidate_path(&root, &code).expect("candidate path"),
        &prepared.candidate,
    )
    .expect("rewrite");

    let object = gate::confirm(&root, &response)
        .expect("recovery is idempotent")
        .object;
    assert_eq!(object.rev, 2, "the event must not be applied a second time");
    assert_eq!(object.sections.len(), 1);
    assert!(gate::find(&root, &code).is_err());
}

#[test]
fn preparing_after_an_unprojected_event_keeps_the_confirmed_change() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "interrupted confirmation");
    let first = gate::prepare(&root, payload(Action::SectionAdded, &id, "first")).expect("first");
    let event = candidate_event(&first.candidate);
    append_admitted_raw(&root, &event);

    // A later action must first replay the confirmed tail. Otherwise it is
    // prepared at the same revision, its event collides with this one, and
    // purging can erase the first confirmed section.
    let second =
        gate::prepare(&root, payload(Action::SectionAdded, &id, "second")).expect("second");
    assert_eq!(second.candidate.binding.expected_rev, 2);
    let object = gate::confirm(&root, &format!("CONFIRM {}", second.candidate.challenge))
        .expect("confirm second")
        .object;
    assert_eq!(object.rev, 3);
    let text: Vec<_> = object
        .sections
        .iter()
        .map(|section| section.text.as_str())
        .collect();
    assert_eq!(text, vec!["first", "second"]);
}

#[test]
fn re_confirming_after_append_before_projection_does_not_duplicate_the_event() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "interrupted confirmation");
    let prepared =
        gate::prepare(&root, payload(Action::SectionAdded, &id, "once")).expect("prepare");
    let code = prepared.candidate.challenge.clone();
    let event = candidate_event(&prepared.candidate);
    append_admitted_raw(&root, &event);

    let object = gate::confirm(&root, &format!("CONFIRM {code}"))
        .expect("the retry is idempotent")
        .object;
    assert_eq!(object.rev, 2);
    assert_eq!(object.sections[0].text, "once");
    assert_eq!(
        store::load_events(&root, &id).expect("events").len(),
        2,
        "retrying must not append a second rev 2 event"
    );
    assert!(gate::find(&root, &code).is_err());
}

#[test]
fn re_confirming_after_append_before_projection_recovers_an_object_creation() {
    let (_dir, root) = workspace();
    let id = engr::model::new_id();
    let prepared =
        gate::prepare(&root, payload(Action::ObjectCreated, &id, "created once")).expect("prepare");
    let event = candidate_event(&prepared.candidate);
    append_admitted_raw(&root, &event);

    let code = prepared.candidate.challenge.clone();
    let object = gate::confirm(&root, &format!("CONFIRM {code}"))
        .expect("the retry is idempotent")
        .object;
    assert_eq!(object.rev, 1);
    assert_eq!(object.title, "created once");
    assert_eq!(
        store::load_events(&root, &id).expect("events").len(),
        1,
        "retrying must not append a second rev 1 event"
    );
    assert!(gate::find(&root, &code).is_err());
}

/// `git check-ignore -q`: 0 ignored, 1 not, anything else is a broken invocation
/// and must not be read as "not ignored".
fn ignored(root: &Path, relative: &str) -> bool {
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["check-ignore", "-q", "--no-index", relative])
        .status()
        .expect("git check-ignore");
    match status.code() {
        Some(0) => true,
        Some(1) => false,
        other => panic!("git check-ignore {relative}: {other:?}"),
    }
}

/// `git add -A` is how a workspace gets staged, and a candidate's filename *is* a
/// live challenge code. If one can reach the repository by accident then the code
/// no longer travels to a single human, and the gate stops meaning anything.
///
/// The negative half matters as much: an over-broad rule here would silently stop
/// the record from ever being committed, and look-back lives in git.
#[test]
fn a_live_challenge_code_is_kept_out_of_git() {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().to_path_buf();
    std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["init", "-q", "."])
        .status()
        .expect("git init");
    store::init(&root).expect("init");

    let id = engr::model::new_id();
    let prepared =
        gate::prepare(&root, payload(Action::ObjectCreated, &id, "a title")).expect("prepare");
    let code = &prepared.candidate.challenge.clone();

    assert!(ignored(&root, ".engr/lock"));
    assert!(ignored(&root, &format!(".engr/candidates/{code}.json")));

    assert!(!ignored(&root, ".engr/format.json"));
    assert!(!ignored(&root, &format!(".engr/objects/{id}.json")));
    assert!(!ignored(&root, &format!(".engr/events/{id}.jsonl")));
}

/// A candidate prepared before Backlog left the gate cannot be admitted.
///
/// Admission and Backlog bookkeeping became two operations, and the declared
/// Backlog material left `PreparedContext` — which the candidate's integrity
/// hash covered. So a candidate that was outstanding across that change names an
/// integrity value the current build cannot reproduce.
///
/// What matters is which way it fails. Unknown fields are ignored on read, so
/// the danger would be recomputing a hash that happens to match and admitting a
/// candidate whose prepared context is not the one being checked. It does not:
/// the stored value was taken over the wider context and no longer agrees, so
/// the candidate is refused, told to be prepared again, and never rendered as
/// though it were current.
#[test]
fn a_candidate_that_still_declares_backlog_material_is_refused_not_reinterpreted() {
    let (_dir, root) = workspace();
    let prepared = gate::prepare(
        &root,
        payload(Action::ObjectCreated, &engr::model::new_id(), "a record"),
    )
    .expect("prepare");
    let challenge = prepared.candidate.challenge.clone();
    let path = store::candidate_path(&root, &challenge).expect("path");

    // Reconstruct what the earlier build wrote: the same candidate, with the
    // declared Backlog material still in its prepared context, and an integrity
    // value taken over that wider context.
    let mut stored: serde_json::Value = store::read_json(&path).expect("candidate");
    let backlog = serde_json::json!([{ "item": "0195", "section": 1 }]);
    let object = stored.as_object_mut().expect("object");
    object.insert("backlog".to_owned(), backlog);
    object.insert("version".to_owned(), serde_json::json!(2));
    write_raw(&path, &stored).expect("rewrite as the earlier build");

    let error = gate::find(&root, &challenge).expect_err("prepared under a different contract");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(
        error.message.contains("did not survive being read"),
        "the refusal says what to do about it: {}",
        error.message
    );

    // And it is refused wherever a candidate is loaded, not only at confirm —
    // rendering it would present a prepared context nobody can check.
    assert!(gate::pending(&root).is_err() || gate::pending(&root).expect("pending").is_empty());
    let response = format!("CONFIRM {challenge}");
    assert!(gate::confirm(&root, &response).is_err());
}

/// The durable write boundary must enforce each envelope generation, not only
/// the ordinary admission path. `prepare` refusing is not enough: a candidate
/// is a file, and `confirm` loads one that is already on disk.
///
/// Without this, `append_event` accepts a version 1 record carrying the merge
/// shape version 1 never defined — and `load_events` then refuses that same
/// record, so the supported confirmation path writes history its own next read
/// rejects.
#[test]
fn the_event_write_boundary_refuses_a_shape_its_generation_never_defined() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "write boundary");
    admit(&root, payload(Action::SectionAdded, &id, "one"));
    admit(&root, payload(Action::SectionAdded, &id, "two"));

    let merged = payload(
        Action::SectionMerged {
            merge: Merge::Into {
                destination: 1,
                sources: vec![2],
            },
        },
        &id,
        "together",
    );
    let event = engr::model::Event {
        format: engr::model::EVENT_FORMAT.to_owned(),
        version: engr::EVENT_ENVELOPE_VERSION_V0,
        event_id: engr::model::new_id(),
        rev: 4,
        time: "2026-08-23T00:00:00Z".to_owned(),
        provenance: Provenance::confirmed("TEST00".to_owned(), merged.sha256().expect("hash")),
        payload: merged,
    };

    let error = store::check_appendable(&root, &event)
        .expect_err("a generation may only carry the shapes it defined");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    store::load_events(&root, &id).expect("and the history is still readable");
}

/// There is no public writer for a persisted Object, and every read says so.
///
/// The library used to expose one that validated: a caller could hand it an
/// arbitrary current-shape Object and, so long as the seals were freshly
/// consistent, have it published. Locking that call closed a race, not the
/// authority boundary — nothing about a self-consistent resealed projection
/// says any Event, Human Gate or Rule Review produced it. So the primitive is
/// gone from the public surface, and what remains for a direct caller is
/// writing bytes from outside, which every trust surface then reports.
#[test]
fn there_is_no_public_write_boundary_that_could_promote_authority() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "authority boundary");
    admit(&root, payload(Action::SectionAdded, &id, "wording"));

    let mut object = store::load_object(&root, &id).expect("object");
    object.sections[0].admission = engr::semantics::Admission::Agent;
    save_raw(&root, &object).expect("bytes can always be put on disk from outside");

    let report = engr::ops::verify(&root, &id).expect("verify");
    assert!(
        !report.passed(),
        "a change nothing admitted is a broken record"
    );
    assert!(report.object_tampered || !report.tampered.is_empty());
    assert!(
        engr::integrity::check_stored_object_integrity(
            &store::load_object(&root, &id).expect("still loads")
        )
        .is_err(),
        "the seal admitted at the gate does not cover this"
    );
}

/// The generation guard has to guard the generation, not only the shapes it
/// defines. An ordinary payload carrying a version this build does not support is
/// the same self-corrupting write one level up: `append_event` writes it and
/// `load_events` refuses it.
#[test]
fn the_event_write_boundary_refuses_an_unsupported_generation() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "generation boundary");
    admit(&root, payload(Action::SectionAdded, &id, "one"));
    let before = std::fs::read(store::events_path(&root, &id)).expect("events");

    let ordinary = payload(Action::SectionAdded, &id, "an ordinary payload");
    let event = engr::model::Event {
        format: engr::model::EVENT_FORMAT.to_owned(),
        version: engr::EVENT_ENVELOPE_VERSION + 1,
        event_id: engr::model::new_id(),
        rev: 3,
        time: "2026-08-23T00:00:00Z".to_owned(),
        provenance: Provenance::confirmed("TEST00".to_owned(), ordinary.sha256().expect("hash")),
        payload: ordinary,
    };

    let error = store::check_appendable(&root, &event)
        .expect_err("this build does not support that generation");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert_eq!(
        std::fs::read(store::events_path(&root, &id)).expect("events"),
        before,
        "nothing was written"
    );
    store::load_events(&root, &id).expect("and the history is still readable");
}

/// The read-side counterpart of the Object write boundary. Writes must not drop
/// P3-only authority state; reads must not silently reinterpret it.
///
/// A file carrying `admission` is not a version 2 file. Reconstructing `human`
/// from it is only exact for the *exact* version 2 representation — for a file
/// that already carries a field version 2 never defined, it answers a question
/// the file was trying to answer differently.
#[test]
fn a_v2_object_carrying_p3_only_fields_fails_closed() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "read boundary");
    admit(&root, payload(Action::SectionAdded, &id, "wording"));
    let path = store::object_path(&root, &id);
    let current = store::load_object(&root, &id).expect("current object");
    let mut predecessor: serde_json::Value =
        serde_json::to_value(&current).expect("predecessor object");
    predecessor
        .as_object_mut()
        .expect("object")
        .remove("sha256");
    let section = predecessor["sections"][0].as_object_mut().expect("section");
    section.remove("admission");
    let admitted_at = section.remove("admitted_at").expect("admitted_at");
    section.insert("confirmed_at".to_owned(), admitted_at);
    section.insert(
        "sha256".to_owned(),
        serde_json::Value::String(
            current.sections[0]
                .recomputed_sha256()
                .expect("legacy Section seal"),
        ),
    );
    std::fs::write(
        store::engr_dir(&root).join("format.json"),
        r#"{"format":"engr-workspace","version":2}"#,
    )
    .expect("v2 authority");

    for injected in ["admission", "admitted_at"] {
        let mut value = predecessor.clone();
        value["sections"][0]
            .as_object_mut()
            .expect("section")
            .insert(
                injected.to_owned(),
                serde_json::Value::String("agent".to_owned()),
            );
        std::fs::write(&path, serde_json::to_vec_pretty(&value).expect("json")).expect("write");

        let error = store::load_object(&root, &id)
            .expect_err("a field this version never defined is not this version's file");
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{injected}");
    }
}

/// The same rule for a snapshot, decoded under the version the snapshot itself
/// records. Historical reads go through the same struct, so the guarantee has to
/// hold there or a reference could pin a file nothing would accept today.
#[test]
fn a_historical_snapshot_carrying_p3_only_fields_fails_closed() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "historical read boundary");
    admit(&root, payload(Action::SectionAdded, &id, "wording"));
    let path = store::object_path(&root, &id);

    let current = store::load_object(&root, &id).expect("current object");
    let mut value = serde_json::to_value(&current).expect("object");
    value.as_object_mut().expect("object").remove("sha256");
    let section = value["sections"][0].as_object_mut().expect("section");
    section.remove("admission");
    let admitted_at = section.remove("admitted_at").expect("admitted_at");
    section.insert("confirmed_at".to_owned(), admitted_at);
    section.insert(
        "sha256".to_owned(),
        serde_json::Value::String(
            current.sections[0]
                .recomputed_sha256()
                .expect("legacy Section seal"),
        ),
    );
    value["sections"][0]
        .as_object_mut()
        .expect("section")
        .insert(
            "admission".to_owned(),
            serde_json::Value::String("agent".to_owned()),
        );
    std::fs::write(&path, serde_json::to_vec_pretty(&value).expect("json")).expect("write");
    std::fs::write(
        store::engr_dir(&root).join("format.json"),
        r#"{"format":"engr-workspace","version":2}"#,
    )
    .expect("v2 authority");
    let commit = commit_all(&root, "a snapshot claiming more than its version defines");

    let error = engr::git::object_at(&root, &commit, &id)
        .expect_err("a snapshot is read under its own version, and refused when it disagrees");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
}

/// The EventStore analogue of the Object read boundary. A record carrying
/// admission provenance is not a record of the generation that had only one
/// door, and the check has to happen before a typed decode can drop the field
/// that says so.
///
/// Without this, the dropped field leaves the stored `payload_sha256` verifying
/// — it was never inside the payload — and replay reaches `human` for bytes that
/// explicitly claimed `agent`. Reconciliation can then make that authoritative.
#[test]
fn a_retained_event_carrying_admission_provenance_is_refused() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "event read boundary");
    admit(&root, payload(Action::SectionAdded, &id, "wording"));
    let path = store::events_path(&root, &id);
    let good = std::fs::read_to_string(&path).expect("events");

    let mut lines = Vec::new();
    for line in good.lines() {
        let mut event: serde_json::Value = serde_json::from_str(line).expect("event");
        event.as_object_mut().expect("event").insert(
            "admission".to_owned(),
            serde_json::json!({ "kind": "agent" }),
        );
        lines.push(serde_json::to_string(&event).expect("event"));
    }
    std::fs::write(&path, format!("{}\n", lines.join("\n"))).expect("write");

    let error = store::load_events(&root, &id)
        .expect_err("a record claiming an admission this generation never had");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
}

/// The durable Event write path is part of the workspace-generation boundary,
/// and a direct library caller reaches it without going through the gate.
#[test]
fn appending_an_event_requires_a_workspace_this_build_may_write() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "append boundary");
    let path = store::events_path(&root, &id);
    let before = std::fs::read(&path).expect("events");

    let format = store::engr_dir(&root).join("format.json");
    std::fs::write(&format, r#"{"format":"engr-workspace","version":99}"#).expect("format");

    let added = payload(Action::SectionAdded, &id, "wording");
    let event = engr::model::Event {
        format: engr::model::EVENT_FORMAT.to_owned(),
        version: engr::EVENT_ENVELOPE_VERSION_V0,
        event_id: engr::model::new_id(),
        rev: 2,
        time: "2026-08-23T00:00:00Z".to_owned(),
        provenance: Provenance::confirmed("TEST00".to_owned(), added.sha256().expect("hash")),
        payload: added,
    };

    let error = store::check_appendable(&root, &event)
        .expect_err("this build does not write a workspace at that version");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert_eq!(
        std::fs::read(&path).expect("events"),
        before,
        "and the store is byte-for-byte what it was"
    );
}

/// A historical record spells an absent basis `"based_on": null`, because that
/// is what engr emitted before no-basis became an absent field. The generation
/// guard must tell an unknown field apart from a spelling this generation once
/// wrote — history is read under its own contract, and the current serializer's
/// one spelling is not that whole contract.
#[test]
fn current_events_reject_a_noncanonical_explicit_absent_basis() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "legacy spelling");
    admit(&root, payload(Action::SectionAdded, &id, "wording"));
    let path = store::events_path(&root, &id);

    let mut lines = Vec::new();
    for line in std::fs::read_to_string(&path).expect("events").lines() {
        let mut event: serde_json::Value = serde_json::from_str(line).expect("event");
        let object = event.as_object_mut().expect("event");
        assert!(
            !object.contains_key("based_on"),
            "this build omits it, which is why the older spelling has to be tolerated"
        );
        object.insert("based_on".to_owned(), serde_json::Value::Null);
        lines.push(serde_json::to_string(&event).expect("event"));
    }
    std::fs::write(&path, format!("{}\n", lines.join("\n"))).expect("write");

    let error = store::load_events(&root, &id).expect_err("Event v2 has one exact shape");
    assert!(error.message.contains("exact canonical shape"));
}

/// The durable Event write path must not accept a record its own next read
/// refuses. The generation is only part of that contract: the rest of what
/// `load_events` demands has to be demanded here too, or a direct library caller
/// writes history nothing can load.
#[test]
fn appending_an_event_enforces_the_contract_its_own_read_applies() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "append contract");
    let path = store::events_path(&root, &id);
    let before = std::fs::read(&path).expect("events");

    let added = payload(Action::SectionAdded, &id, "wording");
    let sound = admissible_human_event(&root, added);

    let mut wrong_format = sound.clone();
    wrong_format.format = "not-an-engr-event".to_owned();
    let mut wrong_confirmation = sound.clone();
    if let Provenance::Tagged { admission } = &mut wrong_confirmation.provenance {
        admission.confirmation = None;
    }
    let mut wrong_rev = sound.clone();
    wrong_rev.rev = 9;
    let mut wrong_object = sound.clone();
    wrong_object.payload.object = engr::model::new_id();

    for (what, event) in [
        ("format", wrong_format),
        ("human confirmation", wrong_confirmation),
        ("revision continuity", wrong_rev),
        ("object identity", wrong_object),
    ] {
        let error = store::check_appendable(&root, &event).unwrap_err_or_else_note(what);
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{what}");
        assert_eq!(
            std::fs::read(&path).expect("events"),
            before,
            "{what}: nothing was written"
        );
    }

    store::check_appendable(&root, &sound).expect("a sound record is still admissible");
    store::load_events(&root, &id).expect("and reads back");
}

trait NoteErr {
    fn unwrap_err_or_else_note(self, what: &str) -> engr::Error;
}

impl NoteErr for engr::Result<()> {
    fn unwrap_err_or_else_note(self, what: &str) -> engr::Error {
        match self {
            Ok(()) => panic!("{what}: append accepted a record load_events refuses"),
            Err(error) => error,
        }
    }
}

/// Two callers, one predecessor, one winner.
///
/// The durable append reads the tail to refuse a revision the next load would
/// reject, so a read-then-append with nothing held between them is both writers
/// agreeing on the same predecessor and both taking it. `confirm` holds the
/// writer lock across the whole operation, which is what makes that impossible
/// — and since the raw append is no longer reachable from outside the crate,
/// the gate is where the property is observable and where it matters.
#[test]
fn two_callers_cannot_both_admit_the_same_predecessor() {
    let (dir, root) = workspace();
    let id = new_object(&root, "one predecessor");
    let prepared = gate::prepare(&root, payload(Action::SectionAdded, &id, "wording"))
        .expect("one prepared candidate");
    let response = format!("CONFIRM {}", prepared.candidate.challenge);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let outcomes: Vec<bool> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let barrier = std::sync::Arc::clone(&barrier);
                let root = root.clone();
                let response = response.clone();
                scope.spawn(move || {
                    barrier.wait();
                    gate::confirm(&root, &response).is_ok()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("thread"))
            .collect()
    });

    assert_eq!(
        outcomes.iter().filter(|ok| **ok).count(),
        1,
        "exactly one of two callers may take a predecessor"
    );
    let events = store::load_events(&root, &id).expect("and the history still loads");
    assert_eq!(events.len(), 2, "one admission, not two");
    assert_eq!(events[1].rev, 2);
    let object = ops::effective(&root, &id).expect("object");
    assert_eq!(object.rev, 2);
    assert_eq!(object.sections.len(), 1, "the wording landed exactly once");
    drop(dir);
}

/// A record can be perfectly well formed, contiguous, and still not something
/// this history can arrive at. `.engr/events/<id>.jsonl` is append-only and is
/// never purged, so a record that cannot be replayed is not a mistake somebody
/// can take back — it durably poisons every read that reconstructs the Object.
#[test]
fn the_append_path_refuses_a_record_the_reducer_could_not_replay() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "replayability");
    let path = store::events_path(&root, &id);
    let before = std::fs::read(&path).expect("events");

    // Structurally valid, contiguous, correct payload hash — and it revises a
    // Section that does not exist.
    let absent = payload(Action::SectionRevised { section: 999 }, &id, "wording");
    let event = direct_human_event(&root, &id, absent, 2);

    let error = store::check_appendable(&root, &event)
        .expect_err("history must be able to arrive at what it records");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert_eq!(
        std::fs::read(&path).expect("events"),
        before,
        "and nothing was written"
    );
    engr::ops::effective(&root, &id).expect("the read surface is unpoisoned");
}

/// The first record of a history has to be one a missing Object can be
/// reconstructed from. Continuity says nothing here — there is no predecessor to
/// be contiguous with — so without this an empty history accepts a beginning it
/// can never replay.
#[test]
fn the_append_path_refuses_a_first_record_no_object_could_come_from() {
    let (_dir, root) = workspace();
    let id = engr::model::new_id();
    let path = store::events_path(&root, &id);

    let added = payload(Action::SectionAdded, &id, "wording");
    let not_a_beginning = engr::model::Event {
        format: engr::model::EVENT_FORMAT.to_owned(),
        version: engr::EVENT_ENVELOPE_VERSION,
        event_id: engr::model::new_id(),
        rev: 1,
        time: "2026-08-23T00:00:00Z".to_owned(),
        provenance: Provenance::Tagged {
            admission: TaggedAdmission {
                kind: engr::semantics::Admission::Human,
                confirmation: Some(HumanConfirmation {
                    challenge: "TEST00".to_owned(),
                    candidate_digest: format!("1:{}", "0".repeat(64)),
                }),
                rule_review: None,
            },
        },
        payload: added,
    };
    let mut skipping = not_a_beginning.clone();
    skipping.rev = 2;

    for (what, event) in [
        ("an action no object begins with", not_a_beginning),
        ("a revision nothing precedes", skipping),
    ] {
        let error = store::check_appendable(&root, &event)
            .expect_err("a history must be able to start where it says it starts");
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{what}");
        assert!(!path.exists(), "{what}: no history was created");
    }
}

/// Retained Event v1 had only confirmation provenance. A tagged admission under
/// that generation would retroactively change what its envelope means.
#[test]
fn retained_event_generation_cannot_carry_tagged_admission() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "provenance boundary");
    let path = store::events_path(&root, &id);
    let before = std::fs::read(&path).expect("events");

    let added = payload(Action::SectionAdded, &id, "wording");
    let tagged = engr::model::Event {
        format: engr::model::EVENT_FORMAT.to_owned(),
        version: engr::EVENT_ENVELOPE_VERSION_V0,
        event_id: engr::model::new_id(),
        rev: 2,
        time: "2026-08-23T00:00:00Z".to_owned(),
        provenance: Provenance::Tagged {
            admission: engr::model::TaggedAdmission {
                kind: engr::semantics::Admission::Agent,
                confirmation: None,
                rule_review: None,
            },
        },
        payload: added,
    };

    let error = store::check_appendable(&root, &tagged)
        .expect_err("that provenance belongs to Event generation 2");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert_eq!(std::fs::read(&path).expect("events"), before);

    // And refused on the way back in, because a record can arrive by other means.
    let line = serde_json::to_string(&tagged).expect("event");
    let mut history = String::from_utf8(before).expect("utf8");
    history.push_str(&line);
    history.push('\n');
    std::fs::write(&path, history).expect("write");
    let error = store::load_events(&root, &id).expect_err("nor read under this generation");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
}

/// A well-formed Event is not an admitted one.
///
/// Every shape check can pass on a record a caller assembled: the schema is
/// exact, the revision follows, the reducer can replay it, and the provenance
/// scalars are syntactically perfect. None of that says a person was shown
/// anything or that any Rule was read. Without a proof at this boundary, a
/// direct library caller appends the record and lets recovery project it into
/// current authority — which is the whole admission model, bypassed by the one
/// door that writes durable history.
#[test]
fn a_direct_caller_cannot_append_a_human_event_nobody_was_shown() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "forged human provenance");
    let path = store::events_path(&root, &id);
    let before = std::fs::read(&path).expect("events");

    // Prepared for real, then the challenge swapped for one that was never
    // minted. Everything else about the record is exactly right.
    let admissible = admissible_human_event(&root, payload(Action::SectionAdded, &id, "wording"));
    let mut forged = admissible.clone();
    if let Provenance::Tagged { admission } = &mut forged.provenance {
        admission.confirmation = Some(HumanConfirmation {
            challenge: "ZZZZZZ".to_owned(),
            candidate_digest: admissible
                .human_confirmation()
                .expect("confirmation")
                .candidate_digest
                .clone(),
        });
    }
    let error = store::check_appendable(&root, &forged)
        .expect_err("a challenge nobody minted admits nothing");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(
        error.message.contains("not admitted through the gate"),
        "{error}"
    );

    // And the digest, against the real challenge: the pair is what the Event
    // persists, so neither half stands on its own.
    let mut swapped = admissible.clone();
    if let Provenance::Tagged { admission } = &mut swapped.provenance {
        admission.confirmation = Some(HumanConfirmation {
            challenge: admissible
                .human_confirmation()
                .expect("confirmation")
                .challenge
                .clone(),
            candidate_digest: format!("1:{}", "b".repeat(64)),
        });
    }
    let error = store::check_appendable(&root, &swapped)
        .expect_err("a candidate digest nothing produced admits nothing");
    assert_eq!(error.code, engr::EXIT_INVARIANT);

    assert_eq!(
        std::fs::read(&path).expect("events"),
        before,
        "neither forgery was written"
    );
    store::check_appendable(&root, &admissible).expect("the prepared record is still admissible");
}

/// The same, for the Agent door.
///
/// `Provenance::validate` reads a ReviewDigest for spelling. It cannot tell an
/// attestation made against a Rule set somebody read from sixty-four invented
/// hex characters, so the digest is recomputed here against the live applicable
/// Rules for exactly this mutation.
#[test]
fn a_direct_caller_cannot_append_an_agent_event_no_rule_review_produced() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "forged agent provenance");
    let path = store::events_path(&root, &id);
    let before = std::fs::read(&path).expect("events");

    let forged = engr::model::Event {
        format: engr::model::EVENT_FORMAT.to_owned(),
        version: engr::EVENT_ENVELOPE_VERSION,
        event_id: engr::model::new_id(),
        rev: 2,
        time: "2026-08-25T00:00:00Z".to_owned(),
        payload: payload(Action::SectionAdded, &id, "admitted by nobody"),
        provenance: Provenance::Tagged {
            admission: TaggedAdmission {
                kind: engr::semantics::Admission::Agent,
                confirmation: None,
                rule_review: Some(engr::model::ReviewProvenance {
                    outcome: engr::model::ReviewOutcome::Passed,
                    review_digest: format!("1:{}", "c".repeat(64)),
                }),
            },
        },
    };
    let error = store::check_appendable(&root, &forged)
        .expect_err("an invented review digest admits nothing");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(
        error.message.contains("Rule Review that is not the one"),
        "{error}"
    );

    // And with no review at all: semantic Agent admission needs an applicable
    // usable Object Rule, and a title is the sole exception.
    let mut bare = forged.clone();
    if let Provenance::Tagged { admission } = &mut bare.provenance {
        admission.rule_review = None;
    }
    let error = store::check_appendable(&root, &bare)
        .expect_err("no Rule Review, no semantic Agent admission");
    assert_eq!(error.code, engr::EXIT_SCHEMA);

    assert_eq!(
        std::fs::read(&path).expect("events"),
        before,
        "neither forgery was written"
    );
}

/// Put JSON on disk without going through any write path, the way a hand edit,
/// a git merge or another tool would.
///
/// The library has no public writer for a persisted resource, and that is the
/// point being relied on here: these fixtures are simulating bytes that arrived
/// from outside, so they write bytes from outside.
fn write_raw<T: serde::Serialize>(path: &std::path::Path, value: &T) -> engr::Result<()> {
    let text = engr::proof::canonical_bytes(value, "test fixture")?;
    std::fs::write(path, text).map_err(|error| engr::tool_error(path.display(), error))
}

/// Put an Object on disk directly, for a fixture that needs one there.
fn save_raw(root: &std::path::Path, object: &engr::model::Object) -> engr::Result<()> {
    write_raw(&engr::store::object_path(root, &object.id), object)
}

/// A persisted Event-v2 record *is* its canonical bytes, not merely a value
/// that parses to the right thing.
///
/// Comparing parsed values has already lost the answer: member order,
/// insignificant whitespace and any duplicate member name the parser collapsed
/// are gone before the comparison happens. An EventStore arrives through a git
/// merge, a hand edit or a copy as readily as through an append, so this is a
/// read-boundary rule and not a property of the writer.
#[test]
fn an_event_record_that_is_not_its_canonical_bytes_is_refused() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "canonical records");
    let path = store::events_path(&root, &id);
    let original = std::fs::read_to_string(&path).expect("events");
    let line = original.lines().next().expect("one record").to_owned();
    let parsed: serde_json::Value = serde_json::from_str(&line).expect("record");

    let members: Vec<String> = parsed
        .as_object()
        .expect("a record is a JSON object")
        .iter()
        .rev()
        .map(|(key, value)| {
            format!(
                "{}:{}",
                serde_json::to_string(key).expect("key"),
                serde_json::to_string(value).expect("value")
            )
        })
        .collect();
    for (what, rewritten) in [
        ("reordered members", format!("{{{}}}", members.join(","))),
        (
            "insignificant whitespace",
            serde_json::to_string_pretty(&parsed)
                .expect("pretty")
                .replace('\n', " "),
        ),
        ("a duplicate member", line.replacen('{', r#"{"rev":1,"#, 1)),
    ] {
        std::fs::write(&path, format!("{rewritten}\n")).expect("rewrite");
        let error = store::load_events(&root, &id)
            .err()
            .unwrap_or_else(|| panic!("{what}: this must be refused"));
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{what}");
    }

    std::fs::write(&path, &original).expect("restore");
    store::load_events(&root, &id).expect("the canonical bytes read back");
}

/// Revision zero is the Object before any Event, and no writer emits it.
///
/// Adjacency cannot refuse it: a `0, 1, 2 …` log is perfectly contiguous, and
/// recovery filters records at or below the projection as old evidence — so an
/// impossible record could sit in the log being silently skipped rather than
/// reported. The lower bound belongs to the record contract.
#[test]
fn event_revisions_start_at_one() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "revision domain");
    let path = store::events_path(&root, &id);
    let original = std::fs::read_to_string(&path).expect("events");
    let first: engr::model::Event =
        serde_json::from_str(original.lines().next().expect("record")).expect("event");

    let mut zero = first.clone();
    zero.rev = 0;
    let record = engr::proof::canonical_bytes(&zero, "event").expect("canonical");
    std::fs::write(&path, format!("{record}\n{original}")).expect("prefix with rev 0");

    let error = store::load_events(&root, &id).expect_err("there is no revision zero");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("start at 1"), "{error}");
}

/// The Event file is the complete audit trail, not merely a suffix that can be
/// replayed over the current projection. Losing its first records must not be
/// blessed by a later append or reconciliation.
#[test]
fn a_purged_event_prefix_is_refused() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "complete history");
    gate::confirm(
        &root,
        &format!(
            "CONFIRM {}",
            gate::prepare(&root, payload(Action::SectionAdded, &id, "second record"))
                .expect("prepare")
                .candidate
                .challenge
        ),
    )
    .expect("confirm");

    let path = store::events_path(&root, &id);
    let retained = std::fs::read_to_string(&path)
        .expect("history")
        .lines()
        .skip(1)
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, format!("{retained}\n")).expect("truncate prefix");

    let error = store::load_events(&root, &id).expect_err("a prefix was lost");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("starts at revision"), "{error}");
    assert_eq!(
        ops::reconcile(&root, &id)
            .expect_err("recovery cannot accept it")
            .code,
        engr::EXIT_SCHEMA
    );
}

/// A retained v1 Event may precede v2, but never follow it: no current writer
/// can produce that regression and replaying it would reintroduce old authority
/// after mixed-authority history began.
#[test]
fn retained_event_v1_cannot_follow_event_v2() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "event generations");
    let path = store::events_path(&root, &id);
    let v2 = std::fs::read_to_string(&path).expect("v2 creation");
    let old_payload = payload(Action::SectionAdded, &id, "old generation tail");
    let v1 = engr::model::Event {
        format: engr::model::EVENT_FORMAT.to_owned(),
        version: engr::EVENT_ENVELOPE_VERSION_V0,
        event_id: engr::model::new_id(),
        rev: 2,
        time: "2026-08-27T00:00:00Z".to_owned(),
        payload: old_payload.clone(),
        provenance: Provenance::confirmed("234567", old_payload.sha256().expect("hash")),
    };
    std::fs::write(
        &path,
        format!("{}{}\n", v2, serde_json::to_string(&v1).expect("v1")),
    )
    .expect("append retained generation");

    let error = store::load_events(&root, &id).expect_err("generation cannot go backward");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(
        error.message.contains("cannot follow generation 2"),
        "{error}"
    );
}

/// Candidate envelopes are deleted after confirmation, but their digest remains
/// durable evidence. It is recomputed from Event history rather than accepted
/// as a syntactically plausible long-lived label.
#[test]
fn a_human_event_candidate_digest_is_rechecked_from_history() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "durable candidate proof");
    let path = store::events_path(&root, &id);
    let mut event: serde_json::Value = serde_json::from_str(
        std::fs::read_to_string(&path)
            .expect("history")
            .lines()
            .next()
            .expect("creation"),
    )
    .expect("event");
    event["admission"]["confirmation"]["candidate_digest"] =
        serde_json::Value::String(format!("1:{}", "b".repeat(64)));
    write_raw(&path, &event).expect("replace with canonical forged event");

    let error = store::load_events(&root, &id).expect_err("digest was not admitted");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("candidate digest"), "{error}");
    assert_eq!(
        ops::effective(&root, &id)
            .expect_err("authority cannot use forged Event evidence")
            .code,
        engr::EXIT_SCHEMA
    );
}

#[test]
fn a_historical_v3_format_must_be_jcs() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "historical format");
    let format = store::engr_dir(&root).join("format.json");
    let canonical = std::fs::read(&format).expect("canonical format");
    std::fs::write(
        &format,
        "{ \"version\": 3, \"format\": \"engr-workspace\" }",
    )
    .expect("noncanonical v3 format");
    let commit = commit_all(&root, "noncanonical historical v3 format");
    std::fs::write(&format, canonical).expect("restore working format");

    let error = engr::git::object_at(&root, &commit, &id)
        .expect_err("historical v3 format must have its generation spelling");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("not persisted as JCS"), "{error}");
}

/// A transition whose numbers no identity can carry is refused before anything
/// is minted.
///
/// An Object at the shared ceiling is itself entirely valid; what is not
/// representable is the transition out of it. Some of those numbers appear
/// nowhere in the payload — the reducer allocates them — so a per-payload check
/// passes, and for an operation whose CandidateDigest excludes `rev` the hash
/// succeeds too. A person would then be holding a code for a mutation that can
/// never be admitted.
#[test]
fn preparation_refuses_a_transition_no_identity_can_carry() {
    let (_dir, root) = workspace();
    let ceiling = engr::proof::MAX_SAFE_INTEGER;

    let set_rev: fn(&mut engr::model::Object, u64) = |object, value| object.rev = value;
    let set_counter: fn(&mut engr::model::Object, u64) =
        |object, value| object.next_section_id = value;
    for (what, at_ceiling) in [
        ("the revision", set_rev),
        ("the section counter", set_counter),
    ] {
        let id = new_object(&root, "at the ceiling");
        let object = store::load_object(&root, &id).expect("object");
        let seal = object.sha256.clone().expect("aggregate seal");
        let resealed = engr::integrity::mutate(&object, &seal, |object| {
            at_ceiling(object, ceiling);
            Ok(())
        })
        .expect("reseal at the ceiling");
        save_raw(&root, &resealed.object).expect("put it on disk");

        let events = std::fs::read(store::events_path(&root, &id)).expect("events");
        let error = gate::prepare(&root, payload(Action::SectionAdded, &id, "one more"))
            .err()
            .unwrap_or_else(|| panic!("{what}: this must be refused"));
        assert_eq!(error.code, engr::EXIT_USAGE, "{what}");
        assert!(error.message.contains("safe integer"), "{what}: {error}");
        assert!(
            gate::pending(&root).expect("candidates").is_empty(),
            "{what}: no candidate was minted"
        );
        assert_eq!(
            std::fs::read(store::events_path(&root, &id)).expect("events"),
            events,
            "{what}: and nothing durable moved"
        );
    }
}

/// Two envelopes can share a CandidateDigest and differ in challenge.
///
/// The digest names the semantic transition, not the envelope, so an older
/// identical candidate restored after a later one was applied would match on
/// the digest alone. Event v2 persists the pair precisely so a record proves
/// confirmation of *its* challenge, and reporting the older envelope as the
/// newer one's idempotent retry would say a person answered for something they
/// never saw.
#[test]
fn an_identical_candidate_with_another_challenge_is_not_an_applied_retry() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "two envelopes");
    let proposal = || payload(Action::SectionAdded, &id, "the same wording");

    let first = gate::prepare(&root, proposal()).expect("first envelope");
    let first_path = store::candidate_path(&root, &first.candidate.challenge).expect("path");
    let kept = std::fs::read_to_string(&first_path).expect("keep the envelope");

    // A second proposal for the same mutation supersedes it. Same transition,
    // same digest, different challenge.
    let second = gate::prepare(&root, proposal()).expect("second envelope");
    assert_eq!(
        first.candidate.candidate_digest, second.candidate.candidate_digest,
        "the same transition has the same digest"
    );
    assert_ne!(first.candidate.challenge, second.candidate.challenge);

    gate::confirm(&root, &format!("CONFIRM {}", second.candidate.challenge)).expect("admit B");

    // A restores from a backup. It is stale, and it is not B's retry.
    std::fs::write(&first_path, &kept).expect("restore the older envelope");
    let restored = gate::find(&root, &first.candidate.challenge).expect("it still reads");
    match gate::candidate_state(&root, &restored).expect("classify") {
        gate::CandidateState::Stale { .. } => {}
        other => panic!("an envelope nobody answered is not applied: {other:?}"),
    }
    let error = gate::confirm(&root, &format!("CONFIRM {}", first.candidate.challenge))
        .expect_err("and confirming it admits nothing");
    assert_eq!(error.code, engr::EXIT_STALE);
}

/// The current generation emits exactly one Ref shape, wherever it emits.
///
/// The legacy decoder is retained so historical Objects and Events can still be
/// read under their own generation. Letting that compatibility shape reach an
/// emission path lets a supported writer mint a candidate — and hand a person a
/// code for it — that this same build's reader then refuses as schema-invalid.
#[test]
fn a_legacy_reference_never_reaches_a_current_candidate() {
    let (_dir, root) = workspace();
    let target = new_object(&root, "the target");
    admit(
        &root,
        payload(Action::SectionAdded, &target, "depended upon"),
    );
    let commit = commit_all(&root, "record target");
    let source = new_object(&root, "the source");

    // The writer refuses before anything is minted.
    let mut proposal = payload(Action::SectionAdded, &source, "stands on the target");
    proposal.content.refs = vec![Ref::legacy(&target, 1, "c".repeat(64), &commit)];
    let error = gate::prepare(&root, proposal).expect_err("this generation has one Ref shape");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(
        gate::pending(&root).expect("candidates").is_empty(),
        "and no challenge was handed out"
    );

    // And the reader refuses it in the presentation context too, where a
    // conforming writer of some other build might have put one.
    let good = text_ref(&root, &target, 1, &commit);
    let mut proposal = payload(Action::SectionAdded, &source, "stands on the target");
    proposal.content.refs = vec![good];
    let prepared = gate::prepare(&root, proposal).expect("a selective ref is fine");
    let path = store::candidate_path(&root, &prepared.candidate.challenge).expect("path");
    let mut candidate = prepared.candidate.clone();
    candidate.context.previous_refs = vec![Ref::legacy(&target, 1, "c".repeat(64), &commit)];
    candidate.integrity_sha256 = candidate
        .integrity_digest()
        .expect("recompute the envelope");
    write_raw(&path, &candidate).expect("a self-consistent envelope");

    let error = gate::find(&root, &prepared.candidate.challenge)
        .expect_err("a v3 candidate carries no legacy reference");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("legacy references"), "{error}");
}

/// `sources[]` is a set like any other, and takes the shared order.
///
/// A field-local numeric rule is a second canonicalization algorithm in a
/// protocol that has one, and the two disagree as soon as the ids differ in
/// digit count. Multi-digit is where it shows: `[2, 10]` is ascending, and
/// canonical is `[10, 2]`, because `"10"` sorts before `"2"`. Ruled at PR #52
/// `5413218070`, synchronized to #9/#13/#32, superseding the older wording.
#[test]
fn merge_sources_take_the_shared_canonical_set_order() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "many sections");
    for n in 1..=10 {
        admit(
            &root,
            payload(Action::SectionAdded, &id, &format!("section {n}")),
        );
    }

    let proposal = payload(
        Action::SectionMerged {
            merge: engr::model::Merge::Into {
                destination: 1,
                sources: vec![2, 10],
            },
        },
        &id,
        "one point",
    );
    let prepared = gate::prepare(&root, proposal).expect("prepare the merge");
    let Action::SectionMerged { merge } = &prepared.candidate.payload.action else {
        panic!("a merge");
    };
    assert_eq!(
        merge.consumed(),
        &[10, 2],
        "canonical set order is over JCS bytes, not over the numbers"
    );

    let object = gate::confirm(&root, &format!("CONFIRM {}", prepared.candidate.challenge))
        .expect("confirm")
        .object;
    assert_eq!(
        object.sections.iter().map(|s| s.id).collect::<Vec<_>>(),
        vec![1, 3, 4, 5, 6, 7, 8, 9],
    );
    let event = store::load_events(&root, &id)
        .expect("events")
        .pop()
        .expect("the merge");
    let Action::SectionMerged { merge } = &event.payload.action else {
        panic!("a merge");
    };
    assert_eq!(merge.consumed(), &[10, 2], "and that is what is persisted");
}

/// The workspace root a caller passes is not the path git answers with.
///
/// `rev-parse --show-toplevel` answers in git's coordinates. macOS puts
/// temporary directories under `/var`, a symlink to `/private/var`, which git
/// resolves and the caller does not; Windows answers `C:/…` with forward
/// slashes against a `C:\…` the caller built. Deriving the repository-relative
/// path by stripping one from the other therefore worked on Linux and nowhere
/// else, and the two failure shapes were both quiet: historical resolution
/// reported valid authority as unreadable, and provenance reported committed
/// material as having no commit.
///
/// Reproduced here on any platform by reaching the same repository through a
/// symlink, which is exactly the shape macOS hands every test.
#[test]
fn a_workspace_reached_through_a_symlink_still_resolves_its_own_paths() {
    let dir = TempDir::new().expect("temp dir");
    let real = dir.path().join("real");
    std::fs::create_dir_all(&real).expect("real");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real, dir.path().join("link")).expect("symlink");
    #[cfg(windows)]
    if std::os::windows::fs::symlink_dir(&real, dir.path().join("link")).is_err() {
        // Windows needs privilege for this; the platform is covered by CI.
        return;
    }
    let root = dir.path().join("link");
    assert_ne!(root, real, "the two spellings differ");

    store::init(&root).expect("init");
    let id = new_object(&root, "reached through a link");
    admit(&root, payload(Action::SectionAdded, &id, "depended upon"));
    let commit = commit_all(&root, "record");

    // Historical resolution: the workspace prefix has to be found for this to
    // read at all.
    let historical = engr::git::object_at(&root, &commit, &id)
        .expect("the historical workspace resolves")
        .expect("and holds the object");
    assert_eq!(historical.sections[0].text, "depended upon");

    // Provenance: the object file is committed, and saying otherwise is the
    // falsely quiet answer.
    assert_eq!(
        engr::git::uncommitted(&root, &store::object_path(&root, &id)),
        Some(false),
        "committed material must not report itself as uncommitted"
    );
    assert!(
        engr::git::last_commit_for(&root, &store::object_path(&root, &id)).is_some(),
        "and a commit does hold it"
    );

    // And a selective Ref, which is what actually broke: it resolves the target
    // through the historical workspace.
    let source = new_object(&root, "the source");
    let mut proposal = payload(Action::SectionAdded, &source, "stands on it");
    proposal.content.refs = vec![text_ref(&root, &id, 1, &commit)];
    gate::prepare(&root, proposal).expect("a reference resolves through the link");
}

/// Put an admitted Event in the log without going through any write path.
///
/// There is no public append — Event provenance is deliberately too thin to
/// prove admission from, so the durable write lives behind the gate. What this
/// reproduces is the state a crash leaves: the record is durable and the
/// projection is not, which is exactly what recovery has to cope with. Written
/// as the canonical JCS bytes, because that is what the read boundary requires.
fn append_admitted_raw(root: &Path, event: &engr::model::Event) {
    let path = store::events_path(root, &event.payload.object);
    let line = engr::proof::canonical_bytes(event, "Event v2").expect("canonical");
    let mut existing = std::fs::read_to_string(&path).unwrap_or_default();
    existing.push_str(&line);
    existing.push('\n');
    std::fs::write(&path, existing).expect("write event");
}

/// A title mutation is exempt from *review*, not from *Rules*.
///
/// The exception is that there is no applicable Rule to review against, and it
/// has to be established rather than inferred from the shape of the action.
/// Reading "no review is present and this is a rename" as sufficient let a
/// governed workspace admit a title change no policy ever saw.
#[test]
fn a_title_event_with_no_review_is_refused_where_a_rule_governs() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "governed titles");
    let renamed = payload(Action::ObjectRenamed, &id, "a different title");
    let bare = agent_event_without_review(&renamed, 2);

    // Ungoverned, there is genuinely nothing to review against.
    store::check_appendable(&root, &bare).expect("no applicable Rule, no review to carry");

    std::fs::write(root.join("AGENTS.md"), "the contract\n").expect("basis");
    let rules = engr::rules::dir(&root);
    std::fs::create_dir_all(&rules).expect("rules dir");
    std::fs::write(
        rules.join("titles.md"),
        "---\nid: titles\napplies:\n  domains:\n    - object\nbased_on:\n  - path: AGENTS.md\n---\n\n# Titles\n\nSay what the thing is.\n",
    )
    .expect("rule");

    let error = store::check_appendable(&root, &bare)
        .expect_err("a rule governs this Object, so the title reviews against it");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("even a title mutation"), "{error}");
}

fn agent_event_without_review(payload: &Payload, rev: u64) -> engr::model::Event {
    engr::model::Event {
        format: engr::model::EVENT_FORMAT.to_owned(),
        version: engr::EVENT_ENVELOPE_VERSION,
        event_id: engr::model::new_id(),
        rev,
        time: "2026-08-27T00:00:00Z".to_owned(),
        payload: payload.clone(),
        provenance: Provenance::Tagged {
            admission: TaggedAdmission {
                kind: engr::semantics::Admission::Agent,
                confirmation: None,
                rule_review: None,
            },
        },
    }
}

/// The attempt is why a durable record cannot be the whole admission capability.
///
/// Event provenance is deliberately minimal: outcome and digest, and none of the
/// transient inputs the decision was made from. A caller can therefore name the
/// *correct* live ReviewDigest for a mutation while the applicable ceiling has
/// already been exceeded, and nothing in the record would say so. The gate
/// carries the attempt and refuses; a raw append had nothing to refuse on, which
/// is why there is no longer a public one.
#[test]
fn an_exhausted_attempt_is_refused_and_there_is_no_second_door() {
    let (_dir, root) = workspace();
    // The Object exists before the policy does, as it would in a real workspace.
    let id = new_object(&root, "governed");
    std::fs::write(root.join("AGENTS.md"), "the contract\n").expect("basis");
    let rules = engr::rules::dir(&root);
    std::fs::create_dir_all(&rules).expect("rules dir");
    std::fs::write(
        rules.join("careful.md"),
        "---\nid: careful\napplies:\n  domains:\n    - object\nreview:\n  max_attempts: 1\n  on_exhaustion: reject\nbased_on:\n  - path: AGENTS.md\n---\n\n# Careful\n\nRead this.\n",
    )
    .expect("rule");

    let before = store::load_events(&root, &id).expect("events").len();
    let error = gate::admit_agent(
        &root,
        payload(Action::SectionAdded, &id, "wording"),
        Some(engr::gate::ReviewAttestation {
            review_digest: format!("1:{}", "a".repeat(64)),
            reviewed_rules: vec!["careful".to_owned()],
            attempt: 2,
            result: engr::proof::ReviewResult::Passed,
            explanation: None,
        }),
    )
    .expect_err("attempt 2 is past a ceiling of 1");
    assert_ne!(error.code, 0);
    assert_eq!(
        store::load_events(&root, &id).expect("events").len(),
        before,
        "nothing was admitted"
    );
}

/// An always-present member is not optional just because serde has a default.
///
/// #9 says Event-v2 `refs` is always present, including `[]`, and #25 says the
/// same of the candidate's presentation context. A decoder with `#[serde(default)]`
/// happily fills either in, and `check_nothing_was_dropped` iterates the members
/// that *were* stored, so it cannot see one that is absent. What closes it is
/// comparing the stored value against this build's own serialization of the
/// record it decoded to: the writer always emits `refs`, so a record without it
/// is not the shape, whatever serde was willing to reconstruct.
#[test]
fn an_omitted_always_present_member_is_not_a_second_spelling() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "always present");
    let path = store::events_path(&root, &id);
    let original = std::fs::read_to_string(&path).expect("events");
    let line = original.lines().next().expect("record").to_owned();
    assert!(
        line.contains(r#""refs":[]"#),
        "the writer emits the member: {line}"
    );

    // Drop it. Everything else is untouched, and serde would decode it back.
    let without = line.replacen(r#""refs":[],"#, "", 1);
    assert_ne!(without, line, "the fixture must actually drop it");
    std::fs::write(&path, format!("{without}\n")).expect("rewrite");
    let error = store::load_events(&root, &id).expect_err("refs is always present");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("exact canonical shape"), "{error}");

    std::fs::write(&path, &original).expect("restore");
    store::load_events(&root, &id).expect("and the emitted record reads back");
}

/// The same, for each of the candidate's always-present presentation members.
#[test]
fn a_candidate_cannot_omit_its_always_present_presentation_members() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "candidate shape");
    admit(&root, payload(Action::SectionAdded, &id, "wording"));
    let prepared = gate::prepare(
        &root,
        payload(Action::SectionRevised { section: 1 }, &id, "reworded"),
    )
    .expect("prepare");
    let code = prepared.candidate.challenge.clone();
    let path = store::candidate_path(&root, &code).expect("candidate path");
    let original = std::fs::read_to_string(&path).expect("candidate");

    for member in [
        "previous_text",
        "previous_based_on",
        "previous_refs",
        "previous_semantics_recorded",
    ] {
        let mut stored: serde_json::Value = serde_json::from_str(&original).expect("json");
        assert!(
            stored.as_object().expect("object").contains_key(member),
            "the writer emits {member}"
        );
        stored.as_object_mut().expect("object").remove(member);
        write_raw(&path, &stored).expect("a candidate missing one member");

        let error = gate::find(&root, &code)
            .err()
            .unwrap_or_else(|| panic!("{member}: an always-present member is not optional"));
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{member}");
    }

    std::fs::write(&path, &original).expect("restore");
    gate::find(&root, &code).expect("and the prepared envelope reads back");
}
