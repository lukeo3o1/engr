//! What unresolved staging is, and what it is not.
//!
//! Backlog exists to hold work nobody has settled. These tests pin the two
//! things that would quietly destroy its value: staging becoming readable as
//! authority, and a candidate written against one unresolved point mutating a
//! different one.

use engr::backlog::{self, Produced, Subject};
use engr::model::{Action, Content, Payload, Ref};
use engr::{gate, ops, reference, store};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn workspace() -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().to_path_buf();
    store::init(&root).expect("init");
    (dir, root)
}

fn git(root: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .expect("git");
    assert!(status.success(), "git {args:?}");
}

fn repository(root: &Path) {
    git(root, &["init", "-q"]);
    git(root, &["config", "user.name", "test"]);
    git(root, &["config", "user.email", "test@example.com"]);
}

fn commit_all(root: &Path, message: &str) -> String {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", message]);
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

fn compact(id: &str) -> String {
    reference::encode_uuid(uuid::Uuid::parse_str(id).expect("uuid"))
}

fn item(root: &Path, topic: &str, text: &str) -> String {
    backlog::create(root, topic, text, Vec::new())
        .expect("create backlog item")
        .id
}

// ---------------------------------------------------------------------------
// Storage and model
// ---------------------------------------------------------------------------

#[test]
fn a_backlog_item_is_a_uuidv7_whose_filename_is_its_id() {
    let (_dir, root) = workspace();
    let id = item(
        &root,
        "reconsider the refresh strategy",
        "offline mode may break it",
    );

    let parsed = uuid::Uuid::parse_str(&id).expect("backlog ids are UUIDs");
    assert_eq!(parsed.get_version(), Some(uuid::Version::SortRand));
    assert!(
        backlog::item_path(&root, &id).exists(),
        "the file is named after the id, so it can be found without a resolver"
    );
    assert_eq!(backlog::ids(&root).expect("ids"), vec![id.clone()]);

    // Persisted identity is the dashed UUID; the compact form is a reference
    // codec, not a second identity.
    assert_eq!(
        backlog::resolve_id(&root, &format!("engr:backlog:{}", compact(&id))).expect("reference"),
        id
    );
    assert_eq!(backlog::resolve_id(&root, &id[..8]).expect("prefix"), id);
}

#[test]
fn backlog_files_carry_no_schema_version_of_their_own() {
    let (_dir, root) = workspace();
    let id = item(&root, "topic", "unresolved");
    let stored: serde_json::Value =
        store::read_json(&backlog::item_path(&root, &id)).expect("stored item");
    let stored = stored.as_object().expect("object");
    assert!(
        !stored.contains_key("format") && !stored.contains_key("version"),
        "`.engr/format.json` is the sole workspace schema authority"
    );
}

#[test]
fn section_ids_are_monotonic_and_never_reused() {
    let (_dir, root) = workspace();
    let id = item(&root, "several concerns", "first");
    let second = backlog::add_section(&root, &id, "second", Vec::new()).expect("add");
    let third = backlog::add_section(&root, &id, "third", Vec::new()).expect("add");
    assert_eq!((second, third), (2, 3));

    assert!(!backlog::consume_section(&root, &id, 2).expect("delete"));
    let fourth = backlog::add_section(&root, &id, "fourth", Vec::new()).expect("add");
    assert_eq!(
        fourth, 4,
        "the counter is persisted, so a gap is never handed back out"
    );
    let stored = backlog::load(&root, &id).expect("load");
    assert_eq!(
        stored.sections.iter().map(|s| s.id).collect::<Vec<_>>(),
        vec![1, 3, 4],
        "the gap where §2 was is information: something was there"
    );

    // A merge keeps the destination identity and never hands the source back.
    let merged = backlog::merge_sections(&root, &id, 1, 3, "one point", Vec::new()).expect("merge");
    assert_eq!(merged, 1);
    assert_eq!(
        backlog::add_section(&root, &id, "after merge", Vec::new()).expect("add"),
        5
    );
}

#[test]
fn a_stored_item_with_duplicate_or_impossible_section_ids_is_refused() {
    let (_dir, root) = workspace();
    let id = item(&root, "topic", "first");
    backlog::add_section(&root, &id, "second", Vec::new()).expect("add");
    let path = backlog::item_path(&root, &id);
    let pristine: serde_json::Value = store::read_json(&path).expect("item");

    for (name, corrupt) in [
        (
            "duplicate section ids",
            Box::new(|value: &mut serde_json::Value| {
                value["sections"][1]["id"] = serde_json::json!(1);
            }) as Box<dyn Fn(&mut serde_json::Value)>,
        ),
        (
            "a live section at or past the counter",
            Box::new(|value: &mut serde_json::Value| {
                value["next_section_id"] = serde_json::json!(2);
            }),
        ),
        (
            "a zero section id",
            Box::new(|value: &mut serde_json::Value| {
                value["sections"][0]["id"] = serde_json::json!(0);
            }),
        ),
        (
            "no sections at all",
            Box::new(|value: &mut serde_json::Value| {
                value["sections"] = serde_json::json!([]);
            }),
        ),
    ] {
        let mut value = pristine.clone();
        corrupt(&mut value);
        store::write_json(&path, &value).expect("write corrupt item");
        let error = backlog::load(&root, &id).expect_err(name);
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{name}");
    }

    let mut value = pristine;
    value["id"] = serde_json::json!(engr::model::new_id());
    store::write_json(&path, &value).expect("write mismatched id");
    let error = backlog::load(&root, &id).expect_err("id must match its filename");
    assert!(error.message.contains("does not match its filename"));
}

/// What the write path refuses, a stored file may not contain.
///
/// Two validations that disagree mean the stricter one is decorative: the shape
/// only has to survive one hand-edit to stop being true, and staging is
/// hand-edited by design. These are stored-data faults, so they are EXIT_SCHEMA
/// — not the EXIT_USAGE a person gets for typing the same thing at the CLI.
#[test]
fn stored_backlog_data_is_held_to_what_the_write_path_enforces() {
    let (_dir, root) = workspace();
    let id = item(&root, "topic", "unresolved");
    let path = backlog::item_path(&root, &id);
    let pristine: serde_json::Value = store::read_json(&path).expect("item");

    for (name, corrupt) in [
        (
            "a blank topic",
            Box::new(|value: &mut serde_json::Value| value["topic"] = serde_json::json!("   "))
                as Box<dyn Fn(&mut serde_json::Value)>,
        ),
        (
            "a topic spanning lines",
            Box::new(|value: &mut serde_json::Value| {
                value["topic"] = serde_json::json!("a topic\nwith a body under it")
            }),
        ),
        (
            "a topic that is really a body",
            Box::new(|value: &mut serde_json::Value| {
                value["topic"] = serde_json::json!("x".repeat(121))
            }),
        ),
        (
            "blank section text",
            Box::new(|value: &mut serde_json::Value| {
                value["sections"][0]["text"] = serde_json::json!("  \n ")
            }),
        ),
        (
            "an updated_at that is not a timestamp",
            Box::new(|value: &mut serde_json::Value| {
                value["sections"][0]["updated_at"] = serde_json::json!("last tuesday")
            }),
        ),
        (
            "an updated_at that is not RFC3339",
            Box::new(|value: &mut serde_json::Value| {
                value["sections"][0]["updated_at"] = serde_json::json!("2026-08-17 00:00:00")
            }),
        ),
    ] {
        let mut value = pristine.clone();
        corrupt(&mut value);
        store::write_json(&path, &value).expect("write corrupt item");
        let error = backlog::load(&root, &id).expect_err(name);
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{name}");
    }

    // A topic exactly at the limit is fine; the boundary is not off by one.
    let mut value = pristine;
    value["topic"] = serde_json::json!("x".repeat(120));
    store::write_json(&path, &value).expect("write");
    backlog::load(&root, &id).expect("120 characters is a topic, not a body");
}

/// Backlog files are meant to be hand-edited, so a field engr does not know is
/// a field it would drop on the next ordinary rewrite. Reading such a file as
/// valid is worse than refusing it: the tool claims to understand the shape and
/// then edits it. Lifecycle fields are the case that matters — existence is the
/// only unresolved/resolved signal there is.
#[test]
fn stored_backlog_data_outside_the_schema_is_refused_rather_than_dropped() {
    let (_dir, root) = workspace();
    let object = new_object(&root, "something a point can concern");
    let reference = format!("obj:{}", compact(&object));
    let id = backlog::create(
        &root,
        "topic",
        "unresolved",
        vec![Subject::engr(&reference)],
    )
    .expect("create")
    .id;
    let path = backlog::item_path(&root, &id);

    // Seed an outcome too, so every persisted shape below is one this test
    // actually reaches rather than one it assumes exists.
    let mut pristine: serde_json::Value = store::read_json(&path).expect("item");
    pristine["sections"][0]["produced"] =
        serde_json::json!([{ "target": { "kind": "engr", "ref": reference } }]);
    store::write_json(&path, &pristine).expect("write");
    backlog::load(&root, &id).expect("the seeded shape is the one the writer produces");

    for (name, corrupt) in [
        (
            "a section status the lifecycle deliberately does not have",
            Box::new(|value: &mut serde_json::Value| {
                value["sections"][0]["status"] = serde_json::json!("resolved")
            }) as Box<dyn Fn(&mut serde_json::Value)>,
        ),
        (
            "a resolved flag on a section",
            Box::new(|value: &mut serde_json::Value| {
                value["sections"][0]["resolved"] = serde_json::json!(true)
            }),
        ),
        (
            "resource-local schema markers on the item",
            Box::new(|value: &mut serde_json::Value| {
                value["format"] = serde_json::json!("engr-backlog");
                value["version"] = serde_json::json!(999);
            }),
        ),
        (
            "tracker metadata on the item",
            Box::new(|value: &mut serde_json::Value| {
                value["owner"] = serde_json::json!("someone");
                value["priority"] = serde_json::json!("high");
            }),
        ),
        (
            "an unknown field on a subject",
            Box::new(|value: &mut serde_json::Value| {
                value["sections"][0]["subjects"][0]["blocks"] = serde_json::json!(true)
            }),
        ),
        (
            "an unknown field on a produced outcome",
            Box::new(|value: &mut serde_json::Value| {
                value["sections"][0]["produced"][0]["at"] =
                    serde_json::json!("2026-01-01T00:00:00Z")
            }),
        ),
        (
            "the reserved semantic key on an embedded target",
            Box::new(|value: &mut serde_json::Value| {
                value["sections"][0]["produced"][0]["target"]["type"] = serde_json::json!("engr")
            }),
        ),
    ] {
        let mut value = pristine.clone();
        corrupt(&mut value);
        store::write_json(&path, &value).expect("write corrupt item");
        let error = backlog::load(&root, &id).expect_err(name);
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{name}");
    }
}

#[test]
fn removing_the_last_unresolved_point_removes_the_item() {
    let (_dir, root) = workspace();
    let id = item(&root, "one point only", "the only unresolved thing");
    assert!(
        backlog::consume_section(&root, &id, 1).expect("delete"),
        "an item is a topic that still has unresolved work in it"
    );
    assert!(!backlog::item_path(&root, &id).exists());
    assert!(backlog::ids(&root).expect("ids").is_empty());
}

#[test]
fn backlog_keeps_no_event_log_and_needs_no_confirmation() {
    let (_dir, root) = workspace();
    let id = item(&root, "agent editable", "unresolved");
    backlog::revise_section(&root, &id, 1, "reworded without asking anyone").expect("revise");

    assert!(
        gate::pending(&root).expect("candidates").is_empty(),
        "ordinary backlog editing never mints a challenge"
    );
    assert!(
        store::load_events(&root, &id).expect("events").is_empty(),
        "git is backlog's history; there is no second one"
    );
    let stored = backlog::load(&root, &id).expect("load");
    let section = &stored.sections[0];
    assert_eq!(section.text, "reworded without asking anyone");
    let raw: serde_json::Value = store::read_json(&backlog::item_path(&root, &id)).expect("stored");
    let raw = raw["sections"][0].as_object().expect("section");
    for absent in ["sha256", "confirmed_at", "based_on", "status"] {
        assert!(
            !raw.contains_key(absent),
            "backlog must not borrow the record's {absent} merely for symmetry"
        );
    }
}

#[test]
fn a_legacy_workspace_refuses_backlog_mutation_until_it_is_migrated() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "legacy object");
    let path = store::object_path(&root, &id);
    let mut value: serde_json::Value = store::read_json(&path).expect("object");
    let object = value.as_object_mut().expect("object");
    let state = object.remove("state").expect("state");
    object.insert("status".to_owned(), state);
    store::write_json(&path, &value).expect("legacy object");

    let error = backlog::create(&root, "topic", "unresolved", Vec::new())
        .expect_err("a legacy workspace is read-only until migration");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("engr migrate"));
    assert!(backlog::ids(&root).expect("ids").is_empty());
}

