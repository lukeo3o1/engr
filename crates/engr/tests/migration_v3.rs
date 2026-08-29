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
fn target_version_does_not_discard_an_unpublished_migration_plan() {
    let interrupted = tempfile::tempdir().expect("interrupted");
    let completed = tempfile::tempdir().expect("completed");
    let id = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    predecessor(
        interrupted.path(),
        &[object(id, "target version is not evidence")],
    );
    predecessor(
        completed.path(),
        &[object(id, "target version is not evidence")],
    );
    let stage = staged_plan(interrupted.path(), completed.path(), id);
    let predecessor = std::fs::read(store::object_path(interrupted.path(), id)).expect("source");

    write_raw(
        &store::engr_dir(interrupted.path()).join("format.json"),
        &json!({ "format": "engr-workspace", "version": engr::WORKSPACE_VERSION }),
    )
    .expect("independent version edit");

    let error = store::migrate(interrupted.path())
        .expect_err("the target scalar cannot prove the copies happened");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(stage.exists(), "the recovery plan remains available");
    assert_eq!(
        std::fs::read(store::object_path(interrupted.path(), id)).expect("source after"),
        predecessor,
        "the source was not mistaken for the staged target"
    );
}

#[test]
fn migration_ignores_its_resumable_stage_in_existing_workspaces() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    let id = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    predecessor(root, &[object(id, "local stage")]);

    store::migrate(root).expect("migrate");
    let ignore = std::fs::read_to_string(store::engr_dir(root).join(".gitignore"))
        .expect("migration ignore");
    assert!(ignore.lines().any(|line| line == "/migration-v3/"));
    assert!(ignore.lines().any(|line| line == "/migration-v3.tmp/"));

    std::fs::create_dir_all(store::engr_dir(root).join("migration-v3")).expect("crash stage");
    std::fs::write(
        store::engr_dir(root)
            .join("migration-v3")
            .join("manifest.json"),
        "local recovery state",
    )
    .expect("manifest");
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["init", "-q"])
        .status()
        .expect("git init");
    assert!(status.success());
    let ignored = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["check-ignore", ".engr/migration-v3/manifest.json"])
        .status()
        .expect("git check-ignore");
    assert!(ignored.success(), "the crash stage is local-only");
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

/// A reference to a section that has references of its own still migrates.
///
/// A legacy Ref pins the seal the predecessor took over the predecessor's
/// content. Conversion rewrites `refs`, which that seal covers — so the
/// migrated target hashes to something else, necessarily and correctly. The
/// two numbers are the same only when the target carries no references, which
/// is why every reference in the suite above happened to pass while a chained
/// one could not migrate at all: it was refused as `seals as X, not the legacy
/// reference seal Y`, an accusation of tampering against an untouched
/// workspace. What the pin claims has to be checked in the terms it was
/// written in.
///
/// This is a v2 predecessor, so it pins the defect where it lives rather than
/// where it was found. The released-v1 fixture carries the same shape.
#[test]
fn a_reference_to_a_section_that_has_its_own_references_still_migrates() {
    let temp = tempfile::tempdir().expect("temp");
    let base = "018f7d58-4ca7-7a2e-98f1-9b3014681847";
    let middle = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    let source = "018f7d58-4ca7-7a2e-98f1-9b3014681849";
    let commit = |message: &str| {
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
                message,
            ],
        );
        git(temp.path(), &["rev-parse", "HEAD"])
    };
    let referring = |id: &str, text: &str, reference: engr::model::Ref| {
        let content = Content {
            text: text.to_owned(),
            refs: vec![reference],
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
                "text": content.text,
                "refs": content.refs,
                "sha256": content.sha256().expect("seal"),
                "confirmed_at": "2026-08-25T00:00:00Z"
            }]
        })
    };

    let base_object = object(base, "the bottom of the chain depends on nothing");
    predecessor(temp.path(), std::slice::from_ref(&base_object));
    git(temp.path(), &["init"]);
    let base_commit = commit("the base");

    let base_seal = base_object["sections"][0]["sha256"]
        .as_str()
        .expect("base seal");
    let middle_object = referring(
        middle,
        "the middle of the chain depends on the base",
        engr::model::Ref::legacy(base, 1, base_seal, &base_commit),
    );
    predecessor(temp.path(), &[base_object.clone(), middle_object.clone()]);
    let middle_commit = commit("the middle, which the source will pin");

    let middle_seal = middle_object["sections"][0]["sha256"]
        .as_str()
        .expect("middle seal");
    let source_object = referring(
        source,
        "the source depends on a section that itself depends on something",
        engr::model::Ref::legacy(middle, 1, middle_seal, &middle_commit),
    );
    predecessor(temp.path(), &[base_object, middle_object, source_object]);

    store::migrate(temp.path()).expect("a chained legacy reference migrates");

    let source = store::load_object(temp.path(), source).expect("source");
    let selective = source.sections[0].refs[0]
        .as_selective()
        .expect("selective ref");
    assert_eq!(selective.commit(), middle_commit);
    let middle = store::load_object(temp.path(), middle).expect("middle");
    assert_eq!(
        engr::dependency::evaluate(
            temp.path(),
            &middle,
            middle.sha256.as_deref().expect("object seal"),
            selective,
        )
        .expect("dependency"),
        engr::dependency::Dependency::Unchanged,
        "the converted pin names the migrated target it was taken from"
    );
}

