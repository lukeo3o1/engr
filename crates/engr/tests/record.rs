//! What the record guarantees once it is written: that the wording is the
//! wording that was confirmed, that confirmed history remains, and that drift
//! is noticed.

mod common;

use common::{admit, new_object, text_ref, workspace, Act};
use engr::model::{Content, Payload};
use engr::{gate, integrity, ops, store, view};
use serde_json::Value;
use std::path::Path;
/// Only the containment tests need one, and those are unix-only — a link is the
/// thing they are about, and Windows will not create one without a privilege
/// this build must not require.
#[cfg(unix)]
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
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .expect("git")
            .success());
    }
    engr::git::head(root).expect("HEAD")
}

fn payload(action: Act, object: &str, text: &str) -> Payload {
    common::payload(action, object, common::wording(text))
}

/// Replace the references a proposal carries, resealing the value it sits in.
fn set_refs(payload: &mut Payload, refs: Vec<engr::model::Ref>) {
    payload
        .action
        .value_mut()
        .expect("this action carries a section value")
        .content
        .refs = refs;
}

/// Edit the stored object the way a text editor would — content changed, hash
/// left alone.
fn tamper(root: &Path, id: &str, edit: impl FnOnce(&mut Value)) {
    let path = store::object_path(root, id);
    let mut value: Value = store::read_json(&path).expect("read");
    edit(&mut value);
    write_raw(&path, &value).expect("write");
}

#[test]
fn editing_a_sections_text_is_detected() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "integrity");
    admit(&root, payload(Act::Add, &id, "the confirmed wording"));

    assert!(ops::verify(&root, &id).expect("verify").passed());

    tamper(&root, &id, |value| {
        value["sections"][0]["text"] = Value::String("something nobody confirmed".into());
    });

    let report = ops::verify(&root, &id).expect("verify");
    assert!(!report.passed());
    assert_eq!(report.tampered, vec![1]);
}

#[test]
fn every_confirmed_revision_remains_in_append_only_history() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "durable history");
    admit(&root, payload(Act::Add, &id, "first wording"));
    admit(&root, payload(Act::Revise(1), &id, "second wording"));

    let events = store::load_events(&root, &id).expect("history");
    assert_eq!(events.len(), 3);
    let wording = |event: &engr::model::Event| {
        event
            .action
            .value()
            .expect("a section event carries its value")
            .content
            .text
            .clone()
    };
    assert_eq!(wording(&events[1]), "first wording");
    assert_eq!(wording(&events[2]), "second wording");
}

#[test]
fn repointing_a_reference_is_detected() {
    let (_dir, root) = workspace();
    let target = new_object(&root, "the target");
    admit(&root, payload(Act::Add, &target, "first"));
    admit(&root, payload(Act::Add, &target, "second"));
    let commit = commit_all(&root, "record target");

    let source = new_object(&root, "the source");
    let mut with_ref = payload(Act::Add, &source, "depends on the first");
    set_refs(&mut with_ref, vec![text_ref(&root, &target, 1, &commit)]);
    admit(&root, with_ref);
    assert!(ops::verify(&root, &source).expect("verify").passed());

    // The hash has to cover `refs`, not just `text`. If it covered text alone,
    // swapping which section this depends on would pass verification.
    tamper(&root, &source, |value| {
        value["sections"][0]["refs"][0]["target"] =
            Value::String(engr::proof::section_target(&target, 2).expect("section target"));
    });

    let report = ops::verify(&root, &source).expect("verify");
    assert!(
        !report.passed(),
        "a repointed reference must not pass verification"
    );
}

#[test]
fn reconcile_applies_an_event_the_projection_missed() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "reconciliation");
    let prepared = gate::prepare(&root, payload(Act::Add, &id, "one")).expect("prepare crash tail");
    // Stamped as confirmation stamps it: the Section and the Event state one
    // admission instant, which the durable boundary requires.
    let mut action = prepared.candidate.payload.action.clone();
    if let Some(value) = action.value_mut() {
        value.admitted.at = "2026-08-25T00:00:00Z".to_owned();
    }
    let crashed = engr::model::Event::sealed(
        &id,
        engr::model::new_id(),
        action,
        2,
        engr::model::EventAdmission::human("2026-08-25T00:00:00Z", prepared.candidate.code()),
    )
    .expect("a durable Event whose projection never landed");
    append_admitted_raw(&root, &id, &crashed);

    let object_path = store::object_path(&root, &id);
    let events_path = store::events_path(&root, &id);
    let object_before = std::fs::read(&object_path).expect("raw object");
    let events_before = std::fs::read(&events_path).expect("events");
    let report = ops::verify(&root, &id).expect("verify raw projection");
    // Counted, and not a fault. The Event is admitted and the projection is
    // derived from it; there is nothing here that repair could restore, and
    // `repair` says so — so a failing verdict here would send a reader to a
    // command that disagrees with it.
    assert_eq!(report.unprojected, 1);
    assert!(report.passed());
    assert_eq!(
        std::fs::read(&object_path).expect("object after verify"),
        object_before
    );
    assert_eq!(
        std::fs::read(&events_path).expect("events after verify"),
        events_before
    );

    let recovered = ops::reconcile(&root, &id).expect("reconcile");
    assert_eq!(recovered.rev, 2);
    assert_eq!(recovered.sections.len(), 1);
    assert_eq!(recovered.sections[0].text, "one");
    assert!(ops::verify(&root, &id)
        .expect("verify repaired projection")
        .passed());
}

