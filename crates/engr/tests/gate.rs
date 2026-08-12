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
fn the_counter_survives_a_purge() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "purge and ids");
    admit(&root, payload(Action::SectionAdded, &id, "first"));
    admit(&root, empty(Action::SectionDeleted { section: 1 }, &id));

    ops::purge(&root, &id).expect("purge");
    assert!(store::load_events(&root, &id).expect("events").is_empty());

    let object = admit(&root, payload(Action::SectionAdded, &id, "after the purge"));
    assert_eq!(
        object.sections[0].id, 2,
        "a purge must not reset the id counter"
    );
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
    assert_eq!(object.status, Status::Closed);

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
        .challenge;
    let second =
        gate::prepare(&root, payload(Action::SectionAdded, &id, "second draft")).expect("second");
    assert_eq!(second.superseded, vec![first.clone()]);
    assert!(
        gate::find(&root, &first).is_err(),
        "a human should never hold two codes for the same object"
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

    let mut with_missing_object = payload(Action::SectionAdded, &source, "depends");
    with_missing_object.content.refs = vec![Ref {
        object: engr::model::new_id(),
        section: 1,
        sha256: pinned.clone(),
        commit: "0".repeat(40),
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
        commit: "0".repeat(40),
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
        commit: "0".repeat(40),
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
        commit: "0".repeat(40),
    }];
    gate::prepare(&root, good).expect("a well-formed reference is admitted");
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
    store::write_json(&store::candidate_path(&root, &code), &prepared.candidate).expect("rewrite");

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
        version: engr::FORMAT_VERSION,
        event_id: engr::model::new_id(),
        rev: first.candidate.expected_rev + 1,
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
    assert_eq!(second.candidate.expected_rev, 2);
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
        version: engr::FORMAT_VERSION,
        event_id: engr::model::new_id(),
        rev: prepared.candidate.expected_rev + 1,
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
        version: engr::FORMAT_VERSION,
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

    let code = prepared.candidate.challenge;
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

#[test]
fn purge_keeps_the_event_needed_to_finish_crash_recovery() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "purge recovery");
    let prepared =
        gate::prepare(&root, payload(Action::SectionAdded, &id, "once")).expect("prepare");
    let code = prepared.candidate.challenge.clone();
    let response = format!("CONFIRM {code}");
    gate::confirm(&root, &response).expect("confirm");

    // Stand in for a crash after saving the projection but before clearing the
    // candidate. The matching event is the evidence confirm needs to recognize
    // a retry as idempotent rather than applying it again.
    store::write_json(&store::candidate_path(&root, &code), &prepared.candidate).expect("rewrite");

    let error =
        ops::purge(&root, &id).expect_err("purge must retain an event needed for crash recovery");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(
        !store::load_events(&root, &id).expect("events").is_empty(),
        "the recovery evidence must still be present"
    );

    let (_, object) = gate::confirm(&root, &response).expect("idempotent recovery");
    assert_eq!(object.rev, 2, "the event must not be applied a second time");
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
    let code = &prepared.candidate.challenge;

    assert!(ignored(&root, ".engr/lock"));
    assert!(ignored(&root, &format!(".engr/candidates/{code}.json")));

    assert!(!ignored(&root, ".engr/format.json"));
    assert!(!ignored(&root, &format!(".engr/objects/{id}.json")));
    assert!(!ignored(&root, &format!(".engr/events/{id}.jsonl")));
}