/// The members version 2 added part-way through its own window still migrate.
///
/// `role`, `content` and `relations` arrived on a Section during the v2 window,
/// so an early v2 workspace has none of them and a late one does. Both are
/// version 2. Enumerating each generation's persisted members closed a hole
/// where a *version 1* file could carry them — and the failure mode of that fix
/// is over-tightening, which would strand exactly the workspaces the members
/// were added for. So the optional half of the schema gets a test of its own.
#[test]
fn a_late_v2_section_keeps_the_members_that_generation_added() {
    let temp = tempfile::tempdir().expect("temp");
    let id = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    let content = Content {
        role: Some(engr::semantics::Role::Decision),
        text: "a late v2 section carries everything its generation had".to_owned(),
        content: vec![engr::semantics::Supplement::new(
            "code.rust",
            "fn late() {}",
        )],
        relations: vec![engr::semantics::Relation {
            relation: engr::semantics::RelationType::ImplementedBy,
            target: engr::semantics::Target::File {
                path: "src/lib.rs".to_owned(),
                commit: "a".repeat(40),
            },
        }],
        ..Content::default()
    };
    let late = json!({
        "id": id,
        "title": "migration",
        "state": "open",
        "rev": 2,
        "next_section_id": 2,
        "sections": [{
            "id": 1,
            "role": content.role,
            "text": content.text,
            "content": content.content,
            "refs": content.refs,
            "relations": content.relations,
            "sha256": content.sha256().expect("seal"),
            "confirmed_at": "2026-08-25T00:00:00Z"
        }]
    });
    predecessor(temp.path(), &[late]);

    store::migrate(temp.path()).expect("a late v2 Section migrates");

    let migrated = store::load_object(temp.path(), id).expect("object");
    let section = &migrated.sections[0];
    assert_eq!(section.role, Some(engr::semantics::Role::Decision));
    assert_eq!(section.content, content.content);
    assert_eq!(section.relations, content.relations);
    assert_eq!(section.admission, engr::semantics::Admission::Human);
    integrity::check_stored_object_integrity(&migrated).expect("integrity");
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
        "next_section_id": 3,
        "sections": [
            {
                "id": 2,
                "text": "second open point",
                "updated_at": "2026-08-25T00:00:00Z"
            },
            {
                "id": 1,
                "text": "first open point",
                "updated_at": "2026-08-25T00:00:00Z"
            }
        ]
    });
    let item_path = engr::backlog::item_path(root, item_id);
    std::fs::write(
        &item_path,
        format!("{}\n", serde_json::to_string_pretty(&item).expect("json")),
    )
    .expect("a v2 backlog item");
    let work_path = engr::work::path(root, id);
    let work = json!({
        "state": "active",
        "updated_at": "2026-08-25T00:00:00Z",
        "next_item_id": 3,
        "dependencies": [],
        "blockers": [],
        "items": [
            { "id": 2, "text": "second step", "state": "pending", "commits": [] },
            { "id": 1, "text": "first step", "state": "pending", "commits": [] }
        ]
    });
    std::fs::write(
        &work_path,
        format!("{}\n", serde_json::to_string_pretty(&work).expect("json")),
    )
    .expect("a v2 work sidecar");

    store::migrate(root).expect("migrate");

    let after = std::fs::read_to_string(&item_path).expect("migrated item");
    let value: Value = serde_json::from_str(&after).expect("json");
    assert_eq!(
        after,
        engr::proof::canonical_bytes(&value, "item").expect("canonical"),
        "the retained resource is now the bytes v3 says it is"
    );
    let item = engr::backlog::load(root, item_id).expect("and the current reader accepts it");
    assert_eq!(
        item.sections
            .iter()
            .map(|section| section.id)
            .collect::<Vec<_>>(),
        vec![1, 2],
        "migration normalizes the ordered child list"
    );
    let work = engr::work::load(root, id).expect("migrated sidecar");
    assert_eq!(
        work.items.iter().map(|item| item.id).collect::<Vec<_>>(),
        vec![1, 2],
        "migration normalizes work items too"
    );
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

