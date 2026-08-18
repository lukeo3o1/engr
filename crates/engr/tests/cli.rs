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
            content: Content {
                text: "target".to_owned(),
                based_on: None,
                refs: Vec::new(),
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
            content: Content {
                text: "projection wording".to_owned(),
                based_on: None,
                refs: Vec::new(),
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
            content: Content {
                text: "effective crash-tail wording".to_owned(),
                based_on: None,
                refs: Vec::new(),
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
            content: Content {
                text: "source".to_owned(),
                based_on: None,
                refs: Vec::new(),
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
        content: Content {
            text: "cannot pin stale projection".to_owned(),
            based_on: None,
            refs: vec![engr::model::Ref {
                object: target.clone(),
                section: 1,
                sha256: raw.sha256,
                commit: old_commit.clone(),
            }],
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
            content: Content {
                text: "historically verified effective wording".to_owned(),
                based_on: None,
                refs: vec![engr::model::Ref {
                    object: target,
                    section: 1,
                    sha256: effective.sha256,
                    commit: committed_effective,
                }],
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
            content: Content {
                text: "candidate state".to_owned(),
                based_on: None,
                refs: Vec::new(),
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
            content: Content {
                text: "apply once".to_owned(),
                based_on: None,
                refs: Vec::new(),
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
            content: Content {
                text: "stale candidate".to_owned(),
                based_on: None,
                refs: Vec::new(),
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
            content: Content {
                text: "overtaking mutation".to_owned(),
                based_on: None,
                refs: Vec::new(),
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
    assert_eq!(
        std::fs::read(&format_path).expect("format after migration"),
        format_before,
        "a valid workspace authority is not rewritten"
    );
    assert_eq!(
        migrated_object["format"], "engr-object",
        "compatible legacy marker is preserved"
    );
    assert_eq!(migrated_object["version"], 1);
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&format_path).expect("format"))
            .expect("json")["version"],
        1
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
        content: Content {
            text: "event wording".to_owned(),
            based_on: None,
            refs: Vec::new(),
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
        content: Content {
            text: "reconcile under lock".to_owned(),
            based_on: None,
            refs: Vec::new(),
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
    assert_event_is_rejected(&root, event);
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

/// What a candidate derived from staging shows, and what confirming it says it
/// did. The flags that declare a source are still an open protocol question, so
/// the candidate is prepared through the library — but the screens a human
/// reads are the command line's, and they are what this pins.
#[test]
fn a_candidate_from_staging_shows_what_confirming_will_do_to_it() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    assert!(run_engr(root, &["init"]).status.success(), "init");
    let created = prepare(root, &["prepare", "--new", "--text", "the outcome"]);
    confirm(root, &created);
    let object = created["object"].as_str().expect("object id").to_owned();

    let staging = engr::backlog::create(root, "two points", "settled here", Vec::new())
        .expect("stage")
        .id;
    engr::backlog::add_section(root, &staging, "still open", Vec::new()).expect("second point");

    let compact =
        engr::reference::encode_uuid(uuid::Uuid::parse_str(&object).expect("object id is a uuid"));
    let prepared = gate::prepare_from_backlog(
        root,
        Payload {
            action: Action::SectionAdded,
            object: object.clone(),
            content: Content {
                text: "what the work produced".to_owned(),
                based_on: None,
                refs: Vec::new(),
            },
        },
        vec![
            gate::SourceRequest {
                item: staging.clone(),
                section: 1,
                produced: Vec::new(),
                resolves: true,
            },
            gate::SourceRequest {
                item: staging.clone(),
                section: 2,
                produced: vec![engr::backlog::Produced::object(format!("obj:{compact}"))],
                resolves: false,
            },
        ],
    )
    .expect("prepare from staging");
    let code = prepared.candidate.challenge.clone();

    // Re-rendered hours later, the screen still says what typing the code does.
    let shown = run_engr(root, &["candidate", &code]);
    let shown = String::from_utf8(shown.stdout).expect("utf8");
    assert!(
        shown.contains("§1  resolved by this — will be consumed"),
        "got {shown:?}"
    );
    assert!(
        shown.contains("§2  still unresolved after this"),
        "got {shown:?}"
    );
    assert!(shown.contains(&format!("produced engr:obj:{compact}")));

    let confirmed = run_engr(root, &["confirm", &format!("CONFIRM {code}")]);
    assert!(
        confirmed.status.success(),
        "confirm: {}",
        String::from_utf8_lossy(&confirmed.stderr)
    );
    let confirmed = String::from_utf8(confirmed.stdout).expect("utf8");
    assert!(confirmed.contains("CONFIRMED"));
    assert!(
        confirmed.contains("resolved and consumed"),
        "confirming must say what it did to staging: {confirmed:?}"
    );
    assert!(
        confirmed.contains("recorded 1 produced outcome(s); still unresolved"),
        "including the point it did not settle: {confirmed:?}"
    );

    let stored = engr::backlog::load(root, &staging).expect("the second point survives");
    assert_eq!(stored.sections.len(), 1);
    assert_eq!(stored.sections[0].id, 2);
    assert_eq!(stored.sections[0].produced.len(), 1);
}

/// Three different failures, three different exit codes. Phase 1 fixed that
/// boundary for the record; staging has to keep it, or a script cannot tell a
/// typo from a corrupted workspace.
#[test]
fn the_backlog_cli_separates_bad_input_from_missing_and_malformed() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    assert!(run_engr(root, &["init"]).status.success(), "init");
    let item = engr::backlog::create(root, "topic", "unresolved", Vec::new())
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

/// The confirmation screen names which unresolved point gets consumed, so two
/// different points may never print the same identifier on it. Backlog ids
/// abbreviate against Backlog ids: borrowing the Object width is how two
/// distinct sources become indistinguishable exactly where it matters.
#[test]
fn candidate_rendering_abbreviates_backlog_sources_in_their_own_namespace() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    assert!(run_engr(root, &["init"]).status.success(), "init");
    let created = prepare(root, &["prepare", "--new", "--text", "the outcome"]);
    confirm(root, &created);
    let object = created["object"].as_str().expect("object id").to_owned();

    // Two backlog items differing only in their last character. One object, so
    // the Object width is 8 — which would render both of these identically.
    let ids = [
        "01890f3e-7c54-7cc1-b21e-8f7b2b9d5f6a",
        "01890f3e-7c54-7cc1-b21e-8f7b2b9d5f6b",
    ];
    for id in ids {
        let item = serde_json::json!({
            "id": id,
            "topic": format!("unresolved point in {id}"),
            "next_section_id": 2,
            "sections": [{
                "id": 1,
                "text": "still open",
                "updated_at": "2026-08-17T00:00:00Z",
                "subjects": [],
            }],
        });
        store::write_json(&engr::backlog::item_path(root, id), &item).expect("stage");
    }
    assert_eq!(engr::view::width(root), 8, "the object namespace is narrow");

    let prepared = gate::prepare_from_backlog(
        root,
        Payload {
            action: Action::SectionAdded,
            object: object.clone(),
            content: Content {
                text: "what the work produced".to_owned(),
                based_on: None,
                refs: Vec::new(),
            },
        },
        ids.iter()
            .map(|id| gate::SourceRequest {
                item: (*id).to_owned(),
                section: 1,
                produced: Vec::new(),
                resolves: false,
            })
            .collect(),
    )
    .expect("prepare from two staged points");

    let shown = run_engr(root, &["candidate", &prepared.candidate.challenge]);
    let shown = String::from_utf8(shown.stdout).expect("utf8");
    let rendered: Vec<&str> = shown
        .lines()
        .filter_map(|line| line.strip_prefix("Backlog    "))
        .map(|line| line.split_whitespace().next().expect("an id"))
        .collect();
    assert_eq!(rendered.len(), 2, "both sources are shown: {shown:?}");
    assert_ne!(
        rendered[0], rendered[1],
        "two unresolved points must not render identically: {shown:?}"
    );
    for (id, printed) in ids.iter().zip(&rendered) {
        assert!(
            id.starts_with(printed),
            "{printed} does not abbreviate {id}"
        );
    }
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
            "--sections",
            "1,2",
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
    assert_eq!(item.sections[0].id, 3);
    assert!(
        engr::gate::pending(root).expect("candidates").is_empty(),
        "staging edits never mint a challenge code"
    );

    // A dirty path cannot be pinned, and the refusal says what to do about it.
    std::fs::write(root.join("session.rs"), "fn refresh() { todo!() }\n").expect("edit");
    let refused = run_engr(
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
    assert!(!refused.status.success());
    let message = String::from_utf8_lossy(&refused.stderr).to_string();
    assert!(message.contains("commit it first"), "got {message:?}");

    assert!(run_engr(root, &["backlog", "rm", &id, "--section", "3"])
        .status
        .success());
    assert!(
        engr::backlog::ids(root).expect("ids").is_empty(),
        "removing the last unresolved point removes the topic"
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
