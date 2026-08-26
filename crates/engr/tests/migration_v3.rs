use engr::model::{Action, Content, Event, Payload, Provenance, EVENT_FORMAT};
use engr::{integrity, store};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

fn predecessor(root: &Path, objects: &[Value]) {
    for path in [
        store::objects_dir(root),
        store::events_dir(root),
        store::candidates_dir(root),
        engr::backlog::dir(root),
        engr::work::dir(root),
        engr::collection::dir(root),
    ] {
        std::fs::create_dir_all(path).expect("directory");
    }
    std::fs::write(
        store::engr_dir(root).join("format.json"),
        r#"{"format":"engr-workspace","version":2}"#,
    )
    .expect("format");
    for object in objects {
        let id = object["id"].as_str().expect("id");
        std::fs::write(
            store::object_path(root, id),
            format!("{}\n", serde_json::to_string_pretty(object).expect("json")),
        )
        .expect("object");
        history(root, object);
    }
}

/// The admitted history the stored projection is derivable from.
///
/// A predecessor Object with no Event file cannot be proven and is refused, so
/// a fixture standing in for a real v2 workspace has to carry one — which is
/// what a real v2 workspace has, because the only way an Object got there was
/// through the gate.
fn history(root: &Path, object: &Value) {
    let id = object["id"].as_str().expect("id");
    let mut records = vec![event(
        1,
        Payload {
            action: Action::ObjectCreated,
            object: id.to_owned(),
            becomes: None,
            content: Content {
                text: object["title"].as_str().expect("title").to_owned(),
                ..Content::default()
            },
        },
    )];
    for (index, section) in object["sections"]
        .as_array()
        .expect("sections")
        .iter()
        .enumerate()
    {
        let content: Content =
            serde_json::from_value(section.clone()).expect("section content is a Content");
        records.push(event(
            2 + index as u64,
            Payload {
                action: Action::SectionAdded,
                object: id.to_owned(),
                becomes: None,
                content,
            },
        ));
    }
    let lines: Vec<String> = records
        .iter()
        .map(|record| serde_json::to_string(record).expect("event json"))
        .collect();
    std::fs::write(
        store::events_path(root, id),
        format!("{}\n", lines.join("\n")),
    )
    .expect("events");
}

fn event(rev: u64, payload: Payload) -> Event {
    let payload_sha256 = payload.sha256().expect("payload hash");
    Event {
        format: EVENT_FORMAT.to_owned(),
        version: engr::EVENT_ENVELOPE_VERSION_V0,
        event_id: engr::model::new_id(),
        rev,
        time: "2026-08-25T00:00:00Z".to_owned(),
        payload,
        provenance: Provenance::confirmed("234567", payload_sha256),
    }
}

fn object(id: &str, text: &str) -> Value {
    let content = Content {
        text: text.to_owned(),
        ..Content::default()
    };
    json!({
        "id": id,
        "title": "migration",
        "state": "open",
        "rev": 2,
        "next_section_id": 2,
        "sections": [{
            "id": 1,
            "text": text,
            "refs": [],
            "sha256": content.sha256().expect("seal"),
            "confirmed_at": "2026-08-25T00:00:00Z"
        }]
    })
}

#[test]
fn migration_commits_one_canonical_v3_generation() {
    let temp = tempfile::tempdir().expect("temp");
    let id = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    predecessor(temp.path(), &[object(id, "the gate owns admission")]);

    store::migrate(temp.path()).expect("migrate");

    assert_eq!(
        store::validate_format(temp.path()).expect("format"),
        store::WorkspaceFormat::Current
    );
    let migrated = store::load_object(temp.path(), id).expect("object");
    integrity::check_stored_object_integrity(&migrated).expect("integrity");
    let stored: Value = store::read_json(&store::object_path(temp.path(), id)).expect("raw");
    assert!(stored.get("format").is_none());
    assert!(stored.get("version").is_none());
    assert!(stored.get("sha256").is_some());
    assert!(stored.get("type").is_some_and(Value::is_null));
    let section = &stored["sections"][0];
    for member in [
        "id",
        "admission",
        "role",
        "text",
        "content",
        "based_on",
        "refs",
        "relations",
        "sha256",
        "admitted_at",
    ] {
        assert!(section.get(member).is_some(), "missing {member}: {section}");
    }
    assert!(section.get("confirmed_at").is_none());
    assert_eq!(section["admission"], "human");
}

