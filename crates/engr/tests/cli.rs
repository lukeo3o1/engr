//! What the command line promises the outside world.

use engr::{
    gate,
    model::{Action, Confirmation, Content, Event, Object, Payload, EVENT_FORMAT},
    ops, store,
};
use serde_json::Value;
use std::fs::OpenOptions;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::Duration;
use tempfile::TempDir;

fn run_engr(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_engr"))
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .expect("run engr")
}

fn prepare(root: &Path, args: &[&str]) -> Value {
    let mut args = args.to_vec();
    args.push("--json");
    let output = run_engr(root, &args);
    assert!(
        output.status.success(),
        "prepare failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("prepare prints a candidate JSON document")
}

fn confirm(root: &Path, candidate: &Value) {
    let challenge = candidate["challenge"]
        .as_str()
        .expect("candidate challenge");
    let response = format!("CONFIRM {challenge}");
    let output = run_engr(root, &["confirm", &response]);
    assert!(
        output.status.success(),
        "confirm failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Commit with an identity supplied on the command line.
///
/// A bare `git commit` reads the committer from global config, which exists on a
/// developer machine and on some CI images and not on others. Supplying it per
/// invocation is what the other tests here already do; this is that, named once.
fn commit_as_test(root: &Path, message: &str) {
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
}

fn rewrite_object(root: &Path, id: &str, update: impl FnOnce(&mut serde_json::Map<String, Value>)) {
    let path = store::object_path(root, id);
    let mut value: Value =
        serde_json::from_slice(&std::fs::read(&path).expect("object")).expect("json");
    update(value.as_object_mut().expect("object"));
    std::fs::write(&path, serde_json::to_vec_pretty(&value).expect("json")).expect("object");
}

fn mark_legacy(root: &Path, id: &str) {
    rewrite_object(root, id, |object| {
        object.insert("format".to_owned(), Value::String("engr-object".to_owned()));
        object.insert("version".to_owned(), Value::from(1));
        let state = object.remove("state").expect("state");
        object.insert("status".to_owned(), state);
    });
}

fn assert_migration_preflight_refuses(corrupt: impl FnOnce(&Path, &str, &str)) {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let first = prepare(root, &["prepare", "--new", "--text", "first legacy object"]);
    confirm(root, &first);
    let first_id = first["object"].as_str().expect("object");
    let first_section = prepare(
        root,
        &[
            "prepare",
            "--object",
            first_id,
            "--add",
            "--text",
            "first retained section",
            "--no-based-on",
        ],
    );
    confirm(root, &first_section);
    let second = prepare(
        root,
        &["prepare", "--new", "--text", "second legacy object"],
    );
    confirm(root, &second);
    let second_id = second["object"].as_str().expect("object");
    mark_legacy(root, first_id);
    mark_legacy(root, second_id);
    corrupt(root, first_id, second_id);

    let paths = [
        store::engr_dir(root).join("format.json"),
        store::object_path(root, first_id),
        store::object_path(root, second_id),
        store::events_path(root, first_id),
        store::events_path(root, second_id),
    ];
    let before: Vec<_> = paths
        .iter()
        .map(|path| (path.clone(), std::fs::read(path).expect("snapshot")))
        .collect();

    let output = run_engr(root, &["migrate"]);
    assert_eq!(
        output.status.code(),
        Some(engr::EXIT_SCHEMA),
        "migration unexpectedly succeeded: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    for (path, expected) in before {
        assert_eq!(
            std::fs::read(&path).expect("snapshot after refusal"),
            expected,
            "{} changed despite a failed migration",
            path.display()
        );
    }
}

#[test]
fn reference_admission_uses_the_effective_target_projection() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "tests@example.com"]);
    git(root, &["config", "user.name", "engr tests"]);
    store::init(root).expect("init");

    let target = engr::model::new_id();
    let create_target = gate::prepare(
        root,
        Payload {
            action: Action::ObjectCreated,
            object: target.clone(),
            becomes: None,
            content: Content {
                text: "target".to_owned(),
                based_on: None,
                refs: Vec::new(),
                ..Content::default()
            },
        },
    )
    .expect("prepare target");
    gate::confirm(
        root,
        &format!("CONFIRM {}", create_target.candidate.challenge),
    )
    .expect("confirm target");
    let initial_section = gate::prepare(
        root,
        Payload {
            action: Action::SectionAdded,
            object: target.clone(),
            becomes: None,
            content: Content {
                text: "projection wording".to_owned(),
                based_on: None,
                refs: Vec::new(),
                ..Content::default()
            },
        },
    )
    .expect("prepare section");
    gate::confirm(
        root,
        &format!("CONFIRM {}", initial_section.candidate.challenge),
    )
    .expect("confirm section");
    git(root, &["add", ".engr"]);
    git(root, &["commit", "-qm", "record projected target"]);
    let old_commit = engr::git::head(root).expect("head");
    let raw = store::load_object(root, &target)
        .expect("raw target")
        .section(1)
        .expect("raw section")
        .clone();

    let revision = gate::prepare(
        root,
        Payload {
            action: Action::SectionRevised { section: 1 },
            object: target.clone(),
            becomes: None,
            content: Content {
                text: "effective crash-tail wording".to_owned(),
                based_on: None,
                refs: Vec::new(),
                ..Content::default()
            },
        },
    )
    .expect("prepare revision");
    let revision_event = Event {
        format: EVENT_FORMAT.to_owned(),
        version: engr::EVENT_ENVELOPE_VERSION_V0,
        event_id: engr::model::new_id(),
        rev: revision.candidate.binding.expected_rev + 1,
        time: "2026-08-17T00:00:00Z".to_owned(),
        payload: revision.candidate.payload.clone(),
        confirmation: Confirmation {
            challenge: revision.candidate.challenge.clone(),
            payload_sha256: revision.candidate.payload_sha256.clone(),
        },
    };
    store::append_event(root, &revision_event).expect("append without projection");
    let effective = ops::effective_section(root, &target, 1).expect("effective target section");
    assert_ne!(effective.sha256, raw.sha256);

    let source = engr::model::new_id();
    let create_source = gate::prepare(
        root,
        Payload {
            action: Action::ObjectCreated,
            object: source.clone(),
            becomes: None,
            content: Content {
                text: "source".to_owned(),
                based_on: None,
                refs: Vec::new(),
                ..Content::default()
            },
        },
    )
    .expect("prepare source");
    gate::confirm(
        root,
        &format!("CONFIRM {}", create_source.candidate.challenge),
    )
    .expect("confirm source");

    let stale_projection_ref = Payload {
        action: Action::SectionAdded,
        object: source.clone(),
        becomes: None,
        content: Content {
            text: "cannot pin stale projection".to_owned(),
            based_on: None,
            refs: vec![engr::model::Ref {
                object: target.clone(),
                section: 1,
                sha256: raw.sha256,
                commit: old_commit.clone(),
            }],
            ..Content::default()
        },
    };
    let error = gate::prepare(root, stale_projection_ref)
        .expect_err("gate must reject a stale raw projection reference");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("that section is now"));

    let cli = run_engr(
        root,
        &[
            "prepare",
            "--object",
            &source,
            "--add",
            "--text",
            "CLI must use effective wording",
            "--no-based-on",
            "--ref",
            &format!("{target}:1"),
        ],
    );
    assert_eq!(cli.status.code(), Some(engr::EXIT_INVARIANT));
    assert!(
        String::from_utf8_lossy(&cli.stderr).contains("commit the target wording first"),
        "CLI must not hash the stale projection: {}",
        String::from_utf8_lossy(&cli.stderr)
    );

    store::save_object(
        root,
        &ops::effective(root, &target).expect("effective object"),
    )
    .expect("repair projection");
    git(root, &["add", ".engr"]);
    git(root, &["commit", "-qm", "record recovered target"]);
    let committed_effective = engr::git::head(root).expect("new head");
    gate::prepare(
        root,
        Payload {
            action: Action::SectionAdded,
            object: source,
            becomes: None,
            content: Content {
                text: "historically verified effective wording".to_owned(),
                based_on: None,
                refs: vec![engr::model::Ref {
                    object: target,
                    section: 1,
                    sha256: effective.sha256,
                    commit: committed_effective,
                }],
                ..Content::default()
            },
        },
    )
    .expect("the committed effective wording remains referenceable");
}

#[test]
fn candidate_display_distinguishes_retryable_from_stale() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let id = engr::model::new_id();
    let created = gate::prepare(
        root,
        Payload {
            action: Action::ObjectCreated,
            object: id.clone(),
            becomes: None,
            content: Content {
                text: "candidate state".to_owned(),
                based_on: None,
                refs: Vec::new(),
                ..Content::default()
            },
        },
    )
    .expect("prepare object");
    gate::confirm(root, &format!("CONFIRM {}", created.candidate.challenge))
        .expect("confirm object");

    let retryable = gate::prepare(
        root,
        Payload {
            action: Action::SectionAdded,
            object: id.clone(),
            becomes: None,
            content: Content {
                text: "apply once".to_owned(),
                based_on: None,
                refs: Vec::new(),
                ..Content::default()
            },
        },
    )
    .expect("prepare retryable candidate");
    let retry_code = retryable.candidate.challenge.clone();
    gate::confirm(root, &format!("CONFIRM {retry_code}")).expect("apply candidate");
    store::write_json(
        &store::candidate_path(root, &retry_code).expect("candidate path"),
        &retryable.candidate,
    )
    .expect("restore candidate after deletion crash");

    let shown = run_engr(root, &["candidate", &retry_code]);
    assert!(shown.status.success());
    let shown_text = String::from_utf8_lossy(&shown.stdout);
    assert!(shown_text.contains("already applied"));
    assert!(!shown_text.contains("dead"));
    assert!(!shown_text.contains("Prepare again"));
    let listed = run_engr(root, &["candidate"]);
    assert!(String::from_utf8_lossy(&listed.stdout).contains("retry"));

    let confirmation = run_engr(root, &["confirm", &format!("CONFIRM {retry_code}")]);
    assert!(
        confirmation.status.success(),
        "retry: {}",
        String::from_utf8_lossy(&confirmation.stderr)
    );
    assert_eq!(
        store::load_events(root, &id).expect("events").len(),
        2,
        "idempotent retry must not append another event"
    );
    assert_eq!(
        ops::effective(root, &id).expect("object").sections.len(),
        1,
        "idempotent retry must not add another section"
    );

    let stale = gate::prepare(
        root,
        Payload {
            action: Action::SectionAdded,
            object: id.clone(),
            becomes: None,
            content: Content {
                text: "stale candidate".to_owned(),
                based_on: None,
                refs: Vec::new(),
                ..Content::default()
            },
        },
    )
    .expect("prepare stale candidate");
    let stale_code = stale.candidate.challenge.clone();
    let overtaking = gate::prepare(
        root,
        Payload {
            action: Action::SectionAdded,
            object: id,
            becomes: None,
            content: Content {
                text: "overtaking mutation".to_owned(),
                based_on: None,
                refs: Vec::new(),
                ..Content::default()
            },
        },
    )
    .expect("prepare overtaking candidate");
    gate::confirm(root, &format!("CONFIRM {}", overtaking.candidate.challenge))
        .expect("confirm overtaking candidate");
    store::write_json(
        &store::candidate_path(root, &stale_code).expect("candidate path"),
        &stale.candidate,
    )
    .expect("restore overtaken candidate");

    let stale_view = run_engr(root, &["candidate", &stale_code]);
    assert!(stale_view.status.success());
    assert!(String::from_utf8_lossy(&stale_view.stdout).contains("dead"));
    let stale_list = run_engr(root, &["candidate"]);
    assert!(String::from_utf8_lossy(&stale_list.stdout).contains("stale"));
}

#[test]
fn legacy_workspace_is_readable_but_requires_explicit_migration_to_mutate() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let created = prepare(root, &["prepare", "--new", "--text", "legacy object"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id");
    let current = prepare(root, &["prepare", "--new", "--text", "already current"]);
    confirm(root, &current);
    let current_id = current["object"].as_str().expect("current object id");
    let object_path = store::object_path(root, id);
    let current_path = store::object_path(root, current_id);
    let current_before = std::fs::read(&current_path).expect("current object");
    let events_path = store::events_path(root, id);
    let events_before = std::fs::read(&events_path).expect("events");
    let format_path = store::engr_dir(root).join("format.json");
    std::fs::write(&format_path, r#"{"format":"engr-workspace","version":1}"#).expect("format");
    let format_before = std::fs::read(&format_path).expect("format");
    let mut object: Value =
        serde_json::from_slice(&std::fs::read(&object_path).expect("object")).expect("json");
    object["format"] = Value::String("engr-object".to_owned());
    object["version"] = Value::from(1);
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
    assert!(
        format_path.exists(),
        "Phase 0 already had the version 1 workspace authority"
    );
    let shown = run_engr(root, &["show", id]);
    assert!(
        shown.status.success(),
        "legacy read: {}",
        String::from_utf8_lossy(&shown.stderr)
    );
    let refused = run_engr(root, &["prepare", "--object", id, "--close"]);
    assert_eq!(refused.status.code(), Some(engr::EXIT_SCHEMA));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("engr migrate"));
    let still_legacy: Value =
        serde_json::from_slice(&std::fs::read(&object_path).expect("object")).expect("json");
    assert_eq!(still_legacy["status"], "open");
    assert!(still_legacy.get("state").is_none());

    let migrated = run_engr(root, &["migrate"]);
    assert!(
        migrated.status.success(),
        "migration: {}",
        String::from_utf8_lossy(&migrated.stderr)
    );
    let migrated_object: Value =
        serde_json::from_slice(&std::fs::read(&object_path).expect("object")).expect("json");
    assert_eq!(migrated_object["state"], "open");
    assert!(migrated_object.get("status").is_none());
    assert_eq!(
        std::fs::read(&current_path).expect("current object after migration"),
        current_before,
        "an already current Object is not cosmetically rewritten"
    );
    assert_eq!(
        std::fs::read(&events_path).expect("events after migration"),
        events_before,
        "compatible retained Event history is not rewritten"
    );
    assert_ne!(
        std::fs::read(&format_path).expect("format after migration"),
        format_before,
        "the workspace authority moves forward, because version 1 no longer denotes what this build writes"
    );
    assert_eq!(
        migrated_object["format"], "engr-object",
        "compatible legacy marker is preserved"
    );
    assert_eq!(
        migrated_object["version"], 1,
        "the Object's own legacy envelope marker is compatible and is not touched"
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&format_path).expect("format"))
            .expect("json")["version"],
        engr::WORKSPACE_VERSION,
        "and the workspace authority now names the version this build writes"
    );
}

#[test]
fn migration_refuses_an_invalid_legacy_status_without_partial_rewrites() {
    assert_migration_preflight_refuses(|root, _first, second| {
        rewrite_object(root, second, |object| {
            object.insert("status".to_owned(), Value::String("waiting".to_owned()));
        });
    });
}

#[test]
fn migration_refuses_a_legacy_filename_id_mismatch_without_partial_rewrites() {
    assert_migration_preflight_refuses(|root, first, second| {
        rewrite_object(root, second, |object| {
            object.insert("id".to_owned(), Value::String(first.to_owned()));
        });
    });
}

#[test]
fn migration_refuses_a_legacy_object_missing_a_required_field_without_partial_rewrites() {
    assert_migration_preflight_refuses(|root, _first, second| {
        rewrite_object(root, second, |object| {
            object.remove("title");
        });
    });
}

#[test]
fn migration_refuses_a_malformed_legacy_object_id_without_partial_rewrites() {
    assert_migration_preflight_refuses(|root, _first, second| {
        rewrite_object(root, second, |object| {
            object.insert("id".to_owned(), Value::String("not-a-uuid".to_owned()));
        });
    });
}

#[test]
fn migration_refuses_a_non_v7_legacy_object_id_without_partial_rewrites() {
    assert_migration_preflight_refuses(|root, _first, second| {
        rewrite_object(root, second, |object| {
            object.insert(
                "id".to_owned(),
                Value::String("550e8400-e29b-41d4-a716-446655440000".to_owned()),
            );
        });
    });
}

#[test]
fn migration_refuses_duplicate_legacy_section_ids_without_partial_rewrites() {
    assert_migration_preflight_refuses(|root, first, _second| {
        rewrite_object(root, first, |object| {
            let sections = object
                .get_mut("sections")
                .and_then(Value::as_array_mut)
                .expect("sections");
            let duplicate = sections.first().expect("first section").clone();
            sections.push(duplicate);
        });
    });
}

#[test]
fn migration_refuses_a_legacy_section_counter_collision_without_partial_rewrites() {
    assert_migration_preflight_refuses(|root, first, _second| {
        rewrite_object(root, first, |object| {
            object.insert("next_section_id".to_owned(), Value::from(1));
        });
    });
}

#[test]
fn migration_refuses_unreadable_retained_events_without_partial_rewrites() {
    assert_migration_preflight_refuses(|root, _first, second| {
        let path = store::events_path(root, second);
        let mut event: Value =
            serde_json::from_slice(&std::fs::read(&path).expect("event")).expect("json");
        event["version"] = Value::from(99);
        std::fs::write(&path, serde_json::to_vec(&event).expect("json")).expect("event");
    });
}

