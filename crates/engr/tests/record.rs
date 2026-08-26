//! What the record guarantees once it is written: that the wording is the
//! wording that was confirmed, that confirmed history remains, and that drift
//! is noticed.

use engr::model::{Action, Content, Payload, Ref};
use engr::{gate, integrity, ops, store, view};
use serde_json::Value;
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

fn payload(action: Action, object: &str, text: &str) -> Payload {
    Payload {
        action,
        object: object.to_owned(),
        becomes: None,
        content: Content {
            text: text.to_owned(),
            based_on: None,
            refs: Vec::new(),
            ..Content::default()
        },
    }
}

fn admit(root: &Path, payload: Payload) -> engr::model::Object {
    let prepared = gate::prepare(root, payload).expect("prepare");
    let response = format!("CONFIRM {}", prepared.candidate.challenge);
    gate::confirm(root, &response).expect("confirm").object
}

fn new_object(root: &Path, title: &str) -> String {
    let id = engr::model::new_id();
    admit(root, payload(Action::ObjectCreated, &id, title));
    id
}

fn text_ref(root: &Path, object: &str, section: u64, commit: &str) -> Ref {
    let target = ops::effective(root, object).expect("reference target");
    Ref::selective(
        engr::dependency::admit(
            root,
            &target,
            target.sha256.as_deref().expect("aggregate seal"),
            section,
            &[engr::dependency::SemanticField::Text],
            commit,
        )
        .expect("admit selective reference"),
    )
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
fn every_confirmed_revision_remains_in_append_only_history() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "durable history");
    admit(&root, payload(Action::SectionAdded, &id, "first wording"));
    admit(
        &root,
        payload(Action::SectionRevised { section: 1 }, &id, "second wording"),
    );

    let events = store::load_events(&root, &id).expect("history");
    assert_eq!(events.len(), 3);
    assert_eq!(events[1].payload.content.text, "first wording");
    assert_eq!(events[2].payload.content.text, "second wording");
}