#[test]
fn a_reference_is_drift_once_its_target_is_revised() {
    let (_dir, root) = workspace();
    let target = new_object(&root, "the target");
    admit(&root, payload(Act::Add, &target, "the original basis"));
    let commit = commit_all(&root, "record target");

    let source = new_object(&root, "the source");
    let mut with_ref = payload(Act::Add, &source, "rests on the basis");
    set_refs(&mut with_ref, vec![text_ref(&root, &target, 1, &commit)]);
    admit(&root, with_ref);

    let object = store::load_object(&root, &source).expect("source");
    let assessment = view::assess(&root, &object);
    assert!(
        assessment[0].1.is_ok(),
        "nothing has moved yet, so nothing should be flagged"
    );

    admit(
        &root,
        payload(Act::Revise(1), &target, "the basis, restated differently"),
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
            Act::Add,
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
        rendered.contains("persisted Section does not match the seal admitted at"),
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
        payload(Act::Add, &target, "Ruling: reason codes are numeric."),
    );
    let pinned = store::load_object(&root, &target).expect("target").sections[0]
        .digest
        .clone();
    let commit = commit_all(&root, "record target");

    let source = new_object(&root, "downstream decision");
    let mut with_ref = payload(
        Act::Add,
        &source,
        "Therefore the UI renders them as integers.",
    );
    set_refs(&mut with_ref, vec![text_ref(&root, &target, 1, &commit)]);
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
    // The reported current hash is recomputed from what the target actually
    // says, so rewritten wording moves it. What did not move is the target's
    // stored seal — and that gap between content and seal is what makes this
    // tampering rather than an ordinary revision.
    assert_ne!(
        status.drifted[0].current_sha256.as_deref(),
        Some(pinned.as_str()),
        "content identity has to follow the content"
    );
    assert_eq!(
        store::load_object(&root, &target).expect("target").sections[0].digest,
        pinned,
        "the seal still claims the wording nobody changed it back to"
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
    assert!(rendered.contains("current integrity failed"), "{rendered}");
}

#[test]
fn a_historical_integrity_failure_names_the_historical_side() {
    let (_dir, root) = workspace();
    let target = new_object(&root, "historical target");
    admit(
        &root,
        payload(Act::Add, &target, "the committed dependency"),
    );
    let good_commit = commit_all(&root, "good target");

    let source = new_object(&root, "source");
    let mut with_ref = payload(Act::Add, &source, "depends on target text");
    set_refs(
        &mut with_ref,
        vec![text_ref(&root, &target, 1, &good_commit)],
    );
    let source_object = admit(&root, with_ref);

    tamper(&root, &target, |value| {
        value["sections"][0]["text"] = Value::String("tampered historical text".to_owned());
    });
    let bad_commit = commit_all(&root, "bad historical target");
    let restored = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args([
            "show",
            &format!("{good_commit}:.engr/objects/{target}.json"),
        ])
        .output()
        .expect("git show");
    assert!(restored.status.success());
    std::fs::write(store::object_path(&root, &target), restored.stdout).expect("restore target");

    let original = &source_object.sections[0].refs[0];
    let historical_ref = engr::dependency::SelectiveRef::stored(
        original.target(),
        original.fields().to_vec(),
        bad_commit,
        original.digest(),
    )
    .expect("historical ref");
    let diagnostic_source = integrity::mutate(&source_object, |object| {
        object.sections[0].refs[0] = historical_ref;
        Ok(())
    })
    .expect("reseal fixture")
    .object;

    let status = &view::assess(&root, &diagnostic_source)[0].1;
    assert!(status.stands_on_tampered());
    assert_eq!(status.drifted[0].integrity_side, Some("historical"));
    let rendered = view::render_show(&root, &diagnostic_source);
    assert!(
        rendered.contains("historical integrity failed"),
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
        ..Content::default()
    };
    let added = common::payload(Act::Add, &object, content.clone());
    let deleted = common::payload(Act::Delete(1), &object, content.clone());

    // What the human assents to includes which action it is, so a frozen subject
    // cannot be swapped for a different one carrying the same words.
    let subject = |payload: &Payload| {
        serde_json::to_value(engr::gate::ObjectSubject::of(payload, 0, None).expect("subject"))
            .expect("json")
    };
    assert_ne!(subject(&added), subject(&deleted));
    assert_eq!(subject(&added)["action"], "section.create");
    assert_eq!(subject(&deleted)["action"], "section.delete");
}

/// A dependency that will not load is a failure, not drift.
///
/// This is the one read surface whose job is to say how far wording can be
/// trusted, so flattening "the target is malformed" into "the target moved"
/// would answer the question wrongly in the safe-looking direction. Absence and
/// unreadable authority are different facts and must stay apart.
#[test]
fn a_reference_to_unreadable_authority_reports_a_failure_rather_than_drift() {
    let (_dir, root) = workspace();
    let target = new_object(&root, "upstream decision");
    admit(
        &root,
        payload(Act::Add, &target, "Reason codes are numeric."),
    );
    let commit = commit_all(&root, "record target");

    let source = new_object(&root, "downstream decision");
    let mut with_ref = payload(Act::Add, &source, "So the UI renders integers.");
    set_refs(&mut with_ref, vec![text_ref(&root, &target, 1, &commit)]);
    admit(&root, with_ref);

    // Sound to begin with.
    let object = store::load_object(&root, &source).expect("source");
    assert!(view::assess(&root, &object)[0].1.is_ok());

    // Now the target becomes something this build refuses to read at all —
    // present on disk, and not loadable.
    let path = store::object_path(&root, &target);
    let mut raw: serde_json::Value = store::read_json(&path).expect("read");
    raw["state"] = serde_json::json!("not-a-state");
    write_raw(&path, &raw).expect("write");
    assert!(
        ops::effective(&root, &target).is_err(),
        "the target must genuinely fail to load"
    );

    let object = ops::effective(&root, &source).expect("source still loads");
    let status = &view::assess(&root, &object)[0].1;
    assert!(status.stands_on_unreadable(), "{status:?}");
    assert!(
        !status.stands_on_tampered(),
        "unreadable is not the same claim as tampered"
    );
    assert!(
        status.forged(),
        "and it counts as a failure, so verify cannot pass"
    );
    assert_eq!(status.label(), "REF UNREADABLE");
    assert_eq!(status.key(), "ref_unreadable");

    // The header was never the problem. The line a reader acts on said the
    // target was gone — the exact words the protocol names as the ones
    // malformed authority must never be reported in — which sends someone to
    // recreate a file that is sitting right there, unread.
    let unreadable = view::render_show(&root, &object);
    assert!(
        unreadable.contains("will not load"),
        "the advice must say what is actually wrong: {unreadable}"
    );
    for forbidden in ["no longer exists", "is gone"] {
        assert!(
            !unreadable.contains(forbidden),
            "{forbidden:?} must not appear for unreadable authority: {unreadable}"
        );
    }

    // A target that is genuinely absent is the other answer, and stays drift.
    // Its events go too: a projection alone is not the authority, and while the
    // durable tail survives, the target is recoverable rather than gone.
    write_raw(&path, &raw).expect("restore the broken file");
    std::fs::remove_file(&path).expect("remove");
    std::fs::remove_file(store::events_path(&root, &target)).expect("remove events");
    let object = ops::effective(&root, &source).expect("source");
    let status = &view::assess(&root, &object)[0].1;
    assert!(
        !status.stands_on_unreadable(),
        "absence is not unreadable authority: {status:?}"
    );

    // And it reads differently. Two distinct facts about a dependency that
    // rendered the same sentence were two facts a reader could not act on.
    let absent = view::render_show(&root, &object);
    assert!(absent.contains("no longer exists"), "{absent}");
    assert_ne!(
        unreadable.lines().find(|line| line.contains("advice")),
        absent.lines().find(|line| line.contains("advice")),
    );
}

/// `verify` reports a reference it cannot check, rather than skipping it.
///
/// Both `continue`s that used to be here were silent passes on the one path
/// whose whole job is to say whether the record adds up: an unreadable target
/// let the source PASS while standing on authority nobody could read, and a
/// missing one reported health for a dependency that is not there.
#[test]
fn verify_reports_a_referenced_target_that_is_missing_or_unreadable() {
    let (_dir, root) = workspace();
    let target = new_object(&root, "upstream decision");
    admit(
        &root,
        payload(Act::Add, &target, "Reason codes are numeric."),
    );
    let commit = commit_all(&root, "record target");

    let source = new_object(&root, "downstream decision");
    let mut with_ref = payload(Act::Add, &source, "So the UI renders integers.");
    set_refs(&mut with_ref, vec![text_ref(&root, &target, 1, &commit)]);
    admit(&root, with_ref);
    assert!(ops::verify(&root, &source).expect("verify").passed());

    // (a) the target is present and will not load.
    let path = store::object_path(&root, &target);
    let sound: Value = store::read_json(&path).expect("read");
    let mut broken = sound.clone();
    broken["state"] = Value::String("not-a-state".into());
    write_raw(&path, &broken).expect("write");

    let report = ops::verify(&root, &source).expect("verify still runs");
    assert!(!report.passed(), "unreadable authority is not a pass");
    assert_eq!(report.standing_on_unreadable.len(), 1);
    assert!(report.standing_on_missing.is_empty());
    assert!(
        report.standing_on_tampered.is_empty(),
        "unreadable is not the same claim as tampered"
    );
    assert!(
        !report.standing_on_unreadable[0].reason.is_empty(),
        "and it says why"
    );

    // (b) the target is gone entirely — projection and admitted history both.
    let history = store::events_path(&root, &target);
    let history_bytes = std::fs::read(&history).expect("history");
    std::fs::remove_file(&path).expect("remove");
    std::fs::remove_file(&history).expect("remove events");
    let report = ops::verify(&root, &source).expect("verify still runs");
    assert!(
        !report.passed(),
        "a dependency that is not there is not a pass"
    );
    assert_eq!(report.standing_on_missing.len(), 1);
    assert!(report.standing_on_unreadable.is_empty());

    // (c) the target loads, with its history, but the referenced section is not
    // in it. The history goes back too: a projection with no admitted history at
    // all is its own finding, and it would mask this one.
    std::fs::write(&history, &history_bytes).expect("restore history");
    write_raw(&path, &sound).expect("restore");
    // Removed and resealed, not blanked: an Object with no Sections omits the
    // member, and one that carries an empty list is refused as schema before
    // anything asks which Sections it has. What is being pinned here is absence,
    // so the target has to stay a valid Object that simply no longer holds it.
    let held = store::load_object(&root, &target).expect("target");
    let without = integrity::mutate(&held, |object| {
        object.sections.clear();
        Ok(())
    })
    .expect("reseal")
    .object;
    write_raw(&path, &without).expect("write");
    let report = ops::verify(&root, &source).expect("verify still runs");
    assert!(!report.passed());
    assert_eq!(
        report.standing_on_missing.len(),
        1,
        "a missing section is absence, like a missing object"
    );
}

/// `verify` reads a referenced target through the same authority `show` does.
///
/// `show` and reference admission both go through `ops::effective`, so a target
/// is read with its recoverable crash tail applied. `verify` used to load the
/// stored projection directly, which answered a different question and got two
/// different wrong answers for it: a target whose durable tail will not
/// reconcile has a projection that loads fine, so the source PASSed while `show`
/// called the same dependency unreadable; and a target whose projection is gone
/// but whose events rebuild it is authority that is present, so calling it
/// missing was wrong the other way.
///
/// Malformed authority must not be downgraded into a healthy verification
/// result, and absence must stay absence. Both surfaces have to agree on which
/// is which.
#[test]
fn verify_reads_referenced_targets_through_effective_authority() {
    let (_dir, root) = workspace();
    let target = new_object(&root, "upstream decision");
    admit(
        &root,
        payload(Act::Add, &target, "Reason codes are numeric."),
    );
    let commit = commit_all(&root, "record target");

    let source = new_object(&root, "downstream decision");
    let mut with_ref = payload(Act::Add, &source, "So the UI renders integers.");
    set_refs(&mut with_ref, vec![text_ref(&root, &target, 1, &commit)]);
    admit(&root, with_ref);
    assert!(ops::verify(&root, &source).expect("verify").passed());

    // (a) The projection is untouched and self-consistent; the durable tail
    // behind it will not read. `store::load_object` succeeds here — which is
    // exactly why loading it directly reported a pass.
    let events = store::events_path(&root, &target);
    let sound_events = std::fs::read_to_string(&events).expect("events");
    std::fs::write(
        &events,
        format!("{sound_events}{{\"format\":\"engr-event\"}}\n"),
    )
    .expect("write events");
    assert!(
        store::load_object(&root, &target).is_ok(),
        "the projection alone still loads, which is the trap"
    );
    assert!(
        ops::effective(&root, &target).is_err(),
        "and the authority behind it does not"
    );

    let report = ops::verify(&root, &source).expect("verify still runs");
    assert!(!report.passed(), "unreadable authority is not a pass");
    assert_eq!(report.standing_on_unreadable.len(), 1);
    assert!(report.standing_on_missing.is_empty());
    assert!(report.standing_on_tampered.is_empty());

    // And `show` says the same thing about the same dependency.
    let object = ops::effective(&root, &source).expect("source");
    assert!(
        view::assess(&root, &object)
            .iter()
            .any(|(_, status)| status.stands_on_unreadable()),
        "show and verify must agree on target readability"
    );

    // (b) The projection is gone, and the durable events rebuild it. That is
    // present authority, not absence — the recovery path exists precisely so a
    // crash between appending and projecting is not data loss.
    std::fs::write(&events, &sound_events).expect("restore events");
    std::fs::remove_file(store::object_path(&root, &target)).expect("remove projection");
    assert!(
        ops::effective(&root, &target).is_ok(),
        "the events reconstruct it"
    );
    assert!(
        ops::object_ids(&root)
            .expect("discover Objects")
            .contains(&target),
        "enumeration retains an Object established by admitted history"
    );
    let target_report = ops::verify(&root, &target).expect("verify reconstructed target");
    assert!(target_report.projection_missing);
    assert!(
        !target_report.passed(),
        "a required projection is still unhealthy"
    );
    assert!(
        !store::object_path(&root, &target).exists(),
        "read-only discovery and verification never materialize authority"
    );

    let report = ops::verify(&root, &source).expect("verify still runs");
    assert!(
        report.standing_on_missing.is_empty(),
        "authority recoverable from its own events is not missing"
    );
    assert!(report.standing_on_unreadable.is_empty());
    assert!(
        report.passed(),
        "and the source stands on it as soundly as it did before"
    );
}

/// A selective reference pins the semantic field it declares, not either
/// resource seal. Integrity is established first; the Ref digest then answers
/// the narrower dependency question over `fields=[text]`.
#[test]
fn a_reference_pins_content_identity_rather_than_the_targets_seal() {
    let (_dir, root) = workspace();
    let target = new_object(&root, "upstream decision");
    admit(
        &root,
        payload(Act::Add, &target, "Reason codes are numeric."),
    );
    let target_before = std::fs::read(store::object_path(&root, &target)).expect("target bytes");
    let commit = commit_all(&root, "record target");
    let reference = text_ref(&root, &target, 1, &commit);
    let section_seal = store::load_object(&root, &target).expect("target").sections[0]
        .digest
        .clone();
    assert_ne!(
        reference.digest(),
        section_seal,
        "dependency identity and resource integrity are separate contracts"
    );

    let source = new_object(&root, "downstream decision");
    let mut with_ref = payload(Act::Add, &source, "So the UI renders integers.");
    set_refs(&mut with_ref, vec![reference.clone()]);
    admit(&root, with_ref);
    assert!(ops::verify(&root, &source).expect("verify").passed());

    let object = store::load_object(&root, &source).expect("source");
    assert!(view::assess(&root, &object)[0].1.is_ok());

    // (a) Rewritten outside the gate: the content moves, the seal does not.
    // Creating a reference to it is refused, because pinning the rewritten
    // wording would make an unconfirmed edit look agreed.
    tamper(&root, &target, |value| {
        value["sections"][0]["text"] = Value::String("Reason codes are free text.".into());
    });
    let mut to_forged = payload(Act::Add, &source, "depends on rewritten wording");
    set_refs(&mut to_forged, vec![reference]);
    let error = gate::prepare(&root, to_forged).expect_err("that target cannot be referenced");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("TargetIntegrityFailure"), "{error}");

    // The existing reference reports target integrity failure, not ordinary
    // semantic drift.
    let status = &view::assess(&root, &object)[0].1;
    assert!(status.stands_on_tampered());

    // (b) Revised through the gate: resource seals move coherently, so it is
    // drift rather than tampering — the distinction this whole scheme exists
    // to keep.
    std::fs::write(store::object_path(&root, &target), target_before).expect("restore target");
    admit(
        &root,
        payload(
            Act::Revise(1),
            &target,
            "Reason codes are numeric, and stable across releases.",
        ),
    );
    let status = &view::assess(&root, &object)[0].1;
    assert!(
        !status.stands_on_tampered(),
        "a confirmed revision is not a forgery"
    );
    assert_eq!(status.drifted.len(), 1);
    let revised = store::load_object(&root, &target).expect("target");
    engr::integrity::check_stored_object_integrity(&revised)
        .expect("the admitted revision carries coherent resource seals");
}

