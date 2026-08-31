//! Human Gate and Agent Rule Review are the only ways in. These tests pin both.

mod common;

use common::{admit, new_object, text_ref, workspace, Act};
use engr::model::{Content, EventAdmission, Payload, Ref};
use engr::semantics::{BasedOn, State};
use engr::{gate, ops, store};
use std::collections::BTreeSet;
use std::path::Path;
use tempfile::TempDir;

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
    engr::model::Event::sealed(
        candidate.object(),
        engr::model::new_id(),
        candidate.payload.action.clone(),
        candidate.expected_rev() + 1,
        EventAdmission::human("2026-08-13T00:00:00Z", candidate.code()),
    )
    .expect("an Event for a challenge that was really prepared")
}

/// A Human Event for a change that really was prepared.
///
/// The durable boundary admits one only against the candidate it was prepared
/// as, which is what makes the challenge unforgeable — so a direct caller that
/// wants to reach that boundary goes through `prepare`, exactly as `confirm`
/// does. [`direct_human_event`] stays for records that must be refused before
/// the admission proof is ever reached.
fn admissible_human_event(root: &Path, _object: &str, payload: Payload) -> engr::model::Event {
    let prepared = gate::prepare(root, payload).expect("prepare");
    candidate_event(&prepared.candidate)
}

/// A well-formed Human record naming a challenge nobody minted.
///
/// For the cases that must be refused before the admission proof is reached at
/// all — a shape no history could arrive at, a beginning nothing follows from.
fn unadmitted_human_event(object: &str, payload: Payload, rev: u64) -> engr::model::Event {
    engr::model::Event::sealed(
        object,
        engr::model::new_id(),
        payload.action,
        rev,
        EventAdmission::human("2026-08-23T00:00:00Z", "TEST23"),
    )
    .expect("a well formed record, whatever admitted it")
}

fn direct_human_event(root: &Path, id: &str, payload: Payload, rev: u64) -> engr::model::Event {
    let confirmation = store::load_events(root, id)
        .expect("existing history")
        .into_iter()
        .find_map(|event| event.human_confirmation().cloned())
        .expect("human confirmation");
    engr::model::Event::sealed(
        id,
        engr::model::new_id(),
        payload.action,
        rev,
        EventAdmission::human("2026-08-23T00:00:00Z", &confirmation.challenge),
    )
    .expect("a well formed record, whatever admitted it")
}

fn payload(action: Act, object: &str, text: &str) -> Payload {
    common::payload(action, object, content(text))
}

fn empty(action: Act, object: &str) -> Payload {
    common::payload(action, object, Content::default())
}

/// Adjust the wording a payload already carries.
fn edit(payload: &mut Payload, adjust: impl FnOnce(&mut Content)) {
    adjust(
        &mut payload
            .action
            .value_mut()
            .expect("this action carries wording")
            .content,
    );
}

fn stored_text_ref(object: &str, section: u64, commit: &str, digest: &str) -> Ref {
    engr::dependency::SelectiveRef::stored(
        engr::proof::section_target(object, section),
        vec![engr::dependency::SemanticField::Text],
        commit,
        digest,
    )
    .expect("stored selective reference")
}

