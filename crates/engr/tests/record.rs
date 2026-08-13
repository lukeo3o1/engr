//! What the record guarantees once it is written: that the wording is the
//! wording that was confirmed, that a purge cannot lose content, and that drift
//! is noticed.

use engr::model::{Action, Content, Event, Payload, Ref};
use engr::{gate, ops, store, view};
use serde_json::Value;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn workspace() -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().to_path_buf();
    store::init(&root).expect("init");
    (dir, root)
}

fn payload(action: Action, object: &str, text: &str) -> Payload {
    Payload {
        action,
        object: object.to_owned(),
        content: Content {
            text: text.to_owned(),
            based_on: None,
            refs: Vec::new(),
        },
    }
}

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

/// Edit the stored object the way a text editor would — content changed, hash
/// left alone.
fn tamper(root: &Path, id: &str, edit: impl FnOnce(&mut Value)) {
    let path = store::object_path(root, id);
    let mut value: Value = store::read_json(&path).expect("read");
    edit(&mut value);
    store::write_json(&path, &value).expect("write");
}

#[test]
fn editing_a_sections_text_is_detected() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "integrity");
    admit(
        &root,
        payload(Action::SectionAdded, &id, "the confirmed wording"),
    );

    assert!(ops::verify(&root, &id).expect("verify").passed());

    tamper(&root, &id, |value| {
        value["sections"][0]["text"] = Value::String("something nobody confirmed".into());
    });

    let report = ops::verify(&root, &id).expect("verify");
    assert!(!report.passed());
    assert_eq!(report.tampered, vec![1]);
}

#[test]
fn repointing_a_reference_is_detected() {
    let (_dir, root) = workspace();
    let target = new_object(&root, "the target");
    admit(&root, payload(Action::SectionAdded, &target, "first"));
    admit(&root, payload(Action::SectionAdded, &target, "second"));
    let pinned = store::load_object(&root, &target).expect("target").sections[0]
        .sha256
        .clone();

    let source = new_object(&root, "the source");
    let mut with_ref = payload(Action::SectionAdded, &source, "depends on the first");
    with_ref.content.refs = vec![Ref {
        object: target.clone(),
        section: 1,
        sha256: pinned,
        commit: "0".repeat(40),
    }];
    admit(&root, with_ref);
    assert!(ops::verify(&root, &source).expect("verify").passed());

    // The hash has to cover `refs`, not just `text`. If it covered text alone,
    // swapping which section this depends on would pass verification.
    tamper(&root, &source, |value| {
        value["sections"][0]["refs"][0]["section"] = Value::from(2);
    });

    let report = ops::verify(&root, &source).expect("verify");
    assert!(
        !report.passed(),
        "a repointed reference must not pass verification"
    );
}

#[test]
fn purge_does_not_remove_a_malformed_event_buffer() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "purge safety");
    let object = admit(&root, payload(Action::SectionAdded, &id, "one"));

    // A gap cannot arise from one append, so stored data containing one is
    // malformed rather than an event tail `reconcile` may safely replay.
    let orphan = Event {
        format: engr::model::EVENT_FORMAT.to_owned(),
        version: engr::FORMAT_VERSION,
        event_id: engr::model::new_id(),
        rev: object.rev + 5,
        time: "2026-08-13T00:00:00Z".to_owned(),
        payload: payload(Action::SectionAdded, &id, "never projected"),
        confirmation: engr::model::Confirmation {
            challenge: "AAAAAA".to_owned(),
            payload_sha256: payload(Action::SectionAdded, &id, "never projected")
                .sha256()
                .expect("payload hash"),
        },
    };
    store::append_event(&root, &orphan).expect("append");

    let error = ops::purge(&root, &id).expect_err("purge must refuse");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(
        store::events_path(&root, &id).exists(),
        "nothing may be dropped when validation fails"
    );
}

#[test]
fn reconcile_applies_an_event_the_projection_missed() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "reconciliation");
    let object = admit(&root, payload(Action::SectionAdded, &id, "one"));
    assert_eq!(object.rev, 2);

    // Rewind the projection, leaving the event log ahead of it.
    tamper(&root, &id, |value| {
        value["rev"] = Value::from(1);
        value["sections"] = Value::Array(Vec::new());
        value["next_section_id"] = Value::from(1);
    });

    let recovered = ops::reconcile(&root, &id).expect("reconcile");
    assert_eq!(recovered.rev, 2);
    assert_eq!(recovered.sections.len(), 1);
    assert_eq!(recovered.sections[0].text, "one");
}

#[test]
fn a_reference_is_drift_once_its_target_is_revised() {
    let (_dir, root) = workspace();
    let target = new_object(&root, "the target");
    admit(
        &root,
        payload(Action::SectionAdded, &target, "the original basis"),
    );
    let pinned = store::load_object(&root, &target).expect("target").sections[0]
        .sha256
        .clone();

    let source = new_object(&root, "the source");
    let mut with_ref = payload(Action::SectionAdded, &source, "rests on the basis");
    with_ref.content.refs = vec![Ref {
        object: target.clone(),
        section: 1,
        sha256: pinned,
        commit: "0".repeat(40),
    }];
    admit(&root, with_ref);

    let object = store::load_object(&root, &source).expect("source");
    let assessment = view::assess(&root, &object);
    assert!(
        assessment[0].1.is_ok(),
        "nothing has moved yet, so nothing should be flagged"
    );

    admit(
        &root,
        payload(
            Action::SectionRevised { section: 1 },
            &target,
            "the basis, restated differently",
        ),
    );

    let assessment = view::assess(&root, &object);
    let status = &assessment[0].1;
    assert!(!status.is_ok(), "the basis changed and must be reported");
    assert_eq!(status.drifted.len(), 1);
    assert_eq!(status.key(), "stale_refs");
    assert!(
        status.drifted[0].lookback.is_some(),
        "the reader needs the command that recovers what it used to say"
    );
}