/// A section standing on a target that is gone does not verify, on any surface.
///
/// `verify` already treated a missing referenced target as a failure while
/// `show` called it ordinary drift — one workspace state with two verdicts,
/// which is exactly the cross-surface disagreement the roadmap review named.
/// The ruling settled it as failure: a section that explicitly leans on another
/// section which no longer exists is not out of date, it is standing on
/// nothing.
#[test]
fn a_reference_to_a_target_that_is_gone_is_a_failure_on_every_surface() {
    let (_dir, root) = workspace();
    let target = new_object(&root, "upstream decision");
    admit(
        &root,
        payload(Act::Add, &target, "Reason codes are numeric."),
    );
    let commit = commit_all(&root, "record target");

    let source = new_object(&root, "downstream decision");
    let mut with_ref = payload(Act::Add, &source, "So the UI renders integers.");
    set_refs(&mut with_ref, vec![text_ref(&root, &target, 1, &commit)]);
    admit(&root, with_ref);

    let object = ops::effective(&root, &source).expect("source");
    assert!(view::assess(&root, &object)[0].1.is_ok());
    assert!(ops::verify(&root, &source).expect("verify").passed());

    // The target stops existing: projection and durable history both.
    std::fs::remove_file(store::object_path(&root, &target)).expect("remove");
    std::fs::remove_file(store::events_path(&root, &target)).expect("remove events");

    let status = &view::assess(&root, &object)[0].1;
    assert!(status.stands_on_missing(), "{status:?}");
    assert!(
        status.forged(),
        "standing on nothing is not a section whose wording can be trusted"
    );
    assert!(!status.is_ok());
    assert_eq!(status.label(), "REF MISSING");
    assert_eq!(status.key(), "ref_missing");
    assert!(
        !status.stands_on_unreadable(),
        "absence and unreadable authority stay different facts"
    );
    assert!(!status.stands_on_tampered());

    // And the two surfaces now say the same thing about the same state.
    let report = ops::verify(&root, &source).expect("verify");
    assert!(!report.passed());
    assert_eq!(report.standing_on_missing.len(), 1);
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

/// A duplicate member is where two conforming JSON readers may disagree.
///
/// Parsing into a value collapses a repeated key silently — one slot per name —
/// so an exact-members check made after parsing can only ever see one. That is
/// not a tidiness question: another conforming stack may reject the document or
/// select the other occurrence, so the bytes no longer have one meaning. The
/// canonical-bytes rule settles it without a second check, because the collapsed
/// value no longer re-serializes to the bytes that had both.
#[test]
fn a_current_resource_with_a_duplicate_member_is_not_this_generations_bytes() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "one meaning");
    admit(&root, payload(Act::Add, &id, "wording"));

    let path = store::object_path(&root, &id);
    let original = std::fs::read_to_string(&path).expect("object bytes");
    store::load_object(&root, &id).expect("the canonical bytes load");

    for (what, rewritten) in [
        (
            "a duplicated top-level member",
            original.replacen('{', r#"{"state":"open","#, 1),
        ),
        (
            "a duplicated Section member",
            original.replacen(r#""sections":[{"#, r#""sections":[{"id":1,"#, 1),
        ),
    ] {
        assert_ne!(
            rewritten, original,
            "{what}: the fixture must change something"
        );
        std::fs::write(&path, &rewritten).expect("write");
        let error = store::load_object(&root, &id)
            .err()
            .unwrap_or_else(|| panic!("{what}: this must be refused"));
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{what}");
        assert!(error.message.contains("canonical"), "{what}: {error}");
    }

    std::fs::write(&path, &original).expect("restore");
    store::load_object(&root, &id).expect("and it reads back");
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

/// An Event stream has one framing, and damage to it is not read past.
///
/// A blank line used to be skipped. That gave a current stream a second
/// spelling the writer never emits, and it read past the first symptom of a
/// truncated write, a partial copy or a bad merge — whitespace where a record
/// belongs. The missing final delimiter is the sharper form of the same fault:
/// a complete JSON object with nothing after it is a record whose write did not
/// finish, and accepting it is what would let the next append concatenate onto
/// that line and fuse two events into one, permanently, in a file that is never
/// rewritten.
#[test]
fn a_blank_or_unterminated_event_record_is_refused() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "one framing");
    admit(&root, payload(Act::Add, &id, "wording"));
    let path = store::events_path(&root, &id);
    let original = std::fs::read_to_string(&path).expect("events");
    assert_eq!(original.lines().count(), 2, "two admitted records");
    assert!(
        original.ends_with('\n'),
        "the writer terminates every record"
    );

    for (what, rewritten) in [
        (
            "a blank line before the first record",
            format!("\n{original}"),
        ),
        (
            "a blank line between records",
            original.replacen('\n', "\n\n", 1),
        ),
        ("a blank line after the last", format!("{original}\n")),
        ("a whitespace-only line", format!("{original}   \n")),
    ] {
        std::fs::write(&path, &rewritten).expect("write");
        let error = store::load_events(&root, &id)
            .err()
            .unwrap_or_else(|| panic!("{what}: this must be refused"));
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{what}");
        assert!(error.message.contains("no blank ones"), "{what}: {error}");
    }

    std::fs::write(&path, original.trim_end_matches('\n')).expect("write");
    let error = store::load_events(&root, &id)
        .expect_err("an unterminated last record is a truncated history");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("delimiter"), "{error}");

    // And nothing can be appended onto it. This is the case that matters most:
    // the refusal has to happen before the write, or the damage becomes durable.
    let error = gate::prepare(&root, payload(Act::Add, &id, "another"))
        .expect_err("a truncated stream cannot be built on");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("delimiter"), "{error}");

    std::fs::write(&path, &original).expect("restore");
    assert_eq!(
        store::load_events(&root, &id)
            .expect("and it reads back")
            .len(),
        2
    );
}