#[test]
fn migration_refuses_retained_events_that_cannot_reconcile_without_partial_rewrites() {
    assert_migration_preflight_refuses(|root, _first, second| {
        rewrite_object(root, second, |object| {
            object.insert("rev".to_owned(), Value::from(0));
        });
        let path = store::events_path(root, second);
        let mut event: Event =
            serde_json::from_slice(&std::fs::read(&path).expect("event")).expect("json");
        event.payload.action = Action::SectionDeleted { section: 1 };
        event.payload.content = Content {
            text: String::new(),
            based_on: None,
            refs: Vec::new(),
            ..Content::default()
        };
        event.confirmation.payload_sha256 = event.payload.sha256().expect("payload hash");
        std::fs::write(&path, serde_json::to_vec(&event).expect("json")).expect("event");
    });
}

#[test]
fn legacy_crash_tail_reads_the_effective_object_without_writing() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let created = prepare(root, &["prepare", "--new", "--text", "crash-tail object"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id");
    let added = prepare(
        root,
        &[
            "prepare",
            "--object",
            id,
            "--add",
            "--text",
            "recovered confirmed wording",
            "--no-based-on",
        ],
    );
    confirm(root, &added);

    mark_legacy(root, id);
    rewrite_object(root, id, |object| {
        object.insert("rev".to_owned(), Value::from(1));
        object.insert("next_section_id".to_owned(), Value::from(1));
        object.insert("sections".to_owned(), Value::Array(Vec::new()));
    });

    let format_path = store::engr_dir(root).join("format.json");
    let object_path = store::object_path(root, id);
    let events_path = store::events_path(root, id);
    let before = [
        (
            format_path.clone(),
            std::fs::read(&format_path).expect("format"),
        ),
        (
            object_path.clone(),
            std::fs::read(&object_path).expect("object"),
        ),
        (
            events_path.clone(),
            std::fs::read(&events_path).expect("events"),
        ),
    ];
    let lock_path = store::engr_dir(root).join("lock");
    if lock_path.exists() {
        std::fs::remove_file(&lock_path).expect("remove old writer lock");
    }

    let shown = run_engr(root, &["show", id]);
    assert!(
        shown.status.success(),
        "legacy crash-tail show failed: {}",
        String::from_utf8_lossy(&shown.stderr)
    );
    assert!(
        String::from_utf8_lossy(&shown.stdout).contains("recovered confirmed wording"),
        "show must render the confirmed recovery tail"
    );
    let listed = run_engr(root, &["ls", "--sections"]);
    assert!(listed.status.success(), "legacy crash-tail ls failed");
    assert!(String::from_utf8_lossy(&listed.stdout).contains("recovered confirmed wording"));
    let verified = run_engr(root, &["verify", id]);
    assert_eq!(verified.status.code(), Some(engr::EXIT_INVARIANT));
    assert!(String::from_utf8_lossy(&verified.stdout).contains("FAIL"));
    assert!(
        String::from_utf8_lossy(&verified.stdout).contains("1 events are not reflected"),
        "a legacy read may recover in memory but verify must not call the raw projection synchronized"
    );

    assert!(
        !lock_path.exists(),
        "a legacy read must not create the workspace writer lock"
    );
    for (path, expected) in before {
        assert_eq!(
            std::fs::read(&path).expect("snapshot after read"),
            expected,
            "{} changed while reading a legacy crash tail",
            path.display()
        );
    }
}

#[test]
fn runtime_and_migration_reject_the_same_future_event_gap() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let created = prepare(root, &["prepare", "--new", "--text", "gap object"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id");
    mark_legacy(root, id);

    let events_path = store::events_path(root, id);
    let mut event: Event =
        serde_json::from_slice(&std::fs::read(&events_path).expect("event")).expect("json");
    event.rev = 3;
    std::fs::write(&events_path, serde_json::to_vec(&event).expect("json")).expect("event");
    let object_path = store::object_path(root, id);
    let format_path = store::engr_dir(root).join("format.json");
    let before = [
        (
            object_path.clone(),
            std::fs::read(&object_path).expect("object"),
        ),
        (
            events_path.clone(),
            std::fs::read(&events_path).expect("events"),
        ),
        (
            format_path.clone(),
            std::fs::read(&format_path).expect("format"),
        ),
    ];

    let read = run_engr(root, &["show", id]);
    assert_eq!(
        read.status.code(),
        Some(engr::EXIT_SCHEMA),
        "a future revision gap is corrupt stored recovery data: {}",
        String::from_utf8_lossy(&read.stderr)
    );
    let migrate = run_engr(root, &["migrate"]);
    assert_eq!(migrate.status.code(), Some(engr::EXIT_SCHEMA));
    for (path, expected) in before {
        assert_eq!(
            std::fs::read(&path).expect("snapshot after refusal"),
            expected,
            "{} changed despite the rejected future gap",
            path.display()
        );
    }
}

#[test]
fn the_coordinated_phase_three_generation_is_implemented_and_not_written() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let created = prepare(root, &["prepare", "--new", "--text", "durable boundary"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object").to_owned();
    let section = prepare(
        root,
        &[
            "prepare",
            "--object",
            &id,
            "--add",
            "--text",
            "wording",
            "--no-based-on",
        ],
    );
    confirm(root, &section);

    // A version has exactly one canonical interpretation for current resources.
    // Version 3 is being implemented in slices, so nothing here may claim it:
    // not the authority, not a resource, not a record.
    let authority: Value = serde_json::from_slice(
        &std::fs::read(store::engr_dir(root).join("format.json")).expect("format"),
    )
    .expect("json");
    assert_eq!(authority["version"], Value::from(engr::WORKSPACE_VERSION));
    assert_ne!(
        authority["version"],
        Value::from(engr::PHASE_3_WORKSPACE_VERSION),
        "an unfinished generation is never what a workspace says it is"
    );

    let object: Value =
        serde_json::from_slice(&std::fs::read(store::object_path(root, &id)).expect("object"))
            .expect("json");
    let stored = object["sections"][0].as_object().expect("section");
    assert!(stored.contains_key("confirmed_at"));
    for absent in ["admission", "admitted_at", "sha256_object"] {
        assert!(
            !stored.contains_key(absent),
            "{absent} belongs to the coordinated Phase-3 contract, which is not durable yet"
        );
    }

    let events = std::fs::read_to_string(store::events_path(root, &id)).expect("events");
    for line in events.lines() {
        let event: Value = serde_json::from_str(line).expect("event");
        assert_eq!(
            event["version"],
            Value::from(engr::EVENT_ENVELOPE_VERSION_V0)
        );
        assert!(event.get("admission").is_none());
    }

    // And the model that generation describes is nevertheless here, and works.
    let loaded = store::load_object(root, &id).expect("object");
    assert_eq!(
        loaded.sections[0].admission,
        engr::semantics::Admission::Human,
        "at this version the human gate is the only door, so that is what every section came through"
    );
    assert!(!loaded.sections[0].admitted_at.is_empty());
}

#[test]
fn show_json_uses_state_for_the_object_and_status_for_each_section() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let created = prepare(root, &["prepare", "--new", "--text", "json contract"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object");
    let section = prepare(
        root,
        &[
            "prepare",
            "--object",
            id,
            "--add",
            "--text",
            "checked section",
            "--no-based-on",
        ],
    );
    confirm(root, &section);

    let output = run_engr(root, &["show", id, "--format", "json"]);
    assert!(
        output.status.success(),
        "show json failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let shown: Value = serde_json::from_slice(&output.stdout).expect("show JSON");
    assert_eq!(shown["state"], "open");
    assert!(shown.get("status").is_none());
    assert_eq!(shown["sections"][0]["status"], "ok");
}

#[test]
fn malformed_canonical_references_are_usage_errors_at_the_cli_boundary() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let created = prepare(root, &["prepare", "--new", "--text", "reference input"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id");
    let malformed = "engr:obj:not-a-compact-uuid";

    let show = run_engr(root, &["show", malformed]);
    assert_eq!(show.status.code(), Some(engr::EXIT_USAGE));

    let object = run_engr(root, &["prepare", "--object", malformed, "--close"]);
    assert_eq!(object.status.code(), Some(engr::EXIT_USAGE));

    let reference = run_engr(
        root,
        &[
            "prepare",
            "--object",
            id,
            "--add",
            "--text",
            "dependent wording",
            "--no-based-on",
            "--ref",
            malformed,
        ],
    );
    assert_eq!(
        reference.status.code(),
        Some(engr::EXIT_USAGE),
        "{}",
        String::from_utf8_lossy(&reference.stderr)
    );
}

#[test]
fn whole_object_arguments_reject_valid_but_unsupported_selectors_as_usage() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let created = prepare(root, &["prepare", "--new", "--text", "selector input"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id");
    let compact = engr::reference::encode_uuid(uuid::Uuid::parse_str(id).expect("uuid"));
    let with_section = format!("engr:obj:{compact}:3");
    let with_snapshot = format!("engr:obj:{compact}@{}", "a".repeat(40));

    for spec in [&with_section, &with_snapshot] {
        assert_eq!(
            run_engr(root, &["show", spec]).status.code(),
            Some(engr::EXIT_USAGE)
        );
        assert_eq!(
            run_engr(root, &["prepare", "--object", spec, "--close"])
                .status
                .code(),
            Some(engr::EXIT_USAGE)
        );
    }
}

#[test]
fn title_actions_reject_references_as_cli_usage() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");

    let created = run_engr(
        root,
        &[
            "prepare",
            "--new",
            "--text",
            "title",
            "--ref",
            "not-a-reference",
        ],
    );
    assert_eq!(created.status.code(), Some(engr::EXIT_USAGE));

    let object = prepare(root, &["prepare", "--new", "--text", "existing title"]);
    confirm(root, &object);
    let id = object["object"].as_str().expect("object");
    let renamed = run_engr(
        root,
        &[
            "prepare",
            "--object",
            id,
            "--rename",
            "--text",
            "renamed title",
            "--ref",
            "not-a-reference",
        ],
    );
    assert_eq!(renamed.status.code(), Some(engr::EXIT_USAGE));
}

#[test]
fn unknown_workspace_version_refuses_mutation() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    std::fs::write(
        store::engr_dir(root).join("format.json"),
        r#"{"format":"engr-workspace","version":99}"#,
    )
    .expect("format");
    let output = run_engr(root, &["prepare", "--new", "--text", "no guessing"]);
    assert_eq!(output.status.code(), Some(engr::EXIT_SCHEMA));
}

#[test]
fn dirty_source_requires_an_explicit_repository_basis_choice() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    git(root, &["init", "-q"]);
    std::fs::write(root.join("source.txt"), "committed\n").expect("source");
    git(root, &["add", "."]);
    git(
        root,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "baseline",
        ],
    );

    let created = prepare(root, &["prepare", "--new", "--text", "basis choices"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id");
    std::fs::write(root.join("source.txt"), "dirty\n").expect("source");

    let rejected = run_engr(
        root,
        &["prepare", "--object", id, "--add", "--text", "assertion"],
    );
    assert_eq!(rejected.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("--no-based-on"));

    let explicit = prepare(
        root,
        &[
            "prepare",
            "--object",
            id,
            "--add",
            "--text",
            "external assertion",
            "--no-based-on",
        ],
    );
    assert!(
        explicit.get("based_on").is_none(),
        "no basis is represented by an absent field"
    );
}

#[test]
fn revision_candidate_renders_a_contextual_unified_diff() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let created = prepare(root, &["prepare", "--new", "--text", "diffs"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id");
    let old = (1..=20)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let added = prepare(
        root,
        &[
            "prepare",
            "--object",
            id,
            "--add",
            "--text",
            &old,
            "--no-based-on",
        ],
    );
    confirm(root, &added);
    let revised = old.replace("line 10", "line ten");
    let output = run_engr(
        root,
        &[
            "prepare",
            "--object",
            id,
            "--revise",
            "1",
            "--text",
            &revised,
            "--no-based-on",
        ],
    );
    assert!(output.status.success());
    let rendered = String::from_utf8_lossy(&output.stdout);
    assert!(rendered.contains("@@"));
    assert!(rendered.contains("-line 10"));
    assert!(rendered.contains("+line ten"));
    assert!(
        !rendered.contains(" line 1\n"),
        "distant context was not omitted"
    );

    let appended = format!("{old}\nline 21");
    let output = run_engr(
        root,
        &[
            "prepare",
            "--object",
            id,
            "--revise",
            "1",
            "--text",
            &appended,
            "--no-based-on",
        ],
    );
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("+line 21"));

    let removed = old.replace("line 10\n", "");
    let output = run_engr(
        root,
        &[
            "prepare",
            "--object",
            id,
            "--revise",
            "1",
            "--text",
            &removed,
            "--no-based-on",
        ],
    );
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("-line 10"));
}

#[test]
fn revision_candidate_renders_basis_and_reference_changes() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    git(root, &["init", "-q"]);
    git(root, &["add", "."]);
    git(
        root,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "workspace",
        ],
    );
    let basis = String::from_utf8(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("git")
            .stdout,
    )
    .expect("utf8");
    let basis = basis.trim();

    let target = prepare(root, &["prepare", "--new", "--text", "target"]);
    confirm(root, &target);
    let target_id = target["object"].as_str().expect("target id");
    let target_section = prepare(
        root,
        &[
            "prepare",
            "--object",
            target_id,
            "--add",
            "--text",
            "pinned wording",
            "--no-based-on",
        ],
    );
    confirm(root, &target_section);
    git(root, &["add", ".engr"]);
    git(
        root,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "target wording",
        ],
    );

    let source = prepare(root, &["prepare", "--new", "--text", "source"]);
    confirm(root, &source);
    let source_id = source["object"].as_str().expect("source id");
    let source_section = prepare(
        root,
        &[
            "prepare",
            "--object",
            source_id,
            "--add",
            "--text",
            "dependent wording",
            "--based-on",
            basis,
        ],
    );
    confirm(root, &source_section);

    let reference = format!("{target_id}:1");
    let added_ref = prepare(
        root,
        &[
            "prepare",
            "--object",
            source_id,
            "--revise",
            "1",
            "--text",
            "dependent wording",
            "--no-based-on",
            "--ref",
            &reference,
        ],
    );
    let code = added_ref["challenge"].as_str().expect("challenge");
    let rendered = run_engr(root, &["candidate", code]);
    let rendered = String::from_utf8_lossy(&rendered.stdout);
    assert!(rendered.contains("Based on -"));
    assert!(rendered.contains("Based on + none (explicit)"));
    assert!(rendered.contains("Ref      +"));
    assert!(rendered.contains("sha256"));
    assert!(rendered.contains("commit"));
    confirm(root, &added_ref);

    let removed_ref = prepare(
        root,
        &[
            "prepare",
            "--object",
            source_id,
            "--revise",
            "1",
            "--text",
            "dependent wording",
            "--no-based-on",
        ],
    );
    let code = removed_ref["challenge"].as_str().expect("challenge");
    let rendered = run_engr(root, &["candidate", code]);
    assert!(String::from_utf8_lossy(&rendered.stdout).contains("Ref      -"));
}

#[test]
fn implicit_head_fails_when_source_cleanliness_is_unknown() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    git(root, &["init", "-q"]);
    git(root, &["add", "."]);
    git(
        root,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "baseline",
        ],
    );
    let created = prepare(root, &["prepare", "--new", "--text", "unknown clean state"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id");
    std::fs::write(root.join(".git/index"), "not an index").expect("corrupt index");

    let output = run_engr(
        root,
        &["prepare", "--object", id, "--add", "--text", "wording"],
    );
    assert_eq!(output.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&output.stderr).contains("could not determine"));
}

fn event_workspace() -> (TempDir, std::path::PathBuf, Event) {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path().to_path_buf();
    store::init(&root).expect("init");
    let object =
        Object::new(engr::model::new_id(), "event validation".to_owned()).expect("new object");
    let id = object.id.clone();
    store::save_object(&root, &object).expect("save object");
    let payload = Payload {
        action: Action::SectionAdded,
        object: id,
        becomes: None,
        content: Content {
            text: "event wording".to_owned(),
            based_on: None,
            refs: Vec::new(),
            ..Content::default()
        },
    };
    let payload_sha256 = payload.sha256().expect("payload hash");
    let event = Event {
        format: EVENT_FORMAT.to_owned(),
        version: engr::EVENT_ENVELOPE_VERSION_V0,
        event_id: engr::model::new_id(),
        rev: 1,
        time: "2026-08-13T00:00:00Z".to_owned(),
        payload,
        confirmation: Confirmation {
            challenge: "234567".to_owned(),
            payload_sha256,
        },
    };
    (workspace, root, event)
}