/// A stage cannot hand itself authority the binary does not have.
///
/// The versions here are the ones with no defined route into v3, which since
/// the released-v1 compatibility ruling means the ones above it: 0, 1 and 2 are
/// all migratable now, so a plan claiming one of those is testing something
/// else — that it matches the workspace it was prepared from, which
/// `a_manifest_cannot_relabel_the_generation_it_was_prepared_from` in the
/// released-v1 suite covers.
#[test]
fn staged_migration_refuses_unrecognized_or_unfrozen_source_versions() {
    for version in [4, 9] {
        let interrupted = tempfile::tempdir().expect("interrupted");
        let completed = tempfile::tempdir().expect("completed");
        let id = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
        predecessor(interrupted.path(), &[object(id, "version authority")]);
        predecessor(completed.path(), &[object(id, "version authority")]);
        let stage = staged_plan(interrupted.path(), completed.path(), id);
        let manifest_path = stage.join("manifest.json");
        let mut manifest: Value = store::read_json(&manifest_path).expect("manifest");
        manifest["source_version"] = json!(version);
        write_raw(&manifest_path, &manifest).expect("rewritten manifest");
        write_raw(
            &store::engr_dir(interrupted.path()).join("format.json"),
            &json!({ "format": "engr-workspace", "version": version }),
        )
        .expect("declared version");

        let error = store::migrate(interrupted.path())
            .expect_err("a staged plan cannot widen migration authority");
        assert_eq!(error.code, engr::EXIT_SCHEMA, "version {version}");
        assert!(error.message.contains("no defined migration"), "{error}");
    }
}

#[test]
fn staged_manifest_resources_are_not_filesystem_capabilities() {
    let interrupted = tempfile::tempdir().expect("interrupted");
    let completed = tempfile::tempdir().expect("completed");
    let id = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    predecessor(interrupted.path(), &[object(id, "path capability")]);
    predecessor(completed.path(), &[object(id, "path capability")]);
    let stage = staged_plan(interrupted.path(), completed.path(), id);
    let manifest_path = stage.join("manifest.json");
    let mut manifest: Value = store::read_json(&manifest_path).expect("manifest");
    manifest["resources"]["../victim"] = Value::String("a".repeat(64));
    manifest["source"]["../victim"] = Value::String("b".repeat(64));
    write_raw(&manifest_path, &manifest).expect("forged manifest");

    let victim = interrupted.path().join("victim");
    let error = store::migrate(interrupted.path()).expect_err("a path escape is not a resource");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(!victim.exists(), "nothing was published outside .engr");
}

/// A stage that agrees with itself is not evidence of anything.
///
/// The staging directory is operational data that outlives a crash, sitting in
/// the repository where anyone who can reach the workspace can edit it. Section
/// and aggregate seals are unkeyed, so a staged Object can be rewritten and
/// resealed into a file that decodes as current v3 and verifies its own
/// integrity; point `manifest.objects[id]` at it and the stage is consistent at
/// every place resume looked. Meanwhile the predecessor Object and the history
/// that derives it have not moved at all — preflight never produced this target
/// and never could have.
///
/// Refusing it is what stops an interrupted migration from being a way to write
/// authority, which is #31's rule that migration must not legitimize state
/// merely by resealing it.
#[test]
fn a_staged_object_agreeing_with_its_own_manifest_is_still_not_authority() {
    let interrupted = tempfile::tempdir().expect("interrupted");
    let completed = tempfile::tempdir().expect("completed");
    let id = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    for root in [interrupted.path(), completed.path()] {
        predecessor(root, &[object(id, "object derivation")]);
    }
    let before = std::fs::read(store::object_path(interrupted.path(), id)).expect("predecessor");
    let history = std::fs::read(store::events_path(interrupted.path(), id)).expect("history");
    let stage = staged_plan(interrupted.path(), completed.path(), id);

    // Rewrite the staged Object and reseal it properly. Seals are unkeyed, so
    // this is something anyone who can reach the staging directory can do, and
    // the result is a genuinely valid current-v3 Object rather than corruption.
    let staged_path = stage.join("objects").join(format!("{id}.json"));
    let migrated = std::fs::read_to_string(&staged_path).expect("staged object");
    let staged_object: engr::model::Object =
        serde_json::from_str(&migrated).expect("the stage holds a current v3 Object");
    let expected = staged_object.sha256.clone().expect("aggregate seal");
    let forged = integrity::mutate(&staged_object, &expected, |next| {
        next.title = "an Object nobody admitted".to_owned();
        Ok(())
    })
    .expect("reseal")
    .object;
    integrity::check_stored_object_integrity(&forged)
        .expect("the substitution is self-consistent, which is the point");
    let forged = engr::proof::canonical_bytes(&forged, "forged object").expect("canonical");
    std::fs::write(&staged_path, &forged).expect("swap the staged object");

    // ...and make the manifest agree with it, so every check resume used to
    // make on a staged Object now passes.
    let manifest_path = stage.join("manifest.json");
    let mut manifest: Value = store::read_json(&manifest_path).expect("manifest");
    manifest["objects"][id] = Value::String(engr::proof::sha256_of(&forged));
    write_raw(&manifest_path, &manifest).expect("consistent forged pair");

    let error = store::migrate(interrupted.path())
        .expect_err("a resealed stage cannot become current state");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(
        error.message.contains("not the canonical migration"),
        "{error}"
    );
    assert_eq!(
        std::fs::read(store::object_path(interrupted.path(), id)).expect("after"),
        before,
        "the predecessor it claimed to come from never moved"
    );
    assert_eq!(
        std::fs::read(store::events_path(interrupted.path(), id)).expect("after"),
        history,
        "and neither did the history that derives it"
    );
}