/// Every Event stream is published whole, so an unlocked reader never sees a
/// prefix of one.
///
/// The append is a republish through the same temporary-file-and-rename path
/// every other resource uses, which is what makes the two visible states
/// complete-old and complete-new. A genuinely appending write has a third
/// state, and this is the surface that would show it: `load_events` decodes
/// against the exact record bytes, so a partially written record cannot be read
/// as a valid one.
///
/// The unlocked reader below is a smoke test and cannot be more than that — a
/// torn read is a race, and a race that does not happen proves nothing. What is
/// checked deterministically is the mechanism: on a platform with inode
/// identity, a published stream is a *different file* each time, which an
/// in-place append can never be.
#[test]
fn an_event_stream_is_never_visible_half_written() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "whole or nothing");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let path = store::events_path(&root, &id);
        let before = std::fs::metadata(&path).expect("stream").ino();
        admit(&root, payload(Act::Add, &id, "published, not appended to"));
        let after = std::fs::metadata(&path).expect("stream").ino();
        assert_ne!(
            before, after,
            "an Event stream is staged and renamed into place, never written in place"
        );
    }
    let writing = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let reader = std::thread::spawn({
        let root = root.clone();
        let id = id.clone();
        let writing = std::sync::Arc::clone(&writing);
        move || {
            let mut reads = 0;
            while writing.load(std::sync::atomic::Ordering::Relaxed) {
                // No lock: this is the unprotected reader the publication has to
                // be safe for.
                match store::load_events(&root, &id) {
                    Ok(events) => {
                        reads += 1;
                        for (index, event) in events.iter().enumerate() {
                            assert_eq!(
                                event.rev,
                                index as u64 + 1,
                                "a visible stream is contiguous from 1"
                            );
                        }
                    }
                    Err(error) => panic!("a reader saw a stream it could not decode: {error}"),
                }
            }
            reads
        }
    });
    for revision in 0..12 {
        admit(
            &root,
            payload(Act::Add, &id, &format!("wording {revision}")),
        );
    }
    writing.store(false, std::sync::atomic::Ordering::Relaxed);
    let reads = reader.join().expect("the reader must not have panicked");
    assert!(reads > 0, "the reader has to have read something");
    // No staging file survives a completed publication.
    let staged = store::events_path(&root, &id).with_extension("jsonl.tmp");
    assert!(!staged.exists(), "{} was left behind", staged.display());
}

/// A Section that asserts nothing is refused wherever a persisted one is read.
///
/// `text` is required and may be empty only beside non-empty literal content.
/// The rule used to live only at the mutation boundary, so a stored Object whose
/// Section was blanked out and resealed loaded cleanly — valid seals, a shape
/// the contract forbids — and every read surface then presented a blank as
/// admitted knowledge.
#[test]
fn a_section_that_asserts_nothing_is_refused_on_the_read_path() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "blank");
    admit(&root, payload(Act::Add, &id, "wording"));

    // The stored Object, held to the shape boundary rather than to a seal: this
    // is refused for what it says, not for whether it was tampered with.
    let path = store::object_path(&root, &id);
    let original = std::fs::read_to_string(&path).expect("object bytes");
    tamper(&root, &id, |value| {
        value["sections"][0]["text"] = Value::String(String::new());
    });
    let error =
        store::load_object(&root, &id).expect_err("an empty text with no content is not a Section");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("asserts nothing"), "{error}");
    std::fs::write(&path, &original).expect("restore");
    store::load_object(&root, &id).expect("and it reads back");

    // And the Event that would carry the same value, whose seal is correct — so
    // the refusal cannot be the seal.
    let blank = engr::model::Event::sealed(
        &id,
        engr::model::new_id(),
        engr::model::Action::SectionCreated {
            // The Section and the Event state one admission instant, which the
            // durable boundary requires — so a fixture that disagreed about that
            // would be refused for the wrong reason.
            value: engr::model::SectionValue::new(
                engr::semantics::Admitted::new(
                    engr::semantics::Admission::Human,
                    "2026-08-23T00:00:00Z",
                ),
                Content::default(),
            ),
            becomes: None,
        },
        3,
        engr::model::EventAdmission::human("2026-08-23T00:00:00Z", "TEST23"),
    )
    .expect("a sealed record, whatever it says");
    let events = std::fs::read_to_string(store::events_path(&root, &id)).expect("events");
    append_admitted_raw(&root, &id, &blank);
    let error = store::load_events(&root, &id)
        .expect_err("an Event cannot carry a Section value the contract forbids");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("text"), "{error}");
    std::fs::write(store::events_path(&root, &id), &events).expect("restore");
    store::load_events(&root, &id).expect("and it reads back");
}