#[test]
fn a_backlog_write_is_never_seen_half_finished() {
    let (_dir, root) = workspace();
    let id = item(&root, "atomic", "first");
    backlog::add_section(&root, &id, "second", Vec::new()).expect("add");
    let path = backlog::item_path(&root, &id);
    let text = std::fs::read_to_string(&path).expect("read");
    serde_json::from_str::<serde_json::Value>(&text).expect("a complete JSON document");
    assert!(
        !path.with_extension("json.tmp").exists(),
        "the temporary file is renamed into place, never left behind"
    );
}

#[test]
fn ordinary_backlog_mutations_serialize_through_the_workspace_lock() {
    use std::sync::{Arc, Barrier};

    let (_dir, root) = workspace();
    let id = item(&root, "contended", "first");
    let start = Arc::new(Barrier::new(2));
    let (first, second) = std::thread::scope(|scope| {
        let first_start = Arc::clone(&start);
        let second_start = Arc::clone(&start);
        let first_root = &root;
        let second_root = &root;
        let first_id = id.clone();
        let second_id = id.clone();
        let first = scope.spawn(move || {
            first_start.wait();
            backlog::add_section(first_root, &first_id, "second", Vec::new())
        });
        let second = scope.spawn(move || {
            second_start.wait();
            backlog::add_section(second_root, &second_id, "third", Vec::new())
        });
        (first.join().expect("first"), second.join().expect("second"))
    });
    let (first, second) = (first.expect("first add"), second.expect("second add"));
    assert_ne!(
        first, second,
        "read-modify-write under one lock cannot hand out the same id twice"
    );
    let stored = backlog::load(&root, &id).expect("load");
    assert_eq!(
        stored.sections.len(),
        3,
        "neither concurrent write may be lost"
    );
    assert_eq!(stored.next_section_id, 4);
}

