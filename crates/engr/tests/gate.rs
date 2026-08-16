//! The gate is the only way in. These tests pin that.

use engr::model::{Action, Content, Payload, Ref, Status};
use engr::{gate, ops, store};
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
    }
}

fn payload(action: Action, object: &str, text: &str) -> Payload {
    Payload {
        action,
        object: object.to_owned(),
        content: content(text),
    }
}

fn empty(action: Action, object: &str) -> Payload {
    Payload {
        action,
        object: object.to_owned(),
        content: Content {
            text: String::new(),
            based_on: None,
            refs: Vec::new(),
        },
    }
}

/// Prepare, then confirm with the exact phrase.
fn admit(root: &Path, payload: Payload) -> engr::model::Object {
    let prepared = gate::prepare(root, payload).expect("prepare");
    let response = format!("CONFIRM {}", prepared.candidate.challenge);
    gate::confirm(root, &response).expect("confirm").1
}

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
        .1;
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

#[test]
fn direct_confirmation_refuses_a_legacy_workspace_without_partial_mutation() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "legacy boundary");
    let prepared = gate::prepare(&root, empty(Action::ObjectClosed, &id)).expect("prepare");
    let code = prepared.candidate.challenge.clone();
    let object_path = store::object_path(&root, &id);
    let candidate_path = store::candidate_path(&root, &code).expect("candidate path");
    let events_path = store::events_path(&root, &id);

    let mut object: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&object_path).expect("object")).expect("json");
    object["format"] = serde_json::Value::String("engr-object".to_owned());
    object["version"] = serde_json::Value::from(1);
    let state = object
        .as_object_mut()
        .expect("object")
        .remove("state")
        .expect("state");
    object["status"] = state;
    std::fs::write(
        &object_path,
        serde_json::to_vec_pretty(&object).expect("json"),
    )
    .expect("legacy object");

    let object_before = std::fs::read(&object_path).expect("object snapshot");
    let events_before = std::fs::read(&events_path).expect("events snapshot");
    let candidate_before = std::fs::read(&candidate_path).expect("candidate snapshot");
    for response in [
        format!("CONFIRM {code} but leave it open"),
        format!("CONFIRM {code}"),
    ] {
        let error = gate::confirm(&root, &response).expect_err("legacy confirmation is refused");
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
fn merging_produces_a_new_id_and_removes_what_it_absorbed() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "merging");
    admit(&root, payload(Action::SectionAdded, &id, "one"));
    admit(&root, payload(Action::SectionAdded, &id, "two"));
    let object = admit(
        &root,
        payload(
            Action::SectionMerged {
                absorbs: vec![1, 2],
            },
            &id,
            "one and two together",
        ),
    );
    let ids: Vec<u64> = object.sections.iter().map(|section| section.id).collect();
    assert_eq!(ids, vec![3]);
    assert_eq!(object.sections[0].text, "one and two together");
}