/// One way a stored candidate can be rewritten on disk, named so the matrix
/// below reads as a list of risks rather than a list of closure types.
type Tamper = (&'static str, Box<dyn Fn(&mut serde_json::Value)>);

#[test]
fn a_candidate_is_only_admitted_by_the_exact_phrase() {
    let (_dir, root) = workspace();
    let id = engr::model::new_id();
    let prepared = gate::prepare(&root, payload(Act::Create, &id, "a title")).expect("prepare");
    let code = prepared.candidate.code().to_owned();

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
    let prepared = gate::prepare(&root, payload(Act::Create, &id, "a title")).expect("prepare");
    let code = prepared.candidate.code().to_owned();

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
    let prepared = gate::prepare(&root, payload(Act::Add, &id, "pending")).expect("prepare");
    let code = prepared.candidate.code().to_owned();
    let format_path = store::version_path(&root);
    let object_path = store::object_path(&root, &id);
    let outside_path = root.join("outside.json");
    std::fs::write(&outside_path, "outside candidate storage").expect("outside fixture");
    let candidate_path = store::challenge_path(&root, &code).expect("candidate path");
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
    let prepared = gate::prepare(&root, empty(Act::Close, &id)).expect("prepare");
    let code = prepared.candidate.code().to_owned();
    let object_path = store::object_path(&root, &id);
    let candidate_path = store::challenge_path(&root, &code).expect("candidate path");
    let events_path = store::events_path(&root, &id);

    std::fs::remove_file(store::version_path(&root)).expect("drop the generation marker");
    std::fs::write(
        store::engr_dir(&root).join("format.json"),
        format!(
            r#"{{"format":"{}","version":{}}}"#,
            engr::PREDECESSOR_WORKSPACE_FORMAT,
            engr::PREDECESSOR_WORKSPACE_VERSION
        ),
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
    admit(&root, payload(Act::Add, &id, "first"));
    admit(&root, payload(Act::Add, &id, "second"));
    let object = admit(&root, empty(Act::Delete(2), &id));
    assert_eq!(object.next_section_id, 3);

    // The counter, not max(existing) + 1, decides the next id — otherwise this
    // section would take §2 and every outside reference to the deleted one would
    // silently point at different content.
    let object = admit(&root, payload(Act::Add, &id, "third"));
    let ids: Vec<u64> = object.sections.iter().map(|section| section.id).collect();
    assert_eq!(ids, vec![1, 3]);
}

#[test]
fn merging_keeps_the_destination_id_and_removes_its_sources() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "merging");
    admit(&root, payload(Act::Add, &id, "one"));
    admit(&root, payload(Act::Add, &id, "two"));
    let object = admit(
        &root,
        payload(
            Act::Merge {
                destination: 1,
                sources: vec![2],
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
    admit(&root, payload(Act::Add, &id, "one"));
    admit(&root, payload(Act::Add, &id, "two"));

    let object = admit(
        &root,
        payload(
            Act::Merge {
                destination: 1,
                sources: vec![2],
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
    admit(&root, payload(Act::Add, &target, "depended on"));
    admit(&root, payload(Act::Add, &target, "the other"));
    let commit = commit_all(&root, "record target wording");

    let mut dependent = payload(Act::Add, &source, "rests on §1");
    edit(&mut dependent, |content| {
        content.based_on = Some(BasedOn::new(&commit));
        content.refs = vec![text_ref(&root, &target, 1, &commit)];
    });
    admit(&root, dependent);

    let error = gate::prepare(
        &root,
        payload(
            Act::Merge {
                destination: 2,
                sources: vec![1],
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
    admit(&root, payload(Act::Add, &id, "one"));
    let object = admit(&root, empty(Act::Close, &id));
    assert_eq!(object.state, State::Closed);

    let error =
        gate::prepare(&root, payload(Act::Add, &id, "two")).expect_err("a closed object is sealed");
    assert_eq!(error.code, engr::EXIT_INVARIANT);

    admit(&root, empty(Act::Reopen, &id));
    admit(&root, payload(Act::Add, &id, "two"));
}

#[test]
fn one_live_candidate_per_object() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "candidates");
    let first = gate::prepare(&root, payload(Act::Add, &id, "first draft"))
        .expect("first")
        .candidate
        .code()
        .to_owned();
    let second = gate::prepare(&root, payload(Act::Add, &id, "second draft")).expect("second");
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
        let error = gate::prepare(&root, payload(Act::Create, id, "invalid identity"))
            .expect_err("a direct caller cannot bypass Object identity validation");
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{id}");
    }

    // A Ref names an Object too, and that identity is refused where the Ref is
    // built rather than where the payload is prepared. There is no way to hand
    // the gate a reference onto an identity no Object could hold.
    for id in ["not-a-uuid", "550e8400-e29b-41d4-a716-446655440000"] {
        let error = engr::dependency::SelectiveRef::stored(
            format!("obj:{id}:1"),
            vec![engr::dependency::SemanticField::Text],
            "0".repeat(40),
            format!("1:{}", "0".repeat(64)),
        )
        .expect_err("a Ref Object identity is persisted data too");
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{id}");
    }
    assert!(
        gate::pending(&root).expect("pending candidates").is_empty(),
        "rejected direct inputs must not leave a candidate behind"
    );
}

/// A title has no basis and no references, and in the activated generation that
/// is a fact about the shape rather than a rule applied to it: `object.created`
/// and `object.renamed` carry a title and nothing else, so there is no member a
/// hidden basis could arrive in. This pins the shape, and pins that a stored
/// record which invents one is refused rather than read past.
#[test]
fn title_actions_carry_a_title_and_nothing_else() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "old title");
    let admitted = common::admitted(&root, common::rename(&id, "new title"));
    let stored = serde_json::to_value(&admitted.event).expect("event as json");
    assert_eq!(
        stored["data"],
        serde_json::json!({ "title": "new title" }),
        "a renamed title has no room for a basis or a reference"
    );

    // A stored record that invents one is refused rather than read past. The
    // envelope cannot use `deny_unknown_fields` — the action is flattened into
    // it, and serde will not combine the two — so what closes it is the read
    // boundary comparing the stored bytes against this build's own
    // serialization of the record it decoded to.
    let path = store::events_path(&root, &id);
    let good = std::fs::read_to_string(&path).expect("events");
    let mut invented = stored.clone();
    invented["data"]["based_on"] = serde_json::json!({ "commit": "0".repeat(40) });
    let rewritten = engr::proof::canonical_bytes(&invented, "event").expect("canonical");
    let kept = good.lines().next().expect("the creation record");
    std::fs::write(&path, [kept, &rewritten, ""].join("\n")).expect("write");

    let error =
        store::load_events(&root, &id).expect_err("a title record that invents a basis is refused");
    assert_eq!(error.code, engr::EXIT_SCHEMA);

    std::fs::write(&path, &good).expect("restore");
    assert_eq!(
        store::load_events(&root, &id).expect("events").len(),
        2,
        "and nothing extra entered Event history"
    );
}

#[test]
fn public_gate_mutations_serialize_direct_callers() {
    use std::sync::{Arc, Barrier};

    let (_dir, root) = workspace();
    let id = new_object(&root, "direct lock");
    let first = payload(Act::Add, &id, "first proposal");
    let second = payload(Act::Add, &id, "second proposal");
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

    let response = format!("CONFIRM {}", candidates[0].code());
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
    let target = admit(&root, payload(Act::Add, &target, "target wording"));
    let target_id = target.id;
    let commit = commit_all(&root, "record target wording");
    let reference = text_ref(&root, &target_id, 1, &commit);

    let mut direct = payload(Act::Add, &source.to_ascii_uppercase(), "dependent wording");
    edit(&mut direct, |content| {
        content.based_on = Some(BasedOn::new("HEAD"));
        content.refs = vec![reference];
    });

    let prepared = gate::prepare(&root, direct).expect("direct payload is canonicalized");
    assert_eq!(prepared.candidate.payload.object, source);
    let content = prepared.candidate.payload.content();
    assert_eq!(content.based_on.expect("basis").commit, commit);
    assert_eq!(
        engr::dependency::parse_target(content.refs[0].target())
            .expect("target")
            .0,
        target_id
    );
    assert_eq!(content.refs[0].commit(), commit);
    assert!(prepared.candidate.challenge.digest.starts_with("1:"));

    let response = format!("CONFIRM {}", prepared.candidate.code());
    let event = gate::confirm(&root, &response)
        .expect("confirm canonical candidate")
        .event;
    let content = event.payload(&source).content();
    assert_eq!(content.based_on.expect("basis").commit, commit);
    assert_eq!(content.refs[0].commit(), commit);
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
    admit(&root, payload(Act::Add, &target, "depended upon"));
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
    let mut forged_current = payload(Act::Add, &source, "depends on forged wording");
    edit(&mut forged_current, |content| {
        content.refs = vec![good_ref.clone()];
    });
    let error = gate::prepare(&root, forged_current)
        .expect_err("a reference cannot trust a stale stored target hash");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("TargetIntegrityFailure"), "{error}");
    std::fs::write(&target_path, &target_before).expect("restore target");

    admit(
        &root,
        payload(Act::Revise(1), &target, "new uncommitted wording"),
    );
    let mut uncommitted = payload(Act::Add, &source, "depends on new wording");
    edit(&mut uncommitted, |content| {
        content.refs = vec![good_ref.clone()]
    });
    let error = gate::prepare(&root, uncommitted)
        .expect_err("a commit cannot be paired with newer uncommitted wording");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("Drifted"));

    admit(&root, payload(Act::Revise(1), &target, "depended upon"));

    let mut with_missing_object = payload(Act::Add, &source, "depends");
    edit(&mut with_missing_object, |content| {
        content.refs = vec![stored_text_ref(
            &engr::model::new_id(),
            1,
            &commit,
            &format!("1:{}", "0".repeat(64)),
        )]
    });
    assert_eq!(
        gate::prepare(&root, with_missing_object)
            .expect_err("a reference to a missing object is refused")
            .code,
        engr::EXIT_NOT_FOUND
    );

    let mut with_missing_section = payload(Act::Add, &source, "depends");
    edit(&mut with_missing_section, |content| {
        content.refs = vec![stored_text_ref(
            &target,
            99,
            &commit,
            &format!("1:{}", "0".repeat(64)),
        )]
    });
    assert_eq!(
        gate::prepare(&root, with_missing_section)
            .expect_err("a reference to a missing section is refused")
            .code,
        engr::EXIT_NOT_FOUND
    );

    let mut with_wrong_hash = payload(Act::Add, &source, "depends");
    edit(&mut with_wrong_hash, |content| {
        content.refs = vec![stored_text_ref(
            &target,
            1,
            &commit,
            &format!("1:{}", "0".repeat(64)),
        )]
    });
    assert_eq!(
        gate::prepare(&root, with_wrong_hash)
            .expect_err("a reference cannot pin something the target never said")
            .code,
        engr::EXIT_INVARIANT
    );

    let mut good = payload(Act::Add, &source, "depends");
    edit(&mut good, |content| content.refs = vec![good_ref]);
    gate::prepare(&root, good).expect("a well-formed reference is admitted");
}

/// A reference resolves against the snapshot's own workspace authority. A
/// commit taken by this generation decodes under this generation's rules, and a
/// commit whose authority this build does not read is refused rather than
/// interpreted under whichever rules happen to be current.
///
/// The predecessor half of that question — a commit taken before the migration
/// — is pinned in `migration.rs` against the real released bundle.
#[test]
fn historical_references_decode_the_snapshot_workspace_format() {
    let (_dir, root) = workspace();
    let target = new_object(&root, "historical target");
    let source = new_object(&root, "historical source");
    admit(&root, payload(Act::Add, &target, "historical wording"));
    let pinned = store::load_object(&root, &target).expect("target").sections[0]
        .digest
        .clone();
    let current_commit = commit_all(&root, "current historical snapshot");
    let engr::git::HistoricalObject::Current(current) =
        engr::git::object_at(&root, &current_commit, &target)
            .expect("current snapshot format")
            .expect("current target")
    else {
        panic!("a snapshot of this generation decodes as this generation");
    };
    assert_eq!(current.section(1).expect("current section").digest, pinned);

    let mut reference = payload(Act::Add, &source, "uses that wording");
    edit(&mut reference, |content| {
        content.refs = vec![text_ref(&root, &target, 1, &current_commit)]
    });
    let prepared = gate::prepare(&root, reference).expect("a current reference is admitted");
    gate::discard(&root, prepared.candidate.code()).expect("discard test candidate");

    let version_path = store::version_path(&root);
    let version_before = std::fs::read(&version_path).expect("VERSION");
    std::fs::write(&version_path, "99\n").expect("unsupported workspace generation");
    let unsupported_commit = commit_all(&root, "unsupported historical snapshot");
    std::fs::write(&version_path, &version_before).expect("restore workspace generation");
    let error = engr::git::object_at(&root, &unsupported_commit, &target)
        .expect_err("an unknown historical workspace generation is refused");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("workspace generation"), "{error}");

    let mut unsupported_reference = payload(Act::Add, &source, "must not guess");
    edit(&mut unsupported_reference, |content| {
        content.refs = vec![stored_text_ref(
            &target,
            1,
            &unsupported_commit,
            &format!("1:{}", "0".repeat(64)),
        )]
    });
    let error = gate::prepare(&root, unsupported_reference)
        .expect_err("a reference cannot decode an unsupported historical workspace");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("cannot be interpreted"), "{error}");
}