// ---------------------------------------------------------------------------
// Subjects
// ---------------------------------------------------------------------------

#[test]
fn a_subject_may_name_an_object_a_section_or_another_unresolved_point() {
    let (_dir, root) = workspace();
    let object = new_object(&root, "authentication");
    admit(
        &root,
        payload(Action::SectionAdded, &object, "refresh tokens rotate"),
    );
    let other = item(&root, "another topic", "also unresolved");

    let subjects = vec![
        Subject::engr(format!("obj:{}", compact(&object))),
        Subject::engr(format!("obj:{}:1", compact(&object))),
        Subject::engr(format!("backlog:{}", compact(&other))),
        Subject::engr(format!("backlog:{}:1", compact(&other))),
    ];
    let id = backlog::create(
        &root,
        "concerns four things",
        "unresolved",
        subjects.clone(),
    )
    .expect("create")
    .id;
    assert_eq!(
        backlog::load(&root, &id).expect("load").sections[0].subjects,
        subjects
    );
}

#[test]
fn backlog_subjects_may_form_a_cycle() {
    let (_dir, root) = workspace();
    let first = item(&root, "first", "unresolved");
    let second = backlog::create(
        &root,
        "second",
        "unresolved",
        vec![Subject::engr(format!("backlog:{}:1", compact(&first)))],
    )
    .expect("create")
    .id;
    backlog::set_subjects(
        &root,
        &first,
        1,
        vec![Subject::engr(format!("backlog:{}:1", compact(&second)))],
    )
    .expect("subjects[] is navigation, not a dependency DAG");

    assert!(backlog::load(&root, &first).is_ok());
    assert!(backlog::load(&root, &second).is_ok());
}