#[test]
fn migration_resumes_after_objects_were_copied_but_before_format_advanced() {
    let interrupted = tempfile::tempdir().expect("interrupted");
    let completed = tempfile::tempdir().expect("completed");
    let id = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    predecessor(
        interrupted.path(),
        &[object(id, "resume the staged generation")],
    );
    predecessor(
        completed.path(),
        &[object(id, "resume the staged generation")],
    );
    let predecessor_bytes =
        std::fs::read_to_string(store::object_path(interrupted.path(), id)).expect("predecessor");
    let predecessor_events =
        std::fs::read_to_string(store::events_path(interrupted.path(), id)).expect("history");
    store::migrate(completed.path()).expect("reference migration");

    let migrated =
        std::fs::read_to_string(store::object_path(completed.path(), id)).expect("migrated object");
    let stage = store::engr_dir(interrupted.path()).join("migration-v3");
    std::fs::create_dir_all(stage.join("objects")).expect("stage objects");
    std::fs::write(stage.join("objects").join(format!("{id}.json")), &migrated)
        .expect("staged object");
    let mut staged_objects = serde_json::Map::new();
    staged_objects.insert(
        id.to_owned(),
        Value::String(engr::proof::sha256_of(&migrated)),
    );
    write_raw(
        &stage.join("manifest.json"),
        &json!({
            "source_version": 2,
            "target_version": engr::WORKSPACE_VERSION,
            "objects": staged_objects,
            "resources": {},
            "source": {
                format!("objects/{id}.json"): engr::proof::sha256_of(&predecessor_bytes),
                format!("events/{id}.jsonl"): engr::proof::sha256_of(&predecessor_events)
            }
        }),
    )
    .expect("manifest");

    // A crash can leave any prefix of the copy loop installed while format.json
    // still names the predecessor. Re-copying the sealed plan is idempotent.
    std::fs::write(store::object_path(interrupted.path(), id), &migrated)
        .expect("partially installed object");

    store::migrate(interrupted.path()).expect("resume");

    assert_eq!(
        store::validate_format(interrupted.path()).expect("format"),
        store::WorkspaceFormat::Current
    );
    assert!(!stage.exists());
    assert_eq!(
        std::fs::read_to_string(store::object_path(interrupted.path(), id)).expect("resumed"),
        migrated
    );
}