#[test]
fn sibling_references_are_allowed_but_direct_self_reference_is_not() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "self reference");
    admit(&root, payload(Act::Add, &id, "the first"));
    let commit = commit_all(&root, "record sibling");

    let mut inward = payload(Act::Add, &id, "the second");
    edit(&mut inward, |content| {
        content.refs = vec![text_ref(&root, &id, 1, &commit)]
    });
    admit(&root, inward);

    let commit = commit_all(&root, "record dependent sibling");
    let mut direct = payload(Act::Revise(2), &id, "self-dependent");
    edit(&mut direct, |content| {
        content.refs = vec![text_ref(&root, &id, 2, &commit)]
    });
    let error = gate::prepare(&root, direct).expect_err("a direct self reference is refused");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("cannot directly reference itself"));
}

/// A Challenge prepared by a generator this build cannot interpret is refused
/// rather than reinterpreted. Pending Challenges are local and short-lived, so
/// the cost of preparing again is a moment; the cost of the other answer is a
/// person confirming a question under rules nobody agreed on.
#[test]
fn a_challenge_from_another_generator_cannot_be_confirmed() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "generator identity");
    admit(&root, payload(Act::Add, &id, "old wording"));
    let prepared =
        gate::prepare(&root, payload(Act::Revise(1), &id, "new wording")).expect("prepare");
    let code = prepared.candidate.code().to_owned();
    let path = store::challenge_path(&root, &code).expect("path");
    let mut stored: serde_json::Value = store::read_json(&path).expect("challenge");
    stored["generator"]["fingerprint"] = serde_json::json!(format!("1:{}", "0".repeat(64)));
    write_raw(&path, &stored).expect("another generator's challenge");

    let error = gate::confirm(&root, &format!("CONFIRM {code}"))
        .expect_err("a generator this build cannot interpret admits nothing");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert_eq!(
        ops::effective(&root, &id).expect("object").sections[0].text,
        "old wording"
    );
}