#[test]
fn a_subject_cannot_name_a_collection_or_a_snapshot_or_nonsense() {
    let (_dir, root) = workspace();
    let object = compact(&engr::model::new_id());
    for reference in [
        "collection:abcdefghjk".to_owned(),
        format!("obj:{object}@0123456789abcdef0123456789abcdef01234567"),
        format!("obj:{object}:0003"),
        "obj:not-a-compact-uuid".to_owned(),
        format!("engr:obj:{object}"),
    ] {
        let error = backlog::create(
            &root,
            "topic",
            "unresolved",
            vec![Subject::engr(&reference)],
        )
        .expect_err(&reference);
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{reference}");
    }
}

#[test]
fn exactly_duplicated_subjects_are_refused() {
    let (_dir, root) = workspace();
    let object = compact(&engr::model::new_id());
    let subject = Subject::engr(format!("obj:{object}:2"));
    let error = backlog::create(
        &root,
        "topic",
        "unresolved",
        vec![subject.clone(), subject.clone()],
    )
    .expect_err("subjects[] is a set");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("same subject twice"));

    // Order alone is not duplication.
    backlog::create(
        &root,
        "topic",
        "unresolved",
        vec![subject, Subject::engr(format!("obj:{object}:3"))],
    )
    .expect("two distinct subjects");
}

#[test]
fn file_and_symbol_subjects_pin_a_full_committed_object_id() {
    let (_dir, root) = workspace();
    repository(&root);
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::write(root.join("src/session.rs"), "fn refresh() {}\n").expect("source");
    let head = commit_all(&root, "session");

    let id = backlog::create(
        &root,
        "refresh handling",
        "offline mode may invalidate this",
        vec![
            Subject::File {
                path: "src/session.rs".to_owned(),
                commit: backlog::pin(&root, "src/session.rs", None).expect("pin").0,
                dirty: false,
            },
            Subject::Symbol {
                path: "src/session.rs".to_owned(),
                symbol: "refresh".to_owned(),
                commit: backlog::pin(&root, "src/session.rs", None).expect("pin").0,
                dirty: false,
            },
        ],
    )
    .expect("create")
    .id;

    for subject in &backlog::load(&root, &id).expect("load").sections[0].subjects {
        let commit = match subject {
            Subject::File { commit, .. } | Subject::Symbol { commit, .. } => commit,
            Subject::Engr { .. } => unreachable!("no engr subject here"),
        };
        assert_eq!(commit, &head, "the resolved id is persisted, never `HEAD`");
        assert!(engr::model::is_canonical_git_oid(commit));
    }
}

#[test]
fn a_subject_path_must_be_a_normalized_repository_path() {
    let (_dir, root) = workspace();
    repository(&root);
    std::fs::write(root.join("file.rs"), "\n").expect("source");
    commit_all(&root, "file");
    for path in ["/etc/passwd", "../outside.rs", "src\\windows.rs", "a//b.rs"] {
        assert!(backlog::pin(&root, path, None).is_err(), "{path}");
    }
}

#[test]
fn the_record_still_cannot_depend_on_unconfirmed_staging() {
    let (_dir, root) = workspace();
    repository(&root);
    let object = new_object(&root, "record");
    let staging = item(&root, "unresolved", "not confirmed by anyone");
    commit_all(&root, "record and staging");

    // `refs[]` names an Object and a section of it. A Backlog id put there
    // resolves to no Object, which is the only answer that keeps a confirmed
    // section from standing on wording nobody read.
    let mut proposal = payload(Action::SectionAdded, &object, "stands on something");
    proposal.content.refs = vec![Ref {
        object: staging.clone(),
        section: 1,
        sha256: "0".repeat(64),
        commit: engr::git::head(&root).expect("HEAD"),
    }];
    let error = gate::prepare(&root, proposal).expect_err("a record ref cannot target backlog");
    assert_eq!(error.code, engr::EXIT_NOT_FOUND);
    assert!(error.message.contains("does not exist"));
}

// ---------------------------------------------------------------------------
// The resolution basis
// ---------------------------------------------------------------------------

/// Write an item with exact timestamps, which `create` cannot: it uses the
/// clock, and these tests are about values the clock never produces.
fn staged(root: &Path, id: &str, topic: &str, sections: serde_json::Value) {
    let item = serde_json::json!({
        "id": id,
        "topic": topic,
        "next_section_id": sections.as_array().expect("sections").len() + 1,
        "sections": sections,
    });
    store::write_json(&backlog::item_path(root, id), &item).expect("stage");
}

/// RFC3339 carries an offset, and offset timestamps do not sort as text.
/// `updated_at` is what an agent reads to decide which unresolved point was
/// touched most recently, so comparing the strings answers that question wrong.
#[test]
fn activity_is_compared_as_an_instant_not_as_text() {
    let (_dir, root) = workspace();
    let id = engr::model::new_id();
    staged(
        &root,
        &id,
        "offsets",
        serde_json::json!([
            // 17:00Z — earlier, but sorts later as a string.
            {"id": 1, "text": "first", "updated_at": "2026-08-17T01:00:00+08:00", "subjects": []},
            {"id": 2, "text": "second", "updated_at": "2026-08-16T20:00:00Z", "subjects": []},
        ]),
    );

    let item = backlog::load(&root, &id).expect("load");
    assert_eq!(
        item.updated_at(),
        "2026-08-16T20:00:00Z",
        "20:00Z is three hours after 01:00+08:00, whatever the strings do"
    );
    assert!(
        "2026-08-17T01:00:00+08:00" > "2026-08-16T20:00:00Z",
        "and text comparison really would have picked the other one"
    );
}

