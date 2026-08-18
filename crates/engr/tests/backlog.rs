//! What unresolved staging is, and what it is not.
//!
//! Backlog exists to hold work nobody has settled. These tests pin the two
//! things that would quietly destroy its value: staging becoming readable as
//! authority, and a candidate written against one unresolved point mutating a
//! different one.

use engr::backlog::{self, Produced, Reconciliation, Subject};
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

/// Prepare a record mutation derived from one unresolved point, and confirm it.
fn admit_from(
    root: &Path,
    payload: Payload,
    sources: Vec<gate::SourceRequest>,
) -> Vec<backlog::Outcome> {
    let prepared = gate::prepare_from_backlog(root, payload, sources).expect("prepare");
    let response = format!("CONFIRM {}", prepared.candidate.challenge);
    gate::confirm(root, &response).expect("confirm").backlog
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

    assert!(!backlog::delete_section(&root, &id, 2).expect("delete"));
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

    // Merging also takes a fresh id rather than reusing either absorbed one.
    let merged =
        backlog::merge_sections(&root, &id, &[1, 3], "one point", Vec::new()).expect("merge");
    assert_eq!(merged, 5);
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
        backlog::delete_section(&root, &id, 1).expect("delete"),
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
                commit: backlog::pin(&root, "src/session.rs", None).expect("pin"),
            },
            Subject::Symbol {
                path: "src/session.rs".to_owned(),
                symbol: "refresh".to_owned(),
                commit: backlog::pin(&root, "src/session.rs", None).expect("pin"),
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
fn a_dirty_path_cannot_silently_pin_head() {
    let (_dir, root) = workspace();
    repository(&root);
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::write(root.join("src/session.rs"), "fn refresh() {}\n").expect("source");
    let committed = commit_all(&root, "session");
    std::fs::write(root.join("src/session.rs"), "fn refresh() { todo!() }\n").expect("edit");

    let error = backlog::pin(&root, "src/session.rs", None)
        .expect_err("HEAD would not describe what was actually read");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("commit it first"));

    assert_eq!(
        backlog::pin(&root, "src/session.rs", Some(&committed))
            .expect("an explicit committed choice"),
        committed,
        "choosing a committed revision explicitly remains available"
    );

    // A file git has never seen is dirty in the sense that matters here.
    std::fs::write(root.join("src/new.rs"), "fn added() {}\n").expect("untracked");
    assert!(backlog::pin(&root, "src/new.rs", None).is_err());

    // And a path absent from the chosen commit cannot be pinned at all.
    let error = backlog::pin(&root, "src/new.rs", Some(&committed))
        .expect_err("a snapshot that never held the path is false provenance");
    assert!(error.message.contains("does not exist at commit"));
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

#[test]
fn the_resolution_basis_covers_text_and_subjects_and_nothing_else() {
    let (_dir, root) = workspace();
    let object = compact(&engr::model::new_id());
    let subjects = vec![
        Subject::engr(format!("obj:{object}:1")),
        Subject::engr(format!("obj:{object}:2")),
    ];
    let id = backlog::create(&root, "topic", "unresolved", subjects.clone())
        .expect("create")
        .id;
    let basis = backlog::load(&root, &id).expect("load").sections[0]
        .resolution_basis()
        .expect("basis");

    // Reordering an unordered set states the same thing.
    let reordered: Vec<Subject> = subjects.iter().rev().cloned().collect();
    backlog::set_subjects(&root, &id, 1, reordered).expect("reorder");
    assert_eq!(
        backlog::load(&root, &id).expect("load").sections[0]
            .resolution_basis()
            .expect("basis"),
        basis
    );

    // Neither does renaming the topic, nor the section's own id, nor activity.
    backlog::rename(&root, &id, "a different topic").expect("rename");
    let second = backlog::add_section(&root, &id, "unresolved", subjects.clone()).expect("add");
    let mut item = backlog::load(&root, &id).expect("load");
    assert_ne!(second, 1);
    assert_eq!(
        item.section(second)
            .expect("section")
            .resolution_basis()
            .expect("basis"),
        basis,
        "the basis is the unresolved statement, not where it sits"
    );
    item.sections[0].updated_at = "2001-01-01T00:00:00Z".to_owned();
    assert_eq!(item.sections[0].resolution_basis().expect("basis"), basis);
    item.sections[0].produced = vec![Produced::object(format!("obj:{object}"))];
    assert_eq!(
        item.sections[0].resolution_basis().expect("basis"),
        basis,
        "an accumulated outcome is progress, not a change to what is unresolved"
    );

    // Text and subjects do move it.
    backlog::revise_section(&root, &id, 1, "reworded").expect("revise");
    assert_ne!(
        backlog::load(&root, &id).expect("load").sections[0]
            .resolution_basis()
            .expect("basis"),
        basis
    );
    backlog::revise_section(&root, &id, 1, "unresolved").expect("restore");
    backlog::set_subjects(&root, &id, 1, vec![subjects[0].clone()]).expect("drop one subject");
    assert_ne!(
        backlog::load(&root, &id).expect("load").sections[0]
            .resolution_basis()
            .expect("basis"),
        basis
    );
}

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

/// `subjects[]` is a set, and the resolution basis already treats it as one.
/// Activity has to agree: reordering is not work, and reporting it as work is a
/// false signal in the exact field triage reads.
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
    let (basis, activity) = (
        before.sections[0].resolution_basis().expect("basis"),
        before.sections[0].updated_at.clone(),
    );

    std::thread::sleep(std::time::Duration::from_millis(1100));
    backlog::set_subjects(&root, &id, 1, subjects.iter().rev().cloned().collect())
        .expect("reorder");
    let after = backlog::load(&root, &id).expect("load");
    assert_eq!(
        after.sections[0].resolution_basis().expect("basis"),
        basis,
        "the same set states the same unresolved thing"
    );
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
    assert_ne!(
        backlog::load(&root, &id).expect("load").sections[0]
            .resolution_basis()
            .expect("basis"),
        basis
    );

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

/// `produced[]` claims authoritative knowledge already exists. A syntactically
/// perfect reference to an Object nobody ever created is that claim being
/// false, so it is refused before a challenge is minted — while an outcome the
/// candidate itself brings into being is exactly the case the field is for.
#[test]
fn a_declared_outcome_must_name_authority_this_candidate_leaves_behind() {
    let (_dir, root) = workspace();
    let staging = item(&root, "outcomes", "unresolved");
    let object = new_object(&root, "the object");
    let existing = compact(&object);
    let absent = compact(&engr::model::new_id());

    let refuse = |outcome: Produced, action: Action, target: &str| {
        let error = gate::prepare_from_backlog(
            &root,
            payload(action, target, "some wording"),
            vec![gate::SourceRequest {
                item: staging.clone(),
                section: 1,
                produced: vec![outcome],
                resolves: false,
            }],
        )
        .expect_err("an outcome that will not exist is refused");
        assert_eq!(error.code, engr::EXIT_NOT_FOUND);
        assert!(
            gate::pending(&root).expect("candidates").is_empty(),
            "and no code is minted for it"
        );
        error
    };

    let error = refuse(
        Produced::object(format!("obj:{absent}")),
        Action::SectionAdded,
        &object,
    );
    assert!(error.message.contains("does not exist"));
    let error = refuse(
        Produced::object(format!("obj:{existing}:9")),
        Action::SectionAdded,
        &object,
    );
    assert!(error.message.contains("§9"));

    // An existing Object, and an existing Section of it, are both accepted.
    admit(&root, payload(Action::SectionAdded, &object, "section one"));
    for reference in [format!("obj:{existing}"), format!("obj:{existing}:1")] {
        let prepared = gate::prepare_from_backlog(
            &root,
            payload(Action::SectionAdded, &object, "another"),
            vec![gate::SourceRequest {
                item: staging.clone(),
                section: 1,
                produced: vec![Produced::object(&reference)],
                resolves: false,
            }],
        )
        .expect("an outcome that exists");
        gate::discard(&root, &prepared.candidate.challenge).expect("clean up");
    }

    // And so is one the candidate itself creates: a whole new Object, and the
    // Section this very action appends.
    let minted = engr::model::new_id();
    let outcomes = admit_from(
        &root,
        payload(Action::ObjectCreated, &minted, "created by this candidate"),
        vec![gate::SourceRequest {
            item: staging.clone(),
            section: 1,
            produced: vec![Produced::object(format!("obj:{}", compact(&minted)))],
            resolves: false,
        }],
    );
    assert!(matches!(
        outcomes[0].result,
        Reconciliation::Recorded { added: 1 }
    ));

    let next = ops::effective(&root, &object)
        .expect("object")
        .next_section_id;
    let outcomes = admit_from(
        &root,
        payload(Action::SectionAdded, &object, "the section this produces"),
        vec![gate::SourceRequest {
            item: staging.clone(),
            section: 1,
            produced: vec![Produced::object(format!("obj:{existing}:{next}"))],
            resolves: false,
        }],
    );
    assert!(matches!(
        outcomes[0].result,
        Reconciliation::Recorded { added: 1 }
    ));
}

/// Existence is checked when the claim is made, never again. An outcome that
/// was real when it was recorded and has since been deleted through the gate is
/// history, and history must not make the staging around it unreadable.
#[test]
fn deleting_a_recorded_outcome_does_not_make_staging_unreadable() {
    let (_dir, root) = workspace();
    let staging = item(&root, "outcomes outlive their targets", "unresolved");
    let object = new_object(&root, "the object");
    admit(&root, payload(Action::SectionAdded, &object, "section one"));

    admit_from(
        &root,
        payload(Action::SectionAdded, &object, "the outcome"),
        vec![gate::SourceRequest {
            item: staging.clone(),
            section: 1,
            produced: vec![Produced::object(format!("obj:{}:1", compact(&object)))],
            resolves: false,
        }],
    );
    admit(
        &root,
        payload(Action::SectionDeleted { section: 1 }, &object, ""),
    );
    assert!(ops::effective(&root, &object)
        .expect("object")
        .section(1)
        .is_err());

    let stored = backlog::load(&root, &staging).expect("staging still loads");
    assert_eq!(stored.sections[0].produced.len(), 1);
}

#[test]
fn produced_outcomes_accumulate_without_resolving_anything() {
    let (_dir, root) = workspace();
    let staging = item(&root, "long running", "several things are unsettled here");

    // Zero outcomes is the ordinary state.
    assert!(backlog::load(&root, &staging).expect("load").sections[0]
        .produced
        .is_empty());

    for text in ["first outcome", "second outcome", "third outcome"] {
        let object = new_object(&root, text);
        let outcomes = admit_from(
            &root,
            payload(Action::SectionAdded, &object, text),
            vec![gate::SourceRequest {
                item: staging.clone(),
                section: 1,
                produced: vec![Produced::object(format!("obj:{}:1", compact(&object)))],
                resolves: false,
            }],
        );
        assert!(matches!(
            outcomes[0].result,
            Reconciliation::Recorded { added: 1 }
        ));
    }

    let stored = backlog::load(&root, &staging).expect("still unresolved");
    assert_eq!(
        stored.sections[0].produced.len(),
        3,
        "one unresolved point may produce several confirmed outcomes"
    );
    assert!(
        backlog::item_path(&root, &staging).exists(),
        "produced.length > 0 is not resolution"
    );
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
        backlog::merge_sections(&root, &staging, &[1, 2], "one point", Vec::new()).expect("merge");
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
fn an_unchanged_source_declared_resolved_is_consumed() {
    let (_dir, root) = workspace();
    let staging = item(
        &root,
        "settled by one change",
        "should sibling refs be allowed?",
    );
    let object = new_object(&root, "sibling refs");

    let outcomes = admit_from(
        &root,
        payload(
            Action::SectionAdded,
            &object,
            "same-object sibling refs are allowed; direct self-reference is not",
        ),
        vec![gate::SourceRequest {
            item: staging.clone(),
            section: 1,
            produced: vec![Produced::object(format!("obj:{}:1", compact(&object)))],
            resolves: true,
        }],
    );
    assert!(matches!(
        outcomes[0].result,
        Reconciliation::Consumed { item_removed: true }
    ));
    assert!(
        !backlog::item_path(&root, &staging).exists(),
        "consuming the last unresolved point removes the topic"
    );
}

#[test]
fn consuming_one_point_leaves_the_others_unresolved() {
    let (_dir, root) = workspace();
    let staging = item(&root, "two points", "first");
    backlog::add_section(&root, &staging, "second", Vec::new()).expect("add");
    let object = new_object(&root, "outcome");

    let outcomes = admit_from(
        &root,
        payload(Action::SectionAdded, &object, "settles the first point"),
        vec![gate::SourceRequest {
            item: staging.clone(),
            section: 1,
            produced: Vec::new(),
            resolves: true,
        }],
    );
    assert!(matches!(
        outcomes[0].result,
        Reconciliation::Consumed {
            item_removed: false
        }
    ));
    let stored = backlog::load(&root, &staging).expect("the topic survives");
    assert_eq!(
        stored.sections.iter().map(|s| s.id).collect::<Vec<_>>(),
        vec![2]
    );
}

#[test]
fn a_source_edited_after_prepare_is_left_entirely_alone() {
    let (_dir, root) = workspace();
    let staging = item(&root, "moving target", "the original unresolved wording");
    let object = new_object(&root, "outcome");

    for resolves in [false, true] {
        let prepared = gate::prepare_from_backlog(
            &root,
            payload(
                Action::SectionAdded,
                &object,
                &format!("outcome {resolves}"),
            ),
            vec![gate::SourceRequest {
                item: staging.clone(),
                section: 1,
                produced: vec![Produced::object(format!("obj:{}", compact(&object)))],
                resolves,
            }],
        )
        .expect("prepare");

        // Somebody sharpens the unresolved point while the human is reading.
        backlog::revise_section(&root, &staging, 1, &format!("sharpened {resolves}"))
            .expect("revise after prepare");

        let admitted = gate::confirm(&root, &format!("CONFIRM {}", prepared.candidate.challenge))
            .expect("the object change a human confirmed still enters the record");
        assert_eq!(
            admitted.object.sections.last().expect("section").text,
            format!("outcome {resolves}"),
            "a stale backlog source must never invalidate confirmed wording"
        );
        assert!(matches!(
            admitted.backlog[0].result,
            Reconciliation::SourceChanged
        ));
        assert!(admitted.backlog[0].needs_attention());

        let stored = backlog::load(&root, &staging).expect("the newer point is untouched");
        assert_eq!(stored.sections[0].text, format!("sharpened {resolves}"));
        assert!(
            stored.sections[0].produced.is_empty(),
            "an old candidate must not attach outcomes to newer unresolved wording"
        );
    }
}

#[test]
fn a_backlog_edit_racing_confirmation_cannot_consume_the_newer_wording() {
    use fs2::FileExt;
    use std::sync::{Arc, Barrier};

    let (_dir, root) = workspace();
    let staging = item(&root, "raced", "the wording the candidate compared against");
    let object = new_object(&root, "outcome");
    let prepared = gate::prepare_from_backlog(
        &root,
        payload(Action::SectionAdded, &object, "confirmed outcome"),
        vec![gate::SourceRequest {
            item: staging.clone(),
            section: 1,
            produced: Vec::new(),
            resolves: true,
        }],
    )
    .expect("prepare");
    let response = format!("CONFIRM {}", prepared.candidate.challenge);

    // Hold the writer lock so neither side can start, then release it and let
    // the two contend for the moment between the basis check and the write.
    let lock = std::fs::OpenOptions::new()
        .write(true)
        .open(store::engr_dir(&root).join("lock"))
        .expect("open lock");
    lock.lock_exclusive().expect("hold the lock");

    let start = Arc::new(Barrier::new(3));
    let admitted = std::thread::scope(|scope| {
        let confirming = Arc::clone(&start);
        let editing = Arc::clone(&start);
        let confirm_root = &root;
        let edit_root = &root;
        let edited = staging.clone();
        let confirm = scope.spawn(move || {
            confirming.wait();
            gate::confirm(confirm_root, &response)
        });
        let edit = scope.spawn(move || {
            editing.wait();
            backlog::revise_section(edit_root, &edited, 1, "sharpened while they were reading")
        });
        start.wait();
        std::thread::sleep(std::time::Duration::from_millis(100));
        FileExt::unlock(&lock).expect("release");
        let admitted = confirm
            .join()
            .expect("confirm thread")
            .expect("the confirmed object change admits either way");
        let _ = edit.join().expect("edit thread");
        admitted
    });
    assert_eq!(
        admitted.object.rev, 2,
        "the record mutation is never the loser"
    );

    // Either order is safe, and each has exactly one legal shape. What must
    // never happen is the newer wording being consumed by a candidate that
    // compared against the older one.
    match backlog::load(&root, &staging) {
        Ok(stored) => {
            assert_eq!(
                stored.sections[0].text, "sharpened while they were reading",
                "the edit won, so the newer wording must be what survives"
            );
            assert!(
                matches!(admitted.backlog[0].result, Reconciliation::SourceChanged),
                "and consumption must have refused, not silently deleted it"
            );
        }
        Err(error) => {
            assert_eq!(error.code, engr::EXIT_NOT_FOUND);
            assert!(
                matches!(
                    admitted.backlog[0].result,
                    Reconciliation::Consumed { item_removed: true }
                ),
                "the item may only be gone because consumption compared and matched"
            );
        }
    }
}

#[test]
fn reconciliation_is_not_repeated_by_an_idempotent_retry() {
    let (_dir, root) = workspace();
    let staging = item(&root, "retried", "still unresolved after this");
    let consumed = item(&root, "retired", "settled by this");
    let object = new_object(&root, "outcome");
    let outcome = Produced::object(format!("obj:{}", compact(&object)));

    let prepared = gate::prepare_from_backlog(
        &root,
        payload(Action::SectionAdded, &object, "the confirmed outcome"),
        vec![
            gate::SourceRequest {
                item: staging.clone(),
                section: 1,
                produced: vec![outcome.clone()],
                resolves: false,
            },
            gate::SourceRequest {
                item: consumed.clone(),
                section: 1,
                produced: Vec::new(),
                resolves: true,
            },
        ],
    )
    .expect("prepare");
    let code = prepared.candidate.challenge.clone();
    let response = format!("CONFIRM {code}");
    let first = gate::confirm(&root, &response).expect("confirm");
    assert!(matches!(
        first.backlog[0].result,
        Reconciliation::Recorded { added: 1 }
    ));
    assert!(matches!(
        first.backlog[1].result,
        Reconciliation::Consumed { item_removed: true }
    ));

    // Reinstate the candidate, as a crash between saving the projection and
    // deleting the file would.
    store::write_json(
        &store::candidate_path(&root, &code).expect("path"),
        &prepared.candidate,
    )
    .expect("restore the candidate");
    let retry = gate::confirm(&root, &response).expect("the retry is idempotent");
    assert_eq!(retry.object.rev, 2, "the event is not applied twice");
    assert_eq!(store::load_events(&root, &object).expect("events").len(), 2);
    assert!(
        matches!(
            retry.backlog[0].result,
            Reconciliation::Recorded { added: 0 }
        ),
        "appending an outcome already listed is what makes the retry safe"
    );
    assert!(matches!(
        retry.backlog[1].result,
        Reconciliation::SourceGone
    ));
    assert_eq!(
        backlog::load(&root, &staging).expect("load").sections[0].produced,
        vec![outcome],
        "the retry must not duplicate produced[]"
    );
    assert!(!backlog::item_path(&root, &consumed).exists());
}

#[test]
fn a_candidate_cannot_attach_outcomes_that_were_not_declared() {
    let (_dir, root) = workspace();
    let staging = item(&root, "undeclared", "unresolved");
    let object = new_object(&root, "changed for its own reasons");

    // An Object changing is not evidence that any unresolved point was worked
    // on. Without a declaration, confirmation touches nothing.
    let admitted = gate::prepare(&root, payload(Action::SectionAdded, &object, "unrelated"))
        .and_then(|prepared| {
            gate::confirm(&root, &format!("CONFIRM {}", prepared.candidate.challenge))
        })
        .expect("confirm");
    assert!(admitted.backlog.is_empty());
    assert!(backlog::load(&root, &staging).expect("load").sections[0]
        .produced
        .is_empty());

    // And a declaration names exactly one source once.
    let error = gate::prepare_from_backlog(
        &root,
        payload(Action::SectionAdded, &object, "declared twice"),
        vec![
            gate::SourceRequest {
                item: staging.clone(),
                section: 1,
                produced: Vec::new(),
                resolves: false,
            },
            gate::SourceRequest {
                item: staging.clone(),
                section: 1,
                produced: Vec::new(),
                resolves: true,
            },
        ],
    )
    .expect_err("one source cannot be declared twice");
    assert_eq!(error.code, engr::EXIT_USAGE);
}

#[test]
fn preparing_against_a_source_that_does_not_exist_is_refused_up_front() {
    let (_dir, root) = workspace();
    let staging = item(&root, "one point", "unresolved");
    let object = new_object(&root, "outcome");

    let error = gate::prepare_from_backlog(
        &root,
        payload(Action::SectionAdded, &object, "outcome"),
        vec![gate::SourceRequest {
            item: staging.clone(),
            section: 9,
            produced: Vec::new(),
            resolves: true,
        }],
    )
    .expect_err("a candidate that cannot reconcile never reaches a human");
    assert_eq!(error.code, engr::EXIT_NOT_FOUND);

    let error = gate::prepare_from_backlog(
        &root,
        payload(Action::SectionAdded, &object, "outcome"),
        vec![gate::SourceRequest {
            item: staging,
            section: 1,
            produced: vec![Produced::object("collection:abcdefghjk")],
            resolves: false,
        }],
    )
    .expect_err("an outcome outside the record is refused before a code is minted");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(gate::pending(&root).expect("candidates").is_empty());
}

#[test]
fn the_declared_sources_are_covered_by_candidate_integrity() {
    let (_dir, root) = workspace();
    let staging = item(&root, "pinned", "unresolved");
    let object = new_object(&root, "outcome");
    let prepared = gate::prepare_from_backlog(
        &root,
        payload(Action::SectionAdded, &object, "outcome"),
        vec![gate::SourceRequest {
            item: staging.clone(),
            section: 1,
            produced: Vec::new(),
            resolves: false,
        }],
    )
    .expect("prepare");
    let code = prepared.candidate.challenge.clone();
    let path = store::candidate_path(&root, &code).expect("path");

    // Flip the one field that decides whether the source survives.
    let mut stored: serde_json::Value = store::read_json(&path).expect("candidate");
    stored["backlog"][0]["resolves"] = serde_json::json!(true);
    store::write_json(&path, &stored).expect("rewrite the candidate");

    let error = gate::confirm(&root, &format!("CONFIRM {code}"))
        .expect_err("reconciliation intent cannot be changed after prepare");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("integrity"));
    assert!(
        backlog::item_path(&root, &staging).exists(),
        "and the rewritten intent must not have been acted on"
    );

    // The declared outcome is inside it too.
    let mut stored: serde_json::Value = store::read_json(&path).expect("candidate");
    stored["backlog"][0]["resolves"] = serde_json::json!(false);
    stored["backlog"][0]["produced"] = serde_json::json!([{"target": {"kind": "engr", "ref": format!("obj:{}", compact(&object))}}]);
    store::write_json(&path, &stored).expect("rewrite the candidate");
    assert!(gate::confirm(&root, &format!("CONFIRM {code}")).is_err());
    assert!(backlog::load(&root, &staging).expect("load").sections[0]
        .produced
        .is_empty());
}

#[test]
fn backlog_is_not_part_of_the_authoritative_payload() {
    let (_dir, root) = workspace();
    let staging = item(&root, "source", "unresolved");
    let object = new_object(&root, "outcome");
    let admitted = {
        let prepared = gate::prepare_from_backlog(
            &root,
            payload(Action::SectionAdded, &object, "outcome"),
            vec![gate::SourceRequest {
                item: staging,
                section: 1,
                produced: Vec::new(),
                resolves: true,
            }],
        )
        .expect("prepare");
        gate::confirm(&root, &format!("CONFIRM {}", prepared.candidate.challenge)).expect("confirm")
    };

    let event = serde_json::to_value(&admitted.event).expect("event json");
    let event = event.as_object().expect("event object");
    assert!(
        !event.contains_key("backlog"),
        "no backlog_promoted event, and no backlog inside the confirmed one"
    );
    // The confirmed history is the same shape it was before Backlog existed.
    let reloaded = store::load_events(&root, &object).expect("events");
    assert_eq!(reloaded.len(), 2);
    assert_eq!(reloaded[1].payload, admitted.event.payload);
}

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