#[test]
fn a_closed_object_refuses_section_changes() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "closing");
    admit(&root, payload(Action::SectionAdded, &id, "one"));
    let object = admit(&root, empty(Action::ObjectClosed, &id));
    assert_eq!(object.state, Status::Closed);

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
    invalid_ref.content.refs = vec![Ref {
        object: "not-a-uuid".to_owned(),
        section: 1,
        sha256: "0".repeat(64),
        commit: "HEAD".to_owned(),
    }];
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
    created.content.refs = vec![Ref {
        object: engr::model::new_id(),
        section: 1,
        sha256: "0".repeat(64),
        commit: "HEAD".to_owned(),
    }];
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
    let pinned = store::load_object(&root, &target_id)
        .expect("target")
        .section(1)
        .expect("target section")
        .sha256
        .clone();
    let commit = commit_all(&root, "record target wording");

    let mut direct = payload(
        Action::SectionAdded,
        &source.to_ascii_uppercase(),
        "dependent wording",
    );
    direct.content.based_on = Some("HEAD".to_owned());
    direct.content.refs = vec![Ref {
        object: target_id.to_ascii_uppercase(),
        section: 1,
        sha256: pinned,
        commit: commit[..12].to_owned(),
    }];

    let prepared = gate::prepare(&root, direct).expect("direct payload is canonicalized");
    assert_eq!(prepared.candidate.payload.object, source);
    assert_eq!(
        prepared.candidate.payload.content.based_on.as_deref(),
        Some(commit.as_str())
    );
    assert_eq!(prepared.candidate.payload.content.refs[0].object, target_id);
    assert_eq!(prepared.candidate.payload.content.refs[0].commit, commit);
    assert_eq!(
        prepared.candidate.payload_sha256,
        prepared
            .candidate
            .payload
            .sha256()
            .expect("canonical payload hash"),
        "the human's candidate hash covers the resolved values"
    );

    let response = format!("CONFIRM {}", prepared.candidate.challenge);
    let (event, _) = gate::confirm(&root, &response).expect("confirm canonical candidate");
    assert_eq!(
        event.payload.content.based_on.as_deref(),
        Some(commit.as_str())
    );
    assert_eq!(event.payload.content.refs[0].commit, commit);
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
    let pinned = store::load_object(&root, &target).expect("target").sections[0]
        .sha256
        .clone();
    let commit = commit_all(&root, "record target");

    let mut tampered = store::load_object(&root, &target).expect("target");
    "edited outside the gate".clone_into(&mut tampered.sections[0].text);
    store::save_object(&root, &tampered).expect("tamper target");
    let mut forged_current = payload(Action::SectionAdded, &source, "depends on forged wording");
    forged_current.content.refs = vec![Ref {
        object: target.clone(),
        section: 1,
        sha256: pinned.clone(),
        commit: commit.clone(),
    }];
    let error = gate::prepare(&root, forged_current)
        .expect_err("a reference cannot trust a stale stored target hash");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("current wording"));
    "depended upon".clone_into(&mut tampered.sections[0].text);
    store::save_object(&root, &tampered).expect("restore target");

    let revised = admit(
        &root,
        payload(
            Action::SectionRevised { section: 1 },
            &target,
            "new uncommitted wording",
        ),
    );
    let mut uncommitted = payload(Action::SectionAdded, &source, "depends on new wording");
    uncommitted.content.refs = vec![Ref {
        object: target.clone(),
        section: 1,
        sha256: revised.section(1).expect("section").sha256.clone(),
        commit: commit.clone(),
    }];
    let error = gate::prepare(&root, uncommitted)
        .expect_err("a commit cannot be paired with newer uncommitted wording");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("commit the target wording first"));

    admit(
        &root,
        payload(
            Action::SectionRevised { section: 1 },
            &target,
            "depended upon",
        ),
    );

    let mut with_missing_object = payload(Action::SectionAdded, &source, "depends");
    with_missing_object.content.refs = vec![Ref {
        object: engr::model::new_id(),
        section: 1,
        sha256: pinned.clone(),
        commit: commit.clone(),
    }];
    assert_eq!(
        gate::prepare(&root, with_missing_object)
            .expect_err("a reference to a missing object is refused")
            .code,
        engr::EXIT_NOT_FOUND
    );

    let mut with_missing_section = payload(Action::SectionAdded, &source, "depends");
    with_missing_section.content.refs = vec![Ref {
        object: target.clone(),
        section: 99,
        sha256: pinned.clone(),
        commit: commit.clone(),
    }];
    assert_eq!(
        gate::prepare(&root, with_missing_section)
            .expect_err("a reference to a missing section is refused")
            .code,
        engr::EXIT_NOT_FOUND
    );

    let mut with_wrong_hash = payload(Action::SectionAdded, &source, "depends");
    with_wrong_hash.content.refs = vec![Ref {
        object: target.clone(),
        section: 1,
        sha256: "0".repeat(64),
        commit: commit.clone(),
    }];
    assert_eq!(
        gate::prepare(&root, with_wrong_hash)
            .expect_err("a reference cannot pin something the target never said")
            .code,
        engr::EXIT_INVARIANT
    );

    let mut good = payload(Action::SectionAdded, &source, "depends");
    good.content.refs = vec![Ref {
        object: target,
        section: 1,
        sha256: pinned,
        commit,
    }];
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
        let mut object: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).expect("object")).expect("json");
        object["format"] = serde_json::Value::String("engr-object".to_owned());
        object["version"] = serde_json::Value::from(1);
        let state = object
            .as_object_mut()
            .expect("object")
            .remove("state")
            .expect("state");
        object["status"] = state;
        std::fs::write(path, serde_json::to_vec_pretty(&object).expect("json"))
            .expect("legacy object");
    }
    std::fs::remove_file(&format_path).expect("remove workspace authority");
    let legacy_commit = commit_all(&root, "legacy v0 historical snapshot");
    assert_eq!(
        engr::git::object_at(&root, &legacy_commit, &target)
            .expect("recognized legacy snapshot")
            .expect("legacy target")
            .section(1)
            .expect("legacy section")
            .sha256,
        pinned
    );

    for (path, bytes) in &objects_before {
        std::fs::write(path, bytes).expect("restore object");
    }
    std::fs::write(&format_path, &format_before).expect("restore workspace authority");
    let mut legacy_reference = payload(Action::SectionAdded, &source, "uses old wording");
    legacy_reference.content.refs = vec![Ref {
        object: target.clone(),
        section: 1,
        sha256: pinned.clone(),
        commit: legacy_commit,
    }];
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
    unsupported_reference.content.refs = vec![Ref {
        object: target,
        section: 1,
        sha256: pinned,
        commit: unsupported_commit,
    }];
    let error = gate::prepare(&root, unsupported_reference)
        .expect_err("a reference cannot decode an unsupported historical workspace");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("workspace version 99"));
}