/// A Challenge is the frozen question: which act, against which Object, at which
/// revision, with exactly which value, and standing behind which review. One
/// digest covers all of it, because a file that could be rewritten in any of
/// those would present one change, admit another, and still pass its own checks.
///
/// Exhaustive by construction, not by sample. The cases below are compared
/// against the two structs' own serialized keys, so a member added to a
/// Challenge or to its Object subject without a case here fails this test rather
/// than quietly becoming a member nothing proves is bound.
#[test]
fn rewriting_a_challenge_is_detected_before_admission() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "challenge integrity");
    admit(&root, payload(Act::Add, &id, "old wording"));

    let envelope: Vec<Tamper> = vec![
        (
            "id",
            Box::new(|value: &mut serde_json::Value| value["id"] = serde_json::json!("ZZZZZZ")),
        ),
        (
            "generator",
            Box::new(|value: &mut serde_json::Value| {
                value["generator"]["version"] = serde_json::json!(99)
            }),
        ),
        (
            "created_at",
            Box::new(|value: &mut serde_json::Value| {
                value["created_at"] = serde_json::json!("2000-01-01T00:00:00Z")
            }),
        ),
        (
            "subject",
            Box::new(|value: &mut serde_json::Value| {
                value["subject"]["type"] = serde_json::json!("migration")
            }),
        ),
        (
            "digest",
            Box::new(|value: &mut serde_json::Value| {
                value["digest"] = serde_json::json!(format!("1:{}", "0".repeat(64)))
            }),
        ),
    ];
    let subject: Vec<Tamper> = vec![
        (
            "action",
            Box::new(|value: &mut serde_json::Value| {
                value["subject"]["data"]["action"] = serde_json::json!("section.delete")
            }),
        ),
        (
            "object",
            Box::new(|value: &mut serde_json::Value| {
                value["subject"]["data"]["object"] = serde_json::json!(engr::model::new_id())
            }),
        ),
        (
            "expected_rev",
            Box::new(|value: &mut serde_json::Value| {
                value["subject"]["data"]["expected_rev"] = serde_json::json!(0)
            }),
        ),
        (
            "value",
            Box::new(|value: &mut serde_json::Value| {
                value["subject"]["data"]["value"]["text"] =
                    serde_json::json!("wording nobody was shown")
            }),
        ),
        (
            "review",
            Box::new(|value: &mut serde_json::Value| {
                value["subject"]["data"]["review"] = serde_json::json!({
                    "outcome": "passed",
                    "digest": format!("1:{}", "0".repeat(64)),
                })
            }),
        ),
    ];

    let members = |value: serde_json::Value| -> BTreeSet<String> {
        value
            .as_object()
            .expect("an object")
            .keys()
            .cloned()
            .collect()
    };
    let named = |cases: &[Tamper]| -> BTreeSet<String> {
        cases.iter().map(|(name, _)| (*name).to_owned()).collect()
    };
    let live = gate::prepare(&root, payload(Act::Revise(1), &id, "new wording")).expect("prepare");
    assert_eq!(
        members(serde_json::to_value(&live.candidate.challenge).expect("challenge")),
        named(&envelope),
        "a Challenge member with no case here is a member nothing proves is bound"
    );
    let mut populated = live.candidate.subject.clone();
    populated.review = Some(engr::model::ReviewProvenance {
        outcome: engr::model::ReviewOutcome::Passed,
        digest: format!("1:{}", "0".repeat(64)),
    });
    assert_eq!(
        members(serde_json::to_value(&populated).expect("subject")),
        named(&subject),
        "an Object subject member with no case here is a member nothing proves is bound"
    );
    gate::discard(&root, live.candidate.code()).expect("clear the untampered challenge");

    for (name, tamper) in envelope.into_iter().chain(subject) {
        let prepared =
            gate::prepare(&root, payload(Act::Revise(1), &id, "new wording")).expect("prepare");
        let code = prepared.candidate.code().to_owned();
        let path = store::challenge_path(&root, &code).expect("path");

        // Untouched, it loads and would confirm exactly as prepared.
        gate::find(&root, &code).expect("an untouched challenge loads");

        let mut stored: serde_json::Value = store::read_json(&path).expect("challenge");
        tamper(&mut stored);
        write_raw(&path, &stored).expect("rewrite challenge");

        let error = gate::find(&root, &code).expect_err(name);
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{name}: {error}");
        let error = gate::confirm(&root, &format!("CONFIRM {code}")).expect_err(name);
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{name}: {error}");
        assert_eq!(
            ops::effective(&root, &id).expect("object").sections[0].text,
            "old wording",
            "{name}: a rewritten challenge must not be admitted"
        );
        gate::discard(&root, &code).expect("clear the tampered challenge");
    }
}

/// A stored Challenge with no digest is refused rather than read as one nothing
/// needed to protect.
#[test]
fn a_challenge_without_a_digest_is_refused_rather_than_trusted() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "envelope integrity");
    let prepared = gate::prepare(&root, payload(Act::Add, &id, "pending")).expect("prepare");
    let code = prepared.candidate.code().to_owned();
    let path = store::challenge_path(&root, &code).expect("path");
    let mut stored: serde_json::Value = store::read_json(&path).expect("challenge");
    stored.as_object_mut().expect("challenge").remove("digest");
    write_raw(&path, &stored).expect("undigested challenge");

    let error =
        gate::confirm(&root, &format!("CONFIRM {code}")).expect_err("no digest, no admission");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("digest"), "{error}");
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
    let a = gate::prepare(&root, payload(Act::Add, &first, "wording A")).expect("prepare A");
    let b = gate::prepare(&root, payload(Act::Add, &second, "wording B")).expect("prepare B");
    let (a_code, b_code) = (a.candidate.code().to_owned(), b.candidate.code().to_owned());
    assert_ne!(a_code, b_code);

    let a_path = store::challenge_path(&root, &a_code).expect("path");
    let mut stored: serde_json::Value = store::read_json(&a_path).expect("candidate A");
    stored["id"] = serde_json::json!(b_code.clone());
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
    let prepared =
        gate::prepare(&root, payload(Act::Add, &stranded, "old envelope")).expect("prepare");
    let code = prepared.candidate.code().to_owned();
    let path = store::challenge_path(&root, &code).expect("path");
    let mut stored: serde_json::Value = store::read_json(&path).expect("challenge");
    stored["generator"]["fingerprint"] = serde_json::json!(format!("1:{}", "0".repeat(64)));
    write_raw(&path, &stored).expect("a challenge this build refuses");

    // Preparing something else works, and does not reuse the stranded code.
    let other = gate::prepare(&root, payload(Act::Add, &unrelated, "unaffected"))
        .expect("an unrelated proposal is unaffected");
    assert_ne!(other.candidate.code(), code);
    assert!(other.superseded.is_empty());
    assert!(
        gate::confirm(&root, &format!("CONFIRM {}", other.candidate.code())).is_ok(),
        "and confirms normally"
    );

    // Proposing the stranded candidate's own object supersedes it.
    let replacement = gate::prepare(&root, payload(Act::Add, &stranded, "prepared again"))
        .expect("prepare again");
    assert_eq!(replacement.superseded, vec![code.clone()]);
    assert!(store::challenge_path(&root, &code)
        .map(|path| !path.exists())
        .unwrap_or(false));
    gate::confirm(&root, &format!("CONFIRM {}", replacement.candidate.code()))
        .expect("the replacement admits");
}

/// The already-applied retry still has to work, and integrity is checked on the
/// way through it: cleanup after a crash is the one path where a candidate is
/// deliberately re-read after its event is durable.
#[test]
fn candidate_integrity_does_not_break_the_idempotent_cleanup_retry() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "integrity retry");
    let prepared = gate::prepare(&root, payload(Act::Add, &id, "apply once")).expect("prepare");
    let code = prepared.candidate.code().to_owned();
    gate::confirm(&root, &format!("CONFIRM {code}")).expect("apply");
    write_raw(
        &store::challenge_path(&root, &code).expect("path"),
        &prepared.candidate.challenge,
    )
    .expect("restore the challenge a crash would have left");

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
    let error = gate::prepare(&root, payload(Act::Create, &engr::model::new_id(), &body))
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
            Act::Create,
            &engr::model::new_id(),
            "a title\nwith a second line",
        ),
    )
    .expect_err("a title cannot span lines");
    assert_eq!(error.code, engr::EXIT_USAGE);

    gate::prepare(
        &root,
        payload(
            Act::Create,
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
            Act::Create,
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
            Act::Create,
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
        payload(Act::Rename, &id, "audit failure reason codes v2"),
    )
    .expect("prepare");
    assert_eq!(
        prepared.candidate.expected_rev(),
        1,
        "a rename is shown against the record as it stands, so the challenge binds
         the revision the old title is read from rather than carrying a copy"
    );
    assert_eq!(
        ops::effective(&root, &id).expect("object").title,
        "audit failure reason codes",
        "and the old title is still what the record holds until the code is answered"
    );

    let object = gate::confirm(&root, &format!("CONFIRM {}", prepared.candidate.code()))
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
    let error = gate::prepare(&root, payload(Act::Rename, &id, &body)).expect_err("too long");
    assert_eq!(error.code, engr::EXIT_USAGE);
    assert!(
        error.message.contains("--rename") && !error.message.contains("--new"),
        "the refusal has to name the flag that was typed: {:?}",
        error.message
    );

    let error = gate::prepare(&root, payload(Act::Rename, &id, "two\nlines"))
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
        payload(Act::Rename, &second, "  Audit Failure Reason Codes  "),
    )
    .expect("a duplicate title is admitted, not refused");
    assert_eq!(prepared.notes.len(), 1);
    let gate::Note::DuplicateTitle { object } = &prepared.notes[0];
    assert_eq!(object, &first);
    // Stored as it will be listed. The duplicate check above already ignores the
    // padding, and a listing that prints what that check ignores puts one row
    // out of column underneath a note saying the two titles match.
    let engr::model::Action::ObjectRenamed { title, .. } = &prepared.candidate.payload.action
    else {
        panic!("a rename")
    };
    assert_eq!(title, "Audit Failure Reason Codes");

    let prepared =
        gate::prepare(&root, payload(Act::Rename, &second, "Retry Policy")).expect("prepare");
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
    admit(&root, empty(Act::Close, &id));

    let error = gate::prepare(&root, payload(Act::Rename, &id, "unsettled"))
        .expect_err("a closed object refuses a rename");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(
        error.message.contains("reopen"),
        "the refusal has to say the way through: {:?}",
        error.message
    );

    admit(&root, empty(Act::Reopen, &id));
    let object = admit(&root, payload(Act::Rename, &id, "unsettled"));
    assert_eq!(object.title, "unsettled");
}