#[test]
fn whole_workspace_preflight_writes_nothing_when_one_predecessor_seal_fails() {
    let temp = tempfile::tempdir().expect("temp");
    let good = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    let bad = "018f7d58-4ca7-7a2e-98f1-9b3014681849";
    let good_object = object(good, "valid predecessor");
    let mut bad_object = object(bad, "invalid predecessor");
    bad_object["sections"][0]["sha256"] = Value::String("0".repeat(64));
    predecessor(temp.path(), &[good_object, bad_object]);
    let before = std::fs::read(store::object_path(temp.path(), good)).expect("before");

    let error = store::migrate(temp.path()).expect_err("bad seal must stop migration");

    assert_eq!(error.code, engr::EXIT_INVARIANT);
    let format: Value =
        store::read_json(&store::engr_dir(temp.path()).join("format.json")).expect("format");
    assert_eq!(format["version"], 2);
    assert_eq!(
        std::fs::read(store::object_path(temp.path(), good)).expect("after"),
        before
    );
    assert!(!store::engr_dir(temp.path()).join("migration-v3").exists());
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git");
    assert!(
        output.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

#[test]
fn legacy_refs_become_full_selective_refs_without_gaining_admission() {
    let temp = tempfile::tempdir().expect("temp");
    let target = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    let source = "018f7d58-4ca7-7a2e-98f1-9b3014681849";
    let target_object = object(target, "the target remains stable");
    predecessor(temp.path(), std::slice::from_ref(&target_object));
    git(temp.path(), &["init"]);
    git(temp.path(), &["add", ".engr"]);
    git(
        temp.path(),
        &[
            "-c",
            "user.name=engr test",
            "-c",
            "user.email=engr@example.invalid",
            "commit",
            "-m",
            "predecessor target",
        ],
    );
    let commit = git(temp.path(), &["rev-parse", "HEAD"]);
    let target_seal = target_object["sections"][0]["sha256"]
        .as_str()
        .expect("target seal");
    let reference = engr::model::Ref::legacy(target, 1, target_seal, &commit);
    let source_content = Content {
        text: "the source relies on all legacy target semantics".to_owned(),
        refs: vec![reference],
        ..Content::default()
    };
    let source_seal = source_content.sha256().expect("source seal");
    let source_object = json!({
        "id": source,
        "title": "source",
        "state": "open",
        "rev": 2,
        "next_section_id": 2,
        "sections": [{
            "id": 1,
            "text": source_content.text,
            "refs": source_content.refs,
            "sha256": source_seal,
            "confirmed_at": "2026-08-25T00:00:00Z"
        }]
    });
    predecessor(temp.path(), &[target_object, source_object]);

    store::migrate(temp.path()).expect("migrate");

    let source = store::load_object(temp.path(), source).expect("source");
    let selective = source.sections[0].refs[0]
        .as_selective()
        .expect("selective ref");
    let fields: Vec<_> = selective
        .fields()
        .iter()
        .map(|field| field.as_str())
        .collect();
    assert_eq!(
        fields,
        ["based_on", "content", "refs", "relations", "role", "text"]
    );
    assert!(!fields.contains(&"admission"));
    let target = store::load_object(temp.path(), target).expect("target");
    assert_eq!(
        engr::dependency::evaluate(
            temp.path(),
            &target,
            target.sha256.as_deref().expect("object seal"),
            selective,
        )
        .expect("dependency"),
        engr::dependency::Dependency::Unchanged
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

/// A predecessor Object with no admitted history cannot be proven, so it does
/// not receive the first v3 aggregate seal.
///
/// Its legacy Section seals say nothing about the Object level at all — not the
/// title, type, state, revision, counter, or which Sections belong to it. The
/// derivability comparison only ever reaches ids the EventStore knows, so an
/// Object absent from it was being sealed on the strength of evidence that
/// cannot answer the question. Under the retained EventStore contract every
/// stored Object has an admitted creation.
#[test]
fn a_predecessor_object_with_no_admitted_history_fails_closed() {
    let temp = tempfile::tempdir().expect("temp");
    let id = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    predecessor(temp.path(), &[object(id, "unprovable")]);
    let before = std::fs::read(store::object_path(temp.path(), id)).expect("before");
    std::fs::remove_file(store::events_path(temp.path(), id)).expect("remove the history");

    let error = store::migrate(temp.path()).expect_err("nothing can prove this projection");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("no admitted history"), "{error}");
    assert_eq!(
        std::fs::read(store::object_path(temp.path(), id)).expect("after"),
        before,
        "and nothing was written"
    );
    assert_ne!(
        store::validate_format(temp.path()).expect("format"),
        store::WorkspaceFormat::Current,
        "the version did not advance"
    );
}

/// Adopting JCS is itself a representation migration.
///
/// The predecessor build wrote its retained resources with a pretty serializer,
/// so those bytes are not what v3 says a current resource is. Advancing
/// `format.json` over them unchanged would leave a workspace full of resources
/// its own reader refuses.
#[test]
fn retained_resources_are_rewritten_into_the_current_representation() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    let id = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    predecessor(root, &[object(id, "with retained resources")]);

    let item_id = "018f7d58-4ca7-7a2e-98f1-9b301468184a";
    let item = json!({
        "id": item_id,
        "topic": "unresolved",
        "next_section_id": 2,
        "sections": [{
            "id": 1,
            "text": "still open",
            "updated_at": "2026-08-25T00:00:00Z"
        }]
    });
    let item_path = engr::backlog::item_path(root, item_id);
    std::fs::write(
        &item_path,
        format!("{}\n", serde_json::to_string_pretty(&item).expect("json")),
    )
    .expect("a v2 backlog item");

    store::migrate(root).expect("migrate");

    let after = std::fs::read_to_string(&item_path).expect("migrated item");
    let value: Value = serde_json::from_str(&after).expect("json");
    assert_eq!(
        after,
        engr::proof::canonical_bytes(&value, "item").expect("canonical"),
        "the retained resource is now the bytes v3 says it is"
    );
    engr::backlog::load(root, item_id).expect("and the current reader accepts it");
}

/// The staged artifact that was validated is the one that gets published.
///
/// Two passes over the staging directory — verify, then reopen to copy — is a
/// window the workspace lock does not close: the stage lives in the repository
/// like everything else. The commit phase keeps each validated value and
/// publishes that, and the digest gate is what makes a swap visible at all.
#[test]
fn a_staged_object_that_no_longer_matches_its_digest_is_not_published() {
    let interrupted = tempfile::tempdir().expect("interrupted");
    let completed = tempfile::tempdir().expect("completed");
    let id = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    predecessor(interrupted.path(), &[object(id, "staged plan")]);
    predecessor(completed.path(), &[object(id, "staged plan")]);
    let stage = staged_plan(interrupted.path(), completed.path(), id);

    let staged = stage.join("objects").join(format!("{id}.json"));
    let mut swapped: Value = store::read_json(&staged).expect("staged object");
    swapped["title"] = Value::String("a plan nobody validated".into());
    std::fs::write(
        &staged,
        engr::proof::canonical_bytes(&swapped, "swapped").expect("canonical"),
    )
    .expect("swap the staged artifact");
    let before = std::fs::read(store::object_path(interrupted.path(), id)).expect("before");

    let error = store::migrate(interrupted.path()).expect_err("that is not the validated plan");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert_eq!(
        std::fs::read(store::object_path(interrupted.path(), id)).expect("after"),
        before,
        "and the workspace still holds its predecessor"
    );
    assert!(
        std::fs::read_to_string(store::engr_dir(interrupted.path()).join("format.json"))
            .expect("format")
            .contains("\"version\":2"),
        "the version did not advance"
    );
}

/// A predecessor that moves after the plan was built stops the commit.
///
/// The manifest records the digest of the bytes preflight decoded. If the
/// workspace has moved since, the plan describes something that is no longer
/// there, and publishing it would overwrite the newer file with the older one.
#[test]
fn a_predecessor_that_moved_after_the_plan_stops_the_commit() {
    let interrupted = tempfile::tempdir().expect("interrupted");
    let completed = tempfile::tempdir().expect("completed");
    let id = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    predecessor(
        interrupted.path(),
        &[object(id, "the validated predecessor")],
    );
    predecessor(completed.path(), &[object(id, "the validated predecessor")]);
    staged_plan(interrupted.path(), completed.path(), id);

    let source = store::object_path(interrupted.path(), id);
    let mut moved: Value = store::read_json(&source).expect("predecessor");
    moved["title"] = Value::String("changed since preflight".into());
    std::fs::write(
        &source,
        format!("{}\n", serde_json::to_string_pretty(&moved).expect("json")),
    )
    .expect("move the predecessor");
    let after_move = std::fs::read(&source).expect("moved bytes");

    let error = store::migrate(interrupted.path()).expect_err("the plan is about other bytes");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert_eq!(
        std::fs::read(&source).expect("after"),
        after_move,
        "the newer file was not overwritten by the older plan"
    );
    assert!(
        std::fs::read_to_string(store::engr_dir(interrupted.path()).join("format.json"))
            .expect("format")
            .contains("\"version\":2"),
        "the version did not advance"
    );
}

/// Build a stage in `root` from a reference migration performed in `reference`,
/// exactly as an interrupted run would leave one.
fn staged_plan(root: &Path, reference: &Path, id: &str) -> std::path::PathBuf {
    let predecessor_bytes =
        std::fs::read_to_string(store::object_path(root, id)).expect("predecessor");
    let predecessor_events =
        std::fs::read_to_string(store::events_path(root, id)).expect("history");
    store::migrate(reference).expect("reference migration");
    let migrated =
        std::fs::read_to_string(store::object_path(reference, id)).expect("migrated object");

    let stage = store::engr_dir(root).join("migration-v3");
    std::fs::create_dir_all(stage.join("objects")).expect("stage objects");
    std::fs::write(stage.join("objects").join(format!("{id}.json")), &migrated)
        .expect("staged object");
    let mut staged_objects = serde_json::Map::new();
    staged_objects.insert(
        id.to_owned(),
        Value::String(engr::proof::sha256_of(&migrated)),
    );
    write_raw(
        &stage.join("manifest.json"),
        &json!({
            "source_version": 2,
            "target_version": engr::WORKSPACE_VERSION,
            "objects": staged_objects,
            "resources": {},
            "source": {
                format!("objects/{id}.json"): engr::proof::sha256_of(&predecessor_bytes),
                format!("events/{id}.jsonl"): engr::proof::sha256_of(&predecessor_events)
            }
        }),
    )
    .expect("manifest");
    stage
}

/// A predecessor Event carrying a number JCS cannot hold stops the migration.
///
/// The record contract's own numeric-domain walk is a generation-2 rule, so it
/// never looks at retained v1 history. Preflight is therefore the only place
/// that asks, and it has to ask before the workspace advances — afterwards the
/// log is append-only history nobody can take back, and every read that
/// reconstructs the Object trips over it.
#[test]
fn a_predecessor_event_outside_the_shared_integer_domain_fails_preflight() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    let id = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    predecessor(root, &[object(id, "history with an impossible number")]);

    let path = store::events_path(root, id);
    let history = std::fs::read_to_string(&path).expect("history");
    let beyond = engr::proof::MAX_SAFE_INTEGER + 1;
    let rewritten: Vec<String> = history
        .lines()
        .map(|line| line.replacen(r#""rev":1"#, &format!(r#""rev":{beyond}"#), 1))
        .collect();
    assert_ne!(
        rewritten.join("\n"),
        history.trim_end(),
        "the fixture must actually plant the number"
    );
    std::fs::write(&path, format!("{}\n", rewritten.join("\n"))).expect("plant it");
    let before = std::fs::read(store::object_path(root, id)).expect("before");

    let error = store::migrate(root).expect_err("JCS cannot carry that number");
    assert!(error.message.contains("safe integer"), "{error}");
    assert_eq!(
        std::fs::read(store::object_path(root, id)).expect("after"),
        before,
        "and nothing was written"
    );
    assert!(
        std::fs::read_to_string(store::engr_dir(root).join("format.json"))
            .expect("format")
            .contains("\"version\":2"),
        "the version did not advance"
    );
}