/// The same protection, for the resources that already had it.
#[test]
fn staged_resource_bytes_must_be_the_migration_of_their_predecessor() {
    let interrupted = tempfile::tempdir().expect("interrupted");
    let completed = tempfile::tempdir().expect("completed");
    let id = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    let backlog_id = "018f7d58-4ca7-7a2e-98f1-9b301468184a";
    for root in [interrupted.path(), completed.path()] {
        predecessor(root, &[object(id, "resource derivation")]);
        let item = json!({
            "id": backlog_id,
            "topic": "unresolved",
            "next_section_id": 2,
            "sections": [{
                "id": 1,
                "text": "open",
                "updated_at": "2026-08-25T00:00:00Z"
            }]
        });
        std::fs::write(
            engr::backlog::item_path(root, backlog_id),
            format!("{}\n", serde_json::to_string_pretty(&item).expect("item")),
        )
        .expect("predecessor item");
    }
    let original =
        std::fs::read_to_string(engr::backlog::item_path(interrupted.path(), backlog_id))
            .expect("original resource");
    let stage = staged_plan(interrupted.path(), completed.path(), id);
    let migrated = std::fs::read_to_string(engr::backlog::item_path(completed.path(), backlog_id))
        .expect("migrated resource");
    let relative = format!("backlog/{backlog_id}.json");
    let staged = stage
        .join("resources")
        .join("backlog")
        .join(format!("{backlog_id}.json"));
    std::fs::create_dir_all(staged.parent().expect("parent")).expect("resource stage");
    std::fs::write(&staged, &migrated).expect("stage resource");
    let manifest_path = stage.join("manifest.json");
    let mut manifest: Value = store::read_json(&manifest_path).expect("manifest");
    manifest["resources"][relative.as_str()] = Value::String(engr::proof::sha256_of(&migrated));
    manifest["source"][relative.as_str()] = Value::String(engr::proof::sha256_of(&original));

    let mut substituted: Value = serde_json::from_str(&migrated).expect("migrated JSON");
    substituted["topic"] = Value::String("a different valid resource".to_owned());
    let substituted = engr::proof::canonical_bytes(&substituted, "substituted resource")
        .expect("canonical substitution");
    std::fs::write(&staged, &substituted).expect("swap staged resource");
    manifest["resources"][relative.as_str()] = Value::String(engr::proof::sha256_of(&substituted));
    write_raw(&manifest_path, &manifest).expect("consistent forged pair");

    let error = store::migrate(interrupted.path())
        .expect_err("a digest pair cannot replace the deterministic migration output");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(
        error.message.contains("not the canonical migration"),
        "{error}"
    );
}

/// A number that has to *become* current v3 state stops the migration.
///
/// This is the case #35 names: §11 fails migration on "a required current-state
/// JSON integer" outside the shared domain, and acceptance criterion 21 scopes
/// the bound to integers "participating in current Section/Ref state or their
/// JCS integrity/digest projections". `next_section_id` is exactly that — it is
/// carried into the migrated Object and sealed with it — so it is refused, and
/// refused as a *schema* fault, because the number was found in a file rather
/// than typed at a command line.
#[test]
fn a_required_current_state_number_outside_the_shared_domain_fails_preflight() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    let id = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    let mut projection = object(id, "a counter no implementation shares");
    projection["next_section_id"] = json!(engr::proof::MAX_SAFE_INTEGER + 1);
    predecessor(root, &[projection]);
    let before = std::fs::read(store::object_path(root, id)).expect("before");

    let error = store::migrate(root).expect_err("JCS cannot carry that number");
    assert!(error.message.contains("safe integer"), "{error}");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
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