#[test]
fn re_confirming_after_a_crash_does_not_apply_twice() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "recovery");
    let prepared = gate::prepare(&root, payload(Act::Add, &id, "once")).expect("prepare");
    let code = prepared.candidate.code().to_owned();
    let response = format!("CONFIRM {code}");
    gate::confirm(&root, &response).expect("confirm");

    // Reinstate the candidate to stand in for a crash between saving the
    // projection and clearing it.
    write_raw(
        &store::challenge_path(&root, &code).expect("challenge path"),
        &prepared.candidate.challenge,
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
    let first = gate::prepare(&root, payload(Act::Add, &id, "first")).expect("first");
    let event = candidate_event(&first.candidate);
    append_admitted_raw(&root, &id, &event);

    // A later action must first replay the confirmed tail. Otherwise it is
    // prepared at the same revision, its event collides with this one, and
    // purging can erase the first confirmed section.
    let second = gate::prepare(&root, payload(Act::Add, &id, "second")).expect("second");
    assert_eq!(second.candidate.expected_rev(), 2);
    let object = gate::confirm(&root, &format!("CONFIRM {}", second.candidate.code()))
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
    let prepared = gate::prepare(&root, payload(Act::Add, &id, "once")).expect("prepare");
    let code = prepared.candidate.code().to_owned();
    let event = candidate_event(&prepared.candidate);
    append_admitted_raw(&root, &id, &event);

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
        gate::prepare(&root, payload(Act::Create, &id, "created once")).expect("prepare");
    let event = candidate_event(&prepared.candidate);
    append_admitted_raw(&root, &id, &event);

    let code = prepared.candidate.code().to_owned();
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
    let (_dir, root) = common::repository();

    let id = engr::model::new_id();
    let prepared = gate::prepare(&root, payload(Act::Create, &id, "a title")).expect("prepare");
    let code = &prepared.candidate.code().to_owned();

    assert!(ignored(&root, ".engr/local/lock"));
    assert!(ignored(
        &root,
        &format!(".engr/local/challenges/{code}.json")
    ));

    assert!(!ignored(&root, ".engr/VERSION"));
    assert!(!ignored(&root, &format!(".engr/objects/{id}.json")));
    assert!(!ignored(
        &root,
        &format!(".engr/eventstore/objects/{id}.jsonl")
    ));
}

/// A Challenge is the members this generation defines, and no others.
///
/// Backlog bookkeeping was once declared inside the prepared context and left
/// it; a stored file that still carries it is not a Challenge with something
/// extra, it is a question this build cannot render. Unknown members are refused
/// rather than ignored, because ignoring one means rendering a question whose
/// declared effects nobody checked and whose digest nobody can reproduce.
#[test]
fn a_challenge_declaring_members_this_generation_never_defined_is_refused() {
    let (_dir, root) = workspace();
    let prepared = gate::prepare(
        &root,
        payload(Act::Create, &engr::model::new_id(), "a record"),
    )
    .expect("prepare");
    let code = prepared.candidate.code().to_owned();
    let path = store::challenge_path(&root, &code).expect("path");

    for (what, invented) in [
        ("the envelope", "/backlog"),
        ("the subject", "/subject/data/backlog"),
    ] {
        let mut stored: serde_json::Value = store::read_json(&path).expect("challenge");
        let (parent, member) = invented.rsplit_once('/').expect("a pointer");
        stored
            .pointer_mut(parent)
            .expect("the member's parent")
            .as_object_mut()
            .expect("an object")
            .insert(
                member.to_owned(),
                serde_json::json!([{ "item": "0195", "section": 1 }]),
            );
        write_raw(&path, &stored).expect("rewrite as an earlier build");

        // Refused wherever a Challenge is loaded, not only at confirm —
        // rendering it would present declared effects nobody can check.
        let error = gate::find(&root, &code).expect_err(what);
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{what}");
        assert!(error.message.contains("backlog"), "{what}: {error}");
        assert!(
            gate::pending(&root).is_err(),
            "{what}: and the strict listing says so"
        );
        assert!(
            gate::confirm(&root, &format!("CONFIRM {code}")).is_err(),
            "{what}: and it admits nothing"
        );
    }
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
    admit(&root, payload(Act::Add, &id, "wording"));

    let mut object = store::load_object(&root, &id).expect("object");
    object.sections[0].admitted.by = engr::semantics::Admission::Agent;
    save_raw(&root, &object).expect("bytes can always be put on disk from outside");

    let report = engr::ops::verify(&root, &id).expect("verify");
    assert!(
        !report.passed(),
        "a change nothing admitted is a broken record"
    );
    assert!(report.object_tampered || !report.tampered.is_empty());
    assert!(
        engr::integrity::check_object_integrity(
            &store::load_object(&root, &id).expect("still loads")
        )
        .is_err(),
        "the seal admitted at the gate does not cover this"
    );
}

/// A snapshot is read under its own workspace generation, and refused when the
/// bytes disagree with what that generation defines.
///
/// Historical reads go through the same decoder as current ones, so the
/// guarantee has to hold there too — otherwise a reference could pin a file
/// nothing would accept today and still resolve.
#[test]
fn a_historical_snapshot_carrying_undefined_members_fails_closed() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "historical read boundary");
    admit(&root, payload(Act::Add, &id, "wording"));
    let path = store::object_path(&root, &id);
    let good = std::fs::read(&path).expect("object");

    let mut value: serde_json::Value = serde_json::from_slice(&good).expect("json");
    value["sections"][0]
        .as_object_mut()
        .expect("section")
        .insert("confirmed_at".to_owned(), serde_json::json!("2026-08-28"));
    write_raw(&path, &value).expect("a member this generation never defined");
    let commit = commit_all(
        &root,
        "a snapshot claiming more than its generation defines",
    );
    std::fs::write(&path, &good).expect("restore");

    let error = engr::git::object_at(&root, &commit, &id)
        .expect_err("a snapshot is read under its own generation, and refused when it disagrees");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
}