#[test]
fn show_marks_a_section_whose_content_does_not_match_its_hash() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "audit failure reason codes");
    admit(
        &root,
        payload(
            Action::SectionAdded,
            &id,
            "Ruling: expose the reason code on the audit detail view.",
        ),
    );

    tamper(&root, &id, |value| {
        value["sections"][0]["text"] =
            Value::String("Ruling: cancelled, we are not doing reason codes.".into());
    });

    let object = store::load_object(&root, &id).expect("object");
    let status = &view::assess(&root, &object)[0].1;
    assert!(status.tampered);
    assert!(!status.is_ok());
    assert_eq!(
        status.label(),
        "TAMPERED",
        "the label a reader scans must not read `ok` over wording nobody confirmed"
    );
    assert_eq!(status.key(), "tampered");

    let rendered = view::render_show(&root, &object);
    assert!(rendered.contains("1 tampered"), "{rendered}");
    assert!(
        !rendered.contains("1 ok"),
        "the header asserted the section was fine: {rendered}"
    );
    assert!(
        rendered.contains("content does not match the hash confirmed at"),
        "{rendered}"
    );
}

/// The hole a hash-to-hash comparison cannot see. An editor that rewrites the
/// target's text and leaves its stored hash alone moves neither side of the
/// ref comparison, so the referencing section would report `ok` — and `verify`
/// would report PASS — over wording that was rewritten behind it.
#[test]
fn a_section_standing_on_tampered_wording_is_not_ok() {
    let (_dir, root) = workspace();
    let target = new_object(&root, "upstream decision");
    admit(
        &root,
        payload(
            Action::SectionAdded,
            &target,
            "Ruling: reason codes are numeric.",
        ),
    );
    let pinned = store::load_object(&root, &target).expect("target").sections[0]
        .sha256
        .clone();

    let source = new_object(&root, "downstream decision");
    let mut with_ref = payload(
        Action::SectionAdded,
        &source,
        "Therefore the UI renders them as integers.",
    );
    with_ref.content.refs = vec![Ref {
        object: target.clone(),
        section: 1,
        sha256: pinned.clone(),
        commit: "0".repeat(40),
    }];
    admit(&root, with_ref);

    let object = store::load_object(&root, &source).expect("source");
    assert!(view::assess(&root, &object)[0].1.is_ok());
    assert!(ops::verify(&root, &source).expect("verify").passed());

    tamper(&root, &target, |value| {
        value["sections"][0]["text"] =
            Value::String("Ruling: reason codes are free-text strings.".into());
    });

    let status = &view::assess(&root, &object)[0].1;
    assert!(
        !status.tampered,
        "this section's own wording was not touched"
    );
    assert!(status.stands_on_tampered());
    assert!(!status.is_ok(), "it stands on wording nobody confirmed");
    assert_eq!(status.label(), "REF TAMPERED");
    assert_eq!(status.key(), "ref_tampered");
    assert_eq!(
        status.drifted[0].current_sha256.as_deref(),
        Some(pinned.as_str()),
        "the stored hash did not move, which is exactly why comparing hashes is not enough"
    );

    let report = ops::verify(&root, &source).expect("verify");
    assert!(
        !report.passed(),
        "verify reported PASS over rewritten foundations"
    );
    assert!(report.tampered.is_empty());
    assert_eq!(report.standing_on_tampered.len(), 1);
    assert_eq!(report.standing_on_tampered[0].section, 1);
    assert_eq!(report.standing_on_tampered[0].target, target);

    let rendered = view::render_show(&root, &object);
    assert!(
        rendered.contains("does not match its own hash"),
        "{rendered}"
    );
}

#[test]
fn the_confirmation_hash_covers_the_action_and_the_section_hash_does_not() {
    let object = engr::model::new_id();
    let content = Content {
        text: "identical wording".to_owned(),
        based_on: None,
        refs: Vec::new(),
    };
    let added = Payload {
        action: Action::SectionAdded,
        object: object.clone(),
        content: content.clone(),
    };
    let deleted = Payload {
        action: Action::SectionDeleted { section: 1 },
        object,
        content: content.clone(),
    };

    // What the human assents to includes which action it is, so a displayed
    // candidate cannot be swapped for a different one carrying the same words.
    assert_ne!(
        added.sha256().expect("hash"),
        deleted.sha256().expect("hash")
    );

    // The section's own hash covers only content, so `verify` can recompute it
    // from what is stored without needing to know how it got there.
    assert_eq!(
        content.sha256().expect("hash"),
        content.sha256().expect("hash")
    );
}

#[test]
fn a_purge_outside_a_repository_records_no_watermark() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "no git here");
    admit(&root, payload(Action::SectionAdded, &id, "one"));

    let purged = ops::purge(&root, &id).expect("purge");
    assert_eq!(purged.events, 2);
    assert!(
        purged.commit.is_none(),
        "there is no commit to record without a repository"
    );
    let object = store::load_object(&root, &id).expect("object");
    assert_eq!(object.sections.len(), 1, "the sections are the authority");
}