#[test]
fn sibling_references_are_allowed_but_direct_self_reference_is_not() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "self reference");
    admit(&root, payload(Action::SectionAdded, &id, "the first"));
    let pinned = store::load_object(&root, &id).expect("object").sections[0]
        .sha256
        .clone();
    let commit = commit_all(&root, "record sibling");

    let mut inward = payload(Action::SectionAdded, &id, "the second");
    inward.content.refs = vec![Ref {
        object: id.clone(),
        section: 1,
        sha256: pinned,
        commit: commit.clone(),
    }];
    admit(&root, inward);

    let second = store::load_object(&root, &id).expect("object").sections[1]
        .sha256
        .clone();
    let commit = commit_all(&root, "record dependent sibling");
    let mut direct = payload(Action::SectionRevised { section: 2 }, &id, "self-dependent");
    direct.content.refs = vec![Ref {
        object: id,
        section: 2,
        sha256: second,
        commit,
    }];
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
    store::write_json(&path, &stored).expect("legacy candidate");

    let error = gate::confirm(&root, &format!("CONFIRM {}", prepared.candidate.challenge))
        .expect_err("semantic history is required");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("prepare it again"));
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
        prepared.candidate.previous_text.as_deref(),
        Some("audit failure reason codes"),
        "a rename shows the change, so the old title has to travel with it"
    );

    let object = gate::confirm(&root, &format!("CONFIRM {}", prepared.candidate.challenge))
        .expect("confirm")
        .1;
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
    store::write_json(
        &store::candidate_path(&root, &code).expect("candidate path"),
        &prepared.candidate,
    )
    .expect("rewrite");

    let (_, object) = gate::confirm(&root, &response).expect("recovery is idempotent");
    assert_eq!(object.rev, 2, "the event must not be applied a second time");
    assert_eq!(object.sections.len(), 1);
    assert!(gate::find(&root, &code).is_err());
}

#[test]
fn preparing_after_an_unprojected_event_keeps_the_confirmed_change() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "interrupted confirmation");
    let first = gate::prepare(&root, payload(Action::SectionAdded, &id, "first")).expect("first");
    let event = engr::model::Event {
        format: engr::model::EVENT_FORMAT.to_owned(),
        version: engr::EVENT_ENVELOPE_VERSION_V0,
        event_id: engr::model::new_id(),
        rev: first.candidate.binding.expected_rev + 1,
        time: "2026-08-13T00:00:00Z".to_owned(),
        payload: first.candidate.payload.clone(),
        confirmation: engr::model::Confirmation {
            challenge: first.candidate.challenge.clone(),
            payload_sha256: first.candidate.payload_sha256.clone(),
        },
    };
    store::append_event(&root, &event).expect("append before the crash");

    // A later action must first replay the confirmed tail. Otherwise it is
    // prepared at the same revision, its event collides with this one, and
    // purging can erase the first confirmed section.
    let second =
        gate::prepare(&root, payload(Action::SectionAdded, &id, "second")).expect("second");
    assert_eq!(second.candidate.binding.expected_rev, 2);
    let object = gate::confirm(&root, &format!("CONFIRM {}", second.candidate.challenge))
        .expect("confirm second")
        .1;
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
    let event = engr::model::Event {
        format: engr::model::EVENT_FORMAT.to_owned(),
        version: engr::EVENT_ENVELOPE_VERSION_V0,
        event_id: engr::model::new_id(),
        rev: prepared.candidate.binding.expected_rev + 1,
        time: "2026-08-13T00:00:00Z".to_owned(),
        payload: prepared.candidate.payload.clone(),
        confirmation: engr::model::Confirmation {
            challenge: code.clone(),
            payload_sha256: prepared.candidate.payload_sha256.clone(),
        },
    };
    store::append_event(&root, &event).expect("append before the crash");

    let (_, object) =
        gate::confirm(&root, &format!("CONFIRM {code}")).expect("the retry is idempotent");
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
    let event = engr::model::Event {
        format: engr::model::EVENT_FORMAT.to_owned(),
        version: engr::EVENT_ENVELOPE_VERSION_V0,
        event_id: engr::model::new_id(),
        rev: 1,
        time: "2026-08-13T00:00:00Z".to_owned(),
        payload: prepared.candidate.payload.clone(),
        confirmation: engr::model::Confirmation {
            challenge: prepared.candidate.challenge.clone(),
            payload_sha256: prepared.candidate.payload_sha256.clone(),
        },
    };
    store::append_event(&root, &event).expect("append before the crash");

    let code = prepared.candidate.challenge.clone();
    let (_, object) =
        gate::confirm(&root, &format!("CONFIRM {code}")).expect("the retry is idempotent");
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