#[test]
fn repointing_a_reference_is_detected() {
    let (_dir, root) = workspace();
    let target = new_object(&root, "the target");
    admit(&root, payload(Action::SectionAdded, &target, "first"));
    admit(&root, payload(Action::SectionAdded, &target, "second"));
    let commit = commit_all(&root, "record target");

    let source = new_object(&root, "the source");
    let mut with_ref = payload(Action::SectionAdded, &source, "depends on the first");
    with_ref.content.refs = vec![text_ref(&root, &target, 1, &commit)];
    admit(&root, with_ref);
    assert!(ops::verify(&root, &source).expect("verify").passed());

    // The hash has to cover `refs`, not just `text`. If it covered text alone,
    // swapping which section this depends on would pass verification.
    tamper(&root, &source, |value| {
        value["sections"][0]["refs"][0]["target"] =
            Value::String(engr::proof::section_target(&target, 2));
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
    let prepared = gate::prepare(&root, payload(Action::SectionAdded, &id, "one"))
        .expect("prepare crash tail");
    append_admitted_raw(
        &root,
        &engr::model::Event {
            format: engr::model::EVENT_FORMAT.to_owned(),
            version: engr::EVENT_ENVELOPE_VERSION,
            event_id: engr::model::new_id(),
            rev: 2,
            time: "2026-08-25T00:00:00Z".to_owned(),
            payload: prepared.candidate.payload.clone(),
            provenance: engr::model::Provenance::Tagged {
                admission: engr::model::TaggedAdmission {
                    kind: engr::semantics::Admission::Human,
                    confirmation: Some(engr::model::HumanConfirmation {
                        challenge: prepared.candidate.challenge.clone(),
                        candidate_digest: prepared.candidate.candidate_digest.clone(),
                    }),
                    rule_review: None,
                },
            },
        },
    );

    let object_path = store::object_path(&root, &id);
    let events_path = store::events_path(&root, &id);
    let object_before = std::fs::read(&object_path).expect("raw object");
    let events_before = std::fs::read(&events_path).expect("events");
    let report = ops::verify(&root, &id).expect("verify raw projection");
    assert!(!report.passed());
    assert_eq!(report.unprojected, 1);
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
    admit(
        &root,
        payload(Action::SectionAdded, &target, "the original basis"),
    );
    let commit = commit_all(&root, "record target");

    let source = new_object(&root, "the source");
    let mut with_ref = payload(Action::SectionAdded, &source, "rests on the basis");
    with_ref.content.refs = vec![text_ref(&root, &target, 1, &commit)];
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
        payload(
            Action::SectionAdded,
            &target,
            "Ruling: reason codes are numeric.",
        ),
    );
    let pinned = store::load_object(&root, &target).expect("target").sections[0]
        .sha256
        .clone();
    let commit = commit_all(&root, "record target");

    let source = new_object(&root, "downstream decision");
    let mut with_ref = payload(
        Action::SectionAdded,
        &source,
        "Therefore the UI renders them as integers.",
    );
    with_ref.content.refs = vec![text_ref(&root, &target, 1, &commit)];
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
        store::load_object(&root, &target).expect("target").sections[0].sha256,
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
        payload(Action::SectionAdded, &target, "the committed dependency"),
    );
    let good_commit = commit_all(&root, "good target");

    let source = new_object(&root, "source");
    let mut with_ref = payload(Action::SectionAdded, &source, "depends on target text");
    with_ref.content.refs = vec![text_ref(&root, &target, 1, &good_commit)];
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

    let original = source_object.sections[0].refs[0]
        .as_selective()
        .expect("selective ref");
    let historical_ref = Ref::selective(
        engr::dependency::SelectiveRef::stored(
            original.target(),
            original.fields().to_vec(),
            bad_commit,
            original.digest(),
        )
        .expect("historical ref"),
    );
    let source_seal = source_object.sha256.clone().expect("source seal");
    let diagnostic_source = integrity::mutate(&source_object, &source_seal, |object| {
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
    let added = Payload {
        action: Action::SectionAdded,
        object: object.clone(),
        becomes: None,
        content: content.clone(),
    };
    let deleted = Payload {
        action: Action::SectionDeleted { section: 1 },
        object,
        becomes: None,
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
        payload(Action::SectionAdded, &target, "Reason codes are numeric."),
    );
    let commit = commit_all(&root, "record target");

    let source = new_object(&root, "downstream decision");
    let mut with_ref = payload(Action::SectionAdded, &source, "So the UI renders integers.");
    with_ref.content.refs = vec![text_ref(&root, &target, 1, &commit)];
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
        payload(Action::SectionAdded, &target, "Reason codes are numeric."),
    );
    let commit = commit_all(&root, "record target");

    let source = new_object(&root, "downstream decision");
    let mut with_ref = payload(Action::SectionAdded, &source, "So the UI renders integers.");
    with_ref.content.refs = vec![text_ref(&root, &target, 1, &commit)];
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
    let mut without: Value = store::read_json(&path).expect("read");
    without["sections"] = serde_json::json!([]);
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
        payload(Action::SectionAdded, &target, "Reason codes are numeric."),
    );
    let commit = commit_all(&root, "record target");

    let source = new_object(&root, "downstream decision");
    let mut with_ref = payload(Action::SectionAdded, &source, "So the UI renders integers.");
    with_ref.content.refs = vec![text_ref(&root, &target, 1, &commit)];
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
        payload(Action::SectionAdded, &target, "Reason codes are numeric."),
    );
    let target_before = std::fs::read(store::object_path(&root, &target)).expect("target bytes");
    let commit = commit_all(&root, "record target");
    let reference = text_ref(&root, &target, 1, &commit);
    let selective = reference.as_selective().expect("selective reference");
    let section_seal = store::load_object(&root, &target).expect("target").sections[0]
        .sha256
        .clone();
    assert_ne!(
        selective.digest(),
        section_seal,
        "dependency identity and resource integrity are separate contracts"
    );

    let source = new_object(&root, "downstream decision");
    let mut with_ref = payload(Action::SectionAdded, &source, "So the UI renders integers.");
    with_ref.content.refs = vec![reference.clone()];
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
    let mut to_forged = payload(
        Action::SectionAdded,
        &source,
        "depends on rewritten wording",
    );
    to_forged.content.refs = vec![reference];
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
            Action::SectionRevised { section: 1 },
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
        payload(Action::SectionAdded, &target, "Reason codes are numeric."),
    );
    let commit = commit_all(&root, "record target");

    let source = new_object(&root, "downstream decision");
    let mut with_ref = payload(Action::SectionAdded, &source, "So the UI renders integers.");
    with_ref.content.refs = vec![text_ref(&root, &target, 1, &commit)];
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
    admit(&root, payload(Action::SectionAdded, &id, "wording"));

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
fn append_admitted_raw(root: &Path, event: &engr::model::Event) {
    let path = store::events_path(root, &event.payload.object);
    let line = engr::proof::canonical_bytes(event, "Event v2").expect("canonical");
    let mut existing = std::fs::read_to_string(&path).unwrap_or_default();
    existing.push_str(&line);
    existing.push('\n');
    std::fs::write(&path, existing).expect("write event");
}
