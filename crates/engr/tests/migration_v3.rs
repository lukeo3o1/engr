use engr::model::Content;
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
        "rev": 1,
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
    store::write_json(
        &stage.join("manifest.json"),
        &json!({
            "source_version": 2,
            "target_version": engr::WORKSPACE_VERSION,
            "objects": staged_objects
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
        "rev": 1,
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