/// Retained Event-v1 history is not walked for the Phase-3 numeric domain, and
/// nothing escapes because of it.
///
/// #35 scopes that domain to current state, so preflight does not re-interpret
/// immutable history under a rule written for a later generation. The reason
/// that costs no safety is structural rather than a second check: an Event's
/// only numbers are its `rev` and the Section ids an action names, and neither
/// can be out of domain in a workspace that still migrates. `rev` is replayed
/// contiguously from 1, so an out-of-domain one is not history the migration
/// declines to read — it is a tail that cannot reconcile at all.
///
/// So this pins the *absence* of a historical-only case. A v1 record carrying
/// such a number is refused on its own terms, and the workspace stays where it
/// was.
#[test]
fn an_out_of_domain_event_number_cannot_be_history_the_projection_never_reads() {
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

    let error = store::migrate(root).expect_err("that rev cannot be replayed");
    assert!(
        !error.message.contains("safe integer"),
        "history is no longer walked for the current-state domain: {error}"
    );
    assert!(
        error.message.contains("does not immediately follow"),
        "it is refused as a revision tail that cannot reconcile: {error}"
    );
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

/// A predecessor whose projection is missing but whose history rebuilds it
/// migrates, atomically.
///
/// This is the EventStore's crash-recovery role: a v2 workspace can legitimately
/// hold admitted history for an Object whose file never landed. Preflight
/// already admitted that case and rebuilt the projection — but the commit phase
/// then compared the workspace's Object files against the set the plan
/// *publishes*, and a reconstructed Object is by definition not among them, so
/// the plan preflight had just accepted could never commit. The two sets are
/// different questions: what the predecessor had, and what v3 will hold.
#[test]
fn a_projection_rebuilt_from_admitted_history_migrates_with_the_rest() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    let kept = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    let lost = "018f7d58-4ca7-7a2e-98f1-9b3014681849";
    predecessor(root, &[object(kept, "the projection that landed")]);
    predecessor(root, &[object(lost, "the projection that did not")]);

    // The crash: history is durable, the projection never reached disk.
    std::fs::remove_file(store::object_path(root, lost)).expect("lose the projection");
    assert!(!store::object_path(root, lost).exists());

    store::migrate(root).expect("the recovery case is the one this admits");

    assert_eq!(
        store::validate_format(root).expect("format"),
        store::WorkspaceFormat::Current
    );
    for id in [kept, lost] {
        let migrated = store::load_object(root, id).expect("object");
        integrity::check_stored_object_integrity(&migrated).expect("integrity");
        assert_eq!(migrated.sections.len(), 1, "{id}");
    }
    assert_eq!(
        store::load_object(root, lost).expect("rebuilt").sections[0].text,
        "the projection that did not",
        "rebuilt from the history that was durable"
    );
}

/// The manifest names exactly what preflight read, and nothing else.
///
/// The closing walk used to *become* the manifest, so anything it found was
/// promoted to "expected predecessor" whether or not preflight had ever looked
/// at it. Now the manifest is the captured set and the walk may only agree with
/// it — so every file under `.engr` has to be accounted for while it is being
/// read, Rules included. Remove that accounting and this fails: the walk finds
/// a file the plan never named.
///
/// A Rule's digest is taken from the bytes the loader parsed rather than from a
/// second read of the path, for the reason artifact-exact identity always
/// requires it — `.engr/rules` is editable outside the workspace lock. That half
/// is structural here (one read feeds both) and is pinned at the binding level
/// in the Rule tests.
#[test]
fn every_predecessor_file_is_accounted_for_while_it_is_read() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    let id = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    predecessor(root, &[object(id, "with policy")]);

    let rules = engr::rules::dir(root);
    std::fs::create_dir_all(&rules).expect("rules dir");
    std::fs::write(root.join("AGENTS.md"), "the contract\n").expect("basis");
    std::fs::write(
        rules.join("architecture.md"),
        "---\nid: architecture\napplies:\n  domains:\n    - object\nbased_on:\n  - path: AGENTS.md\n---\n\n# Architecture\n\nBody.\n",
    )
    .expect("rule");
    // Not a Rule, and the loader reads only `.md`. It still has to be accounted
    // for, or the closing set comparison can never be exact.
    std::fs::write(rules.join("notes.txt"), "not a rule\n").expect("stray file");

    store::migrate(root).expect("a workspace with policy migrates");
    assert_eq!(
        store::validate_format(root).expect("format"),
        store::WorkspaceFormat::Current
    );
    assert_eq!(
        engr::rules::load_all(root).expect("rules").len(),
        1,
        "and the policy came through untouched"
    );
}

/// A file that appears after the plan was built stops the commit.
///
/// It was never enumerated, so nothing schema-, JCS-, replay- or Rule-validated
/// it; accepting it into the expected predecessor set and then advancing the
/// generation would leave an unvalidated resource sitting in current v3.
#[test]
fn a_file_that_appeared_after_the_plan_stops_the_commit() {
    let interrupted = tempfile::tempdir().expect("interrupted");
    let completed = tempfile::tempdir().expect("completed");
    let id = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    predecessor(interrupted.path(), &[object(id, "the validated set")]);
    predecessor(completed.path(), &[object(id, "the validated set")]);
    staged_plan(interrupted.path(), completed.path(), id);

    let arrival =
        engr::backlog::item_path(interrupted.path(), "018f7d58-4ca7-7a2e-98f1-9b301468184b");
    std::fs::write(&arrival, "{}\n").expect("a file nobody validated");

    let error = store::migrate(interrupted.path()).expect_err("that was not in the plan");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(
        error.message.contains("appeared after migration preflight"),
        "{error}"
    );
    assert!(
        std::fs::read_to_string(store::engr_dir(interrupted.path()).join("format.json"))
            .expect("format")
            .contains("\"version\":2"),
        "the version did not advance"
    );
}