/// Shortening a timestamp by cutting the string moves it. With an offset and
/// fractional seconds, trimming at the `.` and appending `Z` reports an instant
/// eight hours from the one recorded.
#[test]
fn rendering_activity_to_the_second_never_moves_the_instant() {
    let (_dir, root) = workspace();
    let id = engr::model::new_id();
    staged(
        &root,
        &id,
        "rendered",
        serde_json::json!([
            {"id": 1, "text": "point", "updated_at": "2026-08-17T10:00:00.123456+08:00", "subjects": []},
        ]),
    );
    let item = backlog::load(&root, &id).expect("load");

    for rendered in [
        engr::view::render_backlog_show(&root, &item),
        engr::view::render_backlog_ls(&root, std::slice::from_ref(&item), None),
    ] {
        assert!(
            rendered.contains("2026-08-17T02:00:00Z"),
            "the same instant, in UTC, to the second: {rendered:?}"
        );
        assert!(
            !rendered.contains("2026-08-17T10:00:00Z"),
            "and never the local reading relabelled as UTC: {rendered:?}"
        );
    }

    // The stored value keeps its own precision and offset; only the column is
    // normalized.
    assert_eq!(
        item.sections[0].updated_at,
        "2026-08-17T10:00:00.123456+08:00"
    );
    let json: serde_json::Value =
        serde_json::from_str(&engr::view::render_backlog_json(&item).expect("json")).expect("json");
    assert_eq!(
        json["sections"][0]["updated_at"],
        item.sections[0].updated_at
    );
}

/// `subjects[]` is a set, so reordering it states the same unresolved thing.
/// Activity has to agree: reordering is not work, and reporting it as work is a
/// false signal in the exact field triage reads.
///
/// This used to assert through the removed resolution fingerprint. `updated_at`
/// is the surface that actually carries the claim, so it is what the test reads
/// now -- the fingerprint was only ever a proxy for it.
#[test]
fn reordering_an_equivalent_subject_set_is_not_activity() {
    let (_dir, root) = workspace();
    let object = compact(&engr::model::new_id());
    let subjects = vec![
        Subject::engr(format!("obj:{object}:1")),
        Subject::engr(format!("obj:{object}:2")),
    ];
    let id = backlog::create(&root, "topic", "unresolved", subjects.clone())
        .expect("create")
        .id;
    let before = backlog::load(&root, &id).expect("load");
    let activity = before.sections[0].updated_at.clone();

    std::thread::sleep(std::time::Duration::from_millis(1100));
    backlog::set_subjects(&root, &id, 1, subjects.iter().rev().cloned().collect())
        .expect("reorder");
    let after = backlog::load(&root, &id).expect("load");
    assert_eq!(
        after.sections[0].updated_at, activity,
        "so it cannot look freshly worked on"
    );
    assert_eq!(
        after.sections[0].subjects,
        subjects.iter().rev().cloned().collect::<Vec<_>>(),
        "the caller's order is still what gets stored"
    );

    // Rewriting the identical text is not work either.
    backlog::revise_section(&root, &id, 1, "unresolved").expect("idempotent write");
    assert_eq!(
        backlog::load(&root, &id).expect("load").sections[0].updated_at,
        activity
    );

    // A real change to either does advance it.
    backlog::set_subjects(&root, &id, 1, vec![subjects[0].clone()]).expect("drop one");
    let changed = backlog::load(&root, &id).expect("load").sections[0]
        .updated_at
        .clone();
    assert_ne!(changed, activity);

    std::thread::sleep(std::time::Duration::from_millis(1100));
    backlog::revise_section(&root, &id, 1, "reworded").expect("revise");
    assert_ne!(
        backlog::load(&root, &id).expect("load").sections[0].updated_at,
        changed
    );
}

#[test]
fn a_topic_rename_does_not_refresh_section_activity() {
    let (_dir, root) = workspace();
    let id = item(&root, "before", "unresolved");
    let before = backlog::load(&root, &id).expect("load").sections[0]
        .updated_at
        .clone();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    backlog::rename(&root, &id, "after").expect("rename");
    let after = backlog::load(&root, &id).expect("load");
    assert_eq!(after.topic, "after");
    assert_eq!(
        after.sections[0].updated_at, before,
        "renaming a topic is not activity on any unresolved point"
    );

    backlog::revise_section(&root, &id, 1, "reworded").expect("revise");
    assert_ne!(
        backlog::load(&root, &id).expect("load").sections[0].updated_at,
        before,
        "rewording one is"
    );
}

// ---------------------------------------------------------------------------
// produced[]
// ---------------------------------------------------------------------------