fn assert_event_is_rejected(root: &Path, event: Event) {
    let id = event.payload.object.clone();
    store::append_event(root, &event).expect("write event");
    let output = run_engr(root, &["verify", &id]);
    assert_eq!(
        output.status.code(),
        Some(engr::EXIT_SCHEMA),
        "malformed events must be rejected as stored-data errors: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_event_to(root: &Path, id: &str, event: &Event) {
    let line = serde_json::to_string(event).expect("serialize event");
    std::fs::write(store::events_path(root, id), format!("{line}\n")).expect("write event");
}

/// The installers echo this line back as proof the binary they placed runs, and
/// `latest` never changes — so the part in parentheses is the only thing that
/// says which build it is. The shape is pinned and the contents are not, because
/// `unknown` is the honest answer when there is no git to ask.
#[test]
fn the_version_names_the_commit_it_was_built_from() {
    let output = Command::new(env!("CARGO_BIN_EXE_engr"))
        .arg("--version")
        .output()
        .expect("run engr --version");
    assert!(output.status.success(), "--version did not exit cleanly");
    let line = String::from_utf8(output.stdout).expect("utf8");
    let line = line.trim();
    let commit = line
        .strip_prefix("engr latest (")
        .and_then(|rest| rest.strip_suffix(')'))
        .unwrap_or_else(|| panic!("expected `engr latest (<commit>)`, got {line:?}"));
    assert!(!commit.is_empty(), "nothing was stamped in: {line:?}");
}

#[test]
fn stale_listing_includes_closed_objects_whose_basis_moved() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "tests@example.com"]);
    git(root, &["config", "user.name", "engr tests"]);
    std::fs::write(root.join("basis.txt"), "initial basis\n").expect("write basis");
    git(root, &["add", "basis.txt"]);
    git(root, &["commit", "-qm", "basis"]);

    let init = run_engr(root, &["init"]);
    assert!(
        init.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    let created = prepare(root, &["prepare", "--new", "--text", "closed record"]);
    let object = created["object"].as_str().expect("object id").to_owned();
    confirm(root, &created);

    let added = prepare(
        root,
        &[
            "prepare",
            "--add",
            "--object",
            &object,
            "--text",
            "basis wording",
        ],
    );
    confirm(root, &added);

    let closed = prepare(root, &["prepare", "--close", "--object", &object]);
    confirm(root, &closed);
    git(root, &["add", ".engr"]);
    git(root, &["commit", "-qm", "record closed object"]);

    std::fs::write(root.join("basis.txt"), "changed basis\n").expect("change basis");
    git(root, &["add", "basis.txt"]);
    git(root, &["commit", "-qm", "basis moved"]);

    let output = run_engr(root, &["ls", "--stale"]);
    assert!(
        output.status.success(),
        "ls --stale failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let listing = String::from_utf8(output.stdout).expect("utf8 listing");
    assert!(
        listing.contains("closed"),
        "a closed object whose basis moved must be surfaced by `ls --stale`; got {listing:?}"
    );
}

/// `confirm` asks for the object file to be committed. Counting that commit
/// made every section stale the moment its own record was saved, so the tool's
/// instructions broke the tool's signal and the only way back to zero was to
/// re-confirm every section — until the next commit.
#[test]
fn committing_the_record_does_not_move_its_own_basis() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "tests@example.com"]);
    git(root, &["config", "user.name", "engr tests"]);
    std::fs::create_dir(root.join("src")).expect("mkdir src");
    std::fs::write(root.join("src/audit.go"), "package audit\n").expect("write source");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "the basis"]);

    let init = run_engr(root, &["init"]);
    assert!(init.status.success());

    let created = prepare(root, &["prepare", "--new", "--text", "reason codes"]);
    let object = created["object"].as_str().expect("object id").to_owned();
    confirm(root, &created);
    let added = prepare(
        root,
        &[
            "prepare",
            "--add",
            "--object",
            &object,
            "--text",
            "Ruling: expose the reason code.",
        ],
    );
    confirm(root, &added);

    // Exactly what `confirm` tells the user to do next.
    git(root, &["add", ".engr"]);
    git(root, &["commit", "-qm", "record the ruling"]);

    let output = run_engr(root, &["show", &object]);
    let shown = String::from_utf8(output.stdout).expect("utf8");
    assert!(
        !shown.contains("basis moved"),
        "saving the record moved the record's own basis: {shown}"
    );
    assert!(shown.contains("1 ok"), "{shown}");

    // A real change to the code the ruling was made against still counts.
    std::fs::write(root.join("src/audit.go"), "package audit\n// reworked\n").expect("edit source");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "rework the audit package"]);

    let output = run_engr(root, &["show", &object]);
    let shown = String::from_utf8(output.stdout).expect("utf8");
    assert!(
        shown.contains("basis moved"),
        "a change outside the record must still be reported: {shown}"
    );
    assert!(
        shown.contains("1 commits and 1 files"),
        "the two halves of the sentence have to be filtered the same way: {shown}"
    );
}

#[test]
fn an_object_file_must_match_its_embedded_id() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");

    let object = Object::new(engr::model::new_id(), "mismatched storage key".to_owned())
        .expect("new object");
    store::write_json(&store::object_path(root, "wrong"), &object).expect("write object");

    let output = run_engr(root, &["show", "wrong"]);
    assert_eq!(
        output.status.code(),
        Some(engr::EXIT_SCHEMA),
        "a filename/id mismatch is malformed stored data, not a usable object: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn show_waits_for_the_workspace_writer_lock_before_reconciling() {
    use fs2::FileExt;

    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    let init = run_engr(root, &["init"]);
    assert!(init.status.success(), "init");
    let created = prepare(root, &["prepare", "--new", "--text", "locked read"]);
    let object = created["object"].as_str().expect("object id").to_owned();
    confirm(root, &created);

    let payload = Payload {
        action: Action::SectionAdded,
        object: object.clone(),
        becomes: None,
        content: Content {
            text: "reconcile under lock".to_owned(),
            based_on: None,
            refs: Vec::new(),
            ..Content::default()
        },
    };
    let payload_sha256 = payload.sha256().expect("payload hash");
    store::append_event(
        root,
        &Event {
            format: EVENT_FORMAT.to_owned(),
            version: engr::EVENT_ENVELOPE_VERSION_V0,
            event_id: engr::model::new_id(),
            rev: 2,
            time: "2026-08-13T00:00:00Z".to_owned(),
            payload,
            confirmation: Confirmation {
                challenge: "234567".to_owned(),
                payload_sha256,
            },
        },
    )
    .expect("append unprojected event");

    let lock_path = store::engr_dir(root).join("lock");
    let lock = OpenOptions::new()
        .write(true)
        .open(lock_path)
        .expect("open workspace lock");
    lock.lock_exclusive().expect("hold workspace lock");

    let mut child = Command::new(env!("CARGO_BIN_EXE_engr"))
        .arg("--root")
        .arg(root)
        .args(["show", &object])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start show");
    std::thread::sleep(Duration::from_millis(200));
    let while_locked = child.try_wait().expect("inspect show process");
    FileExt::unlock(&lock).expect("release workspace lock");

    assert!(
        while_locked.is_none(),
        "show must not reconcile and write while another writer holds the lock"
    );
    let output = child.wait_with_output().expect("wait for show");
    assert!(
        output.status.success(),
        "show failed after the lock released: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        store::load_object(root, &object)
            .expect("load reconciled object")
            .rev,
        2,
        "show must have reconciled the pending event after acquiring the lock"
    );
}

#[test]
fn unsupported_event_versions_are_rejected() {
    let (_workspace, root, mut event) = event_workspace();
    event.version += 1;
    let id = event.payload.object.clone();

    // Refused on the way in, so nothing writes one...
    let error = store::append_event(&root, &event).expect_err("this build emits one generation");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(
        !store::events_path(&root, &id).exists(),
        "nothing was written"
    );

    // ...and refused on the way out, because a file can arrive by other means
    // than this build writing it.
    write_event_to(&root, &id, &event);
    let output = run_engr(&root, &["verify", &id]);
    assert_eq!(
        output.status.code(),
        Some(engr::EXIT_SCHEMA),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn events_must_belong_to_their_object_file() {
    let (_workspace, root, mut event) = event_workspace();
    let path_id = event.payload.object.clone();
    event.payload.object = engr::model::new_id();
    event.confirmation.payload_sha256 = event.payload.sha256().expect("payload hash");
    write_event_to(&root, &path_id, &event);
    let output = run_engr(&root, &["verify", &path_id]);
    assert_eq!(output.status.code(), Some(engr::EXIT_SCHEMA));
}

#[test]
fn invalid_event_payloads_are_rejected() {
    let (_workspace, root, mut event) = event_workspace();
    event.payload.content.text.clear();
    event.confirmation.payload_sha256 = event.payload.sha256().expect("payload hash");
    assert_event_is_rejected(&root, event);
}

#[test]
fn event_confirmation_hashes_are_verified() {
    let (_workspace, root, mut event) = event_workspace();
    event.confirmation.payload_sha256 = "0".repeat(64);
    assert_event_is_rejected(&root, event);
}

#[test]
fn duplicate_event_revisions_are_rejected() {
    let (_workspace, root, event) = event_workspace();
    store::append_event(&root, &event).expect("write first event");
    store::append_event(&root, &event).expect("write duplicate event");
    let output = run_engr(&root, &["verify", &event.payload.object]);
    assert_eq!(output.status.code(), Some(engr::EXIT_SCHEMA));
}

#[test]
fn event_revisions_must_be_contiguous_within_history() {
    let (_workspace, root, event) = event_workspace();
    store::append_event(&root, &event).expect("write first event");
    let mut skipped = event.clone();
    skipped.rev += 2;
    store::append_event(&root, &skipped).expect("write skipped event");
    let output = run_engr(&root, &["verify", &event.payload.object]);
    assert_eq!(output.status.code(), Some(engr::EXIT_SCHEMA));
}

/// Reversed deliberately. This test used to assert the opposite — that any
/// newer commit, empty or not, means the basis moved — on the reasoning that
/// HEAD moving is not something the tool should second-guess. Excluding the
/// record's own files from the comparison makes that untenable: the same rule
/// that stops `commit .engr` from moving a section's basis also stops a commit
/// that changes nothing at all from moving it. That is the right answer to the
/// question the signal is actually asked — did what I decided against change?
/// — and an empty commit is the clearest case of no.
#[test]
fn an_empty_commit_is_not_the_basis_moving() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "tests@example.com"]);
    git(root, &["config", "user.name", "engr tests"]);
    std::fs::write(root.join("basis.txt"), "initial basis\n").expect("write basis");
    git(root, &["add", "basis.txt"]);
    git(root, &["commit", "-qm", "basis"]);

    let init = run_engr(root, &["init"]);
    assert!(init.status.success(), "init");
    let created = prepare(root, &["prepare", "--new", "--text", "closed record"]);
    let object = created["object"].as_str().expect("object id").to_owned();
    confirm(root, &created);
    let added = prepare(
        root,
        &[
            "prepare",
            "--add",
            "--object",
            &object,
            "--text",
            "basis wording",
        ],
    );
    confirm(root, &added);
    let closed = prepare(root, &["prepare", "--close", "--object", &object]);
    confirm(root, &closed);
    git(root, &["add", ".engr"]);
    git(root, &["commit", "-qm", "record closed object"]);
    git(
        root,
        &["commit", "--allow-empty", "-qm", "no source changes"],
    );

    let output = run_engr(root, &["ls", "--stale"]);
    assert!(output.status.success(), "ls --stale");
    let listing = String::from_utf8(output.stdout).expect("utf8 listing");
    assert_eq!(
        listing, "all ok\n",
        "neither saving the record nor an empty commit changed what the ruling was made against"
    );

    // The closed object still surfaces the moment something real moves, which
    // is the guarantee this test was written to protect.
    std::fs::write(root.join("basis.txt"), "changed basis\n").expect("change basis");
    git(root, &["add", "basis.txt"]);
    git(root, &["commit", "-qm", "basis moved"]);
    let output = run_engr(root, &["ls", "--stale"]);
    let listing = String::from_utf8(output.stdout).expect("utf8 listing");
    assert!(
        listing.contains("closed"),
        "a closed object whose basis really moved must still surface; got {listing:?}"
    );
}

/// The two domains share a workspace and nothing else. A reader who runs the
/// record commands must not be shown a word of unconfirmed staging, and a
/// reader who runs the staging commands must not be able to mistake what they
/// are looking at.
#[test]
fn record_surfaces_never_mix_in_unconfirmed_staging() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    assert!(run_engr(root, &["init"]).status.success(), "init");
    let created = prepare(root, &["prepare", "--new", "--text", "confirmed record"]);
    confirm(root, &created);
    let object = created["object"].as_str().expect("object id").to_owned();
    let section = prepare(
        root,
        &[
            "prepare",
            "--object",
            &object,
            "--add",
            "--text",
            "confirmed wording",
            "--no-based-on",
        ],
    );
    confirm(root, &section);

    let staged = run_engr(
        root,
        &[
            "backlog",
            "new",
            "--topic",
            "reconsider the confirmed wording",
            "--text",
            "unconfirmed exploratory wording",
        ],
    );
    assert!(
        staged.status.success(),
        "backlog new: {}",
        String::from_utf8_lossy(&staged.stderr)
    );
    let staged = String::from_utf8(staged.stdout).expect("utf8");
    assert!(staged.contains("UNCONFIRMED STAGING"), "got {staged:?}");

    for args in [
        vec!["ls"],
        vec!["ls", "--all", "--sections"],
        vec!["show", &object],
        vec!["verify"],
    ] {
        let output = run_engr(root, &args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).expect("utf8");
        assert!(
            !text.contains("unconfirmed exploratory wording")
                && !text.contains("reconsider the confirmed wording"),
            "{args:?} must not mix staging into the record: {text:?}"
        );
    }

    // `verify` stays record-oriented: staging existing is not a record verdict.
    let verify = run_engr(root, &["verify"]);
    assert!(String::from_utf8_lossy(&verify.stdout).contains("PASS"));

    // And structured staging output carries the boundary as a field, because
    // that is what travels furthest from the banner.
    let shown = run_engr(root, &["backlog", "ls"]);
    let listing = String::from_utf8(shown.stdout).expect("utf8");
    assert!(listing.contains("UNCONFIRMED STAGING"));
    let id = listing
        .lines()
        .nth(1)
        .and_then(|line| line.split_whitespace().next())
        .expect("a listed item")
        .to_owned();
    let json = run_engr(root, &["backlog", "show", &id, "--format", "json"]);
    let json: Value = serde_json::from_slice(&json.stdout).expect("backlog json");
    assert_eq!(json["authority"], "unconfirmed_staging");
    assert_eq!(
        json["sections"][0]["text"],
        "unconfirmed exploratory wording"
    );
}

/// Three different failures, three different exit codes. Phase 1 fixed that
/// boundary for the record; staging has to keep it, or a script cannot tell a
/// typo from a corrupted workspace.
#[test]
fn the_backlog_cli_separates_bad_input_from_missing_and_malformed() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    assert!(run_engr(root, &["init"]).status.success(), "init");
    let item = engr::backlog::create(
        root,
        "topic",
        "unresolved",
        Vec::new(),
        &engr::backlog::Prepared::first(),
    )
    .expect("stage")
    .id;
    let compact =
        engr::reference::encode_uuid(uuid::Uuid::parse_str(&item).expect("uuid is a uuid"));
    let absent =
        engr::reference::encode_uuid(uuid::Uuid::parse_str(&engr::model::new_id()).expect("uuid"));

    let code = |args: &[&str]| run_engr(root, args).status.code();

    // Legal syntax, wrong shape for the command: usage.
    assert_eq!(
        code(&["backlog", "show", &format!("engr:backlog:{compact}:1")]),
        Some(engr::EXIT_USAGE),
        "every backlog command addresses a whole item"
    );
    assert_eq!(
        code(&["backlog", "show", &format!("engr:obj:{compact}")]),
        Some(engr::EXIT_USAGE),
        "and a backlog command does not address an Object"
    );
    // Malformed canonical reference the person typed: usage, not schema.
    for malformed in ["engr:backlog:not-a-compact-uuid", "engr:nonsense:abc"] {
        assert_eq!(
            code(&["backlog", "show", malformed]),
            Some(engr::EXIT_USAGE),
            "{malformed}"
        );
    }
    assert_eq!(
        code(&[
            "backlog",
            "add",
            &item,
            "--text",
            "concerns",
            "--subject",
            "engr:obj:not-a-compact-uuid",
        ]),
        Some(engr::EXIT_USAGE),
        "a malformed --subject is a mistyped argument"
    );
    assert_eq!(
        code(&[
            "backlog",
            "add",
            &item,
            "--text",
            "concerns",
            "--subject",
            &format!("engr:collection:{compact}"),
        ]),
        Some(engr::EXIT_USAGE)
    );
    assert_eq!(
        code(&[
            "backlog",
            "add",
            &item,
            "--text",
            "concerns",
            "--subject-file",
            "../outside.rs",
        ]),
        Some(engr::EXIT_USAGE),
        "and so is a path that is not repository-relative"
    );

    // Well-formed, but there is no such item: not found.
    assert_eq!(
        code(&["backlog", "show", &format!("engr:backlog:{absent}")]),
        Some(engr::EXIT_NOT_FOUND)
    );
    assert_eq!(
        code(&["backlog", "show", "0198ffff"]),
        Some(engr::EXIT_NOT_FOUND)
    );

    // The stored file itself is wrong: schema, reached through a valid argument.
    let path = engr::backlog::item_path(root, &item);
    let mut stored: Value = store::read_json(&path).expect("item");
    stored["sections"][0]["updated_at"] = Value::String("last tuesday".to_owned());
    store::write_json(&path, &stored).expect("corrupt the stored item");
    assert_eq!(
        code(&["backlog", "show", &item]),
        Some(engr::EXIT_SCHEMA),
        "a malformed workspace is not the caller's argument being wrong"
    );
    assert_eq!(code(&["backlog", "ls"]), Some(engr::EXIT_SCHEMA));
}