/// A persisted resource is the bytes git tracks **at that path**.
///
/// A link breaks that in a way no digest can see: git records the link — its
/// target's name, as a blob — while engr reads and writes the target's
/// contents. So `.engr`, a resource directory, or one resource file can
/// redirect the record outside the repository entirely, and the history a
/// reviewer reads is then not the state the tool is using. Refused rather than
/// followed, at every component, exactly as the Rule loader already refuses it
/// for policy.
#[test]
#[cfg(unix)]
fn nothing_on_the_way_to_a_resource_may_be_a_link() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "contained");
    admit(&root, payload(Act::Add, &id, "wording"));
    let link_refused = |what: &str, error: Option<engr::Error>| {
        let error = error.unwrap_or_else(|| panic!("{what}: this must be refused"));
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{what}");
        assert!(
            error.message.contains("link to somewhere else"),
            "{what}: {error}"
        );
    };

    // One resource file, redirected. The bytes behind it are perfectly valid —
    // that is the point: what is wrong is how they were reached.
    let path = store::object_path(&root, &id);
    let outside = root.join("outside-object.json");
    std::fs::rename(&path, &outside).expect("move the object out");
    std::os::unix::fs::symlink(&outside, &path).expect("symlink");
    link_refused(
        "a redirected object file",
        store::load_object(&root, &id).err(),
    );
    link_refused(
        "and every read surface through it",
        ops::effective(&root, &id).err(),
    );
    std::fs::remove_file(&path).expect("remove the link");
    std::fs::rename(&outside, &path).expect("put it back");
    store::load_object(&root, &id).expect("and it reads again");

    // A directory in the middle of the way.
    for (what, dir) in [
        ("objects", store::objects_dir(&root)),
        ("eventstore", store::eventstore_dir(&root)),
    ] {
        let moved = root.join(format!("outside-{what}"));
        std::fs::rename(&dir, &moved).expect("move");
        std::os::unix::fs::symlink(&moved, &dir).expect("symlink");
        // Reading the Object touches both trees, so one call covers whichever
        // component is redirected; enumeration and the write that would land in
        // it are asked separately.
        link_refused(
            &format!("reading through {what}"),
            ops::effective(&root, &id).err(),
        );
        link_refused(
            &format!("enumerating through {what}"),
            ops::object_ids(&root).err(),
        );
        link_refused(
            &format!("writing through {what}"),
            gate::prepare(&root, payload(Act::Add, &id, "more")).err(),
        );
        std::fs::remove_file(&dir).expect("remove the link");
        std::fs::rename(&moved, &dir).expect("put it back");
    }
    admit(&root, payload(Act::Add, &id, "and writing works again"));

    // And `.engr` itself, which is the link that would redirect everything.
    let engr_dir = store::engr_dir(&root);
    let moved = root.join("outside-engr");
    std::fs::rename(&engr_dir, &moved).expect("move the workspace");
    std::os::unix::fs::symlink(&moved, &engr_dir).expect("symlink");
    assert!(
        store::objects_dir(&root)
            .join(format!("{id}.json"))
            .is_file(),
        "everything behind the link is intact"
    );
    link_refused(
        "a redirected workspace",
        store::find_root(Some(root.as_path())).err(),
    );
    link_refused("a read through it", store::load_object(&root, &id).err());
    std::fs::remove_file(&engr_dir).expect("remove the link");
    std::fs::rename(&moved, &engr_dir).expect("put it back");
    assert_eq!(
        store::find_root(Some(root.as_path())).expect("restored"),
        root,
        "the refusal is about the link, not about the workspace"
    );
}

/// The staging entry a publication writes through is part of the resource path.
///
/// Its name is `<resource>.tmp` and therefore entirely predictable, so checking
/// only the destination left the boundary bypassable: a link planted at the
/// staging name was followed by an ordinary create — engr wrote the outside
/// target — and the rename then moved *the link itself* into the canonical
/// resource path. Every resource that publishes went through that door.
#[test]
#[cfg(unix)]
fn a_link_planted_at_the_staging_name_is_refused() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "staged publication");
    let outside = TempDir::new().expect("outside");

    for (what, resource) in [
        ("the Object", store::object_path(&root, &id)),
        ("its Event stream", store::events_path(&root, &id)),
    ] {
        let target = outside.path().join("captured");
        std::fs::write(&target, "not this workspace's bytes\n").expect("outside file");
        let staged = std::path::PathBuf::from({
            let mut name = resource.clone().into_os_string();
            name.push(".tmp");
            name
        });
        std::os::unix::fs::symlink(&target, &staged).expect("plant the link");

        let error = gate::prepare(&root, payload(Act::Add, &id, "wording"))
            .and_then(|prepared| {
                gate::confirm(&root, &format!("CONFIRM {}", prepared.candidate.code()))
            })
            .expect_err(&format!(
                "{what}: publishing through a link must be refused"
            ));
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{what}: {error}");
        assert!(
            error.message.contains("link to somewhere else"),
            "{what}: {error}"
        );
        // The outside file is untouched, and the link is still a link rather
        // than having been renamed into the record.
        assert_eq!(
            std::fs::read_to_string(&target).expect("outside file"),
            "not this workspace's bytes\n",
            "{what}: the target was written through"
        );
        assert!(
            std::fs::symlink_metadata(&staged)
                .expect("the link")
                .file_type()
                .is_symlink(),
            "{what}: the link was consumed"
        );
        std::fs::remove_file(&staged).expect("remove the link");
        std::fs::remove_file(&target).expect("remove the target");
    }

    // And with the staging names clear, the same mutation lands.
    admit(&root, payload(Act::Add, &id, "wording"));
    assert!(ops::verify(&root, &id).expect("verify").passed());
}

/// A crashed publication leaves a staging file, and that must not wedge the
/// workspace.
///
/// The exclusive create is what refuses a planted link; a leftover regular file
/// from a crash between the create and the rename is the one thing that can
/// legitimately be there, and it is removed rather than treated as an attack.
#[test]
fn a_leftover_staging_file_from_a_crash_does_not_wedge_the_workspace() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "crashed publication");
    for resource in [
        store::object_path(&root, &id),
        store::events_path(&root, &id),
    ] {
        let mut name = resource.into_os_string();
        name.push(".tmp");
        std::fs::write(std::path::PathBuf::from(name), "half a write").expect("leftover");
    }
    admit(&root, payload(Act::Add, &id, "wording admitted afterwards"));
    assert!(ops::verify(&root, &id).expect("verify").passed());
}

/// Discovery ascends only on established absence.
///
/// `is_dir()` gives two answers where there are three, and reports a dangling
/// link, a regular file or an unreadable entry as though nothing were there. In
/// a walk that ascends on "nothing there", that carries the caller past the
/// workspace they are standing in and into an ancestor's — where the next
/// command lands on somebody else's record.
#[test]
#[cfg(unix)]
fn discovery_does_not_walk_past_an_engr_it_cannot_establish() {
    let outer = TempDir::new().expect("temp dir");
    let ancestor = outer.path().to_path_buf();
    store::init(&ancestor).expect("the ancestor workspace");
    let nested = ancestor.join("nested");
    std::fs::create_dir_all(&nested).expect("nested directory");

    // Nothing here: the walk finds the ancestor, which is the whole point of
    // walking up.
    assert_eq!(
        store::find_root(Some(nested.as_path()))
            .expect_err("not a workspace")
            .code,
        engr::EXIT_NOT_FOUND
    );

    for (what, plant) in [
        ("a dangling link", 0),
        ("a regular file", 1),
        ("a live link to the ancestor's own workspace", 2),
    ] {
        let here = store::engr_dir(&nested);
        match plant {
            0 => std::os::unix::fs::symlink(nested.join("nowhere"), &here).expect("symlink"),
            1 => std::fs::write(&here, "not a directory").expect("file"),
            _ => std::os::unix::fs::symlink(store::engr_dir(&ancestor), &here).expect("symlink"),
        }
        let error = store::find_root(Some(nested.as_path()))
            .expect_err(&format!("{what}: this must not read as absence"));
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{what}: {error}");
        std::fs::remove_file(&here).expect("remove");
    }
}