/// The EventStore counterpart. A record carrying a member this generation never
/// defined is refused before a typed decode can drop it.
///
/// `Event` cannot use `deny_unknown_fields` — the action is flattened into the
/// envelope, and serde will not combine the two — so the rule is enforced by
/// comparing the stored bytes against this build's own canonical serialization
/// of the record it decoded to. A dropped member changes those bytes, which is
/// the stricter check and not the weaker one.
#[test]
fn an_event_carrying_undefined_members_is_refused() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "event read boundary");
    admit(&root, payload(Act::Add, &id, "wording"));
    let path = store::events_path(&root, &id);
    let good = std::fs::read_to_string(&path).expect("events");

    for injected in [
        serde_json::json!({ "admission": { "kind": "agent" } }),
        serde_json::json!({ "object": engr::model::new_id() }),
        serde_json::json!({ "payload_sha256": format!("1:{}", "0".repeat(64)) }),
    ] {
        let mut lines = Vec::new();
        for line in good.lines() {
            let mut event: serde_json::Value = serde_json::from_str(line).expect("event");
            for (key, value) in injected.as_object().expect("an object") {
                event
                    .as_object_mut()
                    .expect("event")
                    .insert(key.clone(), value.clone());
            }
            lines.push(engr::proof::canonical_bytes(&event, "event").expect("canonical"));
        }
        std::fs::write(&path, format!("{}\n", lines.join("\n"))).expect("write");

        let error = store::load_events(&root, &id)
            .expect_err("a record claiming more than this generation defines");
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{injected}");
    }

    std::fs::write(&path, &good).expect("restore");
    store::load_events(&root, &id).expect("and the emitted records read back");
}

/// The durable Event write path is part of the workspace-generation boundary,
/// and a direct library caller reaches it without going through the gate.
#[test]
fn appending_an_event_requires_a_workspace_this_build_may_write() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "append boundary");
    let path = store::events_path(&root, &id);
    let before = std::fs::read(&path).expect("events");
    let event = admissible_human_event(&root, &id, payload(Act::Add, &id, "wording"));

    for generation in ["99\n", "1", ""] {
        std::fs::write(store::version_path(&root), generation).expect("VERSION");
        let error = store::check_appendable(&root, &event)
            .expect_err("this build does not write a workspace at that generation");
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{generation:?}");
        assert_eq!(
            std::fs::read(&path).expect("events"),
            before,
            "{generation:?}: and the store is byte-for-byte what it was"
        );
    }

    std::fs::write(store::version_path(&root), engr::WORKSPACE_VERSION_FILE).expect("restore");
    store::check_appendable(&root, &event).expect("and this generation may write it");
}

/// An absent basis is an absent member. `"based_on": null` is a second spelling
/// of the same fact, and a record is its exact canonical shape or it is not a
/// record this generation reads.
#[test]
fn current_events_reject_a_noncanonical_explicit_absent_basis() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "canonical omission");
    admit(&root, payload(Act::Add, &id, "wording"));
    let path = store::events_path(&root, &id);
    let good = std::fs::read_to_string(&path).expect("events");

    let mut lines = Vec::new();
    for line in good.lines() {
        let mut event: serde_json::Value = serde_json::from_str(line).expect("event");
        if let Some(value) = event.pointer_mut("/data/value") {
            let value = value.as_object_mut().expect("a section value");
            assert!(
                !value.contains_key("based_on"),
                "this build omits it, which is what makes the null spelling a second one"
            );
            value.insert("based_on".to_owned(), serde_json::Value::Null);
        }
        lines.push(engr::proof::canonical_bytes(&event, "event").expect("canonical"));
    }
    std::fs::write(&path, format!("{}\n", lines.join("\n"))).expect("write");

    let error = store::load_events(&root, &id).expect_err("an Event has one exact shape");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
}

/// An empty array or object is omitted, never written. A record that spells one
/// out is a second encoding of the same fact, and the digest is taken over one.
#[test]
fn an_empty_member_written_out_is_not_a_second_spelling() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "canonical omission");
    admit(&root, payload(Act::Add, &id, "wording"));
    let path = store::events_path(&root, &id);
    let good = std::fs::read_to_string(&path).expect("events");
    let line = good.lines().last().expect("the section record").to_owned();
    assert!(
        !line.contains(r#""refs""#),
        "the writer omits an empty set: {line}"
    );

    let mut event: serde_json::Value = serde_json::from_str(&line).expect("event");
    event["data"]["value"]["refs"] = serde_json::json!([]);
    let written = engr::proof::canonical_bytes(&event, "event").expect("canonical");
    std::fs::write(
        &path,
        format!("{}{written}\n", &good[..good.len() - line.len() - 1]),
    )
    .expect("write");

    let error = store::load_events(&root, &id).expect_err("an empty member is omitted");
    assert_eq!(error.code, engr::EXIT_SCHEMA);

    std::fs::write(&path, &good).expect("restore");
    store::load_events(&root, &id).expect("and the emitted records read back");
}