/// Backlog CRUD through the command line, including the one refusal that keeps
/// a subject from claiming provenance it does not have.
#[test]
fn the_backlog_namespace_edits_staging_without_a_challenge_code() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    assert!(run_engr(root, &["init"]).status.success(), "init");
    git(root, &["init", "-q"]);
    git(root, &["config", "user.name", "test"]);
    git(root, &["config", "user.email", "test@example.com"]);
    std::fs::write(root.join("session.rs"), "fn refresh() {}\n").expect("source");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "source"]);

    let created = run_engr(
        root,
        &[
            "backlog",
            "new",
            "--topic",
            "refresh strategy",
            "--text",
            "offline mode may invalidate it",
            "--subject-file",
            "session.rs",
            "--subject-symbol",
            "session.rs",
            "refresh",
        ],
    );
    assert!(
        created.status.success(),
        "backlog new: {}",
        String::from_utf8_lossy(&created.stderr)
    );
    let id = engr::backlog::ids(root).expect("ids").remove(0);

    assert!(
        run_engr(root, &["backlog", "add", &id, "--text", "second point"])
            .status
            .success()
    );
    assert!(run_engr(
        root,
        &[
            "backlog",
            "revise",
            &id,
            "--section",
            "2",
            "--text",
            "reworded"
        ]
    )
    .status
    .success());
    assert!(run_engr(
        root,
        &[
            "backlog",
            "merge",
            &id,
            "--into",
            "1",
            "--sections",
            "2",
            "--text",
            "one point"
        ]
    )
    .status
    .success());
    assert!(
        run_engr(root, &["backlog", "rename", &id, "--topic", "refresh"])
            .status
            .success()
    );

    let item = engr::backlog::load(root, &id).expect("item");
    assert_eq!(item.topic, "refresh");
    assert_eq!(item.sections.len(), 1);
    assert_eq!(
        item.sections[0].id, 1,
        "the merge destination survives as itself; nothing new was allocated"
    );
    assert!(
        engr::gate::pending(root).expect("candidates").is_empty(),
        "staging edits never mint a challenge code"
    );

    // A dirty path is pinned and marked rather than refused: losing the context
    // is worse than recording that the baseline is inexact.
    std::fs::write(root.join("session.rs"), "fn refresh() { todo!() }\n").expect("edit");
    let staged = run_engr(
        root,
        &[
            "backlog",
            "add",
            &id,
            "--text",
            "concerns dirty source",
            "--subject-file",
            "session.rs",
        ],
    );
    assert!(
        staged.status.success(),
        "got {}",
        String::from_utf8_lossy(&staged.stderr)
    );
    let item = engr::backlog::load(root, &id).expect("item");
    let stored = serde_json::to_value(item.sections.last().expect("section")).expect("json");
    assert_eq!(
        stored["subjects"][0]["dirty"],
        serde_json::json!(true),
        "the subject records that what was read is not what it pins: {stored}"
    );
    let shown = run_engr(root, &["backlog", "show", &id]);
    assert!(
        String::from_utf8_lossy(&shown.stdout).contains("uncommitted changes"),
        "and the surface says so to whoever reads it"
    );

    // Consuming one point leaves the others: the topic goes only when the last
    // one does, and that is the same mutation rather than a second command.
    let remaining = engr::backlog::load(root, &id).expect("item").sections.len();
    assert_eq!(
        remaining, 2,
        "the dirty subject was staged as its own point"
    );
    assert!(
        run_engr(root, &["backlog", "consume", &id, "--section", "3"])
            .status
            .success()
    );
    assert_eq!(
        engr::backlog::load(root, &id).expect("item").sections.len(),
        1,
        "one point consumed, the topic still has unresolved work"
    );
    let last = engr::backlog::load(root, &id).expect("item").sections[0].id;
    assert!(run_engr(
        root,
        &["backlog", "consume", &id, "--section", &last.to_string()]
    )
    .status
    .success());
    assert!(
        engr::backlog::ids(root).expect("ids").is_empty(),
        "consuming the last unresolved point removes the topic with it"
    );
    let empty = run_engr(root, &["backlog", "ls"]);
    assert!(String::from_utf8_lossy(&empty.stdout).contains("nothing unresolved"));
}

/// Installed from a release archive there is no checkout, so the document that
/// says what the tool guarantees would otherwise not be on the machine the tool
/// is on. It also has to work before `init`: the protocol is what someone reads
/// to decide whether to adopt engr at all.
#[test]
fn the_protocol_prints_without_a_workspace_and_byte_for_byte() {
    let empty = TempDir::new().expect("temp dir");
    let output = Command::new(env!("CARGO_BIN_EXE_engr"))
        .current_dir(empty.path())
        .arg("protocol")
        .output()
        .expect("run engr");
    assert!(
        output.status.success(),
        "engr protocol must need no workspace: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Byte for byte, so `engr protocol > PROTOCOL.md` reproduces the document
    // rather than something one newline away from it.
    let printed = String::from_utf8(output.stdout).expect("utf-8");
    assert_eq!(printed, engr::PROTOCOL);
    assert!(printed.starts_with("# engr protocol v0"));
}

/// A workspace with one committed source file, for the surfaces that pin one.
fn repository_with_source(root: &Path) {
    git(root, &["init", "-q"]);
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::write(root.join("src/verifier.rs"), "fn verify() {}\n").expect("source");
    git(root, &["add", "."]);
    git(
        root,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "source",
        ],
    );
}

/// The code a candidate screen ends with.
fn code_from(screen: &str) -> String {
    screen
        .rsplit("CONFIRM ")
        .next()
        .expect("a candidate screen ends with its code")
        .trim()
        .to_owned()
}