/// A migrated workspace whose retained history still carries a legacy Ref.
///
/// The Ref is converted away in the current Object, but the Event-v1 records it
/// was admitted through keep it, so any replay of that history has to convert it
/// again before comparing against a digest taken over the migrated spelling.
fn migrated_with_legacy_ref(root: &Path) -> (&'static str, &'static str, String) {
    let target = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    let source = "018f7d58-4ca7-7a2e-98f1-9b3014681849";
    let target_object = object(target, "the target remains stable");
    predecessor(root, std::slice::from_ref(&target_object));
    git(root, &["init"]);
    git(root, &["add", ".engr"]);
    git(
        root,
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
    let commit = git(root, &["rev-parse", "HEAD"]);
    let target_seal = target_object["sections"][0]["sha256"]
        .as_str()
        .expect("target seal");
    let content = Content {
        text: "the source relies on all legacy target semantics".to_owned(),
        refs: vec![engr::model::Ref::legacy(target, 1, target_seal, &commit)],
        ..Content::default()
    };
    let source_object = json!({
        "id": source,
        "title": "source",
        "state": "open",
        "rev": 2,
        "next_section_id": 2,
        "sections": [{
            "id": 1,
            "text": content.text,
            "refs": content.refs,
            "sha256": content.sha256().expect("source seal"),
            "confirmed_at": "2026-08-25T00:00:00Z"
        }]
    });
    predecessor(root, &[target_object, source_object]);
    store::migrate(root).expect("migrate");
    (target, source, commit)
}

/// Make the pinned commit unresolvable, the way losing history actually does.
fn forget_commit(root: &Path, commit: &str) {
    let (directory, rest) = commit.split_at(2);
    let loose = root.join(".git").join("objects").join(directory).join(rest);
    std::fs::remove_file(&loose).expect("the pinned commit was a loose object");
}

fn admit_human(root: &Path, payload: Payload) {
    let prepared = engr::gate::prepare(root, payload).expect("prepare");
    engr::gate::confirm(root, &format!("CONFIRM {}", prepared.candidate.challenge))
        .expect("confirm");
}

/// A lost historical commit says something about what depends on it, not about
/// whether the Object exists.
///
/// The Human CandidateDigest recheck replays retained v1 history, and a legacy
/// Ref in that history has to be converted before its Section can be compared
/// against a digest taken over the migrated spelling. Converting the whole
/// Object made every legacy Ref in it a precondition of the recheck — so one
/// unrelated pinned commit going away failed `load_events`, and since
/// `ops::replay` reads events before the Object, a perfectly sound current
/// Object answered `EXIT_NOT_FOUND`. Absent, rather than "some provenance is
/// gone", for a record sitting right there.
///
/// `object.renamed` hashes title and lifecycle and names no Section at all, so
/// no legacy Ref anywhere is one of its digest inputs. It authenticates without
/// that commit, and the Object stays readable.
#[test]
fn an_unrelated_lost_commit_does_not_make_a_current_object_unreadable() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    let (_, source, commit) = migrated_with_legacy_ref(root);

    admit_human(
        root,
        Payload {
            action: Action::ObjectRenamed,
            object: source.to_owned(),
            becomes: None,
            content: Content {
                text: "renamed after the generation advanced".to_owned(),
                ..Content::default()
            },
        },
    );
    forget_commit(root, &commit);

    let events = store::load_events(root, source).expect("history still authenticates");
    assert!(
        events
            .iter()
            .any(|event| event.version == engr::EVENT_ENVELOPE_VERSION),
        "the rename really is a v2 record, so the recheck really ran"
    );
    let object = engr::ops::effective(root, source).expect("the object is still readable");
    assert_eq!(object.title, "renamed after the generation advanced");
}

/// The other half: an operation whose digest really does read that Section.
///
/// Narrowing the conversion is not a licence to skip it. `section.revised` hashes
/// the Section either side, and the before-state is the one carrying the legacy
/// Ref — so with its commit gone the recheck cannot be completed, and it must
/// say so rather than authenticate on material it could not reconstruct.
#[test]
fn an_operation_whose_digest_needs_the_lost_commit_still_refuses() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    let (_, source, commit) = migrated_with_legacy_ref(root);

    admit_human(
        root,
        Payload {
            action: Action::SectionRevised { section: 1 },
            object: source.to_owned(),
            becomes: None,
            content: Content {
                text: "revised the section that carries the reference".to_owned(),
                ..Content::default()
            },
        },
    );
    forget_commit(root, &commit);

    let error = store::load_events(root, source)
        .expect_err("the recheck cannot be completed without its own input");
    assert_ne!(
        error.code,
        engr::EXIT_NOT_FOUND,
        "and it is not reported as the Object being absent: {error}"
    );
}