/// The durable Event write path must not accept a record its own next read
/// refuses. The workspace generation is only part of that contract: the rest of
/// what `load_events` demands has to be demanded here too, or a direct library
/// caller writes history nothing can load.
#[test]
fn appending_an_event_enforces_the_contract_its_own_read_applies() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "append contract");
    let path = store::events_path(&root, &id);
    let before = std::fs::read(&path).expect("events");

    let sound = admissible_human_event(&root, &id, payload(Act::Add, &id, "wording"));

    // Each variant is *correctly sealed* for exactly what it says, so what
    // refuses it is the rule it breaks rather than an arithmetic slip. The one
    // exception is the broken seal itself, and its refusal is the interesting
    // one: the owning Object is recovered from the seal, so a record whose
    // digest describes nothing names no stream to be appended to.
    let mut broken_seal = sound.clone();
    broken_seal.digest = format!("1:{}", "0".repeat(64));
    let unconfirmed = engr::model::Event::sealed(
        &id,
        sound.id.clone(),
        sound.action.clone(),
        sound.rev,
        engr::model::EventAdmission {
            by: engr::semantics::Admission::Human,
            at: sound.metadata.admitted.at.clone(),
            confirmation: None,
            review: None,
        },
    )
    .expect("sealed over what it says");
    let wrong_rev = engr::model::Event::sealed(
        &id,
        sound.id.clone(),
        sound.action.clone(),
        9,
        sound.metadata.admitted.clone(),
    )
    .expect("sealed over what it says");
    let wrong_id = engr::model::Event::sealed(
        &id,
        "not-a-uuid".to_owned(),
        sound.action.clone(),
        sound.rev,
        sound.metadata.admitted.clone(),
    )
    .expect("sealed over what it says");

    for (what, code, event) in [
        ("its own seal", engr::EXIT_NOT_FOUND, broken_seal),
        ("human confirmation", engr::EXIT_SCHEMA, unconfirmed),
        ("revision continuity", engr::EXIT_SCHEMA, wrong_rev),
        ("event identity", engr::EXIT_SCHEMA, wrong_id),
    ] {
        let error = store::check_appendable(&root, &event).unwrap_err_or_else_note(what);
        assert_eq!(error.code, code, "{what}: {error}");
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
    let prepared =
        gate::prepare(&root, payload(Act::Add, &id, "wording")).expect("one prepared candidate");
    let response = format!("CONFIRM {}", prepared.candidate.code());
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
/// this history can arrive at. The EventStore is append-only and is never
/// purged, so a record that cannot be replayed is not a mistake somebody can
/// take back — it durably poisons every read that reconstructs the Object.
#[test]
fn the_append_path_refuses_a_record_the_reducer_could_not_replay() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "replayability");
    let path = store::events_path(&root, &id);
    let before = std::fs::read(&path).expect("events");

    // Structurally valid, contiguous, correctly sealed — and it revises a
    // Section that does not exist.
    let absent = payload(Act::Revise(999), &id, "wording");
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
/// be contiguous with — so without this an empty history would accept a
/// beginning it can never replay.
///
/// Asked at the gate, because that is the door: an Object with no history has no
/// stored file either, and the read-only boundary recovers the owning Object
/// from the seal — so a record for an Object nothing knows about is refused one
/// step earlier, for naming no stream at all. Both refusals are here, because
/// both are the property.
#[test]
fn the_append_path_refuses_a_first_record_no_object_could_come_from() {
    let (_dir, root) = workspace();
    let id = engr::model::new_id();
    let path = store::events_path(&root, &id);

    let error = gate::prepare(&root, payload(Act::Add, &id, "wording"))
        .expect_err("an object begins with its creation");
    assert_ne!(error.code, 0);
    assert!(
        gate::pending(&root).expect("candidates").is_empty(),
        "no candidate was minted for a history that cannot start there"
    );
    assert!(!path.exists(), "and no history was created");

    // And the same record offered straight to the durable boundary: the owning
    // Object is recovered from the seal, and no Object answers for this one.
    let error = store::check_appendable(
        &root,
        &unadmitted_human_event(&id, payload(Act::Add, &id, "wording"), 1),
    )
    .expect_err("a record naming no stream is not appendable to one");
    assert_eq!(error.code, engr::EXIT_NOT_FOUND);
    assert!(!path.exists(), "and still no history was created");
}

/// A well-formed Event is not an admitted one.
///
/// Every shape check can pass on a record a caller assembled: the schema is
/// exact, the revision follows, the reducer can replay it, and the seal is
/// perfectly computed over it. None of that says a person was shown anything.
/// Without a proof at this boundary, a direct library caller appends the record
/// and lets recovery project it into current authority — which is the whole
/// admission model, bypassed by the one door that writes durable history.
#[test]
fn a_direct_caller_cannot_append_a_human_event_nobody_was_shown() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "forged human provenance");
    let path = store::events_path(&root, &id);
    let before = std::fs::read(&path).expect("events");

    // Prepared for real, then the challenge swapped for one that was never
    // minted. Everything else about the record is exactly right, seal included.
    let admissible = admissible_human_event(&root, &id, payload(Act::Add, &id, "wording"));
    let forged = engr::model::Event::sealed(
        &id,
        admissible.id.clone(),
        admissible.action.clone(),
        admissible.rev,
        engr::model::EventAdmission::human(&admissible.metadata.admitted.at, "ZZZZZZ"),
    )
    .expect("a perfectly sealed forgery");
    let error = store::check_appendable(&root, &forged)
        .expect_err("a challenge nobody minted admits nothing");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(
        error.message.contains("not admitted through the gate"),
        "{error}"
    );

    // And a code that was minted, for a transition it does not describe.
    let elsewhere = gate::prepare(&root, payload(Act::Add, &id, "some other wording"))
        .expect("a second live challenge");
    let mismatched = engr::model::Event::sealed(
        &id,
        admissible.id.clone(),
        admissible.action.clone(),
        admissible.rev,
        engr::model::EventAdmission::human(
            &admissible.metadata.admitted.at,
            elsewhere.candidate.code(),
        ),
    )
    .expect("sealed against a real code");
    let error = store::check_appendable(&root, &mismatched)
        .expect_err("a code for another transition admits nothing");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(
        error.message.contains("does not describe the transition"),
        "{error}"
    );

    assert_eq!(
        std::fs::read(&path).expect("events"),
        before,
        "neither forgery was written"
    );
}

/// The same, for the Agent door.
///
/// The envelope reads a ReviewDigest for spelling. It cannot tell an attestation
/// made against a Rule set somebody read from sixty-four invented hex
/// characters, so the digest is recomputed here against the live applicable
/// Rules for exactly this mutation.
#[test]
fn a_direct_caller_cannot_append_an_agent_event_no_rule_review_produced() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "forged agent provenance");
    let path = store::events_path(&root, &id);
    let before = std::fs::read(&path).expect("events");

    let action = common::agent_payload(Act::Add, &id, content("admitted by nobody")).action;
    let forged = engr::model::Event::sealed(
        &id,
        engr::model::new_id(),
        action.clone(),
        2,
        engr::model::EventAdmission {
            by: engr::semantics::Admission::Agent,
            at: "2026-08-25T00:00:00Z".to_owned(),
            confirmation: None,
            review: Some(engr::model::ReviewProvenance {
                outcome: engr::model::ReviewOutcome::Passed,
                digest: format!("1:{}", "c".repeat(64)),
            }),
        },
    )
    .expect("a well-formed record");
    let error = store::check_appendable(&root, &forged)
        .expect_err("an invented review digest admits nothing");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(
        error.message.contains("Rule Review that is not the one"),
        "{error}"
    );

    // And with no review at all: semantic Agent admission needs an applicable
    // usable Object Rule, and a title is the sole exception.
    let bare = engr::model::Event::sealed(
        &id,
        engr::model::new_id(),
        action,
        2,
        engr::model::EventAdmission {
            by: engr::semantics::Admission::Agent,
            at: "2026-08-25T00:00:00Z".to_owned(),
            confirmation: None,
            review: None,
        },
    )
    .expect("a well-formed record");
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

/// A persisted Event record *is* its canonical bytes, not merely a value that
/// parses to the right thing.
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
/// reported. The lower bound belongs to the record contract, which is why it is
/// checked on a record whose seal is perfectly correct for `rev = 0`.
#[test]
fn event_revisions_start_at_one() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "revision domain");
    let path = store::events_path(&root, &id);
    let original = std::fs::read_to_string(&path).expect("events");
    let first: engr::model::Event =
        serde_json::from_str(original.lines().next().expect("record")).expect("event");

    let zero = engr::model::Event::sealed(
        &id,
        first.id.clone(),
        first.action.clone(),
        0,
        first.metadata.admitted.clone(),
    )
    .expect("a record sealed for revision zero");
    let record = engr::proof::canonical_bytes(&zero, "event").expect("canonical");
    std::fs::write(&path, format!("{record}\n{original}")).expect("prefix with rev 0");

    let error = store::load_events(&root, &id).expect_err("there is no revision zero");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("counted from 1"), "{error}");
}