#[test]
fn produced_targets_are_authoritative_objects_and_sections_only() {
    let object = compact(&engr::model::new_id());
    Produced::object(format!("obj:{object}"))
        .validate()
        .expect("whole Object");
    Produced::object(format!("obj:{object}:3"))
        .validate()
        .expect("Object Section");
    for rejected in [
        format!("backlog:{object}"),
        format!("backlog:{object}:1"),
        "collection:abcdefghjk".to_owned(),
        format!("obj:{object}@0123456789abcdef0123456789abcdef01234567"),
    ] {
        assert!(
            Produced::object(&rejected).validate().is_err(),
            "produced[] answers what the record gained: {rejected}"
        );
    }

    // file and symbol targets cannot even be spelled: the field is the shared
    // embedded engr target, and nothing else deserializes into it.
    for target in [
        serde_json::json!({"target": {"kind": "file", "path": "src/a.rs", "commit": "0"}}),
        serde_json::json!({"target": {"kind": "symbol", "path": "src/a.rs", "symbol": "f", "commit": "0"}}),
    ] {
        assert!(serde_json::from_value::<Produced>(target).is_err());
    }
}

#[test]
fn merging_unresolved_points_keeps_what_they_already_produced() {
    let (_dir, root) = workspace();
    let object = compact(&engr::model::new_id());
    let staging = item(&root, "two halves", "first half");
    backlog::add_section(&root, &staging, "second half", Vec::new()).expect("add");

    let mut stored = backlog::load(&root, &staging).expect("load");
    stored.sections[0].produced = vec![Produced::object(format!("obj:{object}:1"))];
    stored.sections[1].produced = vec![
        Produced::object(format!("obj:{object}:1")),
        Produced::object(format!("obj:{object}:2")),
    ];
    store::write_json(&backlog::item_path(&root, &staging), &stored).expect("seed outcomes");

    let merged =
        backlog::merge_sections(&root, &staging, 1, 2, "one point", Vec::new()).expect("merge");
    let section = backlog::load(&root, &staging).expect("load");
    let section = section.section(merged).expect("merged section");
    assert_eq!(
        section.produced,
        vec![
            Produced::object(format!("obj:{object}:1")),
            Produced::object(format!("obj:{object}:2")),
        ],
        "the outcomes happened; merging says the points were one, not that they did not"
    );
}

// ---------------------------------------------------------------------------
// Confirmation-time reconciliation
// ---------------------------------------------------------------------------

#[test]
fn effective_authority_is_unchanged_by_anything_in_staging() {
    let (_dir, root) = workspace();
    let object = new_object(&root, "record");
    admit(
        &root,
        payload(Action::SectionAdded, &object, "confirmed wording"),
    );
    let before = ops::effective(&root, &object).expect("effective");

    backlog::create(
        &root,
        "reconsider",
        "the confirmed wording may be wrong",
        vec![Subject::engr(format!("obj:{}:1", compact(&object)))],
    )
    .expect("stage a doubt");

    assert_eq!(
        ops::effective(&root, &object).expect("effective"),
        before,
        "a doubt in staging does not change what the record says"
    );
    let report = ops::verify(&root, &object).expect("verify");
    assert!(report.passed(), "record verification stays record-oriented");
}

/// A dirty target pins its baseline and says so, instead of being refused.
///
/// The earlier rule rejected it: with the path modified, the pinned commit does
/// not describe what the agent read, so the subject was called a false snapshot.
/// That trade was wrong. Refusing loses the context altogether — the agent did
/// read something and now cannot say so — while the honest answer costs nothing:
/// keep the recoverable baseline and record that it is inexact.
///
/// `dirty` is target-local. It says nothing about the repository as a whole, and
/// for a symbol it means the **containing file** was modified — proving a diff
/// touches one symbol's own range would need parsing and AST mapping, which the
/// protocol refuses to require for context metadata.
#[test]
fn a_dirty_target_pins_its_baseline_and_records_that_it_is_inexact() {
    let (_dir, root) = workspace();
    repository(&root);
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::write(root.join("src/session.rs"), "fn refresh() {}\n").expect("source");
    let committed = commit_all(&root, "session");

    let (commit, dirty) = backlog::pin(&root, "src/session.rs", None).expect("clean");
    assert_eq!(commit, committed);
    assert!(!dirty, "a clean target carries no marker");

    std::fs::write(root.join("src/session.rs"), "fn refresh() { todo!() }\n").expect("edit");
    let (commit, dirty) =
        backlog::pin(&root, "src/session.rs", None).expect("dirty is not refused");
    assert_eq!(commit, committed, "the baseline is still recoverable");
    assert!(dirty, "and the subject says the observed target was not it");

    // Naming a revision explicitly does not make the working file match it, so
    // the marker is about what was read rather than which commit was chosen.
    let (_, dirty) = backlog::pin(&root, "src/session.rs", Some(&committed)).expect("explicit");
    assert!(dirty);

    // A clean subject stays byte-for-byte what it was: the field is absent, not
    // `false`, so nothing that already exists gains a key.
    let clean = Subject::File {
        path: "src/session.rs".to_owned(),
        commit: committed.clone(),
        dirty: false,
    };
    let json = serde_json::to_value(&clean).expect("json");
    assert!(json.get("dirty").is_none(), "absent when clean: {json}");

    // An untracked file is still refused, and for a different reason worth
    // keeping straight: `dirty` says the baseline is inexact, and an untracked
    // file has no baseline at all. Marking it dirty would claim a commit
    // reconstructs something it has never held.
    std::fs::write(root.join("src/new.rs"), "fn added() {}\n").expect("untracked");
    for revision in [None, Some(committed.as_str())] {
        let error = backlog::pin(&root, "src/new.rs", revision)
            .expect_err("no commit holds it, so there is nothing to pin");
        assert!(
            error.message.contains("does not exist at commit"),
            "{}",
            error.message
        );
    }
}