/// A stale candidate stays readable when unrelated provenance disappears.
///
/// `validate_candidate` reconstructs the predecessor a pending candidate was
/// written against, and it kept converting the whole migrated Object to do it.
/// That contradicted the invariant the same function records at its end: a stale
/// candidate is a valid historical description of what the Human was shown, and
/// is deliberately kept readable so the candidate surface can say why it is
/// dead. Converting everything made one unrelated legacy Ref's pinned commit a
/// precondition for rendering it.
///
/// `object.renamed` proves itself from `TitleLifecycle`. With no stored Rule
/// Review there is no whole-Object projection in play either, so nothing this
/// candidate must self-authenticate lives in that Section.
#[test]
fn a_stale_candidate_is_still_renderable_after_an_unrelated_commit_is_lost() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    let (_, source, commit) = migrated_with_legacy_ref(root);

    let prepared = engr::gate::prepare(
        root,
        Payload {
            action: Action::ObjectRenamed,
            object: source.to_owned(),
            becomes: None,
            content: Content {
                text: "the name the human was shown".to_owned(),
                ..Content::default()
            },
        },
    )
    .expect("prepare");
    let challenge = prepared.candidate.challenge.clone();
    assert!(
        prepared.candidate.context.rule_review.is_none(),
        "no applicable Object Rule, so no whole-Object review projection"
    );

    // An Agent rename gets there first, so the pending candidate goes stale on
    // its own binding. Confirming another *Human* one would supersede and
    // discard it instead, and then there would be nothing left to render.
    engr::gate::admit_agent(
        root,
        Payload {
            action: Action::ObjectRenamed,
            object: source.to_owned(),
            becomes: None,
            content: Content {
                text: "a different rename got there first".to_owned(),
                ..Content::default()
            },
        },
        None,
    )
    .expect("agent rename");
    forget_commit(root, &commit);

    let candidate =
        engr::gate::find(root, &challenge).expect("a stale candidate can still explain itself");
    assert_eq!(candidate.challenge, challenge);
}

/// Narrowing what a stale candidate reconstructs is not permission to skip what
/// it hashes.
///
/// `section.revised` proves itself from the target Section either side, and the
/// before-state is the one carrying the legacy Ref — so this candidate's own
/// CandidateDigest inputs are exactly the material that went missing. It has to
/// fail, and it has to fail as something other than the candidate or its Object
/// being absent, which is the distinction #13 keeps NOT_FOUND for.
#[test]
fn a_stale_candidate_that_hashes_the_lost_section_still_fails_closed() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    let (_, source, commit) = migrated_with_legacy_ref(root);

    let prepared = engr::gate::prepare(
        root,
        Payload {
            action: Action::SectionRevised { section: 1 },
            object: source.to_owned(),
            becomes: None,
            content: Content {
                text: "replace the section that stood on history".to_owned(),
                ..Content::default()
            },
        },
    )
    .expect("prepare revision");

    engr::gate::admit_agent(
        root,
        Payload {
            action: Action::ObjectRenamed,
            object: source.to_owned(),
            becomes: None,
            content: Content {
                text: "an agent advanced the title first".to_owned(),
                ..Content::default()
            },
        },
        None,
    )
    .expect("agent rename");
    forget_commit(root, &commit);

    let error = engr::gate::find(root, &prepared.candidate.challenge)
        .expect_err("this candidate hashes the Section whose provenance is gone");
    assert_ne!(
        error.code,
        engr::EXIT_NOT_FOUND,
        "missing required provenance is not the candidate being absent: {error}"
    );
    // And it fails for the right reason. Skipping the conversion instead would
    // also produce an error here -- the digest would be recomputed over an
    // unconverted Section and simply not match -- which looks like a refusal
    // while actually meaning the proof was never reconstructed.
    assert!(
        error.message.contains("historical workspace at commit"),
        "the refusal names the provenance it could not reach: {error}"
    );
}

fn object_rule(root: &Path) {
    std::fs::create_dir_all(engr::rules::dir(root)).expect("rules dir");
    std::fs::write(
        engr::rules::dir(root).join("object-policy.md"),
        "---\nid: object-policy\napplies:\n  domains:\n    - object\n---\n\n# Object policy\n\nReview the exact mutation.\n",
    )
    .expect("rule");
}