/// A namespace of the wrong shape is not an empty namespace.
///
/// `is_dir()` answers two of the three questions an enumerator has to ask, and
/// the missing answer is the dangerous one: a regular file where a resource
/// directory belongs is reported as nothing there, so every enumerator returned
/// an empty set and the workspace read as one with no Objects, no Backlog, no
/// Work, no Collections and no pending Challenges. A refusal is a far better
/// answer than a confident empty one — an agent acts on empty.
#[test]
fn a_resource_namespace_of_the_wrong_shape_is_refused_rather_than_read_as_empty() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "shapes");
    admit(&root, payload(Act::Add, &id, "wording"));

    for (what, dir) in [
        ("objects", store::objects_dir(&root)),
        ("eventstore", store::eventstore_dir(&root)),
        ("backlog", engr::backlog::dir(&root)),
        ("collections", engr::collection::dir(&root)),
        ("work", engr::work::root_dir(&root).join("objects")),
        ("challenges", store::challenges_dir(&root)),
    ] {
        std::fs::remove_dir_all(&dir).expect("clear the namespace");
        std::fs::write(&dir, "not a directory").expect("a file where the directory belongs");

        // Whichever enumerator owns it, and every survey that reaches through
        // one, refuses rather than answering "none".
        let outcomes: Vec<(&str, Option<engr::Error>)> = vec![
            ("objects", store::object_ids(&root).err()),
            ("event ids", ops::object_ids(&root).err()),
            ("backlog", engr::backlog::ids(&root).err()),
            ("collections", engr::collection::ids(&root).err()),
            ("work", engr::work::ids(&root).err()),
            ("challenges", gate::pending_codes(&root).err()),
        ];
        let refused = outcomes
            .iter()
            .filter(|(_, error)| {
                error.as_ref().is_some_and(|error| {
                    error.code == engr::EXIT_SCHEMA && error.message.contains("not a directory")
                })
            })
            .count();
        assert!(
            refused > 0,
            "{what}: every enumerator reading it answered as though it were empty: {outcomes:?}"
        );

        std::fs::remove_file(&dir).expect("remove the file");
        std::fs::create_dir_all(&dir).expect("put the directory back");
    }
}

/// A generation marker that cannot be established is not a missing one.
///
/// `validate_format` asked `VERSION.exists()`, which follows links and reports a
/// dangling one as *no generation marker at all*. With a predecessor bootstrap
/// beside it — which is exactly the workspace a migration source is — that made
/// this build classify the workspace as the released predecessor and hand it to
/// the one path that is entitled to write to a predecessor, instead of refusing
/// a generation authority it could not establish.
///
/// This is not a resource diagnostic. The answer decides **which storage
/// contract may be interpreted**, so the three-way rule belongs here more than
/// anywhere it was already applied.
#[test]
#[cfg(unix)]
fn a_generation_marker_that_cannot_be_established_is_not_a_missing_one() {
    let (_dir, root) = workspace();
    let version = store::version_path(&root);
    // The released predecessor's own bootstrap, sitting beside it. This is what
    // the misread resolves *to*.
    std::fs::write(
        store::engr_dir(&root).join("format.json"),
        r#"{"format":"engr-workspace","version":1}"#,
    )
    .expect("the predecessor bootstrap");

    let elsewhere = root.join("outside-VERSION");
    std::fs::write(&elsewhere, engr::WORKSPACE_VERSION_FILE).expect("a marker somewhere else");

    for what in [
        "a dangling link",
        "a link to a live marker outside",
        "a directory",
    ] {
        std::fs::remove_file(&version).ok();
        match what {
            "a dangling link" => {
                std::os::unix::fs::symlink(root.join("nowhere"), &version).expect("symlink")
            }
            // The bytes behind it are perfectly valid; that is the point. What
            // is wrong is that the record would not be the one this workspace
            // holds.
            "a link to a live marker outside" => {
                std::os::unix::fs::symlink(&elsewhere, &version).expect("symlink")
            }
            _ => std::fs::create_dir(&version).expect("directory"),
        }

        let error = store::validate_format(&root)
            .expect_err(&format!("{what}: this must not classify the workspace"));
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{what}: {error}");

        // And the command that would write on the strength of that
        // classification. A predecessor is precisely what migration may act on.
        let refused = engr::migration::prepare(&root)
            .expect_err(&format!("{what}: migration must not be entitled to write"));
        assert_eq!(refused.code, engr::EXIT_SCHEMA, "{what}: {refused}");

        match what {
            "a directory" => std::fs::remove_dir(&version).expect("remove"),
            _ => std::fs::remove_file(&version).expect("remove"),
        }
    }

    // Put the marker itself back and the workspace is current again: the refusal
    // was about establishing the marker, not about the bootstrap beside it.
    std::fs::write(&version, engr::WORKSPACE_VERSION_FILE).expect("restore the marker");
    assert_eq!(
        store::validate_format(&root).expect("a workspace with its own marker"),
        store::WorkspaceFormat::Current
    );
}

/// Every workspace directory is created through the durable layout primitive.
///
/// A directory entry lives in its **parent's** metadata, so `create_dir_all`
/// gives a new directory no durable name — and the guarantee `rename_durably`
/// provides for a published resource is worth nothing if the directory it was
/// published into can be lost by the same power failure that spared the file.
/// `.engr/eventstore/objects` was the live case: created by `init`, never
/// established in `.engr/eventstore`, and therefore losable while the Object
/// published beside it survived. That is the projection ahead of history.
///
/// **Nothing observable distinguishes a flushed directory entry from an
/// unflushed one**, so what a test can hold is the property the guarantee
/// actually rests on: that there is one route, and nothing takes another. The
/// single exception is named here rather than allowed by pattern, because an
/// exception nobody has to justify is how the next one arrives.
#[test]
fn every_workspace_directory_is_created_through_the_durable_layout_primitive() {
    assert_eq!(
        call_sites("create_dir_all("),
        Vec::new(),
        "every directory this build creates goes through `store::create_dir_durably`. \
         `.git/info` was the one exception and is no longer: the file under it is what \
         keeps a live challenge code out of `git add -A` while the workspace is still the \
         predecessor, so it is published like anything else that has to survive a crash"
    );
}

/// Nothing is written in place over a file that has to survive a crash.
///
/// `fs::write` truncates and then writes, so a power failure inside it leaves
/// neither the old content nor the new — and for a file whose *previous* content
/// belongs to somebody else, that is damage rather than a lost update. It also
/// flushes nothing, so a caller that goes on to publish something durable can
/// return success having established the durable half and lost the other.
///
/// `.git/info/exclude` was where that bit: the exclusion protecting a live
/// Challenge code was written in place, while the Challenge it protects was made
/// durable a moment later. Everything goes through `store::write_text` /
/// `store::write_json` now — staged beside the destination, flushed, renamed
/// over it, directory flushed.
///
/// This names `fs::write` rather than claiming to catch every in-place write,
/// because that is the one that replaces a whole file and so reinstates the
/// truncation window wholesale.
#[test]
fn no_file_that_must_survive_a_crash_is_written_in_place() {
    assert_eq!(
        call_sites("fs::write("),
        Vec::new(),
        "publish through `store::write_text` or `store::write_json`, which stage beside \
         the destination and rename over it, so a crash leaves the complete old file \
         rather than a truncated one"
    );
}