/// `dirty` is observation detail, not part of which target is meant.
///
/// Re-observing the same file at the same commit against a modified worktree
/// concerns the same thing, so it must not read as fresh work in the field
/// triage sorts by.
#[test]
fn the_dirty_marker_is_not_part_of_subject_identity() {
    let (_dir, root) = workspace();
    repository(&root);
    std::fs::write(root.join("file.rs"), "fn a() {}\n").expect("source");
    let committed = commit_all(&root, "file");
    let clean = Subject::File {
        path: "file.rs".to_owned(),
        commit: committed.clone(),
        dirty: false,
    };
    let observed_dirty = Subject::File {
        path: "file.rs".to_owned(),
        commit: committed,
        dirty: true,
    };
    assert_ne!(clean, observed_dirty, "structurally they still differ");

    let id = backlog::create(&root, "topic", "unresolved", vec![clean])
        .expect("create")
        .id;
    let before = backlog::load(&root, &id).expect("load").sections[0]
        .updated_at
        .clone();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    backlog::set_subjects(&root, &id, 1, vec![observed_dirty]).expect("re-observe");
    assert_eq!(
        backlog::load(&root, &id).expect("load").sections[0].updated_at,
        before,
        "the same target re-observed is not activity"
    );
}

/// `produced[]` is the second of two independent operations.
///
/// Admitting an Object no longer reaches into staging, so recording what a point
/// produced is an ordinary Backlog edit the agent performs afterwards. That is
/// the trade #8 chose: forgetting leaves the bookkeeping stale and the admitted
/// record perfectly valid, where an inferred link would eventually consume a
/// point nobody meant to resolve.
#[test]
fn recording_an_outcome_is_a_separate_operation_from_admitting_it() {
    let (_dir, root) = workspace();
    let object = new_object(&root, "the decision that came out of it");
    let id = item(&root, "topic", "unresolved");

    // Admission alone records nothing here.
    assert!(
        backlog::load(&root, &id).expect("load").sections[0]
            .produced
            .is_empty(),
        "confirming an Object does not touch staging"
    );

    let outcome = Produced::object(format!("obj:{}", compact(&object)));
    assert!(backlog::record_produced(&root, &id, 1, outcome.clone()).expect("record"));
    assert_eq!(
        backlog::load(&root, &id).expect("load").sections[0].produced,
        vec![outcome.clone()]
    );

    // A set: claiming the same outcome twice is not an error and not a
    // duplicate, so a retried command is harmless.
    assert!(!backlog::record_produced(&root, &id, 1, outcome.clone()).expect("again"));
    assert_eq!(
        backlog::load(&root, &id).expect("load").sections[0]
            .produced
            .len(),
        1
    );

    // And it is not a resolution signal: the point is still there.
    assert_eq!(
        backlog::load(&root, &id).expect("load").sections.len(),
        1,
        "an outcome does not settle the point that produced it"
    );

    // Mutable bookkeeping, so a mistaken entry is correctable.
    assert!(backlog::forget_produced(&root, &id, 1, &outcome).expect("forget"));
    assert!(!backlog::forget_produced(&root, &id, 1, &outcome).expect("idempotent"));
    assert!(backlog::load(&root, &id).expect("load").sections[0]
        .produced
        .is_empty());
}

/// Existence is checked when the claim is made, and in that direction only.
///
/// A target must exist to be claimed — otherwise `produced[]` would record
/// outcomes that never happened. Afterwards the entry is history: the target may
/// be superseded, deleted or absorbed, and the entry becomes an unavailable
/// historical pointer rather than corruption. It never constrains the Object
/// domain, and it is never retargeted to a replacement, because that would
/// rewrite what was actually produced.
#[test]
fn a_produced_target_is_checked_at_the_claim_and_never_again() {
    let (_dir, root) = workspace();
    let object = new_object(&root, "an outcome");
    let id = item(&root, "topic", "unresolved");

    // Forward: a target that does not exist cannot be claimed.
    let missing = Produced::object(format!("obj:{}", compact(&engr::model::new_id())));
    let error = backlog::record_produced(&root, &id, 1, missing).expect_err("no such object");
    assert!(
        error.message.contains("does not exist"),
        "{}",
        error.message
    );

    // Nor a section the object does not have.
    let no_section = Produced::object(format!("obj:{}:9", compact(&object)));
    backlog::record_produced(&root, &id, 1, no_section).expect_err("no such section");

    // A real one is accepted, then the staging around it survives the target
    // going away — loading must not depend on a recorded outcome still
    // resolving.
    let outcome = Produced::object(format!("obj:{}", compact(&object)));
    backlog::record_produced(&root, &id, 1, outcome.clone()).expect("record");
    std::fs::remove_file(engr::store::object_path(&root, &object)).expect("delete the object");
    std::fs::remove_file(engr::store::events_path(&root, &object)).expect("delete its events");
    let loaded = backlog::load(&root, &id).expect("staging still loads");
    assert_eq!(loaded.sections[0].produced, vec![outcome.clone()]);

    // And the bookkeeping is still correctable with the target gone, which is
    // exactly when a mistaken entry is hardest to live with.
    assert!(backlog::forget_produced(&root, &id, 1, &outcome).expect("forget"));
}