/// The screen a human reads before typing a code has to carry the whole
/// destination. A state without its type is a word that means different things
/// on different objects, and attention is what they are actually deciding.
#[test]
fn classifying_shows_the_whole_destination_and_what_it_does_to_the_listing() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let created = prepare(root, &["prepare", "--new", "--text", "verification design"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id").to_owned();

    let screen = run_engr(
        root,
        &[
            "prepare",
            "--object",
            &id,
            "--classify",
            "--type",
            "design",
            "--state",
            "draft",
        ],
    );
    assert!(screen.status.success());
    let shown = String::from_utf8_lossy(&screen.stdout).to_string();
    for line in ["Type       design", "State      draft", "Attention  yes"] {
        assert!(shown.contains(line), "{line:?} missing from {shown}");
    }
    let code = code_from(&shown);
    assert!(run_engr(root, &["confirm", &format!("CONFIRM {code}")])
        .status
        .success());

    let listed = run_engr(root, &["ls"]);
    assert!(String::from_utf8_lossy(&listed.stdout).contains("design/draft"));

    // Accepting it takes it out of the default listing without closing it, and
    // says so before the human commits to that.
    let leaving = prepare(
        root,
        &[
            "prepare",
            "--object",
            &id,
            "--classify",
            "--type",
            "design",
            "--state",
            "accepted",
        ],
    );
    confirm(root, &leaving);
    let listed = run_engr(root, &["ls"]);
    assert!(
        String::from_utf8_lossy(&listed.stdout).contains("no objects"),
        "an accepted design is out of the default attention set"
    );
    let all = run_engr(root, &["ls", "--all"]);
    assert!(String::from_utf8_lossy(&all.stdout).contains("design/accepted"));

    // `--close` is the untyped vocabulary, and a design has no such state.
    let refused = run_engr(root, &["prepare", "--object", &id, "--close"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("a design cannot be closed"));
}

/// The role, the excerpt and the artifact are all part of the assertion, so all
/// of them appear on the candidate screen and in the record afterwards.
#[test]
fn a_section_carries_role_supplementary_content_and_implementation_provenance() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    repository_with_source(root);
    let created = prepare(root, &["prepare", "--new", "--text", "issuer validation"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id").to_owned();

    let screen = run_engr(
        root,
        &[
            "prepare",
            "--object",
            &id,
            "--add",
            "--no-based-on",
            "--role",
            "acceptance_criterion",
            "--text",
            "The verifier must reject an unknown issuer before checking audience.",
            "--content",
            "data.json",
            "{\"error\":\"invalid_issuer\"}",
            "--implemented-by-symbol",
            "src/verifier.rs",
            "verify",
        ],
    );
    assert!(
        screen.status.success(),
        "{}",
        String::from_utf8_lossy(&screen.stderr)
    );
    let shown = String::from_utf8_lossy(&screen.stdout).to_string();
    for fragment in [
        "Role       acceptance_criterion",
        "Content    [0] data.json",
        "implemented_by -> symbol src/verifier.rs :: verify",
        // The body itself, not only its type: a human shown the label has not
        // read what they are admitting.
        "{\"error\":\"invalid_issuer\"}",
    ] {
        assert!(
            shown.contains(fragment),
            "{fragment:?} missing from {shown}"
        );
    }
    let code = code_from(&shown);
    assert!(run_engr(root, &["confirm", &format!("CONFIRM {code}")])
        .status
        .success());

    let shown = String::from_utf8_lossy(&run_engr(root, &["show", &id]).stdout).to_string();
    assert!(shown.contains("§1 [acceptance_criterion]"), "{shown}");
    assert!(shown.contains("content  [0] data.json"), "{shown}");
    assert!(shown.contains("relation implemented_by"), "{shown}");

    let structured = run_engr(root, &["show", &id, "--format", "json"]);
    let value: Value = serde_json::from_slice(&structured.stdout).expect("json");
    assert_eq!(value["state"], "open");
    assert_eq!(value["attention"], Value::Bool(true));
    assert!(
        value.get("type").is_none(),
        "an untyped object says nothing"
    );
    let section = &value["sections"][0];
    assert_eq!(section["role"], "acceptance_criterion");
    assert_eq!(section["content"][0]["type"], "data.json");
    assert_eq!(section["relations"][0]["type"], "implemented_by");
    assert!(
        section["relations"][0]["target"]["commit"]
            .as_str()
            .expect("commit")
            .len()
            >= 40,
        "a relation pins a full resolved object id"
    );

    // The vocabularies are closed, and closed at the command line too.
    for bad in [
        vec!["--role", "rationale"],
        vec!["--content", "text.md", "prose"],
    ] {
        let mut args = vec![
            "prepare",
            "--object",
            &id,
            "--add",
            "--no-based-on",
            "--text",
            "x",
        ];
        args.extend(bad.iter().copied());
        assert!(
            !run_engr(root, &args).status.success(),
            "{bad:?} is outside the vocabulary"
        );
    }
}

/// The first refusal has to send the agent somewhere, and the retry has to be
/// visible to the human who is being asked to admit it anyway.
#[test]
fn an_oversize_section_is_refused_once_and_the_retry_says_so_on_the_screen() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let created = prepare(root, &["prepare", "--new", "--text", "size policy"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id").to_owned();
    let long = "x".repeat(engr::semantics::TEXT_NORMAL + 1);

    let refused = run_engr(
        root,
        &[
            "prepare",
            "--object",
            &id,
            "--add",
            "--no-based-on",
            "--text",
            &long,
        ],
    );
    assert!(!refused.status.success());
    let message = String::from_utf8_lossy(&refused.stderr).to_string();
    assert!(message.contains("engr backlog"), "{message}");
    assert!(message.contains("--oversize"), "{message}");

    let retried = run_engr(
        root,
        &[
            "prepare",
            "--object",
            &id,
            "--add",
            "--no-based-on",
            "--text",
            &long,
            "--oversize",
        ],
    );
    assert!(retried.status.success());
    let shown = String::from_utf8_lossy(&retried.stdout).to_string();
    assert!(
        shown.contains("OVERSIZE   admitted by exception"),
        "the exception is on the screen, above the wording: {shown}"
    );
    let code = code_from(&shown);
    assert!(run_engr(root, &["confirm", &format!("CONFIRM {code}")])
        .status
        .success());
    assert!(!std::fs::read_to_string(store::object_path(root, &id))
        .expect("object")
        .contains("oversize"));
}

/// One command, one confirmation, three facts: the state, the replacement and
/// the reason.
#[test]
fn superseding_names_the_replacement_and_moves_the_state_in_one_confirmation() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let original = prepare(root, &["prepare", "--new", "--text", "redis locks"]);
    confirm(root, &original);
    let original_id = original["object"].as_str().expect("object id").to_owned();
    let replacement = prepare(root, &["prepare", "--new", "--text", "advisory locks"]);
    confirm(root, &replacement);
    let replacement_id = replacement["object"]
        .as_str()
        .expect("object id")
        .to_owned();

    let classified = prepare(
        root,
        &[
            "prepare",
            "--object",
            &original_id,
            "--classify",
            "--type",
            "decision",
            "--state",
            "accepted",
        ],
    );
    confirm(root, &classified);
    // Straight from `accepted`, with nothing in between. That is the object
    // supersession exists for — one that was current until something replaced
    // it — and it is out of the attention set by definition. Sending it back
    // through `proposed` first would confirm a state it was never in.
    let screen = run_engr(
        root,
        &[
            "prepare",
            "--object",
            &original_id,
            "--supersede",
            &replacement_id,
            "--no-based-on",
            "--text",
            "Replaced: advisory locks remove the extra availability dependency.",
        ],
    );
    assert!(
        screen.status.success(),
        "{}",
        String::from_utf8_lossy(&screen.stderr)
    );
    let shown = String::from_utf8_lossy(&screen.stdout).to_string();
    assert!(shown.contains("Role       supersession"), "{shown}");
    assert!(shown.contains("superseded_by -> engr:obj:"), "{shown}");
    assert!(shown.contains("State      superseded"), "{shown}");
    let code = code_from(&shown);
    assert!(run_engr(root, &["confirm", &format!("CONFIRM {code}")])
        .status
        .success());

    let value: Value =
        serde_json::from_slice(&run_engr(root, &["show", &original_id, "--format", "json"]).stdout)
            .expect("json");
    assert_eq!(value["state"], "superseded");
    assert_eq!(value["attention"], Value::Bool(false));
    assert_eq!(value["sections"][0]["role"], "supersession");
    assert_eq!(
        value["sections"][0]["relations"][0]["type"],
        "superseded_by"
    );
    assert_eq!(
        store::load_events(root, &original_id)
            .expect("events")
            .len(),
        3,
        "created, classified, superseded — one semantic action appends one event, \
         and retiring an accepted decision invents no intermediate state"
    );
}

/// A destination belongs to `--classify`, or to an action that needs the object
/// back in the attention set — and to nothing else.
///
/// "Needs the object back" is the operative half. On an object that already
/// needs attention there is nothing to bring back, so a destination on a section
/// action is refused there too: it would be an unrelated change riding along
/// inside a confirmation about something else.
#[test]
fn type_and_state_flags_belong_to_classify_and_nothing_else() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let created = prepare(root, &["prepare", "--new", "--text", "flag discipline"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id").to_owned();

    for (args, expected) in [
        (
            vec!["prepare", "--object", &id, "--close", "--state", "closed"],
            "already names the state it produces",
        ),
        (
            vec!["prepare", "--object", &id, "--classify", "--state", "open"],
            "--classify needs the destination type",
        ),
        (
            vec![
                "prepare",
                "--object",
                &id,
                "--add",
                "--no-based-on",
                "--text",
                "x",
                "--type",
                "design",
            ],
            "a destination needs a --state",
        ),
        (
            vec![
                "prepare",
                "--object",
                &id,
                "--add",
                "--no-based-on",
                "--text",
                "x",
                "--type",
                "design",
                "--state",
                "proposed",
            ],
            "already needs attention",
        ),
        (
            vec!["prepare", "--object", &id, "--close", "--role", "decision"],
            "carries no wording",
        ),
    ] {
        let refused = run_engr(root, &args);
        assert!(!refused.status.success(), "{args:?} must be refused");
        let message = String::from_utf8_lossy(&refused.stderr).to_string();
        assert!(message.contains(expected), "{args:?}: got {message:?}");
    }

    // `--untyped` is a word rather than the absence of `--type`, so "no type"
    // and "I forgot to say" cannot look the same.
    let untyped = prepare(
        root,
        &[
            "prepare",
            "--object",
            &id,
            "--classify",
            "--untyped",
            "--state",
            "closed",
        ],
    );
    confirm(root, &untyped);
    let value: Value =
        serde_json::from_slice(&run_engr(root, &["show", &id, "--format", "json"]).stdout)
            .expect("json");
    assert_eq!(value["state"], "closed");
    assert!(value.get("type").is_none());
}

/// Removing supplementary content shows the human what is being removed.
///
/// `content[]` is ordered and repeated types are valid, so with two `code.rs`
/// entries the heading names a position, not a thing. A body is hashed with the
/// section and is as authoritative as the wording above it — removed wording
/// already appears in the text diff, and a removed body has to appear the same
/// way, or the screen is asking for a confirmation of something it did not show.
#[test]
fn a_removed_supplementary_body_is_shown_and_a_changed_one_is_shown_as_a_diff() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let created = prepare(root, &["prepare", "--new", "--text", "two excerpts"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id").to_owned();

    // A body long enough that showing all of it would be the wrong answer for a
    // modification, so the two presentations are actually distinguishable.
    let long = (0..40)
        .map(|line| format!("let step_{line} = {line};"))
        .collect::<Vec<_>>()
        .join("\n");
    let changed = long.replace("let step_7 = 7;", "let step_7 = 700;");

    let added = prepare(
        root,
        &[
            "prepare",
            "--object",
            &id,
            "--add",
            "--no-based-on",
            "--text",
            "Both excerpts stand behind this assertion.",
            "--content",
            "code.rs",
            &long,
            "--content",
            "code.rs",
            "fn discarded() { todo!() }",
        ],
    );
    confirm(root, &added);

    // Keep the first entry, change it, and drop the second.
    let screen = run_engr(
        root,
        &[
            "prepare",
            "--object",
            &id,
            "--revise",
            "1",
            "--no-based-on",
            "--text",
            "Both excerpts stand behind this assertion.",
            "--content",
            "code.rs",
            &changed,
        ],
    );
    assert!(
        screen.status.success(),
        "{}",
        String::from_utf8_lossy(&screen.stderr)
    );
    let shown = String::from_utf8_lossy(&screen.stdout).to_string();

    assert!(
        shown.contains("── content [1] code.rs ── removed"),
        "{shown}"
    );
    assert!(
        shown.contains("fn discarded() { todo!() }"),
        "the removed body is shown, not only which position it sat in: {shown}"
    );

    // The modified body is a diff against the previous one, which is the same
    // presentation the wording gets — and the reason the protocol says a body
    // is shown in full when it is added or removed rather than in every case.
    assert!(shown.contains("-let step_7 = 7;"), "{shown}");
    assert!(shown.contains("+let step_7 = 700;"), "{shown}");
    assert!(
        !shown.contains("let step_39 = 39;"),
        "an unchanged tail forty lines away is not context: {shown}"
    );

    let code = code_from(&shown);
    assert!(run_engr(root, &["confirm", &format!("CONFIRM {code}")])
        .status
        .success());
    let value: Value =
        serde_json::from_slice(&run_engr(root, &["show", &id, "--format", "json"]).stdout)
            .expect("json");
    let entries = value["sections"][0]["content"].as_array().expect("content");
    assert_eq!(entries.len(), 1, "the second entry really is gone");
    assert_eq!(entries[0]["body"], changed);
}

/// The exception is the retry of a refusal, and the command line cannot skip
/// the refusal by reaching for the flag first.
#[test]
fn the_oversize_flag_is_refused_until_engr_has_refused_the_proposal() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let created = prepare(root, &["prepare", "--new", "--text", "admission order"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id").to_owned();
    let long = "x".repeat(engr::semantics::TEXT_NORMAL + 1);

    let straight_to_it = run_engr(
        root,
        &[
            "prepare",
            "--object",
            &id,
            "--add",
            "--no-based-on",
            "--oversize",
            "--text",
            &long,
        ],
    );
    assert!(
        !straight_to_it.status.success(),
        "the first prepare must refuse, whatever flags it carried"
    );
    let message = String::from_utf8_lossy(&straight_to_it.stderr).to_string();
    assert!(message.contains("retry of a refusal"), "{message}");

    // And an exception over content that breaks nothing is refused too, so the
    // flag never becomes something an agent can just always pass.
    let nothing_to_except = run_engr(
        root,
        &[
            "prepare",
            "--object",
            &id,
            "--add",
            "--no-based-on",
            "--oversize",
            "--text",
            "brief",
        ],
    );
    assert!(!nothing_to_except.status.success());
    assert!(
        String::from_utf8_lossy(&nothing_to_except.stderr).contains("no exception to make"),
        "{}",
        String::from_utf8_lossy(&nothing_to_except.stderr)
    );
    // Nor on an action that has no section content to measure at all, where the
    // exception could only ever be a claim the screen makes on engr's behalf.
    for action in [
        vec!["--rename", "--text", "a better title"],
        vec!["--classify", "--untyped", "--state", "closed"],
    ] {
        let mut args = vec!["prepare", "--object", &id, "--oversize"];
        args.extend(action.iter().copied());
        let refused = run_engr(root, &args);
        assert!(!refused.status.success(), "{action:?}");
        assert!(
            String::from_utf8_lossy(&refused.stderr).contains("no size exception to make"),
            "{action:?}: {}",
            String::from_utf8_lossy(&refused.stderr)
        );
    }
    assert!(
        String::from_utf8_lossy(&run_engr(root, &["candidate"]).stdout).contains("nothing"),
        "and no attempt left a code awaiting a human"
    );
}

/// Give a stored Section bodies a text editor could have put there.
///
/// The hash is recomputed, so this is valid persisted authority rather than
/// corruption — exactly what a workspace written by any build may hold, since
/// nothing on the read path normalizes a body.
fn seed_bodies(root: &Path, id: &str, bodies: &[&str]) {
    let mut object = store::load_object(root, id).expect("load");
    let section = &mut object.sections[0];
    section.content = bodies
        .iter()
        .map(|body| engr::semantics::Supplement::new("code.rs", *body))
        .collect();
    section.sha256 = section.recomputed_sha256().expect("hash");
    store::save_object(root, &object).expect("save");
    assert!(
        run_engr(root, &["verify", id]).status.success(),
        "the seeded section must be valid stored authority"
    );
}

/// The screen cannot draw two different authoritative bodies the same way.
///
/// A terminal shows `"x"`, `"x\n"` and `"x   "` identically, and a body of
/// nothing but spaces as nothing at all — yet each is a different literal
/// inside a different Section hash. Nothing normalizes a body, on the way in or
/// on the read path, so any build's workspace may hold all of these; the gate's
/// obligation is to say what it is showing, not to change it.
#[test]
fn a_body_whose_ending_is_invisible_is_described_where_the_human_reads_it() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let created = prepare(root, &["prepare", "--new", "--text", "invisible endings"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id").to_owned();

    let added = prepare(
        root,
        &[
            "prepare",
            "--object",
            &id,
            "--add",
            "--no-based-on",
            "--text",
            "Three excerpts stand behind this assertion.",
            "--content",
            "code.rs",
            "placeholder one",
            "--content",
            "code.rs",
            "placeholder two",
            "--content",
            "code.rs",
            "placeholder three",
        ],
    );
    confirm(root, &added);

    // Now they hold what a previous build, a hand edit, or a body that simply
    // ended that way would leave behind.
    seed_bodies(root, &id, &["let x = 1;\n", "let y = 2;   ", "   "]);

    // Removing all three: the screen has to convey what is being removed.
    let screen = run_engr(
        root,
        &[
            "prepare",
            "--object",
            &id,
            "--revise",
            "1",
            "--no-based-on",
            "--text",
            "Three excerpts stand behind this assertion.",
        ],
    );
    assert!(
        screen.status.success(),
        "{}",
        String::from_utf8_lossy(&screen.stderr)
    );
    let shown = String::from_utf8_lossy(&screen.stdout).to_string();
    for expected in [
        "── content [0] code.rs ── removed, ends with 1 newline",
        "── content [1] code.rs ── removed, ends with 3 spaces",
        "── content [2] code.rs ── removed, 3 spaces, and nothing else",
    ] {
        assert!(
            expected_line(&shown, expected),
            "{expected:?} not in {shown}"
        );
    }
    // The bodies themselves are still printed exactly, untrimmed.
    assert!(shown.contains("let x = 1;\n"), "{shown}");
    assert!(shown.contains("let y = 2;   \n"), "{shown}");

    // And a revision that only moves trailing whitespace — which the line diff
    // below cannot show — is named on both sides.
    let screen = run_engr(
        root,
        &[
            "prepare",
            "--object",
            &id,
            "--revise",
            "1",
            "--no-based-on",
            "--text",
            "Three excerpts stand behind this assertion.",
            "--content",
            "code.rs",
            "let x = 1;\n\n",
            "--content",
            "code.rs",
            "let y = 2;   ",
            "--content",
            "code.rs",
            "   ",
        ],
    );
    assert!(
        screen.status.success(),
        "{}",
        String::from_utf8_lossy(&screen.stderr)
    );
    let shown = String::from_utf8_lossy(&screen.stdout).to_string();
    assert!(
        expected_line(
            &shown,
            "── content [0] code.rs ── previous ends with 1 newline; candidate ends with 2 newlines"
        ),
        "{shown}"
    );
    // The other two are unchanged, so they are not shown at all.
    assert!(!shown.contains("content [1]"), "{shown}");
    assert!(!shown.contains("content [2]"), "{shown}");
}

/// Whether a line is present exactly, trailing spaces and all.
fn expected_line(screen: &str, line: &str) -> bool {
    screen.lines().any(|candidate| candidate == line)
}

/// Content order is the order the caller wrote, whichever flag spelled it.
///
/// `content[]` is ordered and moving an entry is a revision, so grouping the
/// inline entries ahead of the file-backed ones would be authoritative input
/// being silently rearranged. Both spellings stay available and mixing them
/// stays legal; what changed is that the sequence survives.
#[test]
fn mixed_inline_and_file_backed_content_keeps_the_order_it_was_written_in() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let created = prepare(root, &["prepare", "--new", "--text", "content order"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id").to_owned();

    let second = root.join("second.json");
    std::fs::write(&second, "{\"second\":true}").expect("write");
    let fourth = root.join("fourth.json");
    std::fs::write(&fourth, "{\"fourth\":true}").expect("write");

    // Inline, file, inline, file — the interleaving the two lists cannot hold.
    let screen = run_engr(
        root,
        &[
            "prepare",
            "--object",
            &id,
            "--add",
            "--no-based-on",
            "--text",
            "four excerpts, in this order",
            "--content",
            "code.rs",
            "let first = 1;",
            "--content-file",
            "data.json",
            second.to_str().expect("path"),
            "--content",
            "code.rs",
            "let third = 3;",
            "--content-file",
            "data.json",
            fourth.to_str().expect("path"),
        ],
    );
    assert!(
        screen.status.success(),
        "{}",
        String::from_utf8_lossy(&screen.stderr)
    );
    let shown = String::from_utf8_lossy(&screen.stdout).to_string();
    let headings: Vec<&str> = shown
        .lines()
        .filter(|line| line.starts_with("Content    ["))
        .collect();
    assert_eq!(
        headings,
        vec![
            "Content    [0] code.rs",
            "Content    [1] data.json",
            "Content    [2] code.rs",
            "Content    [3] data.json",
        ],
        "the candidate screen shows the caller's order: {shown}"
    );

    let code = code_from(&shown);
    assert!(run_engr(root, &["confirm", &format!("CONFIRM {code}")])
        .status
        .success());
    let value: Value =
        serde_json::from_slice(&run_engr(root, &["show", &id, "--format", "json"]).stdout)
            .expect("json");
    let bodies: Vec<&str> = value["sections"][0]["content"]
        .as_array()
        .expect("content")
        .iter()
        .map(|entry| entry["body"].as_str().expect("body"))
        .collect();
    assert_eq!(
        bodies,
        vec![
            "let first = 1;",
            "{\"second\":true}",
            "let third = 3;",
            "{\"fourth\":true}",
        ],
        "and the record stores it"
    );
}

/// Execution memory is reachable, and reads as what it is.
///
/// Every screen that prints it says so, because the failure this domain can
/// cause is not corruption — it is a reader taking a finished checklist for a
/// settled decision.
#[test]
fn work_is_its_own_namespace_and_every_screen_says_it_is_not_the_record() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    repository_with_source(root);
    let created = prepare(root, &["prepare", "--new", "--text", "the auth design"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id").to_owned();

    let empty = run_engr(root, &["work", "ls"]);
    assert!(empty.status.success());
    let shown = String::from_utf8_lossy(&empty.stdout).to_string();
    assert!(shown.contains("EXECUTION MEMORY"), "{shown}");
    assert!(shown.contains("confirmed by nobody"), "{shown}");
    assert!(shown.contains("no execution memory"), "{shown}");

    let started = run_engr(
        root,
        &[
            "work",
            "start",
            &id,
            "--summary",
            "Parser done. show still uses the old resolver.",
        ],
    );
    assert!(
        started.status.success(),
        "{}",
        String::from_utf8_lossy(&started.stderr)
    );
    assert!(run_engr(
        root,
        &["work", "item", "add", &id, "--text", "migrate the parser"]
    )
    .status
    .success());
    assert!(run_engr(
        root,
        &["work", "item", "state", &id, "--item", "1", "--state", "done"]
    )
    .status
    .success());
    assert!(run_engr(
        root,
        &[
            "work",
            "block",
            &id,
            "--reason",
            "waiting on the compat result"
        ]
    )
    .status
    .success());

    let listed = String::from_utf8_lossy(&run_engr(root, &["work", "ls"]).stdout).to_string();
    assert!(listed.contains("blocked"), "derived, not stored: {listed}");
    assert!(listed.contains("the auth design"), "{listed}");

    // And the record's own commands are untouched by any of it.
    for command in [
        vec!["ls"],
        vec!["ls", "--all", "--sections"],
        vec!["show", &id],
        vec!["verify"],
    ] {
        let output = run_engr(root, &command);
        assert!(output.status.success(), "{command:?}");
        let shown = String::from_utf8_lossy(&output.stdout).to_string();
        for leak in ["EXECUTION MEMORY", "migrate the parser", "compat result"] {
            assert!(
                !shown.contains(leak),
                "{command:?} leaked {leak:?} from the sidecar: {shown}"
            );
        }
    }
    let structured: Value =
        serde_json::from_slice(&run_engr(root, &["show", &id, "--format", "json"]).stdout)
            .expect("json");
    assert!(structured.get("work").is_none(), "{structured}");
    assert_eq!(structured["state"], "open", "the Object is where it was");

    // Structured Work output travels furthest from any banner — into another
    // tool, with no screen in between — so the boundary has to be a field.
    // `{"state": "active"}` alone is indistinguishable from an Object's state.
    let sidecar: Value =
        serde_json::from_slice(&run_engr(root, &["work", "show", &id, "--format", "json"]).stdout)
            .expect("json");
    assert_eq!(
        sidecar["authority"], "execution_memory",
        "the JSON surface must say what it is: {sidecar}"
    );
    assert_eq!(sidecar["standing"], "blocked");
    assert_eq!(sidecar["state"], "active");
}

/// A human said stop, and every screen that touches it says so.
///
/// The rule itself is the agent's to follow — engr cannot tell who asked, so it
/// does not refuse. What it can do is never let the signal pass unremarked: the
/// state screen states the rule, and deleting says what went with the sidecar.
#[test]
fn pausing_work_survives_an_agent_trying_to_delete_it() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let created = prepare(root, &["prepare", "--new", "--text", "paused work"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id").to_owned();
    assert!(run_engr(root, &["work", "start", &id]).status.success());
    assert!(run_engr(root, &["work", "pause", &id]).status.success());

    let shown = String::from_utf8_lossy(&run_engr(root, &["work", "show", &id]).stdout).to_string();
    assert!(shown.contains("State      paused"), "{shown}");
    assert!(
        shown.contains("do not resume it on your own"),
        "the screen states the rule the tool cannot enforce: {shown}"
    );

    let removed = run_engr(root, &["work", "rm", &id]);
    assert!(
        removed.status.success(),
        "the deletion is carried out: the rule is normative, not mechanical"
    );
    assert!(
        String::from_utf8_lossy(&removed.stdout).contains("stop signal went with it"),
        "but it does not pass unremarked: {}",
        String::from_utf8_lossy(&removed.stdout)
    );
    assert!(!run_engr(root, &["work", "show", &id]).status.success());

    // Deleting an active one says only that there is nothing left to hand off.
    assert!(run_engr(root, &["work", "start", &id]).status.success());
    let removed = run_engr(root, &["work", "rm", &id]);
    assert!(removed.status.success());
    assert!(!String::from_utf8_lossy(&removed.stdout).contains("stop signal"));
}

/// A work target names a whole Object or Backlog item, and must exist.
#[test]
fn work_targets_are_checked_at_the_command_line() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let created = prepare(root, &["prepare", "--new", "--text", "depends"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id").to_owned();
    let other = prepare(root, &["prepare", "--new", "--text", "the prerequisite"]);
    confirm(root, &other);
    let other = other["object"].as_str().expect("object id").to_owned();
    let compact = engr::reference::encode_uuid_str(&other).expect("compact");
    assert!(run_engr(root, &["work", "start", &id]).status.success());

    let ok = run_engr(
        root,
        &[
            "work",
            "depend",
            &id,
            "--on",
            &format!("engr:obj:{compact}"),
            "--reason",
            "the token format is confirmed there",
        ],
    );
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );
    let shown = String::from_utf8_lossy(&ok.stdout).to_string();
    assert!(shown.contains("Depends on"), "{shown}");
    assert!(
        shown.contains("the token format is confirmed there"),
        "{shown}"
    );

    let absent = engr::reference::encode_uuid_str(&engr::model::new_id()).expect("compact");
    for (spec, expected) in [
        (format!("engr:obj:{compact}:1"), "whole Object"),
        // A well-formed Collection id, so what refuses it is the kind and not the
        // spelling: Work does not point at planning metadata.
        ("engr:collection:0123456789".to_owned(), "cannot target"),
        (format!("engr:obj:{absent}"), "does not exist"),
        (format!("obj:{compact}"), "must be an engr: reference"),
    ] {
        let refused = run_engr(root, &["work", "depend", &id, "--on", &spec]);
        assert!(!refused.status.success(), "{spec}");
        let message = String::from_utf8_lossy(&refused.stderr).to_string();
        assert!(message.contains(expected), "{spec}: {message}");
    }
}

/// Every surface an agent can reach describes the same rule.
///
/// `paused` has one behaviour and four places that state it: the protocol
/// compiled into the binary, `--help`, the screens, and the source. When an
/// accepted design moves, they have to move together — an agent that reads
/// `--help` and concludes the opposite of `engr protocol` has been handed two
/// rules and will follow the convenient one. Pinned here because the drift is
/// invisible: nothing else fails when only the wording is stale.
#[test]
fn every_surface_agrees_that_the_paused_rule_is_the_agents_to_follow() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");

    // `--help` is the surface an agent reaches for first and the one most
    // likely to disagree, because nothing breaks when it does.
    let help = String::from_utf8_lossy(&run_engr(root, &["work", "rm", "--help"]).stdout)
        .to_string()
        + &String::from_utf8_lossy(&run_engr(root, &["work", "--help"]).stdout);
    for stale in ["Refused while paused", "refused", "cannot be deleted"] {
        assert!(
            !help.contains(stale),
            "help still claims the reverted rule ({stale:?}): {help}"
        );
    }

    // The compiled-in protocol is canonical, and says the rule is the agent's.
    // Whitespace-normalized, so a reflowed paragraph does not fail a test about
    // meaning — the thing being pinned is what the sentence says.
    let protocol = String::from_utf8_lossy(&run_engr(root, &["protocol"]).stdout)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        protocol.contains("MUST NOT delete a paused work object"),
        "the protocol states the normative rule"
    );
    assert!(
        protocol.contains("MUST NOT turn it into a lifecycle rule by refusing the deletion"),
        "and states that an implementation must not enforce it"
    );

    // And the behaviour itself, one more time, against the same binary the two
    // documents above came out of.
    let created = prepare(root, &["prepare", "--new", "--text", "surfaces agree"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id").to_owned();
    assert!(run_engr(root, &["work", "start", &id]).status.success());
    assert!(run_engr(root, &["work", "pause", &id]).status.success());
    let removed = run_engr(root, &["work", "rm", &id]);
    assert!(
        removed.status.success(),
        "{}",
        String::from_utf8_lossy(&removed.stderr)
    );
    assert!(String::from_utf8_lossy(&removed.stdout).contains("stop signal went with it"));
}

/// Planning is reachable, reads as planning, and stays out of the record.
#[test]
fn collections_are_their_own_namespace_and_never_reach_the_record() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let created = prepare(root, &["prepare", "--new", "--text", "the auth design"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id").to_owned();
    let compact = engr::reference::encode_uuid_str(&id).expect("compact");

    let empty = run_engr(root, &["collection", "ls"]);
    assert!(empty.status.success());
    let shown = String::from_utf8_lossy(&empty.stdout).to_string();
    assert!(shown.contains("PLANNING"), "{shown}");
    assert!(shown.contains("confirmed by nobody"), "{shown}");
    assert!(shown.contains("no collections"), "{shown}");

    let made = run_engr(
        root,
        &[
            "collection",
            "new",
            "--name",
            "Q3 authentication",
            "--start",
            "2026-07-01",
            "--end",
            "2026-09-30",
        ],
    );
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
    let plan = String::from_utf8_lossy(&made.stdout).to_string();
    let plan_id = plan
        .lines()
        .find_map(|line| line.strip_prefix("Collection "))
        .expect("the id is on the screen")
        .trim()
        .to_owned();

    let added = run_engr(
        root,
        &[
            "collection",
            "add",
            &plan_id,
            "--target",
            &format!("engr:obj:{compact}"),
            "--order",
            "10",
            "--priority",
            "high",
            "--reason",
            "Blocks the rest of the milestone.",
        ],
    );
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );
    let shown = String::from_utf8_lossy(&added.stdout).to_string();
    assert!(shown.contains("[high]"), "{shown}");
    assert!(
        shown.contains("Blocks the rest of the milestone."),
        "{shown}"
    );
    // Derived attention, not `open`: half the vocabulary does not have that.
    assert!(shown.contains("attention"), "{shown}");
    assert!(shown.contains("the auth design"), "{shown}");

    // The record's own commands see none of it.
    for command in [
        vec!["ls"],
        vec!["ls", "--all", "--sections"],
        vec!["show", &id],
        vec!["verify"],
    ] {
        let output = run_engr(root, &command);
        assert!(output.status.success(), "{command:?}");
        let shown = String::from_utf8_lossy(&output.stdout).to_string();
        for leak in ["PLANNING", "Q3 authentication", "Blocks the rest"] {
            assert!(
                !shown.contains(leak),
                "{command:?} leaked {leak:?}: {shown}"
            );
        }
    }

    // Structured planning output says what it is, like the other two domains.
    let structured: Value = serde_json::from_slice(
        &run_engr(root, &["collection", "show", &plan_id, "--format", "json"]).stdout,
    )
    .expect("json");
    assert_eq!(structured["authority"], "planning", "{structured}");
    assert_eq!(structured["state"], "open");
    assert_eq!(structured["schedule"]["start"], "2026-07-01");
    assert_eq!(structured["members"][0]["priority"]["level"], "high");
}

/// Deleting a plan is the agent's rule to follow, and the screen says what went.
#[test]
fn deleting_a_collection_reports_the_planning_context_it_discarded() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let made = run_engr(root, &["collection", "new", "--name", "a plan"]);
    assert!(made.status.success());
    let plan_id = String::from_utf8_lossy(&made.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("Collection "))
        .expect("id")
        .trim()
        .to_owned();

    let deleted = run_engr(root, &["collection", "delete", &plan_id]);
    assert!(
        deleted.status.success(),
        "the rule is normative, not mechanical: {}",
        String::from_utf8_lossy(&deleted.stderr)
    );
    let said = String::from_utf8_lossy(&deleted.stdout).to_string();
    assert!(said.contains("planning context"), "{said}");
    assert!(!run_engr(root, &["collection", "show", &plan_id])
        .status
        .success());

    // And the help does not claim a refusal the tool does not perform.
    let help = String::from_utf8_lossy(&run_engr(root, &["collection", "--help"]).stdout)
        .to_string()
        + &String::from_utf8_lossy(&run_engr(root, &["collection", "delete", "--help"]).stdout);
    for stale in ["Refused", "refuses", "cannot be deleted"] {
        assert!(
            !help.contains(stale),
            "help claims a guard that is not there: {help}"
        );
    }
}

/// The listing puts what is still being pursued first.
///
/// Pinned with all three states because the bug it replaces was invisible with
/// one: sorting by the *name* of the state puts `cancelled` and `completed`
/// above `open` alphabetically, which is the exact reverse of what a planning
/// listing is for, and nothing about a single open plan would have shown it.
#[test]
fn collection_ls_lists_open_plans_before_closed_ones() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");

    let make = |name: &str| -> String {
        let made = run_engr(root, &["collection", "new", "--name", name]);
        assert!(
            made.status.success(),
            "{}",
            String::from_utf8_lossy(&made.stderr)
        );
        String::from_utf8_lossy(&made.stdout)
            .lines()
            .find_map(|line| line.strip_prefix("Collection "))
            .expect("id")
            .trim()
            .to_owned()
    };
    // Named so that alphabetical order by name would also get this wrong.
    let cancelled = make("aaa dropped");
    let completed = make("bbb finished");
    let open = make("zzz current");
    assert!(run_engr(
        root,
        &["collection", "state", &cancelled, "--state", "cancelled"]
    )
    .status
    .success());
    assert!(run_engr(
        root,
        &["collection", "state", &completed, "--state", "completed"]
    )
    .status
    .success());

    let listed = String::from_utf8_lossy(&run_engr(root, &["collection", "ls"]).stdout).to_string();
    // Rows only: the banner says "what its members mean", which a looser filter
    // would count as a row and quietly shift every position by one.
    let order: Vec<&str> = listed
        .lines()
        .filter(|line| line.contains("need attention"))
        .map(|line| {
            if line.contains("zzz current") {
                "open"
            } else if line.contains("bbb finished") {
                "completed"
            } else {
                "cancelled"
            }
        })
        .collect();
    assert_eq!(
        order,
        vec!["open", "completed", "cancelled"],
        "open plans come first: {listed}"
    );
    assert!(listed.contains(&open[..4]), "{listed}");
}

/// engr can name its own resources to itself.
///
/// Every reference-taking flag across three domains wants
/// `engr:obj:<26-char>` or `engr:backlog:<26-char>`, and until this landed no
/// command printed one — an agent had to implement Crockford Base32 outside the
/// tool to use `--subject`, `--on` or `--target` at all. The test is the round
/// trip rather than the field, because the field is only worth having if what
/// it prints is exactly what the flags accept.
#[test]
fn a_read_surface_prints_the_reference_every_flag_asks_for() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let created = prepare(root, &["prepare", "--new", "--text", "the auth design"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id").to_owned();
    let added = prepare(
        root,
        &[
            "prepare",
            "--object",
            &id,
            "--add",
            "--no-based-on",
            "--text",
            "a section",
        ],
    );
    confirm(root, &added);
    assert!(run_engr(
        root,
        &["backlog", "new", "--topic", "refresh", "--text", "a point"]
    )
    .status
    .success());
    let item = engr::backlog::all(root).expect("backlog")[0].id.clone();

    // Text and structured both carry it, for both domains.
    let object: Value =
        serde_json::from_slice(&run_engr(root, &["show", &id, "--format", "json"]).stdout)
            .expect("json");
    let reference = object["reference"].as_str().expect("reference").to_owned();
    assert!(reference.starts_with("engr:obj:"), "{reference}");
    assert_eq!(
        object["sections"][0]["reference"],
        format!("{reference}:1"),
        "a section names itself too, for --ref and --subject"
    );
    assert!(
        String::from_utf8_lossy(&run_engr(root, &["show", &id]).stdout).contains(&reference),
        "the text surface prints it as well"
    );

    let staged: Value = serde_json::from_slice(
        &run_engr(root, &["backlog", "show", &item, "--format", "json"]).stdout,
    )
    .expect("json");
    let staged_reference = staged["reference"].as_str().expect("reference").to_owned();
    assert!(
        staged_reference.starts_with("engr:backlog:"),
        "{staged_reference}"
    );
    assert!(
        String::from_utf8_lossy(&run_engr(root, &["backlog", "show", &item]).stdout)
            .contains(&staged_reference)
    );

    // And every flag that demanded this shape accepts exactly what was printed.
    let plan = run_engr(root, &["collection", "new", "--name", "a plan"]);
    let plan_id = String::from_utf8_lossy(&plan.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("Collection "))
        .expect("id")
        .trim()
        .to_owned();
    assert!(run_engr(root, &["work", "start", &id]).status.success());

    for (what, args) in [
        (
            "collection add --target, an object",
            vec!["collection", "add", &plan_id, "--target", &reference],
        ),
        (
            "collection add --target, a backlog item",
            vec!["collection", "add", &plan_id, "--target", &staged_reference],
        ),
        (
            "work depend --on",
            vec!["work", "depend", &id, "--on", &staged_reference],
        ),
        (
            "backlog new --subject",
            vec![
                "backlog",
                "new",
                "--topic",
                "t",
                "--text",
                "x",
                "--subject",
                &reference,
            ],
        ),
    ] {
        let output = run_engr(root, &args);
        assert!(
            output.status.success(),
            "{what} refused what engr itself printed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// Every addressable entity says its own canonical reference, exactly.
///
/// The contract is the machine-readable path — an agent must be able to obtain
/// a reference without reimplementing the codec — so this asserts the exact
/// values rather than only that one of them round-trips. Sections included:
/// they are addressable too, and `--ref` and `--subject` take them.
#[test]
fn every_addressable_entity_exposes_its_canonical_reference() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let created = prepare(root, &["prepare", "--new", "--text", "an object"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id").to_owned();
    let added = prepare(
        root,
        &[
            "prepare",
            "--object",
            &id,
            "--add",
            "--no-based-on",
            "--text",
            "a section",
        ],
    );
    confirm(root, &added);
    assert!(run_engr(
        root,
        &["backlog", "new", "--topic", "t", "--text", "a point"]
    )
    .status
    .success());
    let item = engr::backlog::all(root).expect("backlog")[0].id.clone();
    let made = run_engr(root, &["collection", "new", "--name", "a plan"]);
    assert!(made.status.success());
    let plan = String::from_utf8_lossy(&made.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("Collection "))
        .expect("id")
        .trim()
        .to_owned();

    // The exact values, derived independently of the surfaces under test.
    let object_ref = format!(
        "engr:obj:{}",
        engr::reference::encode_uuid_str(&id).expect("compact")
    );
    let item_ref = format!(
        "engr:backlog:{}",
        engr::reference::encode_uuid_str(&item).expect("compact")
    );
    let plan_ref = format!("engr:collection:{plan}");

    let object: Value =
        serde_json::from_slice(&run_engr(root, &["show", &id, "--format", "json"]).stdout)
            .expect("json");
    assert_eq!(object["reference"], object_ref);
    assert_eq!(
        object["sections"][0]["reference"],
        format!("{object_ref}:1")
    );
    assert_eq!(object["id"], id, "identity is not replaced by addressing");

    let staged: Value = serde_json::from_slice(
        &run_engr(root, &["backlog", "show", &item, "--format", "json"]).stdout,
    )
    .expect("json");
    assert_eq!(staged["reference"], item_ref);
    assert_eq!(staged["sections"][0]["reference"], format!("{item_ref}:1"));
    assert_eq!(
        staged["sections"][0]["id"], 1,
        "the section keeps its own id"
    );
    assert_eq!(staged["id"], item);

    let planned: Value = serde_json::from_slice(
        &run_engr(root, &["collection", "show", &plan, "--format", "json"]).stdout,
    )
    .expect("json");
    assert_eq!(planned["reference"], plan_ref);
    assert_eq!(planned["id"], plan);

    // Each one is accepted where it is meant to be used.
    assert!(run_engr(
        root,
        &[
            "backlog",
            "new",
            "--topic",
            "u",
            "--text",
            "x",
            "--subject",
            &format!("{object_ref}:1")
        ]
    )
    .status
    .success());
    assert!(
        run_engr(root, &["collection", "add", &plan, "--target", &item_ref])
            .status
            .success()
    );
}

/// `:0` names a section that cannot exist, so the shared parser refuses it.
#[test]
fn a_zero_section_selector_is_refused_everywhere() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let created = prepare(root, &["prepare", "--new", "--text", "an object"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id").to_owned();
    let compact = engr::reference::encode_uuid_str(&id).expect("compact");

    for spelling in [
        format!("engr:obj:{compact}:0"),
        format!("obj:{compact}:0"),
        format!("engr:backlog:{compact}:0"),
    ] {
        let error = engr::reference::EngrRef::parse_embedded(
            spelling.strip_prefix("engr:").unwrap_or(&spelling),
        )
        .expect_err("section ids start at 1");
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{spelling}");
        assert!(error.message.contains("positive integer"), "{error}");
    }
    // One is still fine, and so is the parser's own error for a non-integer.
    assert!(engr::reference::EngrRef::parse_embedded(&format!("obj:{compact}:1")).is_ok());

    // And it is refused at the command line, through the same parser.
    let refused = run_engr(
        root,
        &[
            "backlog",
            "new",
            "--topic",
            "t",
            "--text",
            "x",
            "--subject",
            &format!("engr:obj:{compact}:0"),
        ],
    );
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("positive integer"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
}

/// The screen a human is required to read names what the action applies to.
///
/// `Payload`'s rustdoc promises that "delete §3 cannot become delete §5 after it
/// was displayed". The hash kept that promise; the screen did not print the
/// section at all, so with two sections carrying identical wording, deleting the
/// first and deleting the second rendered byte-for-byte the same thing. Section
/// ids are never reused, so confirming the wrong one breaks every reference
/// pinning it with no way to put it back.
#[test]
fn a_candidate_screen_distinguishes_two_sections_that_say_the_same_thing() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let created = prepare(root, &["prepare", "--new", "--text", "retry policy"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id").to_owned();
    for _ in 0..2 {
        let added = prepare(
            root,
            &[
                "prepare",
                "--object",
                &id,
                "--add",
                "--no-based-on",
                "--text",
                "Retry with backoff.",
            ],
        );
        confirm(root, &added);
    }

    let screen = |section: &str| {
        let candidate = prepare(root, &["prepare", "--object", &id, "--delete", section]);
        let code = candidate["challenge"]
            .as_str()
            .expect("challenge")
            .to_owned();
        let shown =
            String::from_utf8_lossy(&run_engr(root, &["candidate", &code]).stdout).into_owned();
        run_engr(root, &["confirm", &format!("CONFIRM {code} no")]);
        shown
    };
    let first = screen("1");
    let second = screen("2");

    assert!(first.contains("section.deleted §1"), "{first}");
    assert!(second.contains("section.deleted §2"), "{second}");
    assert_ne!(
        first.lines().next(),
        second.lines().next(),
        "two different mutations must not render the same first line"
    );

    // And the object is named by something a human recognises, not only by an
    // abbreviated uuid they have no way to check.
    assert!(first.contains("retry policy"), "{first}");
}

/// The hard ceiling is not a threshold with an override, and must not read as
/// one. Both refusals used to end with the same sentence, so an agent that
/// learned to add `--oversize` for the first would add it for the second — and
/// burn a cycle discovering the flag it was just told to use does nothing.
#[test]
fn the_two_size_refusals_do_not_end_the_same_way() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let created = prepare(root, &["prepare", "--new", "--text", "bounds"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id").to_owned();

    let refusal = |text: &str, oversize: bool| {
        let mut args = vec![
            "prepare",
            "--object",
            &id,
            "--add",
            "--no-based-on",
            "--text",
            text,
        ];
        if oversize {
            args.push("--oversize");
        }
        let output = run_engr(root, &args);
        assert!(!output.status.success());
        String::from_utf8_lossy(&output.stderr).into_owned()
    };

    let normal = refusal(&"x".repeat(2000), false);
    assert!(
        normal.contains("prepare it again with --oversize"),
        "the first refusal offers the retry: {normal}"
    );

    let hard = refusal(&"x".repeat(6000), false);
    assert!(
        !hard.contains("prepare it again with --oversize"),
        "the hard ceiling must not read as a threshold with an override: {hard}"
    );
    assert!(hard.contains("no flag for this one"), "{hard}");

    // And it means it: the flag an agent might reach for anyway is refused the
    // same way rather than quietly letting the oversize record through.
    let retried = refusal(&"x".repeat(6000), true);
    assert!(retried.contains("no flag for this one"), "{retried}");
}

/// A machine-readable surface has to be readable by a strict machine.
///
/// The section wrapper added an explicit `id` beside a flattened section that
/// already had one, so every section object carried the key twice. Permissive
/// parsers take the last and happen to be right; a typed deserializer — including
/// the one this crate uses — refuses the document outright.
#[test]
fn backlog_json_sections_do_not_repeat_a_key() {
    // Deserialized from the raw text, not from a `Value`. Parsing into a
    // `Value` first collapses a repeated key silently, which is exactly the
    // reading that made this defect invisible.
    #[derive(serde::Deserialize)]
    #[allow(dead_code)]
    struct StrictSection {
        id: u64,
        reference: String,
        text: String,
        updated_at: String,
    }

    #[derive(serde::Deserialize)]
    #[allow(dead_code)]
    struct StrictItem {
        id: String,
        reference: String,
        sections: Vec<StrictSection>,
    }

    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let item = engr::backlog::create(
        root,
        "strictness",
        "a point",
        Vec::new(),
        &engr::backlog::Prepared::first(),
    )
    .expect("backlog");
    engr::backlog::add_section(
        root,
        &item.id,
        "a second point",
        Vec::new(),
        &engr::backlog::Prepared::first(),
    )
    .expect("second");

    let shown = run_engr(root, &["backlog", "show", &item.id, "--format", "json"]);
    assert!(shown.status.success());
    let raw = String::from_utf8_lossy(&shown.stdout).into_owned();
    let parsed: StrictItem = serde_json::from_str(&raw)
        .unwrap_or_else(|error| panic!("a strict reader must accept this: {error}\n{raw}"));
    assert_eq!(parsed.sections.len(), 2);
}

/// A pending candidate presents the identity it was prepared with.
///
/// The screen names the Object by title, which makes the title part of the
/// confirmation context — and a live lookup at render time would have left that
/// part outside the candidate and outside `integrity_sha256`. A title rewritten
/// in the projection afterwards would then change what the candidate presents
/// while its payload hash, its integrity hash and `expected_rev` all still
/// checked out, which is exactly what #20 says must be impossible.
#[test]
fn a_pending_candidate_shows_the_title_it_was_prepared_with() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let created = prepare(root, &["prepare", "--new", "--text", "the auth design"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id").to_owned();
    let added = prepare(
        root,
        &[
            "prepare",
            "--object",
            &id,
            "--add",
            "--no-based-on",
            "--text",
            "Use short-lived tokens.",
        ],
    );
    confirm(root, &added);

    let candidate = prepare(
        root,
        &[
            "prepare",
            "--object",
            &id,
            "--revise",
            "1",
            "--no-based-on",
            "--text",
            "Use short-lived tokens, capped at 15 minutes.",
        ],
    );
    let code = candidate["challenge"]
        .as_str()
        .expect("challenge")
        .to_owned();
    let before =
        String::from_utf8_lossy(&run_engr(root, &["candidate", &code]).stdout).into_owned();
    assert!(before.contains("the auth design"), "{before}");

    // Rewrite the title in the projection the way a text editor would, leaving
    // `rev` alone so the candidate stays fresh by its own binding.
    let path = store::object_path(root, &id);
    let mut stored: Value = store::read_json(&path).expect("object");
    stored["title"] = Value::String("a record this candidate is not about".into());
    store::write_json(&path, &stored).expect("rewrite title");

    let after = String::from_utf8_lossy(&run_engr(root, &["candidate", &code]).stdout).into_owned();
    assert!(
        !after.contains("a record this candidate is not about"),
        "a pending candidate must not present an identity nobody prepared: {after}"
    );
    assert!(after.contains("the auth design"), "{after}");
    assert_eq!(before, after, "the same candidate renders the same screen");

    // And it is still admissible: the projection changed, the prepared context
    // did not, so nothing about the candidate's own integrity moved.
    let admitted = run_engr(root, &["confirm", &format!("CONFIRM {code}")]);
    assert!(
        admitted.status.success(),
        "{}",
        String::from_utf8_lossy(&admitted.stderr)
    );
}

/// `--ref` refuses a target whose file was rewritten outside the gate.
///
/// The pin is derived by recomputing the target's content, not by copying its
/// stored seal, so the disagreement between the two is caught where the
/// reference is built rather than several layers later. Copying the seal would
/// have produced a reference that records agreement to wording nobody confirmed.
#[test]
fn a_reference_refuses_a_target_that_no_longer_matches_its_own_hash() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    git(root, &["init", "-q"]);

    let target = prepare(root, &["prepare", "--new", "--text", "upstream decision"]);
    confirm(root, &target);
    let target = target["object"].as_str().expect("object id").to_owned();
    let added = prepare(
        root,
        &[
            "prepare",
            "--object",
            &target,
            "--add",
            "--no-based-on",
            "--text",
            "Reason codes are numeric.",
        ],
    );
    confirm(root, &added);

    let source = prepare(root, &["prepare", "--new", "--text", "downstream decision"]);
    confirm(root, &source);
    let source = source["object"].as_str().expect("object id").to_owned();

    // The wording changes; the seal beside it does not.
    let path = store::object_path(root, &target);
    let mut stored: Value = store::read_json(&path).expect("object");
    stored["sections"][0]["text"] = Value::String("Reason codes are free text.".into());
    store::write_json(&path, &stored).expect("rewrite");

    let reference = format!("{target}:1");
    let refused = run_engr(
        root,
        &[
            "prepare",
            "--object",
            &source,
            "--add",
            "--no-based-on",
            "--text",
            "So the UI renders integers.",
            "--ref",
            &reference,
        ],
    );
    assert!(!refused.status.success());
    let message = String::from_utf8_lossy(&refused.stderr).to_string();
    assert!(
        message.contains("changed outside the gate"),
        "the refusal must name what is actually wrong: {message}"
    );
}

/// A machine surface does not emit a successful document and then fail.
///
/// `rules show --json` resolved every basis and printed the Rule *before*
/// returning the failure, so a Rule whose required material cannot be resolved
/// looked exactly like a usable one on stdout. A caller that drops the exit
/// status — a pipe, a wrapper, anything that reads output and not status —
/// would consume normative wording as reviewable when engr had already
/// established that it is not.
///
/// The human surface may print `UNUSABLE` and then fail, because a person reads
/// the line. A parser reads the document.
#[test]
fn rules_show_json_emits_nothing_when_the_rule_cannot_be_used() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    std::fs::create_dir_all(engr::rules::dir(root)).expect("rules dir");
    std::fs::write(root.join("AGENTS.md"), "the contract\n").expect("basis");
    std::fs::write(
        engr::rules::dir(root).join("architecture.md"),
        "---\nid: architecture\napplies:\n  domains: [object]\nbased_on:\n  - path: AGENTS.md\n---\n\n# Architecture\n\nThe rule.\n",
    )
    .expect("rule");

    // Usable: the document is emitted and the command succeeds.
    let shown = run_engr(root, &["rules", "show", "architecture", "--json"]);
    assert!(shown.status.success());
    let document: Value =
        serde_json::from_slice(&shown.stdout).expect("a usable rule is a json document");
    assert_eq!(document["id"], "architecture");

    // The material the rule rests on goes away.
    std::fs::remove_file(root.join("AGENTS.md")).expect("remove basis");

    let shown = run_engr(root, &["rules", "show", "architecture", "--json"]);
    assert!(!shown.status.success(), "an unusable rule is a failure");
    assert!(
        shown.stdout.is_empty(),
        "and nothing that looks like a usable rule reaches stdout: {}",
        String::from_utf8_lossy(&shown.stdout)
    );
    assert!(
        String::from_utf8_lossy(&shown.stderr).contains("does not exist"),
        "the reason goes where a failure goes"
    );

    // `ls --json` already carried `usable`, and still does — the two surfaces
    // agree rather than one of them being silently weaker.
    let listed = run_engr(root, &["rules", "ls", "--json"]);
    assert!(listed.status.success());
    let listed: Value = serde_json::from_slice(&listed.stdout).expect("json");
    assert_eq!(listed[0]["usable"], false);
}

/// One malformed Object does not disable the domains that do not depend on it.
///
/// Every command asks whether the workspace still uses the legacy spelling, and
/// that question used to read every Object file — so a single unreadable one
/// made `backlog ls`, every Work command and every Collection command exit with
/// a parse error about a file none of them were going to touch.
///
/// Isolation is not leniency. Object authority still fails closed: loading the
/// malformed Object is refused, and `verify` says so. What changed is that
/// three independent domains stopped being taken down with it.
#[test]
fn a_malformed_object_does_not_take_the_other_domains_down_with_it() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");

    let healthy = prepare(root, &["prepare", "--new", "--text", "a sound record"]);
    confirm(root, &healthy);
    let healthy = healthy["object"].as_str().expect("object id").to_owned();
    let broken = prepare(root, &["prepare", "--new", "--text", "about to be broken"]);
    confirm(root, &broken);
    let broken = broken["object"].as_str().expect("object id").to_owned();
    engr::backlog::create(
        root,
        "staging",
        "an unresolved point",
        Vec::new(),
        &engr::backlog::Prepared::first(),
    )
    .expect("backlog");
    engr::work::start(root, &healthy, Some("underway")).expect("work");
    engr::collection::create(root, "a plan", None, None).expect("collection");

    std::fs::write(store::object_path(root, &broken), "not json at all").expect("break it");

    for args in [
        vec!["backlog", "ls"],
        vec!["work", "show", &healthy],
        vec!["collection", "ls"],
        vec!["show", &healthy],
        vec!["verify", &healthy],
    ] {
        let output = run_engr(root, &args);
        assert!(
            output.status.success(),
            "{args:?} does not depend on the broken object: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // And the broken object itself is still refused, on every path that would
    // have to trust it.
    for args in [vec!["show", &broken], vec!["verify", &broken]] {
        let output = run_engr(root, &args);
        assert!(!output.status.success(), "{args:?} must fail closed");
    }
    assert!(
        engr::ops::effective(root, &broken).is_err(),
        "the library path fails closed too"
    );

    // A file claiming both spellings of the lifecycle field cannot say which it
    // means, so loading it is refused rather than serde quietly taking one.
    let mut confused: Value = store::read_json(&store::object_path(root, &healthy)).expect("read");
    confused["status"] = confused["state"].clone();
    store::write_json(&store::object_path(root, &healthy), &confused).expect("write");
    let error = engr::ops::effective(root, &healthy).expect_err("two spellings, one truth");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("both legacy status"), "{error}");
}

/// A workspace at an older version is refused, not reinterpreted.
///
/// This is the whole reason the Rule review policy moved the workspace version.
/// The same `.engr/rules/*.md` bytes mean different things under version 1 and
/// version 2 — a file with no `review:` block now carries an effective ceiling
/// and an exhaustion action, and an explicit block is an unknown field to the
/// older build. Two builds must not both accept the workspace and disagree about
/// what its rules say, so a build that does not write this version refuses it
/// and says which version it found.
#[test]
fn an_older_workspace_is_refused_rather_than_read_under_the_new_rule_semantics() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    std::fs::create_dir_all(engr::rules::dir(root)).expect("rules dir");
    std::fs::write(
        engr::rules::dir(root).join("policy.md"),
        "---\nid: recording-policy\napplies:\n  domains:\n    - backlog\n---\n\n# Recording policy\n\nSay what changed.\n",
    )
    .expect("rule");

    // Exactly what a version 1 workspace looks like: intact, well formed, and
    // written by a build that had never heard of `review:`.
    let format_path = store::engr_dir(root).join("format.json");
    std::fs::write(&format_path, r#"{"format":"engr-workspace","version":1}"#).expect("format");

    let listed = run_engr(root, &["rules", "ls"]);
    assert_eq!(listed.status.code(), Some(engr::EXIT_SCHEMA));
    let stderr = String::from_utf8_lossy(&listed.stderr).to_string();
    assert!(
        stderr.contains("version 1") && stderr.contains("engr migrate"),
        "the refusal names the version it found and what to do: {stderr}"
    );
    assert!(
        String::from_utf8_lossy(&listed.stdout).is_empty(),
        "and nothing about the rule reaches stdout, because it was never read \
         under this build's semantics"
    );
    // A mutation is refused for the same reason, through the same door.
    let prepared = run_engr(root, &["prepare", "--new", "--text", "not under version 1"]);
    assert_eq!(prepared.status.code(), Some(engr::EXIT_SCHEMA));

    // And the explicit migration is what makes the newer semantics apply.
    let migrated = run_engr(root, &["migrate"]);
    assert!(
        migrated.status.success(),
        "migration: {}",
        String::from_utf8_lossy(&migrated.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&format_path).expect("format"))
            .expect("json")["version"],
        engr::WORKSPACE_VERSION
    );
    let shown = run_engr(root, &["rules", "show", "recording-policy"]);
    assert!(shown.status.success());
    let stdout = String::from_utf8_lossy(&shown.stdout).to_string();
    assert!(
        stdout.contains("5 attempts; on_exhaustion = reject"),
        "after migrating, the same bytes carry the version 2 effective policy: {stdout}"
    );
    // This rule governs `backlog` only, where exhaustion neither refuses nor
    // summons anyone. The line therefore states the policy and not an outcome —
    // the earlier wording asserted the Object consequence for a rule that can
    // never have it.
    assert!(
        !stdout.contains("refused") && !stdout.contains("human confirms"),
        "no read surface claims a universal consequence: {stdout}"
    );
}

/// Moving the workspace version forward must not break provenance recorded
/// before it moved.
///
/// A reference pins a commit, and that snapshot carries whatever version was
/// current when it was taken. If the historical decoder insisted on the newest
/// version, every reference pinned before a migration would stop resolving —
/// the workspace moving forward would retroactively invalidate provenance that
/// was correct when it was recorded, which is the opposite of what pinning is
/// for. Only versions this build actually recognizes are readable; an unknown
/// one is still refused.
#[test]
fn a_snapshot_taken_before_the_migration_is_still_readable_after_it() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    git(root, &["init", "-q"]);
    let created = prepare(root, &["prepare", "--new", "--text", "pinned wording"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id").to_owned();

    // Commit while the workspace still says version 1, which is what every
    // commit made before this change looks like.
    let format_path = store::engr_dir(root).join("format.json");
    std::fs::write(&format_path, r#"{"format":"engr-workspace","version":1}"#).expect("format");
    git(root, &["add", "-A", "."]);
    commit_as_test(root, "before the migration");
    let commit = String::from_utf8(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("git")
            .stdout,
    )
    .expect("utf8")
    .trim()
    .to_owned();

    run_engr(root, &["migrate"]);
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&format_path).expect("format"))
            .expect("json")["version"],
        engr::WORKSPACE_VERSION,
        "the workspace really did move"
    );

    let historical = engr::git::object_at(root, &commit, &id)
        .expect("a snapshot at a recognized older version is readable");
    assert_eq!(
        historical.expect("object present in that snapshot").id,
        id,
        "and it decodes to the object that was pinned"
    );

    // A version nobody here recognizes is still refused, so this is a widening
    // to what is known rather than the check being dropped.
    std::fs::write(&format_path, r#"{"format":"engr-workspace","version":99}"#).expect("format");
    git(root, &["add", "-A", "."]);
    commit_as_test(root, "a version from the future");
    let future = String::from_utf8(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("git")
            .stdout,
    )
    .expect("utf8")
    .trim()
    .to_owned();
    let error =
        engr::git::object_at(root, &future, &id).expect_err("an unknown version is refused");
    assert!(error.message.contains("99"), "{}", error.message);
}

/// No read surface promises a consequence the rule's domain does not have.
///
/// The sharpest case: a Backlog-only rule asking for `human_confirmation`.
/// Backlog exhaustion never routes through the Human Gate, so a line reading
/// "then a human confirms" would be telling a reader the opposite of what will
/// happen — and it would be the surface, not the code, that they act on.
///
/// A rule does not have one consequence. It depends on the domain, and inside
/// Backlog on whether the mutation destroys unresolved work. So the surfaces
/// state the effective policy, and the protocol states the consequence per
/// domain, exactly once.
#[test]
fn rule_surfaces_state_the_policy_rather_than_promising_an_outcome() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    std::fs::create_dir_all(engr::rules::dir(root)).expect("rules dir");
    std::fs::write(
        engr::rules::dir(root).join("staging.md"),
        "---\nid: staging-policy\napplies:\n  domains:\n    - backlog\nreview:\n  max_attempts: 3\n  on_exhaustion: human_confirmation\n---\n\n# Staging policy\n\nSay what is unresolved.\n",
    )
    .expect("rule");

    for args in [vec!["rules", "show", "staging-policy"], vec!["rules", "ls"]] {
        let output = run_engr(root, &args);
        assert!(output.status.success(), "{args:?}");
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        assert!(
            stdout.contains("3 attempts; on_exhaustion = human_confirmation"),
            "{args:?} states the effective policy: {stdout}"
        );
        assert!(
            !stdout.contains("human confirms") && !stdout.contains("refused"),
            "{args:?} must not promise an outcome this domain does not have: {stdout}"
        );
    }

    // The machine surface already carried effective values and no prose
    // consequence; it stays that way.
    let json = run_engr(root, &["rules", "show", "staging-policy", "--json"]);
    let document: Value =
        serde_json::from_slice(&json.stdout).expect("rules show --json is a document");
    assert_eq!(document["review"]["max_attempts"], 3);
    assert_eq!(document["review"]["on_exhaustion"], "human_confirmation");
}

/// Set up a workspace whose backlog is governed by one rule, with one item.
fn governed_backlog(root: &Path, max_attempts: u32) -> String {
    let rules = engr::rules::dir(root);
    std::fs::create_dir_all(&rules).expect("rules dir");
    std::fs::write(
        rules.join("careful.md"),
        format!("---\nid: careful\napplies:\n  domains:\n    - backlog\nreview:\n  max_attempts: {max_attempts}\n---\n\n# Careful\n\nRead it first.\n"),
    )
    .expect("rule");
    engr::backlog::create(
        root,
        "staging",
        "an unresolved point",
        Vec::new(),
        &engr::backlog::Prepared::first(),
    )
    .expect("backlog")
    .id
}

/// What `backlog show --json` says to hand back for a given point.
fn expect_token(root: &Path, id: &str, section: Option<u64>) -> String {
    let shown = run_engr(root, &["backlog", "show", id, "--format", "json"]);
    assert!(shown.status.success());
    let shown: Value = serde_json::from_slice(&shown.stdout).expect("json");
    match section {
        None => shown["expect"]["rename"]
            .as_str()
            .expect("rename")
            .to_owned(),
        Some(n) => shown["sections"]
            .as_array()
            .expect("sections")
            .iter()
            .find(|s| s["id"] == n)
            .unwrap_or_else(|| panic!("§{n}"))["expect"]
            .as_str()
            .expect("expect")
            .to_owned(),
    }
}

/// `--attempt` reaches the mutation, and means the same thing at the CLI as it
/// does in the library.
///
/// A flag that parses and is then dropped is the worst version of this: the
/// command reports success, the point goes in unmarked, and the diagnostic that
/// was supposed to say "this was not reviewed" is simply absent — which is how
/// an unreviewed edit reads as a reviewed one.
#[test]
fn the_backlog_attempt_flag_is_the_one_the_review_is_composed_against() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let id = governed_backlog(root, 2);
    let expect = expect_token(root, &id, Some(1));

    // Counted from 1, so there is no attempt 0 to smuggle past the ceiling.
    let zero = run_engr(
        root,
        &[
            "backlog",
            "add",
            &id,
            "--text",
            "another",
            "--attempt",
            "0",
            "--expect",
            &expect,
        ],
    );
    assert_eq!(zero.status.code(), Some(engr::EXIT_USAGE));

    // Under the ceiling: admitted, and nothing to diagnose.
    let add = run_engr(root, &["backlog", "show", &id, "--format", "json"]);
    let add: Value = serde_json::from_slice(&add.stdout).expect("json");
    let add = add["expect"]["add"].as_str().expect("add").to_owned();
    assert!(run_engr(
        root,
        &[
            "backlog",
            "add",
            &id,
            "--text",
            "another",
            "--attempt",
            "2",
            "--expect",
            &add
        ]
    )
    .status
    .success());
    let stored = engr::backlog::load(root, &id).expect("load");
    assert_eq!(stored.section(2).expect("§2").rule_review, None);

    // Past it: still admitted, and marked with what it went in on.
    let second = expect_token(root, &id, Some(2));
    assert!(run_engr(
        root,
        &[
            "backlog",
            "revise",
            &id,
            "--section",
            "2",
            "--text",
            "reworded",
            "--attempt",
            "7",
            "--expect",
            &second
        ]
    )
    .status
    .success());
    let stored = engr::backlog::load(root, &id).expect("load");
    assert_eq!(
        stored.section(2).expect("§2").rule_review,
        Some(engr::rules::RuleReview {
            attempts: 7,
            limit: 2
        })
    );

    // Except where the mutation would remove the point.
    let second = expect_token(root, &id, Some(2));
    let refused = run_engr(
        root,
        &[
            "backlog",
            "consume",
            &id,
            "--section",
            "2",
            "--attempt",
            "7",
            "--expect",
            &second,
        ],
    );
    assert_eq!(refused.status.code(), Some(engr::EXIT_INVARIANT));
    assert!(run_engr(
        root,
        &[
            "backlog",
            "consume",
            &id,
            "--section",
            "2",
            "--attempt",
            "2",
            "--expect",
            &second
        ]
    )
    .status
    .success());
}

/// A reviewed mutation carries what it was reviewed against, or does not run.
///
/// The review happens before the command is invoked. So a command that reads and
/// writes under one lock still leaves the whole interval between reviewing and
/// running unguarded: a concurrent edit in that gap lands underneath a mutation
/// nobody reviewed against it, and every check inside the lock passes, because
/// the thing they check is what the command read a microsecond ago rather than
/// what the agent read.
#[test]
fn a_reviewed_backlog_mutation_carries_the_predecessor_it_was_reviewed_against() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let id = governed_backlog(root, 5);

    // A rule governs backlog, so a mutation with nothing to anchor it is usage,
    // not silently accepted.
    let bare = run_engr(
        root,
        &[
            "backlog",
            "revise",
            &id,
            "--section",
            "1",
            "--text",
            "reworded",
        ],
    );
    assert_eq!(bare.status.code(), Some(engr::EXIT_USAGE));
    assert!(
        String::from_utf8_lossy(&bare.stderr).contains("--expect"),
        "the refusal says what is missing: {}",
        String::from_utf8_lossy(&bare.stderr)
    );

    // Read it, then somebody else sharpens it before the reviewed change runs.
    let stale = expect_token(root, &id, Some(1));
    engr::backlog::revise_section(
        root,
        &id,
        1,
        "sharpened by someone else",
        &engr::backlog::Prepared::first()
            .against(engr::backlog::Precondition::section(root, &id, 1).expect("observe")),
    )
    .expect("concurrent");

    let refused = run_engr(
        root,
        &[
            "backlog",
            "revise",
            &id,
            "--section",
            "1",
            "--text",
            "reviewed against the old wording",
            "--expect",
            &stale,
        ],
    );
    assert_eq!(refused.status.code(), Some(engr::EXIT_STALE));
    assert_eq!(
        engr::backlog::load(root, &id)
            .expect("load")
            .section(1)
            .expect("§1")
            .text,
        "sharpened by someone else",
        "the reviewed change did not land on top of what it never read"
    );

    // Read it again, and it goes through.
    let current = expect_token(root, &id, Some(1));
    assert!(run_engr(
        root,
        &[
            "backlog",
            "revise",
            &id,
            "--section",
            "1",
            "--text",
            "reviewed against this",
            "--expect",
            &current
        ]
    )
    .status
    .success());
}

/// A merge carries one predecessor per point it touches.
#[test]
fn a_merge_carries_a_predecessor_for_every_point_it_touches() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let id = governed_backlog(root, 5);
    engr::backlog::add_section(
        root,
        &id,
        "a second point",
        Vec::new(),
        &engr::backlog::Prepared::first()
            .against(engr::backlog::Precondition::section_absent(root, &id).expect("observe")),
    )
    .expect("add");

    let first = expect_token(root, &id, Some(1));
    let second = expect_token(root, &id, Some(2));

    // Only one of the two: the judgement was about both.
    let partial = run_engr(
        root,
        &[
            "backlog",
            "merge",
            &id,
            "--into",
            "1",
            "--sections",
            "2",
            "--text",
            "one point",
            "--expect",
            &first,
        ],
    );
    assert!(!partial.status.success());

    assert!(run_engr(
        root,
        &[
            "backlog",
            "merge",
            &id,
            "--into",
            "1",
            "--sections",
            "2",
            "--text",
            "one point",
            "--expect",
            &first,
            "--expect",
            &second
        ]
    )
    .status
    .success());
    let stored = engr::backlog::load(root, &id).expect("load");
    assert_eq!(stored.sections.len(), 1);
    assert_eq!(stored.section(1).expect("§1").text, "one point");
}

/// With no rule governing backlog there is no review, so nothing to anchor.
#[test]
fn an_ungoverned_backlog_mutation_needs_no_predecessor() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let item = engr::backlog::create(
        root,
        "staging",
        "an unresolved point",
        Vec::new(),
        &engr::backlog::Prepared::first(),
    )
    .expect("backlog");

    assert!(run_engr(
        root,
        &[
            "backlog",
            "revise",
            &item.id,
            "--section",
            "1",
            "--text",
            "reworded"
        ]
    )
    .status
    .success());

    // And one may still be given, in which case it is held to.
    let stale = expect_token(root, &item.id, Some(1));
    engr::backlog::revise_section(
        root,
        &item.id,
        1,
        "moved",
        &engr::backlog::Prepared::first(),
    )
    .expect("concurrent");
    let refused = run_engr(
        root,
        &[
            "backlog",
            "revise",
            &item.id,
            "--section",
            "1",
            "--text",
            "later",
            "--expect",
            &stale,
        ],
    );
    assert_eq!(refused.status.code(), Some(engr::EXIT_STALE));
}

/// A rule about unresolved points must not make unresolved points uncreatable.
///
/// Creation binds nothing engr can check, because engr allocates the id. Putting
/// it on the governed path made `backlog new` refuse without `--expect` and
/// panic with one, so the ordinary creation path could neither succeed nor fail
/// safely in any workspace that had a backlog rule — which is every workspace
/// that cares most about unresolved work.
#[test]
fn creating_a_point_from_the_cli_survives_a_rule_that_governs_backlog() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let rules = engr::rules::dir(root);
    std::fs::create_dir_all(&rules).expect("rules dir");
    std::fs::write(
        rules.join("careful.md"),
        "---\nid: careful\napplies:\n  domains:\n    - backlog\nreview:\n  max_attempts: 5\n---\n\n# Careful\n\nRead it first.\n",
    )
    .expect("rule");

    let created = run_engr(
        root,
        &[
            "backlog",
            "new",
            "--topic",
            "a topic",
            "--text",
            "unresolved",
        ],
    );
    assert!(
        created.status.success(),
        "creation must stay reachable: {}",
        String::from_utf8_lossy(&created.stderr)
    );
    assert_eq!(engr::backlog::ids(root).expect("ids").len(), 1);

    // Offering a predecessor is usage, and answered rather than ignored — and
    // above all not a panic.
    let offered = run_engr(
        root,
        &[
            "backlog",
            "new",
            "--topic",
            "another",
            "--text",
            "unresolved",
            "--expect",
            "whatever",
        ],
    );
    assert_eq!(offered.status.code(), Some(engr::EXIT_USAGE));
    let said = String::from_utf8_lossy(&offered.stderr);
    assert!(
        said.contains("engr allocates"),
        "the refusal says why: {said}"
    );
    assert!(
        !said.contains("panicked"),
        "a reachable path must not panic: {said}"
    );
    assert_eq!(engr::backlog::ids(root).expect("ids").len(), 1);
}

/// An explicitly named historical snapshot stays admissible for `implemented_by`.
///
/// Backlog's `dirty` compares the working file with the commit being pinned,
/// because a subject's baseline is meant to reconstruct what was read. The
/// record asks a different question: this wording claims something is
/// implemented *there*, so what the author read must be committed somewhere —
/// not identical to the revision they chose. Sharing the first answer here made
/// engr refuse a perfectly clean worktree, and say the file had uncommitted
/// changes when it had none.
#[test]
fn an_explicit_historical_revision_is_still_a_valid_implemented_by_snapshot() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.name", "test"],
        vec!["config", "user.email", "test@example.com"],
    ] {
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(&args)
            .status()
            .expect("git")
            .success());
    }
    let commit = |message: &str| {
        for args in [vec!["add", "-A"], vec!["commit", "-qm", message]] {
            assert!(std::process::Command::new("git")
                .arg("-C")
                .arg(root)
                .args(&args)
                .status()
                .expect("git")
                .success());
        }
        engr::git::head(root).expect("HEAD")
    };

    std::fs::write(root.join("session.rs"), "fn first() {}\n").expect("write");
    let earlier = commit("first");
    std::fs::write(root.join("session.rs"), "fn second() {}\n").expect("rewrite");
    commit("second");

    let object = prepare(root, &["prepare", "--new", "--text", "an assertion"]);
    confirm(root, &object);
    let object = object["object"].as_str().expect("object").to_owned();

    // Clean worktree; the file has simply moved on since the chosen commit.
    let pinned = prepare(
        root,
        &[
            "prepare",
            "--object",
            &object,
            "--add",
            "--text",
            "this is implemented in the earlier shape",
            "--implemented-by-file",
            "session.rs",
            "--implemented-at",
            &earlier,
        ],
    );
    confirm(root, &pinned);
    let stored = engr::ops::effective(root, &object).expect("object");
    let relation = &stored.section(1).expect("§1").relations[0];
    match &relation.target {
        engr::semantics::Target::File { commit, path } => {
            assert_eq!(commit, &earlier, "the snapshot the author named");
            assert_eq!(path, "session.rs");
        }
        other => panic!("unexpected target {other:?}"),
    }

    // The guard it does keep: nothing the author read is committed at all.
    std::fs::write(root.join("session.rs"), "fn uncommitted() {}\n").expect("edit");
    let refused = run_engr(
        root,
        &[
            "prepare",
            "--object",
            &object,
            "--add",
            "--text",
            "implemented in something unsaved",
            "--implemented-by-file",
            "session.rs",
            "--implemented-at",
            &earlier,
        ],
    );
    assert_eq!(refused.status.code(), Some(engr::EXIT_INVARIANT));
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("uncommitted changes"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
}

/// Migration moves representation; it does not decide the fate of a pending
/// confirmation.
///
/// A candidate prepared under the older contract cannot be admitted afterwards,
/// and that failure belongs at the moment somebody tries to act on it — not to
/// migration, which would either block on material a human is in the middle of
/// or quietly discard it. Both would have migration ruling on Human-Gate state,
/// which is exactly what it must not do.
#[test]
fn migration_neither_blocks_on_a_pending_candidate_nor_disposes_of_it() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");

    let pending = prepare(root, &["prepare", "--new", "--text", "awaiting a human"]);
    let challenge = pending["challenge"].as_str().expect("challenge").to_owned();
    assert_eq!(engr::gate::pending_codes(root).expect("pending").len(), 1);

    // Put the workspace back a generation, leaving the candidate where it is.
    let format_path = engr::store::engr_dir(root).join("format.json");
    std::fs::write(&format_path, r#"{"format":"engr-workspace","version":1}"#).expect("format");

    let migrated = run_engr(root, &["migrate"]);
    assert!(
        migrated.status.success(),
        "migration must not block on a pending candidate: {}",
        String::from_utf8_lossy(&migrated.stderr)
    );

    // Still there. Migration did not rule on it either way.
    assert_eq!(
        engr::gate::pending_codes(root).expect("pending"),
        vec![challenge.clone()],
        "migration does not dispose of material a human was in the middle of"
    );

    // And it is still confirmable, because this candidate's context did not
    // change shape — the fail-closed path is for one that did.
    let confirmed = run_engr(root, &["confirm", &format!("CONFIRM {challenge}")]);
    assert!(
        confirmed.status.success(),
        "{}",
        String::from_utf8_lossy(&confirmed.stderr)
    );
}