/// The Event file is the complete audit trail, not merely a suffix that can be
/// replayed over the current projection. Losing its first records must not be
/// blessed by a later append or reconciliation.
#[test]
fn a_purged_event_prefix_is_refused() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "complete history");
    admit(&root, payload(Act::Add, &id, "second record"));

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

/// A historical Object is its canonical JCS bytes, like every other persisted
/// resource. A snapshot that merely parses to the right value is a second
/// encoding of the material a Ref's digest was taken over.
#[test]
fn a_historical_object_must_be_jcs() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "historical format");
    let path = store::object_path(&root, &id);
    let canonical = std::fs::read_to_string(&path).expect("canonical object");
    let parsed: serde_json::Value = serde_json::from_str(&canonical).expect("json");
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&parsed).expect("pretty"),
    )
    .expect("noncanonical snapshot");
    let commit = commit_all(&root, "noncanonical historical object");
    std::fs::write(&path, &canonical).expect("restore working object");

    let error = engr::git::object_at(&root, &commit, &id)
        .expect_err("a historical Object carries its canonical spelling");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("canonical JCS bytes"), "{error}");
}

/// A transition whose numbers no identity can carry is refused before anything
/// is minted.
///
/// An Object at the shared ceiling is itself entirely valid; what is not
/// representable is the transition out of it. Some of those numbers appear
/// nowhere in the payload — the reducer allocates them — so a per-payload check
/// passes and a person would be holding a code for a mutation that can never be
/// admitted.
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
        let resealed = engr::integrity::mutate(&object, |object| {
            at_ceiling(object, ceiling);
            Ok(())
        })
        .expect("reseal at the ceiling");
        save_raw(&root, &resealed.object).expect("put it on disk");

        let events = std::fs::read(store::events_path(&root, &id)).expect("events");
        let error = gate::prepare(&root, payload(Act::Add, &id, "one more"))
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

/// Two Challenges can name the same transition and are still two questions.
///
/// An older envelope restored from a backup after a later one was applied
/// describes exactly the mutation that landed. Reporting it as that mutation's
/// idempotent retry would say a person answered for something they never saw,
/// so the record proves confirmation of *its own* code and this one is stale.
#[test]
fn an_identical_candidate_with_another_challenge_is_not_an_applied_retry() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "two envelopes");
    let proposal = || payload(Act::Add, &id, "the same wording");

    let first = gate::prepare(&root, proposal()).expect("first envelope");
    let first_path = store::challenge_path(&root, first.candidate.code()).expect("path");
    let kept = std::fs::read_to_string(&first_path).expect("keep the envelope");

    // A second proposal for the same mutation supersedes it: same transition,
    // same frozen subject, a different question.
    let second = gate::prepare(&root, proposal()).expect("second envelope");
    assert_eq!(
        first.candidate.subject.action, second.candidate.subject.action,
        "the same transition"
    );
    assert_eq!(
        first.candidate.subject.expected_rev,
        second.candidate.subject.expected_rev
    );
    assert_ne!(first.candidate.code(), second.candidate.code());
    assert_ne!(
        first.candidate.challenge.digest, second.candidate.challenge.digest,
        "two questions are never one digest"
    );

    gate::confirm(&root, &format!("CONFIRM {}", second.candidate.code())).expect("admit B");

    // A restores from a backup. It is stale, and it is not B's retry.
    std::fs::write(&first_path, &kept).expect("restore the older envelope");
    let restored = gate::find(&root, first.candidate.code()).expect("it still reads");
    match gate::candidate_state(&root, &restored).expect("classify") {
        gate::CandidateState::Stale { .. } => {}
        other => panic!("an envelope nobody answered is not applied: {other:?}"),
    }
    let error = gate::confirm(&root, &format!("CONFIRM {}", first.candidate.code()))
        .expect_err("and confirming it admits nothing");
    assert_eq!(error.code, engr::EXIT_STALE);
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
        admit(&root, payload(Act::Add, &id, &format!("section {n}")));
    }

    let proposal = payload(
        Act::Merge {
            destination: 1,
            sources: vec![2, 10],
        },
        &id,
        "one point",
    );
    let prepared = gate::prepare(&root, proposal).expect("prepare the merge");
    let engr::model::Action::SectionMerged { merge, .. } = &prepared.candidate.payload.action
    else {
        panic!("a merge");
    };
    assert_eq!(
        merge.consumed(),
        &[10, 2],
        "canonical set order is over JCS bytes, not over the numbers"
    );

    let object = gate::confirm(&root, &format!("CONFIRM {}", prepared.candidate.code()))
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
    let engr::model::Action::SectionMerged { merge, .. } = &event.action else {
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
    admit(&root, payload(Act::Add, &id, "depended upon"));
    let commit = commit_all(&root, "record");

    // Historical resolution: the workspace prefix has to be found for this to
    // read at all.
    let engr::git::HistoricalObject::Current(historical) =
        engr::git::object_at(&root, &commit, &id)
            .expect("the historical workspace resolves")
            .expect("and holds the object")
    else {
        panic!("a snapshot of this generation decodes as this generation");
    };
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
    let mut proposal = payload(Act::Add, &source, "stands on it");
    edit(&mut proposal, |content| {
        content.refs = vec![text_ref(&root, &id, 1, &commit)]
    });
    gate::prepare(&root, proposal).expect("a reference resolves through the link");
}

/// Put an admitted Event in the log without going through any write path.
///
/// There is no public append — Event provenance is deliberately too thin to
/// prove admission from, so the durable write lives behind the gate. What this
/// reproduces is the state a crash leaves: the record is durable and the
/// projection is not, which is exactly what recovery has to cope with. Written
/// as the canonical JCS bytes, because that is what the read boundary requires.
fn append_admitted_raw(root: &Path, object: &str, event: &engr::model::Event) {
    let path = store::events_path(root, object);
    let line = engr::proof::canonical_bytes(event, "Event").expect("canonical");
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
    let renamed = common::agent_payload(Act::Rename, &id, content("a different title"));
    let bare = agent_event_without_review(&id, &renamed, 2);

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

fn agent_event_without_review(object: &str, payload: &Payload, rev: u64) -> engr::model::Event {
    engr::model::Event::sealed(
        object,
        engr::model::new_id(),
        payload.action.clone(),
        rev,
        engr::model::EventAdmission {
            by: engr::semantics::Admission::Agent,
            at: "2026-08-27T00:00:00Z".to_owned(),
            confirmation: None,
            review: None,
        },
    )
    .expect("a well-formed record, whatever admitted it")
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
        common::agent_payload(Act::Add, &id, content("wording")),
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