/// No persisted path is probed with a two-state `exists()`.
///
/// The three-way rule — only established absence is absence — was applied to the
/// resource enumerators and then found, a review later, to stop short of the
/// generation boundary: `VERSION`, the migration stage, the predecessor
/// bootstrap and `init`'s own `.engr` were still asked with `exists()`, which
/// follows links and reports a dangling one as nothing there. Those four decide
/// which storage contract may be interpreted and which path may write, so they
/// were the worst place left for it.
///
/// **A rule applied by hand is a rule that stops wherever the last reviewer
/// stopped looking.** This is what makes the next one impossible to add
/// quietly: `store::resource_present`, `store::namespace` and
/// `store::generation_present` are the vocabulary, and there is no exception.
#[test]
fn no_persisted_path_is_probed_with_a_two_state_exists() {
    assert_eq!(
        call_sites(".exists()"),
        Vec::new(),
        "`exists()` follows links and reports a dangling one, a wrong shape and an \
         unreadable entry alike as absence — and absence is what lets work proceed. \
         Ask `store::resource_present`, `store::namespace` or `store::generation_present`, \
         which answer three ways and fail closed on the two that are not absence"
    );
}

/// Absence is published the way presence is.
///
/// A directory entry's *disappearance* lives in the containing directory's
/// metadata exactly as its appearance does, so an unflushed removal can be
/// undone by a power failure after the caller was told the thing was gone. The
/// write side already treated a pathname as a durability boundary; deletion did
/// not, and it is the asymmetric half that matters most: Challenge retirement is
/// how a human **declines**, and unlike post-admission cleanup there is no
/// durable Event that could later classify a resurrected question as already
/// applied. A file that comes back is a live question, and if the Object still
/// stands at its `expected_rev` the gate calls it pending — so a mutation
/// somebody explicitly refused becomes admissible again. A withdrawn migration
/// is the same shape, with a resumable transaction behind it.
///
/// Whether the flush reached the device is not observable here, so what is held
/// is the property it rests on: one route out, and nothing takes another.
#[test]
fn every_removal_makes_the_absence_durable() {
    let mut sites = call_sites("fs::remove_file(");
    sites.extend(call_sites("fs::remove_dir_all("));
    sites.sort();
    assert_eq!(
        sites,
        vec![
            ("store.rs".to_string(), "remove_durably".to_string()),
            ("store.rs".to_string(), "remove_tree_durably".to_string()),
        ],
        "the two primitives are where the raw call lives, by definition; everything else \
         removes through them, so the directory the name is gone from gets flushed — a \
         removal that is not durable is a decision a power failure can undo"
    );
}

/// Where a spelling appears in the crate's own **production** source, by file
/// and function.
///
/// Comment lines are skipped, so prose *about* a construct is not read as a use
/// of it — the doc comments explaining why these are refused would otherwise be
/// the first thing to trip their own guards.
///
/// `#[cfg(test)]` modules are skipped as well, and by their extent rather than
/// by assuming they sit at the end of a file: a test that plants a regular file
/// where a directory belongs is *using* `fs::write` for what it is good at, and
/// the rules here are about what ships. `cargo fmt --check` is enforced, so the
/// closing brace of a top-level module is a `}` in the first column.
fn call_sites(needle: &str) -> Vec<(String, String)> {
    fn defined(trimmed: &str) -> Option<String> {
        let rest = trimmed
            .strip_prefix("pub(crate) ")
            .or_else(|| trimmed.strip_prefix("pub "))
            .unwrap_or(trimmed);
        let rest = rest.strip_prefix("unsafe ").unwrap_or(rest);
        let name = rest.strip_prefix("fn ")?;
        let end = name.find(['(', '<']).unwrap_or(name.len());
        Some(name[..end].to_string())
    }

    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut sites: Vec<(String, String)> = Vec::new();
    for entry in std::fs::read_dir(&source).expect("the source directory") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
            continue;
        }
        let file = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("file name")
            .to_string();
        let text = std::fs::read_to_string(&path).expect("read the source");
        let mut owner = "<not inside a function>".to_string();
        let mut cfg_test = false;
        let mut in_test_module = false;
        for line in text.lines() {
            if in_test_module {
                in_test_module = line != "}";
                continue;
            }
            let trimmed = line.trim_start();
            if trimmed == "#[cfg(test)]" {
                cfg_test = true;
                continue;
            }
            if cfg_test && trimmed.starts_with("mod ") {
                // Only a top-level one; anything nested keeps being read, so a
                // `#[cfg(test)]` in an unexpected place cannot silently hide code.
                assert_eq!(
                    line, trimmed,
                    "{file}: a nested `#[cfg(test)]` module is not something this scan \
                     knows how to bound"
                );
                in_test_module = true;
                cfg_test = false;
                continue;
            }
            cfg_test = false;
            if let Some(name) = defined(trimmed) {
                owner = name;
            }
            if trimmed.starts_with("//") {
                continue;
            }
            if trimmed.contains(needle) {
                sites.push((file.clone(), owner.clone()));
            }
        }
        assert!(
            !in_test_module,
            "{file}: a `#[cfg(test)]` module was never closed, so part of the file went unread"
        );
    }
    sites.sort();
    sites
}

/// The final Backlog consume cannot be reached through an unestablished Work
/// tree.
///
/// `work::exists` stat'ed the sidecar's own path, which answers about whatever
/// the path leads *to*. An intermediate `work/…` link whose target is missing
/// therefore answered `NotFound` — established absence, arrived at through a
/// redirection nobody established — and absence is exactly what lets the last
/// unresolved point be consumed and its parent item removed.
#[test]
#[cfg(unix)]
fn work_reached_through_a_link_does_not_read_as_absence() {
    let (_dir, root) = workspace();
    let item = engr::backlog::create(
        &root,
        "unresolved topic",
        "the only point",
        Vec::new(),
        &engr::backlog::Prepared::first(),
    )
    .expect("backlog item")
    .id;
    let subject = engr::work::Subject::Backlog(item.clone());

    // The sidecar directory is redirected somewhere that holds nothing.
    let backlog_work = engr::work::root_dir(&root).join("backlog");
    let outside = TempDir::new().expect("outside");
    std::fs::remove_dir_all(&backlog_work).expect("clear");
    std::os::unix::fs::symlink(outside.path(), &backlog_work).expect("symlink");

    let error = engr::work::exists(&root, &subject)
        .expect_err("a redirection is not an answer about this workspace");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("link to somewhere else"), "{error}");

    let refused = engr::backlog::consume_section(
        &root,
        &item,
        1,
        &engr::backlog::Prepared::first()
            .against(engr::backlog::Precondition::section(&root, &item, 1).expect("observe")),
    )
    .expect_err("the destructive path must not proceed on an unestablished sidecar tree");
    assert_eq!(refused.code, engr::EXIT_SCHEMA);
    assert_eq!(
        engr::backlog::load(&root, &item)
            .expect("the item survives")
            .sections
            .len(),
        1
    );

    // Put the directory back and the ordinary rule applies again: no sidecar,
    // so the last point resolves.
    std::fs::remove_file(&backlog_work).expect("remove the link");
    std::fs::create_dir_all(&backlog_work).expect("restore");
    assert!(engr::backlog::consume_section(
        &root,
        &item,
        1,
        &engr::backlog::Prepared::first()
            .against(engr::backlog::Precondition::section(&root, &item, 1).expect("observe")),
    )
    .expect("the last point resolves"));
}