/// The exact Rule Review a Human would have been shown for this mutation.
fn attestation(root: &Path, payload: &Payload) -> engr::gate::ReviewAttestation {
    let before = store::load_object(root, &payload.object).expect("before");
    let mut after = before.clone();
    let event = Event {
        format: EVENT_FORMAT.to_owned(),
        version: engr::EVENT_ENVELOPE_VERSION,
        event_id: engr::model::new_id(),
        rev: before.rev + 1,
        time: "2026-08-25T00:00:00Z".to_owned(),
        payload: payload.clone(),
        provenance: Provenance::Tagged {
            admission: engr::model::TaggedAdmission {
                kind: engr::semantics::Admission::Human,
                confirmation: Some(engr::model::HumanConfirmation {
                    challenge: "ABCD-EFGH".to_owned(),
                    candidate_digest: format!("1:{}", "0".repeat(64)),
                }),
                rule_review: None,
            },
        },
    };
    engr::model::project(&mut after, &event).expect("project");
    let mutation = engr::proof::object_review_mutation(&before, &after, payload).expect("mutation");
    let binding = engr::rules::bind_object(root, &mutation, before.rev).expect("binding");
    engr::gate::ReviewAttestation {
        review_digest: binding.digest().expect("digest").to_string(),
        reviewed_rules: binding.rule_ids(),
        attempt: 1,
        result: engr::proof::ReviewResult::Passed,
        explanation: None,
    }
}

/// A *reviewed* stale candidate is scoped exactly like an unreviewed one.
///
/// `object_review_mutation` builds its mutation from `candidate_subject`, so a
/// frozen ReviewDigest reads the same operation-defined projection the
/// CandidateDigest does — #25 defines it as that projection plus `expected_rev`,
/// not a whole-Object snapshot. Widening reviewed candidates therefore bought
/// nothing and cost the same thing the unreviewed case had already lost: an
/// unrelated Section's legacy Ref deciding whether the candidate can be read.
#[test]
fn a_reviewed_stale_candidate_ignores_unrelated_lost_provenance() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    let (_, source, commit) = migrated_with_legacy_ref(root);
    object_rule(root);

    let payload = Payload {
        action: Action::ObjectRenamed,
        object: source.to_owned(),
        becomes: None,
        content: Content {
            text: "a reviewed rename the human was shown".to_owned(),
            ..Content::default()
        },
    };
    let review = attestation(root, &payload);
    let prepared =
        engr::gate::prepare_reviewed(root, payload, engr::gate::Allowance::Normal, review)
            .expect("prepare reviewed");
    assert!(
        prepared.candidate.context.rule_review.is_some(),
        "the fixture must actually carry a stored Rule Review"
    );

    engr::gate::admit_agent(
        root,
        Payload {
            action: Action::ObjectRenamed,
            object: source.to_owned(),
            becomes: None,
            content: Content {
                text: "an agent advanced the title first".to_owned(),
                ..Content::default()
            },
        },
        Some(attestation(
            root,
            &Payload {
                action: Action::ObjectRenamed,
                object: source.to_owned(),
                becomes: None,
                content: Content {
                    text: "an agent advanced the title first".to_owned(),
                    ..Content::default()
                },
            },
        )),
    )
    .expect("agent rename");
    forget_commit(root, &commit);

    let candidate = engr::gate::find(root, &prepared.candidate.challenge)
        .expect("a reviewed stale candidate still explains itself");
    assert!(candidate.context.rule_review.is_some());
}

/// Migration is a maintenance window, and that is now normative.
///
/// #32's ruling on `5454597053` settles what a lock-free reader sees while a
/// coordinated migration is incomplete: not the old state, not the new one, and
/// never a mixture — `unavailable`, failing closed until `engr migrate`
/// finishes. It is the one explicit exception to the old-or-new rule the rest of
/// the protocol relies on.
///
/// Pinned because it is a contract a second implementation has to reproduce, not
/// a side effect of how this one happens to order its checks. A reader is
/// entitled to read the refusal as "mid-migration", so it must not quietly
/// become "damaged" or, worse, a partial answer.
#[test]
fn reads_are_unavailable_while_a_coordinated_migration_is_incomplete() {
    let interrupted = tempfile::tempdir().expect("interrupted");
    let completed = tempfile::tempdir().expect("completed");
    let id = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    for root in [interrupted.path(), completed.path()] {
        predecessor(root, &[object(id, "readable until the window opens")]);
    }
    let root = interrupted.path();
    // A readable predecessor before the window opens.
    store::validate_format(root).expect("the predecessor reads");

    staged_plan(root, completed.path(), id);

    let error = store::validate_format(root).expect_err("the window is open");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(
        error.message.contains("engr migrate"),
        "the refusal says how to close the window: {error}"
    );
    // Not a partial answer, and not silence: the read surfaces refuse too.
    engr::ops::effective(root, id).expect_err("no mixed-generation read");
    store::load_object(root, id).expect_err("no current read either");

    // And completing the migration restores availability.
    store::migrate(root).expect("resume");
    assert_eq!(
        store::validate_format(root).expect("readable again"),
        store::WorkspaceFormat::Current
    );
    engr::ops::effective(root, id).expect("and the record reads");
}
