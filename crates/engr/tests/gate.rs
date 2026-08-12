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