/// A prepared mutation binds exactly what it rests on — no less, and no more.
///
/// No less, or a change in the gap between reading and writing lands under it.
/// No more, or unrelated work cries stale and teaches people to re-prepare
/// without looking, which is how a staleness check stops being read at all.
#[test]
fn a_precondition_binds_what_the_mutation_rests_on_and_not_the_whole_item() {
    let (_dir, root) = workspace();
    let id = item(&root, "topic", "first point");
    backlog::add_section(&root, &id, "second point", Vec::new()).expect("add");

    let bound = backlog::Precondition::section(&root, &id, 1).expect("observe");
    bound.still_holds(&root).expect("nothing has moved yet");

    // An unrelated sibling moving does not stale it: this mutation never read
    // §2, so §2 changing says nothing about whether it is still applicable.
    backlog::revise_section(&root, &id, 2, "second point, sharpened").expect("revise sibling");
    bound
        .still_holds(&root)
        .expect("a sibling is not part of what this rests on");

    // The bound Section moving does.
    backlog::revise_section(&root, &id, 1, "first point, sharpened").expect("revise target");
    let error = bound.still_holds(&root).expect_err("the target moved");
    assert_eq!(error.code, engr::EXIT_STALE);
    assert!(
        error.message.contains("re-prepare"),
        "the refusal says what to do: {}",
        error.message
    );
}

/// The whole Section, not a chosen subset of its fields.
///
/// The removed fingerprint covered `text` and `subjects[]`, so it was blind to
/// everything else — and blind by construction, since a field added later was
/// not in the list. Binding the Section covers what exists now and what is added
/// later without anyone remembering to extend it.
#[test]
fn every_field_of_the_bound_section_stales_it_not_only_the_wording() {
    let (_dir, root) = workspace();
    let object = new_object(&root, "an outcome");
    let id = item(&root, "topic", "unresolved");

    for change in [
        "subjects",
        // `produced[]` is exactly the field the old fingerprint excluded.
        "produced",
    ] {
        let bound = backlog::Precondition::section(&root, &id, 1).expect("observe");
        match change {
            "subjects" => {
                backlog::set_subjects(
                    &root,
                    &id,
                    1,
                    vec![Subject::engr(format!("obj:{}", compact(&object)))],
                )
                .expect("subjects");
            }
            _ => {
                backlog::record_produced(
                    &root,
                    &id,
                    1,
                    Produced::object(format!("obj:{}", compact(&object))),
                )
                .expect("produced");
            }
        }
        let error = bound
            .still_holds(&root)
            .expect_err("this field should stale the mutation");
        assert_eq!(error.code, engr::EXIT_STALE, "{change}");
    }
}

/// The topic is context every Section is read in, so it is bound with them.
#[test]
fn a_topic_change_stales_a_section_mutation_prepared_under_it() {
    let (_dir, root) = workspace();
    let id = item(&root, "original topic", "unresolved");
    let bound = backlog::Precondition::section(&root, &id, 1).expect("observe");
    backlog::rename(&root, &id, "a different topic").expect("rename");
    let error = bound.still_holds(&root).expect_err("the context moved");
    assert!(error.message.contains("topic"), "{}", error.message);
}

/// Adding binds the topic and the id it is about to take, not its siblings.
///
/// Two concurrent adds must not each believe they are creating the same point,
/// which is what binding the next id catches — while a sibling being revised
/// meanwhile has nothing to do with whether this add is still what was reviewed.
#[test]
fn adding_a_point_binds_the_id_it_will_take_and_not_the_siblings() {
    let (_dir, root) = workspace();
    let id = item(&root, "topic", "first");
    let bound = backlog::Precondition::section_absent(&root, &id).expect("observe");
    bound.still_holds(&root).expect("the id is still free");

    backlog::revise_section(&root, &id, 1, "first, sharpened").expect("revise sibling");
    bound
        .still_holds(&root)
        .expect("a sibling is not part of an add");

    // Somebody else takes the id this add was going to use.
    backlog::add_section(&root, &id, "someone else got there", Vec::new()).expect("race");
    let error = bound.still_holds(&root).expect_err("the id was taken");
    assert_eq!(error.code, engr::EXIT_STALE);
}

/// A topic change binds the complete item, because it scopes every point.
#[test]
fn a_topic_mutation_binds_the_whole_item() {
    let (_dir, root) = workspace();
    let id = item(&root, "topic", "first");
    let bound = backlog::Precondition::topic(&root, &id).expect("observe");
    bound.still_holds(&root).expect("unchanged");

    backlog::add_section(&root, &id, "a second point", Vec::new()).expect("add");
    let error = bound
        .still_holds(&root)
        .expect_err("renaming is about all of them, so any of them moving matters");
    assert_eq!(error.code, engr::EXIT_STALE);
}

/// Creating an item binds only that the id is free.
#[test]
fn creating_an_item_binds_only_its_own_absence() {
    let (_dir, root) = workspace();
    let existing = item(&root, "unrelated", "unresolved");
    let fresh = engr::model::new_id();
    let bound = backlog::Precondition::item_absent(fresh.clone());
    bound.still_holds(&root).expect("nothing occupies it");

    // Unrelated Backlog activity cannot stale a creation.
    backlog::add_section(&root, &existing, "more", Vec::new()).expect("add elsewhere");
    bound
        .still_holds(&root)
        .expect("another item is not this one");

    // An item appearing at that id is the one thing that does.
    let taken = backlog::Precondition::item_absent(existing);
    let error = taken.still_holds(&root).expect_err("the id is occupied");
    assert_eq!(error.code, engr::EXIT_STALE);
}