/// A recoverable tail is never applied on top of a projection nothing admitted.
///
/// Both halves of this state are individually legitimate: a durable Event whose
/// projection never landed is exactly what a crash leaves, and reconciliation
/// exists to finish it. But reconciliation starts from the *stored* projection,
/// so if those bytes were rewritten and resealed in the meantime, applying the
/// tail builds an admitted revision on top of wording nobody admitted — and then
/// saves it, resealing the unauthorized semantics into a newer revision and
/// destroying the very bytes `repair` would have compared against.
///
/// The prefix check is what distinguishes the two: it compares only Events up to
/// the projection's own revision, so a legitimate unprojected tail is still
/// recoverable and only the predecessor it would be applied to is judged.
#[test]
fn a_crash_tail_is_not_applied_over_a_projection_history_did_not_produce() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "tail over divergence");
    admit(&root, payload(Act::Add, &id, "the admitted wording"));

    // A durable rev-3 Event whose projection never landed: the ordinary crash.
    let prepared = gate::prepare(&root, payload(Act::Add, &id, "admitted but not projected"))
        .expect("prepare");
    let mut action = prepared.candidate.payload.action.clone();
    if let Some(value) = action.value_mut() {
        value.admitted.at = "2026-09-03T00:00:00Z".to_owned();
    }
    let crashed = engr::model::Event::sealed(
        &id,
        engr::model::new_id(),
        action,
        3,
        engr::model::EventAdmission::human("2026-09-03T00:00:00Z", prepared.candidate.code()),
    )
    .expect("a durable Event whose projection never landed");
    append_admitted_raw(&root, &id, &crashed);

    // And then the rev-2 projection is rewritten and resealed, which every seal
    // still accepts.
    let stored = store::load_object(&root, &id).expect("object");
    let resealed = engr::integrity::mutate(&stored, |object| {
        object.sections[0].text = "wording nobody was ever shown".to_owned();
        Ok(())
    })
    .expect("an out-of-band edit can always be resealed");
    write_raw(&store::object_path(&root, &id), &resealed.object).expect("put it on disk");
    let before = std::fs::read(store::object_path(&root, &id)).expect("bytes");

    // Nothing applies the tail on top of that.
    let error = ops::reconcile(&root, &id).expect_err("reconciliation must refuse");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(
        error
            .message
            .contains("not what its admitted history produced"),
        "{error}"
    );
    gate::prepare(&root, payload(Act::Add, &id, "an ordinary change"))
        .expect_err("and so must an ordinary admission");
    let read = ops::effective(&root, &id).expect("a read surface still diagnoses it");
    assert_eq!(read.rev, 2, "the tail was not projected over it");
    assert_eq!(read.sections[0].text, "wording nobody was ever shown");
    assert_eq!(
        std::fs::read(store::object_path(&root, &id)).expect("bytes"),
        before,
        "and the divergent bytes are still there to compare against"
    );

    // Repair restores the projection, and the tail is then ordinary recoverable
    // history again.
    let repair = gate::prepare_repair(&root, &id).expect("repair prepares");
    gate::confirm(&root, &format!("CONFIRM {}", repair.candidate.code())).expect("repair");
    let recovered = ops::reconcile(&root, &id).expect("the tail reconciles once the base is sound");
    assert_eq!(recovered.sections[0].text, "the admitted wording");
    assert!(
        recovered
            .sections
            .iter()
            .any(|section| section.text == "admitted but not projected"),
        "and the durable Event that was waiting is applied"
    );
}

/// A merge cannot consume a Section an EventStore-established Object stands on.
///
/// The guard walked the Object *files*, and that is not the set of Objects this
/// workspace holds. The supported crash window — the Event is durable, the
/// projection never landed — leaves a real admitted Object with no file, which
/// `ops::effective` reconstructs and every read surface shows. A scan of files
/// alone never asks about it, so the merge consumed a Section it explicitly
/// depends on, and Section ids are never reused: the reference it is left
/// holding can never be made good again.
#[test]
fn a_merge_cannot_consume_a_section_an_unprojected_object_depends_on() {
    let (_dir, root) = workspace();
    let target = new_object(&root, "the target");
    admit(&root, payload(Act::Add, &target, "first"));
    admit(&root, payload(Act::Add, &target, "second"));
    let commit = commit_all(&root, "record the target");

    // A referrer whose projection is then lost the way a crash loses it: the
    // Event is durable, the Object file is not.
    let source = new_object(&root, "the referrer");
    let mut with_ref = payload(Act::Add, &source, "stands on §2");
    set_refs(&mut with_ref, vec![text_ref(&root, &target, 2, &commit)]);
    admit(&root, with_ref);
    std::fs::remove_file(store::object_path(&root, &source)).expect("lose the projection");
    assert_eq!(
        ops::effective(&root, &source)
            .expect("history still establishes it")
            .sections
            .len(),
        1,
        "the Object is still there; only its file is gone"
    );

    let error = gate::prepare(
        &root,
        common::merge(&target, 1, vec![2], common::wording("folded together")),
    )
    .expect_err("§2 is depended on by an Object this workspace holds");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(
        error.message.contains("this merge would consume"),
        "{error}"
    );

    // With the projection back, the same merge is still refused — the guard was
    // about the dependency, not about the file.
    ops::reconcile(&root, &source).expect("the projection is recoverable");
    gate::prepare(
        &root,
        common::merge(&target, 1, vec![2], common::wording("folded together")),
    )
    .expect_err("and it stays refused once the projection is back");
}

/// A workspace is current only once it is whole.
///
/// `VERSION` is the entire statement that a workspace is this generation —
/// `require_current` asks nothing else — so writing it before the layout is
/// complete makes a failure in the remaining window leave an *active* workspace
/// missing part of itself, with nothing afterwards to detect or repair it. The
/// part that was last is the one that matters: a live Challenge's filename is
/// its code, and `/local/` is what keeps `git add -A` from publishing it.
#[test]
fn a_workspace_is_not_current_until_its_ignore_line_exists() {
    // Fully qualified: the `TempDir` import is unix-only, because the link tests
    // are, and this one runs everywhere.
    let dir = tempfile::TempDir::new().expect("temp dir");
    let root = dir.path().to_path_buf();
    store::init(&root).expect("init");

    let ignore = store::engr_dir(&root).join(".gitignore");
    let version = store::version_path(&root);
    assert!(ignore.is_file(), "the ignore line is written");
    assert!(
        std::fs::read_to_string(&ignore)
            .expect("ignore")
            .lines()
            .any(|line| line.trim() == "/local/"),
        "and it is the line that keeps live challenge codes out of git"
    );
    // Ordering, established from the filesystem rather than from reading the
    // code: the marker that activates the workspace is not older than the
    // invariant it certifies.
    let ignored_at = std::fs::metadata(&ignore)
        .and_then(|held| held.modified())
        .expect("ignore mtime");
    let activated_at = std::fs::metadata(&version)
        .and_then(|held| held.modified())
        .expect("version mtime");
    assert!(
        ignored_at <= activated_at,
        "the generation marker must be written last: ignore {ignored_at:?}, VERSION {activated_at:?}"
    );
}

/// `verify` names a dependency that moved, and still passes.
///
/// Both halves are the finding. Passing is correct and deliberate: the target
/// changed in a way somebody was entitled to change it, and whether this wording
/// still holds is a judgement nothing can checksum — drift has never been an
/// integrity failure. Saying *nothing* was the defect. `show` reported "refs
/// moved" and `ls` reported "1 stale" at the same instant, so the one surface
/// whose whole job is to answer "is this sound?" was the only one that did not
/// mention the answer's soft spot, while its own help promised "plus
/// dependencies".
#[test]
fn verify_names_a_dependency_that_moved_and_still_passes() {
    let (_dir, root) = workspace();
    let target = new_object(&root, "the target");
    admit(&root, payload(Act::Add, &target, "the original basis"));
    let commit = commit_all(&root, "record target");

    let source = new_object(&root, "the source");
    let mut with_ref = payload(Act::Add, &source, "rests on the basis");
    set_refs(&mut with_ref, vec![text_ref(&root, &target, 1, &commit)]);
    admit(&root, with_ref);

    let clean = ops::verify(&root, &source).expect("verify");
    assert!(clean.passed());
    assert!(clean.drifted.is_empty(), "nothing has moved yet");

    admit(
        &root,
        payload(Act::Revise(1), &target, "the basis, restated differently"),
    );

    let moved = ops::verify(&root, &source).expect("verify after the target moved");
    assert!(
        moved.passed(),
        "drift is a judgement, not an integrity failure"
    );
    assert_eq!(moved.drifted.len(), 1, "and it is reported all the same");
    assert_eq!(moved.drifted[0].section, 1);
    assert_eq!(moved.drifted[0].target, target);
    assert_eq!(moved.drifted[0].target_section, 1);
    assert_eq!(
        moved.drifted[0]
            .fields
            .iter()
            .map(|field| field.as_str())
            .collect::<Vec<_>>(),
        vec!["text"],
        "named down to the selected field that moved"
    );
}
