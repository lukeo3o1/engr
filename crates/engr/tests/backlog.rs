//! What unresolved staging is, and what it is not.
//!
//! Backlog exists to hold work nobody has settled. These tests pin the two
//! things that would quietly destroy its value: staging becoming readable as
//! authority, and a candidate written against one unresolved point mutating a
//! different one.

mod common;

use common::workspace;
use engr::backlog::{self, Prepared, Produced, Subject};
use engr::{gate, ops, reference, store};
use std::path::Path;

/// Put an Object on disk without going through any write path.
///
/// Written as this generation's canonical bytes on purpose: a hand edit that
/// also changes the *spelling* is refused as schema before anything looks at a
/// seal, and every caller here is asking about the seal.
fn overwrite_object(root: &Path, object: &engr::model::Object) {
    std::fs::write(
        store::object_path(root, &object.id),
        engr::proof::canonical_bytes(object, "object").expect("canonical object"),
    )
    .expect("overwrite object outside the gate");
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

use common::{add, admit, new_object, wording};

fn compact(id: &str) -> String {
    reference::encode_uuid(uuid::Uuid::parse_str(id).expect("uuid"))
}

fn item(root: &Path, title: &str, text: &str) -> String {
    backlog::create(root, title, text, Vec::new(), &Prepared::first())
        .expect("create backlog item")
        .id
}

// Every existing-state backlog mutation carries the exact predecessor it was
// prepared against, whether or not a Rule governs the domain. These read the
// current state the same way a caller does before writing.
fn on_add(root: &Path, id: &str) -> Prepared {
    Prepared::first().against(backlog::Precondition::section_absent(root, id).expect("observe"))
}

fn on_title(root: &Path, id: &str) -> Prepared {
    Prepared::first().against(backlog::Precondition::title(root, id).expect("observe"))
}

fn on_section(root: &Path, id: &str, section: u64) -> Prepared {
    Prepared::first().against(backlog::Precondition::section(root, id, section).expect("observe"))
}

fn on_merge(root: &Path, id: &str, destination: u64, source: u64) -> Prepared {
    Prepared::first()
        .against(backlog::Precondition::merge(root, id, destination, source).expect("observe"))
}

#[test]
fn backlog_section_order_and_allocation_ceiling_are_current_boundaries() {
    let (_dir, root) = workspace();
    let id = item(&root, "ordering", "first");
    backlog::add_section(&root, &id, "second", Vec::new(), &on_add(&root, &id))
        .expect("second section");
    let path = backlog::item_path(&root, &id);
    let mut reversed: serde_json::Value = store::read_json(&path).expect("item");
    reversed["sections"]
        .as_array_mut()
        .expect("sections")
        .reverse();
    write_raw(&path, &reversed).expect("canonical reversed file");
    let error = backlog::load(&root, &id).expect_err("writer-only ordering is not accepted");
    assert_eq!(error.code, engr::EXIT_SCHEMA);

    reversed["sections"]
        .as_array_mut()
        .expect("sections")
        .reverse();
    reversed["next_section_id"] = serde_json::json!(engr::proof::MAX_SAFE_INTEGER);
    write_raw(&path, &reversed).expect("counter at the valid ceiling");
    let before = std::fs::read(&path).expect("before");
    let error = backlog::add_section(&root, &id, "one too far", Vec::new(), &on_add(&root, &id))
        .expect_err("allocation beyond the safe domain");
    assert_eq!(error.code, engr::EXIT_USAGE);
    assert!(error.message.contains("safe-integer"), "{error}");
    assert_eq!(
        std::fs::read(&path).expect("after"),
        before,
        "no write occurred"
    );
}

#[test]
fn a_current_backlog_item_refuses_an_explicit_empty_optional_collection() {
    let (_dir, root) = workspace();
    let id = item(&root, "one spelling", "unresolved");
    let path = backlog::item_path(&root, &id);
    let mut value: serde_json::Value = store::read_json(&path).expect("item");
    value["sections"][0]["produced"] = serde_json::json!([]);
    write_raw(&path, &value).expect("a JCS second spelling");

    let error = backlog::load(&root, &id).expect_err("the writer omits an empty produced list");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("exact shape"), "{error}");
}

#[test]
fn a_declared_current_workspace_is_not_downgraded_by_a_malformed_object() {
    let (_dir, root) = workspace();
    let object = new_object(&root, "schema authority");
    let id = item(&root, "current resource", "unresolved");

    let object_path = store::object_path(&root, &object);
    let mut malformed: serde_json::Value = store::read_json(&object_path).expect("object");
    let state = malformed
        .as_object_mut()
        .expect("object members")
        .remove("state")
        .expect("state");
    malformed["status"] = state;
    write_raw(&object_path, &malformed).expect("legacy spelling in declared v3");

    let path = backlog::item_path(&root, &id);
    let value: serde_json::Value = store::read_json(&path).expect("item");
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&value).expect("non-JCS item"),
    )
    .expect("write non-JCS item");

    let error = backlog::load(&root, &id).expect_err("the unrelated resource stays v3");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("canonical JCS"), "{error}");
    assert!(
        store::load_object(&root, &object).is_err(),
        "status is malformed current data"
    );

    // And the domain still works: one bad Object is one bad Object, not a
    // workspace that has to be migrated before anything else can be written.
    let fresh = backlog::create(&root, "topic", "unresolved", Vec::new(), &Prepared::first())
        .expect("an unrelated domain is not held hostage by it");
    assert_eq!(
        backlog::load(&root, &fresh.id).expect("load").title,
        "topic"
    );
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
    let second =
        backlog::add_section(&root, &id, "second", Vec::new(), &on_add(&root, &id)).expect("add");
    let third =
        backlog::add_section(&root, &id, "third", Vec::new(), &on_add(&root, &id)).expect("add");
    assert_eq!((second, third), (2, 3));

    assert!(!backlog::consume_section(&root, &id, 2, &on_section(&root, &id, 2)).expect("delete"));
    let fourth =
        backlog::add_section(&root, &id, "fourth", Vec::new(), &on_add(&root, &id)).expect("add");
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

    // Merging consumes ids like anything else: §1 goes, and the counter does not
    // hand it back. What merging does *not* do is mint a new one.
    backlog::merge_into(
        &root,
        &id,
        3,
        1,
        "one point",
        Vec::new(),
        &on_merge(&root, &id, 3, 1),
    )
    .expect("merge");
    let stored = backlog::load(&root, &id).expect("load");
    assert_eq!(
        stored.sections.iter().map(|s| s.id).collect::<Vec<_>>(),
        vec![3, 4],
        "the destination kept its id and the source's id is simply gone"
    );
    assert_eq!(
        stored.next_section_id, 5,
        "a merge allocates nothing, so it does not advance the counter"
    );
}

/// A merge does not create a third point; the destination survives as itself.
///
/// Minting a fresh id would silently orphan everything already pointing at the
/// destination — subjects, `produced[]` bookkeeping, a reviewer's own notes —
/// all naming a Section that stopped existing at the moment somebody said these
/// two were the same thing.
#[test]
fn a_merge_keeps_the_destination_identity_and_removes_the_source() {
    let (_dir, root) = workspace();
    let id = item(&root, "several concerns", "first");
    backlog::add_section(&root, &id, "second", Vec::new(), &on_add(&root, &id)).expect("add");
    backlog::add_section(&root, &id, "third", Vec::new(), &on_add(&root, &id)).expect("add");
    let before = backlog::load(&root, &id).expect("load");
    let untouched = before.section(1).expect("§1").updated_at.clone();

    backlog::merge_into(
        &root,
        &id,
        2,
        1,
        "one point",
        Vec::new(),
        &on_merge(&root, &id, 2, 1),
    )
    .expect("merge");

    let stored = backlog::load(&root, &id).expect("load");
    assert_eq!(
        stored.sections.iter().map(|s| s.id).collect::<Vec<_>>(),
        vec![2, 3],
        "the destination survives as itself, and only the named source is gone"
    );
    let merged = stored.section(2).expect("destination");
    assert_eq!(merged.text, "one point");
    assert!(
        merged.updated_at >= untouched,
        "the destination now says something it did not say before"
    );

    // The source id is consumed, not freed.
    let next =
        backlog::add_section(&root, &id, "later", Vec::new(), &on_add(&root, &id)).expect("add");
    assert_eq!(next, 4, "a merged-away id is never handed back out");
}

/// One destination and one source, and no more.
///
/// A merge is one judgement over one pair, with one predecessor for each half.
/// Consuming several unresolved identities in a single atomic apply is a
/// broader destructive operation with a different reviewed subject, so it is
/// not available at all — not as a convenience spelling of repeating the
/// operation, because repeating it is what makes each consumption its own
/// reviewed judgement against its own predecessor.
#[test]
fn a_merge_takes_exactly_one_source() {
    let (_dir, root) = workspace();
    let id = item(&root, "several concerns", "first");
    backlog::add_section(&root, &id, "second", Vec::new(), &on_add(&root, &id)).expect("add");
    backlog::add_section(&root, &id, "third", Vec::new(), &on_add(&root, &id)).expect("add");

    // The predecessor a merge binds is exactly the destination and its one
    // source. A reading that covers a third point is not what this merge rests
    // on, and binding it would be authorizing a wider consumption than the one
    // that was reviewed.
    let three = backlog::Precondition::combine(vec![
        backlog::Precondition::section(&root, &id, 2).expect("observe"),
        backlog::Precondition::section(&root, &id, 1).expect("observe"),
        backlog::Precondition::section(&root, &id, 3).expect("observe"),
    ])
    .expect("combine");
    let error = backlog::merge_into(
        &root,
        &id,
        2,
        1,
        "one point",
        Vec::new(),
        &Prepared::first().against(three),
    )
    .expect_err("a merge binds one pair");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert_eq!(
        backlog::load(&root, &id)
            .expect("load")
            .sections
            .iter()
            .map(|s| s.id)
            .collect::<Vec<_>>(),
        vec![1, 2, 3],
        "a refused merge leaves the item exactly as it was"
    );
}

/// Every participant is checked before anything moves.
#[test]
fn a_merge_naming_a_point_that_is_not_there_changes_nothing() {
    let (_dir, root) = workspace();
    let id = item(&root, "topic", "first");
    backlog::add_section(&root, &id, "second", Vec::new(), &on_add(&root, &id)).expect("add");

    for (name, destination, source) in [
        ("an absent source", 1u64, 9u64),
        ("an absent destination", 9, 1),
    ] {
        // The predecessor cannot be read for a point that is not there, so the
        // caller binds the half that exists. The mutation still refuses.
        let present = if destination == 9 {
            source
        } else {
            destination
        };
        assert!(
            backlog::merge_into(
                &root,
                &id,
                destination,
                source,
                "one point",
                Vec::new(),
                &on_section(&root, &id, present)
            )
            .is_err(),
            "{name} should refuse"
        );
        let stored = backlog::load(&root, &id).expect("load");
        assert_eq!(
            stored.sections.iter().map(|s| s.id).collect::<Vec<_>>(),
            vec![1, 2],
            "{name}: a refused merge leaves the item exactly as it was"
        );
    }

    // A point cannot be merged into itself: that is not a judgement about two
    // points, and taken literally it would remove the destination.
    let error = backlog::merge_into(
        &root,
        &id,
        1,
        1,
        "one point",
        Vec::new(),
        &on_section(&root, &id, 1),
    )
    .expect_err("self-merge is not a merge");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
}

#[test]
fn a_stored_item_with_duplicate_or_impossible_section_ids_is_refused() {
    let (_dir, root) = workspace();
    let id = item(&root, "topic", "first");
    backlog::add_section(&root, &id, "second", Vec::new(), &on_add(&root, &id)).expect("add");
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
        write_raw(&path, &value).expect("write corrupt item");
        let error = backlog::load(&root, &id).expect_err(name);
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{name}");
    }

    let mut value = pristine;
    value["id"] = serde_json::json!(engr::model::new_id());
    write_raw(&path, &value).expect("write mismatched id");
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
            Box::new(|value: &mut serde_json::Value| value["title"] = serde_json::json!("   "))
                as Box<dyn Fn(&mut serde_json::Value)>,
        ),
        (
            "a topic spanning lines",
            Box::new(|value: &mut serde_json::Value| {
                value["title"] = serde_json::json!("a topic\nwith a body under it")
            }),
        ),
        (
            "a topic that is really a body",
            Box::new(|value: &mut serde_json::Value| {
                value["title"] = serde_json::json!("x".repeat(121))
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
        write_raw(&path, &value).expect("write corrupt item");
        let error = backlog::load(&root, &id).expect_err(name);
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{name}");
    }

    // A topic exactly at the limit is fine; the boundary is not off by one.
    let mut value = pristine;
    value["title"] = serde_json::json!("x".repeat(120));
    write_raw(&path, &value).expect("write");
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
        &Prepared::first(),
    )
    .expect("create")
    .id;
    let path = backlog::item_path(&root, &id);

    // Seed an outcome too, so every persisted shape below is one this test
    // actually reaches rather than one it assumes exists.
    let mut pristine: serde_json::Value = store::read_json(&path).expect("item");
    pristine["sections"][0]["produced"] =
        serde_json::json!([{ "target": { "kind": "engr", "ref": reference } }]);
    write_raw(&path, &pristine).expect("write");
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
        write_raw(&path, &value).expect("write corrupt item");
        let error = backlog::load(&root, &id).expect_err(name);
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{name}");
    }
}

#[test]
fn removing_the_last_unresolved_point_removes_the_item() {
    let (_dir, root) = workspace();
    let id = item(&root, "one point only", "the only unresolved thing");
    assert!(
        backlog::consume_section(&root, &id, 1, &on_section(&root, &id, 1)).expect("delete"),
        "an item is a topic that still has unresolved work in it"
    );
    assert!(!backlog::item_path(&root, &id).exists());
    assert!(backlog::ids(&root).expect("ids").is_empty());
}

#[test]
fn backlog_keeps_no_event_log_and_needs_no_confirmation() {
    let (_dir, root) = workspace();
    let id = item(&root, "agent editable", "unresolved");
    backlog::revise_section(
        &root,
        &id,
        1,
        "reworded without asking anyone",
        &on_section(&root, &id, 1),
    )
    .expect("revise");

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
fn a_backlog_write_is_never_seen_half_finished() {
    let (_dir, root) = workspace();
    let id = item(&root, "atomic", "first");
    backlog::add_section(&root, &id, "second", Vec::new(), &on_add(&root, &id)).expect("add");
    let path = backlog::item_path(&root, &id);
    let text = std::fs::read_to_string(&path).expect("read");
    serde_json::from_str::<serde_json::Value>(&text).expect("a complete JSON document");
    assert!(
        !path.with_extension("json.tmp").exists(),
        "the temporary file is renamed into place, never left behind"
    );
}

/// Two concurrent mutations of one item serialize, and the loser is told.
///
/// The lock makes them sequential; the exact predecessor decides what the
/// second one is then allowed to do. It prepared against an allocation state
/// that has since moved, so it is refused rather than landing on top of a point
/// nobody read — and the id it would have received is not handed out twice.
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
        // Both read the same allocation state before either writes, which is
        // exactly the race the predecessor exists to name.
        let first_prepared = on_add(&root, &id);
        let second_prepared = on_add(&root, &id);
        let first = scope.spawn(move || {
            first_start.wait();
            backlog::add_section(first_root, &first_id, "second", Vec::new(), &first_prepared)
        });
        let second = scope.spawn(move || {
            second_start.wait();
            backlog::add_section(
                second_root,
                &second_id,
                "third",
                Vec::new(),
                &second_prepared,
            )
        });
        (first.join().expect("first"), second.join().expect("second"))
    });
    let outcomes = [first, second];
    let landed: Vec<u64> = outcomes
        .iter()
        .filter_map(|o| o.as_ref().ok().copied())
        .collect();
    assert_eq!(
        landed.len(),
        1,
        "one write lands; the other prepared against an allocation state that moved"
    );
    let refused = outcomes
        .iter()
        .find_map(|o| o.as_ref().err())
        .expect("one refusal");
    assert_eq!(refused.code, engr::EXIT_STALE);

    let stored = backlog::load(&root, &id).expect("load");
    assert_eq!(stored.sections.len(), 2, "the refused write left nothing");
    assert_eq!(stored.next_section_id, 3);

    // Reading again and re-preparing is the whole remedy, and the id the
    // refused write would have taken is not handed out twice.
    let retried =
        backlog::add_section(&root, &id, "third", Vec::new(), &on_add(&root, &id)).expect("retry");
    assert_ne!(retried, landed[0], "no id is handed out twice");
    assert_eq!(retried, 3);
}

// ---------------------------------------------------------------------------
// Subjects
// ---------------------------------------------------------------------------

#[test]
fn a_subject_may_name_an_object_a_section_or_another_unresolved_point() {
    let (_dir, root) = workspace();
    let object = new_object(&root, "authentication");
    admit(&root, add(&object, wording("refresh tokens rotate")));
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
        &Prepared::first(),
    )
    .expect("create")
    .id;
    // The same four subjects, in the one order a set is persisted in.
    let mut canonical = subjects.clone();
    engr::proof::canonical_set(&mut canonical, "subject").expect("canonical");
    assert_eq!(
        backlog::load(&root, &id).expect("load").sections[0].subjects,
        canonical
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
        &Prepared::first(),
    )
    .expect("create")
    .id;
    backlog::set_subjects(
        &root,
        &first,
        1,
        vec![Subject::engr(format!("backlog:{}:1", compact(&second)))],
        &on_section(&root, &first, 1),
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
            &Prepared::first(),
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
        &Prepared::first(),
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
        &Prepared::first(),
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
        &Prepared::first(),
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
    let mut content = wording("stands on something");
    content.refs = vec![engr::dependency::SelectiveRef::stored(
        engr::proof::section_target(&staging, 1).expect("section target"),
        vec![engr::dependency::SemanticField::Text],
        engr::git::head(&root).expect("HEAD"),
        format!("1:{}", "0".repeat(64)),
    )
    .expect("a well formed reference at a staging id")];
    let proposal = add(&object, content);
    let error = gate::prepare(&root, proposal).expect_err("a record ref cannot target backlog");
    assert_eq!(error.code, engr::EXIT_NOT_FOUND);
    assert!(error.message.contains("does not exist"));
}

// ---------------------------------------------------------------------------
// The resolution basis
// ---------------------------------------------------------------------------

/// Write an item with exact timestamps, which `create` cannot: it uses the
/// clock, and these tests are about values the clock never produces.
fn staged(root: &Path, id: &str, title: &str, sections: serde_json::Value) {
    let item = serde_json::json!({
        "id": id,
        "title": title,
        "next_section_id": sections.as_array().expect("sections").len() + 1,
        "sections": sections,
    });
    write_raw(&backlog::item_path(root, id), &item).expect("stage");
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
            {"id": 1, "text": "first", "updated_at": "2026-08-17T01:00:00+08:00"},
            {"id": 2, "text": "second", "updated_at": "2026-08-16T20:00:00Z"},
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
            {"id": 1, "text": "point", "updated_at": "2026-08-17T10:00:00.123456+08:00"},
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
    let id = backlog::create(
        &root,
        "topic",
        "unresolved",
        subjects.clone(),
        &Prepared::first(),
    )
    .expect("create")
    .id;
    let before = backlog::load(&root, &id).expect("load");
    let activity = before.sections[0].updated_at.clone();

    std::thread::sleep(std::time::Duration::from_millis(1100));
    backlog::set_subjects(
        &root,
        &id,
        1,
        subjects.iter().rev().cloned().collect(),
        &on_section(&root, &id, 1),
    )
    .expect("reorder");
    let after = backlog::load(&root, &id).expect("load");
    assert_eq!(
        after.sections[0].updated_at, activity,
        "so it cannot look freshly worked on"
    );
    let mut canonical = subjects.clone();
    engr::proof::canonical_set(&mut canonical, "subject").expect("canonical");
    assert_eq!(
        after.sections[0].subjects, canonical,
        "a set has one persisted order, and writing it the other way round reaches it"
    );

    // Rewriting the identical text is not work either.
    backlog::revise_section(&root, &id, 1, "unresolved", &on_section(&root, &id, 1))
        .expect("idempotent write");
    assert_eq!(
        backlog::load(&root, &id).expect("load").sections[0].updated_at,
        activity
    );

    // A real change to either does advance it.
    backlog::set_subjects(
        &root,
        &id,
        1,
        vec![subjects[0].clone()],
        &on_section(&root, &id, 1),
    )
    .expect("drop one");
    let changed = backlog::load(&root, &id).expect("load").sections[0]
        .updated_at
        .clone();
    assert_ne!(changed, activity);

    std::thread::sleep(std::time::Duration::from_millis(1100));
    backlog::revise_section(&root, &id, 1, "reworded", &on_section(&root, &id, 1)).expect("revise");
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
    backlog::rename(&root, &id, "after", &on_title(&root, &id)).expect("rename");
    let after = backlog::load(&root, &id).expect("load");
    assert_eq!(after.title, "after");
    assert_eq!(
        after.sections[0].updated_at, before,
        "renaming a topic is not activity on any unresolved point"
    );

    backlog::revise_section(&root, &id, 1, "reworded", &on_section(&root, &id, 1)).expect("revise");
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
    backlog::add_section(
        &root,
        &staging,
        "second half",
        Vec::new(),
        &on_add(&root, &staging),
    )
    .expect("add");

    let mut stored = backlog::load(&root, &staging).expect("load");
    stored.sections[0].produced = vec![Produced::object(format!("obj:{object}:1"))];
    stored.sections[1].produced = vec![
        Produced::object(format!("obj:{object}:1")),
        Produced::object(format!("obj:{object}:2")),
    ];
    write_raw(&backlog::item_path(&root, &staging), &stored).expect("seed outcomes");

    backlog::merge_into(
        &root,
        &staging,
        1,
        2,
        "one point",
        Vec::new(),
        &on_merge(&root, &staging, 1, 2),
    )
    .expect("merge");
    let stored = backlog::load(&root, &staging).expect("load");
    let section = stored.section(1).expect("destination");
    assert_eq!(
        section.produced,
        vec![
            Produced::object(format!("obj:{object}:1")),
            Produced::object(format!("obj:{object}:2")),
        ],
        "the union of both, deduplicated: the outcomes happened, and merging says \
         the points were one, not that they did not"
    );
}

// ---------------------------------------------------------------------------
// Confirmation-time reconciliation
// ---------------------------------------------------------------------------

#[test]
fn effective_authority_is_unchanged_by_anything_in_staging() {
    let (_dir, root) = workspace();
    let object = new_object(&root, "record");
    admit(&root, add(&object, wording("confirmed wording")));
    let before = ops::effective(&root, &object).expect("effective");

    backlog::create(
        &root,
        "reconsider",
        "the confirmed wording may be wrong",
        vec![Subject::engr(format!("obj:{}:1", compact(&object)))],
        &Prepared::first(),
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

/// `dirty` is not target identity, but it is persisted observation state.
/// Re-observing the same target with changed metadata is activity because the
/// current Backlog bytes and the review bookkeeping both changed.
#[test]
fn changing_the_dirty_observation_is_activity() {
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

    let id = backlog::create(
        &root,
        "topic",
        "unresolved",
        vec![clean],
        &Prepared::first(),
    )
    .expect("create")
    .id;
    let before = backlog::load(&root, &id).expect("load").sections[0]
        .updated_at
        .clone();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    backlog::set_subjects(
        &root,
        &id,
        1,
        vec![observed_dirty],
        &on_section(&root, &id, 1),
    )
    .expect("re-observe");
    assert_ne!(
        backlog::load(&root, &id).expect("load").sections[0].updated_at,
        before,
        "changed persisted observation state is activity"
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
    assert!(
        backlog::record_produced(&root, &id, 1, outcome.clone(), &on_section(&root, &id, 1))
            .expect("record")
    );
    assert_eq!(
        backlog::load(&root, &id).expect("load").sections[0].produced,
        vec![outcome.clone()]
    );

    // A set: claiming the same outcome twice is not an error and not a
    // duplicate, so a retried command is harmless.
    assert!(
        !backlog::record_produced(&root, &id, 1, outcome.clone(), &on_section(&root, &id, 1))
            .expect("again")
    );
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
    assert!(
        backlog::forget_produced(&root, &id, 1, &outcome, &on_section(&root, &id, 1))
            .expect("forget")
    );
    assert!(
        !backlog::forget_produced(&root, &id, 1, &outcome, &on_section(&root, &id, 1))
            .expect("idempotent")
    );
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
    let error = backlog::record_produced(&root, &id, 1, missing, &on_section(&root, &id, 1))
        .expect_err("no such object");
    assert!(
        error.message.contains("does not exist"),
        "{}",
        error.message
    );

    // Nor a section the object does not have.
    let no_section = Produced::object(format!("obj:{}:9", compact(&object)));
    backlog::record_produced(&root, &id, 1, no_section, &on_section(&root, &id, 1))
        .expect_err("no such section");

    // A real one is accepted, then the staging around it survives the target
    // going away — loading must not depend on a recorded outcome still
    // resolving.
    let outcome = Produced::object(format!("obj:{}", compact(&object)));
    backlog::record_produced(&root, &id, 1, outcome.clone(), &on_section(&root, &id, 1))
        .expect("record");
    std::fs::remove_file(engr::store::object_path(&root, &object)).expect("delete the object");
    std::fs::remove_file(engr::store::events_path(&root, &object)).expect("delete its events");
    let loaded = backlog::load(&root, &id).expect("staging still loads");
    assert_eq!(loaded.sections[0].produced, vec![outcome.clone()]);

    // And the bookkeeping is still correctable with the target gone, which is
    // exactly when a mistaken entry is hardest to live with.
    assert!(
        backlog::forget_produced(&root, &id, 1, &outcome, &on_section(&root, &id, 1))
            .expect("forget")
    );
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
    backlog::add_section(&root, &id, "second point", Vec::new(), &on_add(&root, &id)).expect("add");

    let bound = backlog::Precondition::section(&root, &id, 1).expect("observe");
    bound.still_holds(&root).expect("nothing has moved yet");

    // An unrelated sibling moving does not stale it: this mutation never read
    // §2, so §2 changing says nothing about whether it is still applicable.
    backlog::revise_section(
        &root,
        &id,
        2,
        "second point, sharpened",
        &on_section(&root, &id, 2),
    )
    .expect("revise sibling");
    bound
        .still_holds(&root)
        .expect("a sibling is not part of what this rests on");

    // The bound Section moving does.
    backlog::revise_section(
        &root,
        &id,
        1,
        "first point, sharpened",
        &on_section(&root, &id, 1),
    )
    .expect("revise target");
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
                    &on_section(&root, &id, 1),
                )
                .expect("subjects");
            }
            _ => {
                backlog::record_produced(
                    &root,
                    &id,
                    1,
                    Produced::object(format!("obj:{}", compact(&object))),
                    &on_section(&root, &id, 1),
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

/// The title is context every Section is read in, so it is bound with them.
#[test]
fn a_title_change_stales_a_section_mutation_prepared_under_it() {
    let (_dir, root) = workspace();
    let id = item(&root, "original topic", "unresolved");
    let bound = backlog::Precondition::section(&root, &id, 1).expect("observe");
    backlog::rename(&root, &id, "a different topic", &on_title(&root, &id)).expect("rename");
    let error = bound.still_holds(&root).expect_err("the context moved");
    assert!(error.message.contains("title"), "{}", error.message);
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

    backlog::revise_section(
        &root,
        &id,
        1,
        "first, sharpened",
        &on_section(&root, &id, 1),
    )
    .expect("revise sibling");
    bound
        .still_holds(&root)
        .expect("a sibling is not part of an add");

    // Somebody else takes the id this add was going to use.
    backlog::add_section(
        &root,
        &id,
        "someone else got there",
        Vec::new(),
        &on_add(&root, &id),
    )
    .expect("race");
    let error = bound.still_holds(&root).expect_err("the id was taken");
    assert_eq!(error.code, engr::EXIT_STALE);
}

/// A topic change binds the complete item, because it scopes every point.
#[test]
fn a_topic_mutation_binds_the_whole_item() {
    let (_dir, root) = workspace();
    let id = item(&root, "topic", "first");
    let bound = backlog::Precondition::title(&root, &id).expect("observe");
    bound.still_holds(&root).expect("unchanged");

    backlog::add_section(
        &root,
        &id,
        "a second point",
        Vec::new(),
        &on_add(&root, &id),
    )
    .expect("add");
    let error = bound
        .still_holds(&root)
        .expect_err("renaming is about all of them, so any of them moving matters");
    assert_eq!(error.code, engr::EXIT_STALE);
}

/// A merge binds the topic and every point it touches — destination included.
///
/// The destination is not a bystander that merely receives: it is being
/// rewritten, and the judgement that these were one point was made about what it
/// said at the time. A sibling nobody named is still exempt, for the same reason
/// it is exempt everywhere else.
#[test]
fn a_merge_binds_the_topic_and_both_ends_but_not_an_unnamed_sibling() {
    let (_dir, root) = workspace();
    let id = item(&root, "topic", "destination");
    backlog::add_section(&root, &id, "source", Vec::new(), &on_add(&root, &id)).expect("add");
    backlog::add_section(&root, &id, "bystander", Vec::new(), &on_add(&root, &id)).expect("add");

    let bound = backlog::Precondition::merge(&root, &id, 1, 2).expect("observe");
    bound.still_holds(&root).expect("nothing has moved yet");

    backlog::revise_section(
        &root,
        &id,
        3,
        "bystander, sharpened",
        &on_section(&root, &id, 3),
    )
    .expect("revise sibling");
    bound
        .still_holds(&root)
        .expect("a point this merge never read cannot stale it");

    for (name, moved) in [("the destination", 1u64), ("the source", 2)] {
        let bound = backlog::Precondition::merge(&root, &id, 1, 2).expect("observe");
        backlog::revise_section(
            &root,
            &id,
            moved,
            &format!("{name}, sharpened"),
            &on_section(&root, &id, moved),
        )
        .expect("revise");
        let error = bound.still_holds(&root).err();
        let error = error.unwrap_or_else(|| panic!("{name} moving must stale the merge"));
        assert_eq!(error.code, engr::EXIT_STALE, "{name}");
    }

    // The topic scopes every point under it, so it is bound too.
    let bound = backlog::Precondition::merge(&root, &id, 1, 2).expect("observe");
    backlog::rename(&root, &id, "a different topic", &on_title(&root, &id)).expect("rename");
    let error = bound.still_holds(&root).expect_err("the context moved");
    assert!(error.message.contains("title"), "{}", error.message);
}

// ---------------------------------------------------------------------------
// Rule Review
// ---------------------------------------------------------------------------

fn rule(root: &Path, name: &str, review: &str) {
    let dir = engr::rules::dir(root);
    std::fs::create_dir_all(&dir).expect("rules dir");
    std::fs::write(
        dir.join(format!("{name}.md")),
        format!("---\nid: {name}\napplies:\n  domains:\n    - backlog\n{review}---\n\n# {name}\n\nSomething to check against.\n"),
    )
    .expect("write rule");
}

fn marker(root: &Path, id: &str, section: u64) -> Option<engr::rules::RuleReview> {
    backlog::load(root, id)
        .expect("load")
        .section(section)
        .expect("section")
        .review_exhaustion
}

fn attempt(value: u32) -> Prepared {
    Prepared::attempt(engr::rules::Attempt::new(value).expect("attempt"))
}

/// A governed mutation of one point, carrying the predecessor it rests on.
///
/// Under a rule every mutation must say what it was reviewed against, so these
/// read the current point and bind it — which is what an agent does between
/// reading and writing.
fn reviewing(root: &Path, id: &str, section: u64, value: u32) -> Prepared {
    attempt(value).against(backlog::Precondition::section(root, id, section).expect("observe"))
}

/// An exhausted Backlog mutation goes in anyway, and says so.
///
/// This is the whole reason the domain exists. An agent that has been round a
/// self-review more times than a project rule allows still knows something is
/// unresolved, and refusing to let it write that down sends the thought
/// nowhere — there is no other place for it, which is exactly what makes
/// blocking here different from blocking at the record's gate. So the mutation
/// is admitted and marked, and the marker is what stops the soft admission from
/// being silent.
#[test]
fn an_exhausted_point_is_still_written_down_and_carries_the_diagnostic() {
    let (_dir, root) = workspace();
    rule(&root, "careful", "review:\n  max_attempts: 2\n");
    let id = item(&root, "topic", "unresolved");

    backlog::revise_section(
        &root,
        &id,
        1,
        "within the ceiling",
        &reviewing(&root, &id, 1, 2),
    )
    .expect("revise");
    assert_eq!(
        marker(&root, &id, 1),
        None,
        "attempt 2 of 2 is still a review, so there is nothing to diagnose"
    );

    backlog::revise_section(
        &root,
        &id,
        1,
        "past the ceiling",
        &reviewing(&root, &id, 1, 3),
    )
    .expect("soft-admit");
    assert_eq!(
        backlog::load(&root, &id)
            .expect("load")
            .section(1)
            .expect("section")
            .text,
        "past the ceiling",
        "the point an agent could not get reviewed is the one it most needs kept"
    );
    assert_eq!(
        marker(&root, &id, 1),
        Some(engr::rules::RuleReview {
            attempts: 3,
            limit: 2
        })
    );
}

/// `limit` is the earliest ceiling in the applicable set.
///
/// One attempt is compared against every rule's own ceiling, so the smallest is
/// the one that made this exhausted — and it is the number worth recording,
/// because it is the one an agent has to get under.
#[test]
fn the_recorded_limit_is_the_ceiling_that_ran_out_first() {
    let (_dir, root) = workspace();
    rule(&root, "lenient", "review:\n  max_attempts: 9\n");
    rule(&root, "strict", "review:\n  max_attempts: 1\n");
    let id = item(&root, "topic", "unresolved");

    backlog::revise_section(&root, &id, 1, "reworded", &reviewing(&root, &id, 1, 4))
        .expect("soft-admit");
    assert_eq!(
        marker(&root, &id, 1),
        Some(engr::rules::RuleReview {
            attempts: 4,
            limit: 1
        }),
        "the compact diagnostic names the earliest ceiling, not a per-rule history"
    );
}

/// A rule asking for a human does not summon one for Backlog.
///
/// The Object domain escalates on exhaustion because admitting to the record is
/// a claim somebody has to stand behind. Staging claims nothing, and stopping to
/// ask about an unresolved note would cost a person's attention to protect a
/// statement that carries no authority in the first place.
#[test]
fn an_exhausted_backlog_rule_asking_for_a_human_still_does_not_get_one() {
    let (_dir, root) = workspace();
    rule(
        &root,
        "escalating",
        "review:\n  max_attempts: 1\n  on_exhaustion: human_confirmation\n",
    );
    let id = item(&root, "topic", "unresolved");

    backlog::add_section(
        &root,
        &id,
        "a second point",
        Vec::new(),
        &attempt(2).against(backlog::Precondition::section_absent(&root, &id).expect("observe")),
    )
    .expect("admitted, not escalated");
    assert_eq!(
        marker(&root, &id, 2),
        Some(engr::rules::RuleReview {
            attempts: 2,
            limit: 1
        })
    );
    assert!(
        engr::gate::pending(&root).expect("candidates").is_empty(),
        "no human was asked for anything"
    );
}

/// The marker describes the wording standing now, so a later write settles it.
#[test]
fn a_later_mutation_clears_the_marker_or_replaces_it() {
    let (_dir, root) = workspace();
    rule(&root, "careful", "review:\n  max_attempts: 2\n");
    let id = item(&root, "topic", "unresolved");

    backlog::revise_section(&root, &id, 1, "exhausted", &reviewing(&root, &id, 1, 5))
        .expect("soft-admit");
    assert_eq!(
        marker(&root, &id, 1),
        Some(engr::rules::RuleReview {
            attempts: 5,
            limit: 2
        })
    );

    // Exhausted again, differently: the diagnostic is replaced, not accumulated.
    backlog::revise_section(
        &root,
        &id,
        1,
        "exhausted again",
        &reviewing(&root, &id, 1, 9),
    )
    .expect("soft-admit");
    assert_eq!(
        marker(&root, &id, 1),
        Some(engr::rules::RuleReview {
            attempts: 9,
            limit: 2
        }),
        "one diagnostic about the current wording, never a series"
    );

    // And a review that passed says this wording did not need the excuse.
    backlog::revise_section(
        &root,
        &id,
        1,
        "reviewed properly",
        &reviewing(&root, &id, 1, 1),
    )
    .expect("revise");
    assert_eq!(marker(&root, &id, 1), None);
}

/// A write that changed nothing admitted nothing, so it settles nothing.
#[test]
fn an_idempotent_write_neither_earns_a_marker_nor_clears_one() {
    let (_dir, root) = workspace();
    rule(&root, "careful", "review:\n  max_attempts: 1\n");
    let id = item(&root, "topic", "unresolved");
    backlog::revise_section(
        &root,
        &id,
        1,
        "exhausted wording",
        &reviewing(&root, &id, 1, 2),
    )
    .expect("soft-admit");
    let marked = marker(&root, &id, 1);
    assert!(marked.is_some());

    backlog::revise_section(
        &root,
        &id,
        1,
        "exhausted wording",
        &reviewing(&root, &id, 1, 1),
    )
    .expect("no-op");
    assert_eq!(
        marker(&root, &id, 1),
        marked,
        "rewriting the same wording did not re-admit it under a passing review"
    );
}

/// Removal is the one thing an exhausted review does not buy.
///
/// Everywhere else Backlog prefers keeping the thought to losing it. Here there
/// would be nothing left to keep, so the exception cannot reach: the point stays
/// exactly as it was, and nothing is written — not even a marker, because no
/// mutation was admitted for one to describe.
#[test]
fn an_exhausted_review_does_not_get_to_remove_an_unresolved_point() {
    let (_dir, root) = workspace();
    rule(&root, "careful", "review:\n  max_attempts: 2\n");
    let id = item(&root, "topic", "unresolved");
    backlog::add_section(
        &root,
        &id,
        "a second point",
        Vec::new(),
        &attempt(1).against(backlog::Precondition::section_absent(&root, &id).expect("observe")),
    )
    .expect("add");
    let before = backlog::load(&root, &id).expect("load");

    let error = backlog::consume_section(&root, &id, 1, &reviewing(&root, &id, 1, 3))
        .expect_err("not on this one");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(
        error.message.contains("still here"),
        "the refusal says the work was not lost: {}",
        error.message
    );
    assert_eq!(
        backlog::load(&root, &id).expect("load"),
        before,
        "a refused consume writes nothing at all, marker included"
    );

    // A merge removes its source, so it is refused for the same reason.
    let merging = |value: u32| {
        attempt(value).against(backlog::Precondition::merge(&root, &id, 1, 2).expect("observe"))
    };
    let error = backlog::merge_into(&root, &id, 1, 2, "one point", Vec::new(), &merging(3))
        .expect_err("a merge removes its source");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert_eq!(backlog::load(&root, &id).expect("load"), before);

    // Under the ceiling, both go through.
    backlog::merge_into(&root, &id, 1, 2, "one point", Vec::new(), &merging(2)).expect("merge");
    assert!(
        backlog::consume_section(&root, &id, 1, &reviewing(&root, &id, 1, 2)).expect("consume")
    );
}

/// No applicable rule means there is no review to be exhausted.
#[test]
fn a_workspace_with_no_backlog_rule_reviews_nothing_and_marks_nothing() {
    let (_dir, root) = workspace();
    rule(&root, "elsewhere", "review:\n  max_attempts: 1\n");
    // Applies to the record, not to staging.
    let path = engr::rules::dir(&root).join("elsewhere.md");
    let text = std::fs::read_to_string(&path).expect("rule");
    std::fs::write(&path, text.replace("- backlog", "- object")).expect("rewrite");

    let id = item(&root, "topic", "unresolved");
    backlog::revise_section(
        &root,
        &id,
        1,
        "reworded",
        &attempt(50).against(backlog::Precondition::section(&root, &id, 1).expect("observe")),
    )
    .expect("no rule, no review");
    assert_eq!(
        marker(&root, &id, 1),
        None,
        "absent means what it says: nothing governed this"
    );
    assert!(backlog::consume_section(
        &root,
        &id,
        1,
        &attempt(50).against(backlog::Precondition::section(&root, &id, 1).expect("observe")),
    )
    .expect("consume"));
}

/// The marker is persisted, and absent when there is nothing to say.
#[test]
fn the_marker_is_a_section_field_that_is_absent_unless_it_is_needed() {
    let (_dir, root) = workspace();
    rule(&root, "careful", "review:\n  max_attempts: 1\n");
    let id = item(&root, "topic", "unresolved");

    let stored: serde_json::Value =
        store::read_json(&backlog::item_path(&root, &id)).expect("item");
    assert!(
        stored["sections"][0].get("review_exhaustion").is_none(),
        "an ordinary point carries no diagnostic: {}",
        stored["sections"][0]
    );

    backlog::revise_section(&root, &id, 1, "exhausted", &reviewing(&root, &id, 1, 2))
        .expect("soft-admit");
    let stored: serde_json::Value =
        store::read_json(&backlog::item_path(&root, &id)).expect("item");
    assert_eq!(
        stored["sections"][0]["review_exhaustion"],
        serde_json::json!({"attempts": 2, "limit": 1}),
        "two numbers, and deliberately not a review history"
    );

    // And it survives the round trip, since the precondition compares whole
    // Sections and a field that vanished on read would never stale anything.
    assert_eq!(
        marker(&root, &id, 1),
        Some(engr::rules::RuleReview {
            attempts: 2,
            limit: 1
        })
    );
}

/// The marker is part of the Section, so it stales a mutation like any field.
#[test]
fn the_marker_participates_in_the_precondition_like_every_other_field() {
    let (_dir, root) = workspace();
    rule(&root, "careful", "review:\n  max_attempts: 1\n");
    let id = item(&root, "topic", "unresolved");

    let bound = backlog::Precondition::section(&root, &id, 1).expect("observe");
    backlog::revise_section(&root, &id, 1, "exhausted", &reviewing(&root, &id, 1, 2))
        .expect("soft-admit");
    let error = bound.still_holds(&root).expect_err("the point moved");
    assert_eq!(error.code, engr::EXIT_STALE);
}

/// A mutation carrying a precondition is refused when it no longer holds.
///
/// The check belongs inside the mutation, under the same lock as the write.
/// Checking beforehand and writing afterwards leaves open precisely the gap it
/// exists to close.
#[test]
fn a_prepared_mutation_is_refused_when_its_predecessor_moved() {
    let (_dir, root) = workspace();
    let id = item(&root, "topic", "unresolved");
    let bound = backlog::Precondition::section(&root, &id, 1).expect("observe");

    // Somebody else gets there first.
    backlog::revise_section(
        &root,
        &id,
        1,
        "sharpened by someone else",
        &on_section(&root, &id, 1),
    )
    .expect("concurrent");

    let error = backlog::revise_section(
        &root,
        &id,
        1,
        "written against the old wording",
        &Prepared::first().against(bound),
    )
    .expect_err("prepared against something else");
    assert_eq!(error.code, engr::EXIT_STALE);
    assert_eq!(
        backlog::load(&root, &id)
            .expect("load")
            .section(1)
            .expect("section")
            .text,
        "sharpened by someone else",
        "the refused mutation did not land on top of the change it never read"
    );
}

/// An attempt is counted from 1, and the CLI cannot smuggle in a zero.
#[test]
fn there_is_no_attempt_zero() {
    let error = engr::rules::Attempt::new(0).expect_err("no such attempt");
    assert_eq!(error.code, engr::EXIT_USAGE);
}

/// An add binds the identity slot it will receive, not merely that the slot
/// looks free.
///
/// Absence is not the same question. Another writer can take the reserved id and
/// then consume it: the id reads as absent again, while the counter has moved on
/// permanently and this add would now receive a different one. A precondition
/// that only asks "is it absent" says yes to that, and the add lands on an
/// identity nobody reviewed — under a number other subjects may already be
/// pointing at from the first allocation.
#[test]
fn an_add_binds_the_identity_it_will_receive_not_merely_a_free_looking_slot() {
    let (_dir, root) = workspace();
    let id = item(&root, "topic", "first");
    let bound = backlog::Precondition::section_absent(&root, &id).expect("observe");
    bound
        .still_holds(&root)
        .expect("§2 is still what this add would take");

    // Somebody else takes §2 and then judges it resolved. §2 is absent again.
    backlog::add_section(
        &root,
        &id,
        "someone else got there",
        Vec::new(),
        &on_add(&root, &id),
    )
    .expect("race");
    assert!(!backlog::consume_section(&root, &id, 2, &on_section(&root, &id, 2)).expect("consume"));
    assert!(
        backlog::load(&root, &id).expect("load").section(2).is_err(),
        "the id is absent, which is exactly what makes this the interesting case"
    );

    let error = bound
        .still_holds(&root)
        .expect_err("the reserved slot is gone for good, absent or not");
    assert_eq!(error.code, engr::EXIT_STALE);
}

/// A precondition that still holds is not a precondition for *this* change.
///
/// `still_holds` asks the world whether the thing it names has moved, and
/// answers honestly about a thing nobody is mutating. So a caller holding a
/// perfectly valid predecessor for one item could apply it to another and get a
/// clean answer — the exact-predecessor guarantee failing precisely where it
/// looks satisfied, which is worse than not having one.
#[test]
fn a_precondition_for_something_else_does_not_authorize_this_mutation() {
    let (_dir, root) = workspace();
    let mine = item(&root, "mine", "unresolved");
    let theirs = item(&root, "theirs", "also unresolved");
    backlog::add_section(
        &root,
        &mine,
        "a second point",
        Vec::new(),
        &on_add(&root, &mine),
    )
    .expect("add");

    // Another item's predecessor. It genuinely still holds; it authorizes
    // nothing here.
    let elsewhere = backlog::Precondition::section(&root, &theirs, 1).expect("observe");
    elsewhere
        .still_holds(&root)
        .expect("it really has not moved");
    let error = backlog::revise_section(
        &root,
        &mine,
        1,
        "reworded",
        &on_section(&root, &mine, 1).against(elsewhere),
    )
    .expect_err("a predecessor for another item is not one for this");
    assert_eq!(error.code, engr::EXIT_INVARIANT);

    // The right item, the wrong point.
    let sibling = backlog::Precondition::section(&root, &mine, 2).expect("observe");
    let error = backlog::revise_section(
        &root,
        &mine,
        1,
        "reworded",
        &on_section(&root, &mine, 1).against(sibling),
    )
    .expect_err("§2 is not what this changes");
    assert_eq!(error.code, engr::EXIT_INVARIANT);

    // The right item and point, the wrong kind of binding: a whole-item
    // predecessor is what a rename rests on, not a reword.
    let whole = backlog::Precondition::title(&root, &mine).expect("observe");
    let error = backlog::revise_section(
        &root,
        &mine,
        1,
        "reworded",
        &on_section(&root, &mine, 1).against(whole),
    )
    .expect_err("that is a rename's predecessor");
    assert_eq!(error.code, engr::EXIT_INVARIANT);

    assert_eq!(
        backlog::load(&root, &mine)
            .expect("load")
            .section(1)
            .expect("§1")
            .text,
        "unresolved",
        "none of those refusals wrote anything"
    );

    // And the one that does authorize it goes through.
    let exact = backlog::Precondition::section(&root, &mine, 1).expect("observe");
    backlog::revise_section(
        &root,
        &mine,
        1,
        "reworded",
        &on_section(&root, &mine, 1).against(exact),
    )
    .expect("the predecessor this rests on");
}

/// A merge binds the two points it touches, and only them.
#[test]
fn a_merge_precondition_must_cover_exactly_the_destination_and_its_source() {
    let (_dir, root) = workspace();
    let id = item(&root, "topic", "first");
    backlog::add_section(&root, &id, "second", Vec::new(), &on_add(&root, &id)).expect("add");
    backlog::add_section(&root, &id, "third", Vec::new(), &on_add(&root, &id)).expect("add");

    for (name, bound) in [
        (
            "missing the source",
            vec![backlog::Precondition::section(&root, &id, 1).expect("observe")],
        ),
        (
            "a point this merge never touches",
            vec![
                backlog::Precondition::section(&root, &id, 1).expect("observe"),
                backlog::Precondition::section(&root, &id, 2).expect("observe"),
                backlog::Precondition::section(&root, &id, 3).expect("observe"),
            ],
        ),
    ] {
        let partial = backlog::Precondition::combine(bound).expect("combine");
        let error = backlog::merge_into(
            &root,
            &id,
            1,
            2,
            "one point",
            Vec::new(),
            &Prepared::first().against(partial),
        )
        .expect_err("the reviewed judgement was about a different set");
        assert_eq!(error.code, engr::EXIT_INVARIANT, "{name}");
    }

    let exact = backlog::Precondition::merge(&root, &id, 1, 2).expect("observe");
    backlog::merge_into(
        &root,
        &id,
        1,
        2,
        "one point",
        Vec::new(),
        &Prepared::first().against(exact),
    )
    .expect("destination and source, exactly");
}

/// A creation has no predecessor to bind, so it refuses to pretend it does.
///
/// engr mints the identity while creating and a caller cannot choose one, so
/// whatever a caller prepared against, the item created is a different thing.
/// Checking the first and creating the second is not a weaker guarantee than
/// none; it is a false one.
#[test]
fn creating_an_item_refuses_a_precondition_it_could_not_honour() {
    let (_dir, root) = workspace();
    let elsewhere = item(&root, "unrelated", "unresolved");
    let bound = backlog::Precondition::title(&root, &elsewhere).expect("observe");
    bound
        .still_holds(&root)
        .expect("it holds, and it authorizes nothing here");

    let error = backlog::create(
        &root,
        "topic",
        "unresolved",
        Vec::new(),
        &Prepared::first().against(bound),
    )
    .expect_err("the id checked would not be the id created");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert_eq!(
        backlog::ids(&root).expect("ids"),
        vec![elsewhere],
        "and nothing was created"
    );
}

/// An exhausted rename is refused rather than admitted unmarked.
///
/// Every other soft-admission leaves a marker saying the wording went in without
/// a passing review. A rename admits no wording, so there is nowhere for that
/// marker to go — and letting it through anyway would make it the one exhausted
/// change nothing records. What an item-level marker should look like is not
/// settled, so this refuses instead of inventing one.
#[test]
fn an_exhausted_rename_is_refused_rather_than_admitted_with_nothing_to_show() {
    let (_dir, root) = workspace();
    rule(&root, "careful", "review:\n  max_attempts: 2\n");
    let id = item(&root, "original topic", "unresolved");

    let renaming = |value: u32| {
        attempt(value).against(backlog::Precondition::title(&root, &id).expect("observe"))
    };
    let error = backlog::rename(&root, &id, "a different topic", &renaming(3))
        .expect_err("nowhere to record it");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(
        error.message.contains("nowhere to record"),
        "the refusal says why this one is different: {}",
        error.message
    );
    assert_eq!(
        backlog::load(&root, &id).expect("load").title,
        "original topic"
    );
    assert_eq!(
        marker(&root, &id, 1),
        None,
        "and no point was marked for a change that was not about it"
    );

    backlog::rename(&root, &id, "a different topic", &renaming(2)).expect("within the ceiling");
}

/// A produced target is checked at the moment the claim is written.
///
/// Existence is checked exactly once, ever — so it has to be checked where the
/// write happens. Validating first and appending afterwards leaves a gap an
/// Object mutation fits through, and the single check this relationship gets
/// would have been against something that no longer existed when it landed.
#[test]
fn a_produced_target_is_checked_inside_the_lock_that_writes_it() {
    use std::sync::mpsc;

    let (_dir, root) = workspace();
    let id = item(&root, "topic", "unresolved");
    let missing = format!("obj:{}", compact(&engr::model::new_id()));
    let (tx, rx) = mpsc::channel();
    let prepared = on_section(&root, &id, 1);
    std::thread::scope(|scope| {
        // Hold the workspace lock, then ask for a claim on a target that does
        // not exist. If the target were checked before the lock, the refusal
        // would arrive immediately; under the lock it cannot arrive at all until
        // the lock is free.
        engr::store::with_lock(&root, || {
            let claim_root = root.clone();
            let claim_id = id.clone();
            let claim_target = missing.clone();
            let claim_prepared = prepared.clone();
            scope.spawn(move || {
                let outcome = backlog::record_produced(
                    &claim_root,
                    &claim_id,
                    1,
                    Produced::object(claim_target),
                    &claim_prepared,
                );
                tx.send(outcome).expect("send");
            });
            assert!(
                rx.recv_timeout(std::time::Duration::from_millis(300))
                    .is_err(),
                "the target was judged before the lock was held"
            );
            Ok(())
        })
        .expect("held");
    });

    let outcome = rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("it proceeds once the lock is free");
    let error = outcome.expect_err("the target does not exist");
    assert_eq!(error.code, engr::EXIT_NOT_FOUND);
}

/// The marker records an exhausted review, so a stored value that could not have
/// come from one is not a diagnostic — it is a claim about a review that never
/// happened.
#[test]
fn a_stored_marker_that_no_exhausted_review_could_have_written_is_refused() {
    let (_dir, root) = workspace();
    let id = item(&root, "topic", "unresolved");
    let path = backlog::item_path(&root, &id);
    let pristine: serde_json::Value = store::read_json(&path).expect("item");

    for (name, marker) in [
        (
            "a ceiling of zero",
            serde_json::json!({"attempts": 1, "limit": 0}),
        ),
        (
            "an attempt of zero",
            serde_json::json!({"attempts": 0, "limit": 0}),
        ),
        (
            "an attempt within the ceiling",
            serde_json::json!({"attempts": 2, "limit": 5}),
        ),
        (
            "an attempt exactly at the ceiling",
            serde_json::json!({"attempts": 5, "limit": 5}),
        ),
    ] {
        let mut corrupt = pristine.clone();
        corrupt["sections"][0]["review_exhaustion"] = marker;
        write_raw(&path, &corrupt).expect("hand edit");
        let error = backlog::load(&root, &id).unwrap_err();
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{name}");
    }

    // The shape an exhausted review does produce still loads.
    let mut fine = pristine;
    fine["sections"][0]["review_exhaustion"] = serde_json::json!({"attempts": 6, "limit": 5});
    write_raw(&path, &fine).expect("hand edit");
    backlog::load(&root, &id).expect("a real diagnostic");
}

/// The library is the same semantic mutation, so it is held to the same rule.
///
/// Enforcing "a mutation carries the predecessor it was prepared against" only
/// at the command line enforces the command line. A direct caller reaches the
/// same write path, and `Prepared::first()` is an ordinary constructor — so
/// without a check at the domain boundary a point can be reworded, or
/// destroyed, on a reading nobody can show was the one that got applied, while
/// every check that does run passes.
///
/// Rule presence does not enter into it: a rule decides whether there is a
/// *review*, not whether someone else's write can land underneath this one.
#[test]
fn a_library_mutation_cannot_skip_the_predecessor_by_going_direct() {
    let (_dir, root) = workspace();
    let id = item(&root, "topic", "unresolved");
    backlog::add_section(
        &root,
        &id,
        "a second point",
        Vec::new(),
        &on_add(&root, &id),
    )
    .expect("add");

    for governed in [false, true] {
        if governed {
            rule(&root, "careful", "review:\n  max_attempts: 5\n");
        }
        for (name, outcome) in [
            (
                "reword",
                backlog::revise_section(&root, &id, 1, "reworded", &Prepared::first()),
            ),
            (
                "re-subject",
                backlog::set_subjects(&root, &id, 1, Vec::new(), &Prepared::first()),
            ),
            (
                "rename the topic",
                backlog::rename(&root, &id, "another topic", &Prepared::first()).map(|_| ()),
            ),
            (
                "destroy",
                backlog::consume_section(&root, &id, 1, &Prepared::first()).map(|_| ()),
            ),
        ] {
            let error = outcome.unwrap_err();
            assert_eq!(
                error.code,
                engr::EXIT_INVARIANT,
                "{name} (governed: {governed})"
            );
            assert!(
                error.message.contains("prepared against"),
                "{name}: {}",
                error.message
            );
        }
        let stored = backlog::load(&root, &id).expect("load");
        assert_eq!(stored.title, "topic", "no rename landed");
        assert_eq!(
            stored.section(1).expect("§1").text,
            "unresolved",
            "none of those wrote anything (governed: {governed})"
        );
    }

    // Carrying it, the same mutation goes through.
    backlog::revise_section(&root, &id, 1, "reworded", &reviewing(&root, &id, 1, 1))
        .expect("with a predecessor");
}

/// Creating a point stays possible in a workspace that has rules about points.
///
/// Creation binds nothing engr can check, because engr allocates the id. So it
/// is the one mutation exempt from carrying a predecessor — requiring what it
/// cannot express would make `backlog new` impossible in exactly the workspaces
/// that care most about unresolved work.
#[test]
fn creating_a_point_is_still_possible_where_a_rule_governs_backlog() {
    let (_dir, root) = workspace();
    rule(&root, "careful", "review:\n  max_attempts: 5\n");

    let created = backlog::create(&root, "topic", "unresolved", Vec::new(), &attempt(1))
        .expect("a rule about points must not make points uncreatable");
    assert_eq!(created.sections.len(), 1);

    // And an exhausted creation still soft-admits and marks, like any other
    // mutation that preserves the point.
    let exhausted = backlog::create(&root, "another", "unresolved", Vec::new(), &attempt(9))
        .expect("soft-admit");
    assert_eq!(
        exhausted.sections[0].review_exhaustion,
        Some(engr::rules::RuleReview {
            attempts: 9,
            limit: 5
        })
    );
}

/// One target at one commit is one subject, whichever way it was observed.
///
/// Equality strips `dirty` and activity strips `dirty`, so duplicate detection
/// has to strip it too. Comparing raw bytes lets the same file at the same
/// commit sit in one set twice — once clean, once dirty — while every other part
/// of the model insists those are the same subject.
#[test]
fn the_same_target_cannot_appear_twice_by_differing_only_in_dirty() {
    let (_dir, root) = workspace();
    repository(&root);
    std::fs::write(root.join("session.rs"), "fn read() {}\n").expect("write");
    let commit = commit_all(&root, "source");
    let id = item(&root, "topic", "unresolved");

    let path = backlog::item_path(&root, &id);
    let mut stored: serde_json::Value = store::read_json(&path).expect("item");
    stored["sections"][0]["subjects"] = serde_json::json!([
        { "kind": "file", "path": "session.rs", "commit": commit },
        { "kind": "file", "path": "session.rs", "commit": commit, "dirty": true },
    ]);
    write_raw(&path, &stored).expect("hand edit");

    let error = backlog::load(&root, &id).expect_err("that is one subject listed twice");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(
        error.message.contains("same subject twice"),
        "{}",
        error.message
    );
}

/// `dirty` is measured against the commit the subject pins, not against HEAD.
///
/// They are the same question only when the pin is HEAD. Choose an older
/// revision from a perfectly clean worktree and a status check says "clean",
/// while the file is plainly not what that commit reconstructs — which is the
/// claim the subject then goes on to make.
#[test]
fn an_explicit_revision_is_inexact_when_the_file_read_is_not_what_it_holds() {
    let (_dir, root) = workspace();
    repository(&root);
    std::fs::write(root.join("session.rs"), "fn first() {}\n").expect("write");
    let earlier = commit_all(&root, "first");
    std::fs::write(root.join("session.rs"), "fn second() {}\n").expect("rewrite");
    let now = commit_all(&root, "second");

    // The worktree is clean, so nothing about `git status` is interesting here.
    let (pinned, dirty) = backlog::pin(&root, "session.rs", Some(&earlier)).expect("pin");
    assert_eq!(pinned, earlier);
    assert!(
        dirty,
        "what was read is `fn second`, and the pinned commit holds `fn first`"
    );

    // Pinning what the file actually is stays exact.
    let (pinned, dirty) = backlog::pin(&root, "session.rs", Some(&now)).expect("pin");
    assert_eq!(pinned, now);
    assert!(!dirty, "this baseline reconstructs exactly what was read");
}

/// A produced outcome may only claim authority that is still intact.
///
/// Existing is not the same as sound. The effective projection answers whether
/// an Object is there and readable; a section edited outside the gate loads
/// perfectly and reads as authority. This entry asserts that a durably admitted
/// outcome exists, and it is checked once and never again — so the one check it
/// gets cannot be the weaker of the two questions.
#[test]
fn a_produced_outcome_cannot_claim_authority_that_was_edited_outside_the_gate() {
    let (_dir, root) = workspace();
    let sound = new_object(&root, "a sound record");
    admit(&root, add(&sound, wording("confirmed wording")));
    let moved = new_object(&root, "a record that will move");
    admit(&root, add(&moved, wording("also confirmed")));
    let id = item(&root, "topic", "unresolved");

    // The wording moves without the gate. It still loads, and it is still
    // structurally a record — which is exactly the trap.
    let mut tampered = store::load_object(&root, &moved).expect("object");
    "edited outside the gate".clone_into(&mut tampered.sections[0].text);
    overwrite_object(&root, &tampered);

    let before = backlog::load(&root, &id).expect("load");
    for target in [
        format!("obj:{}", compact(&moved)),
        format!("obj:{}:1", compact(&moved)),
    ] {
        let error = backlog::record_produced(
            &root,
            &id,
            1,
            Produced::object(target.clone()),
            &on_section(&root, &id, 1),
        )
        .expect_err("that authority is not intact");
        assert_eq!(error.code, engr::EXIT_INVARIANT, "{target}");
        assert!(
            error.message.contains("not intact"),
            "{target}: {}",
            error.message
        );
    }
    assert_eq!(
        backlog::load(&root, &id).expect("load"),
        before,
        "a refused claim writes nothing"
    );

    // Intact authority is claimable, at both granularities.
    for target in [
        format!("obj:{}", compact(&sound)),
        format!("obj:{}:1", compact(&sound)),
    ] {
        assert!(
            backlog::record_produced(
                &root,
                &id,
                1,
                Produced::object(target.clone()),
                &on_section(&root, &id, 1),
            )
            .expect("intact authority"),
            "{target}"
        );
    }
}

/// Every Section seal passing is not the same claim as the Object being intact.
///
/// `title`, `type`, `state` and the revision are admitted facts with nothing
/// sealing them, so an edit from `open` to `closed` with every Section byte
/// untouched loads perfectly and satisfies every per-Section check. An
/// Object-level outcome claims exactly that authority — creation, a type or
/// state transition, supersession — so it is the claim that must not accept it.
#[test]
fn an_object_level_outcome_refuses_authority_changed_outside_the_gate() {
    let (_dir, root) = workspace();
    let object = new_object(&root, "a record");
    admit(&root, add(&object, wording("confirmed wording")));
    let id = item(&root, "topic", "unresolved");
    let compact = compact(&object);

    // Not a byte of any Section moves; only the Object's own lifecycle does.
    let mut tampered = store::load_object(&root, &object).expect("object");
    tampered.state = engr::semantics::State::Closed;
    overwrite_object(&root, &tampered);
    for section in &tampered.sections {
        engr::integrity::check_section_seal(section).expect("the Section seals still pass");
    }

    let before = backlog::load(&root, &id).expect("load");
    let error = backlog::record_produced(
        &root,
        &id,
        1,
        Produced::object(format!("obj:{compact}")),
        &on_section(&root, &id, 1),
    )
    .expect_err("that object-level authority was not admitted");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(
        error.message.contains("sealed as") && error.message.contains("current contents seal"),
        "the refusal identifies the aggregate integrity failure: {}",
        error.message
    );
    assert_eq!(
        backlog::load(&root, &id).expect("load"),
        before,
        "a refused claim writes nothing"
    );

    // Put it back, and the same claim is admissible.
    tampered.state = engr::semantics::State::Open;
    save_raw(&root, &tampered).expect("restore");
    assert!(backlog::record_produced(
        &root,
        &id,
        1,
        Produced::object(format!("obj:{compact}")),
        &on_section(&root, &id, 1),
    )
    .expect("intact authority"));
}

/// The reconstruction has to be compared, not merely performed.
///
/// Section seals are recomputed over the Sections the *projection* holds, so one
/// removed outside the gate is simply never visited — every remaining seal
/// passes, and the counters do not move because gaps below `next_section_id` are
/// what legitimate admitted deletion looks like. Replaying the history
/// reconstructs the missing Section; the check is only worth anything if that
/// difference is looked at.
#[test]
fn an_object_level_outcome_refuses_an_admitted_section_removed_outside_the_gate() {
    let (_dir, root) = workspace();
    let object = new_object(&root, "a record");
    admit(&root, add(&object, wording("first confirmed wording")));
    admit(&root, add(&object, wording("second confirmed wording")));
    let id = item(&root, "topic", "unresolved");
    let compact = compact(&object);

    // Take §1 out of the projection and leave every counter where it was.
    let mut tampered = store::load_object(&root, &object).expect("object");
    let (rev, next) = (tampered.rev, tampered.next_section_id);
    tampered.sections.retain(|section| section.id != 1);
    assert_eq!((tampered.rev, tampered.next_section_id), (rev, next));
    overwrite_object(&root, &tampered);
    for section in &tampered.sections {
        engr::integrity::check_section_seal(section)
            .expect("every remaining Section seal still passes");
    }

    let error = backlog::record_produced(
        &root,
        &id,
        1,
        Produced::object(format!("obj:{compact}")),
        &on_section(&root, &id, 1),
    )
    .expect_err("an admitted section is missing from what this claims");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
}

/// Absent and unreadable are different answers, even in the wording.
///
/// #13 §4 is explicit that a trust path must never downgrade invalid authority
/// to "not found". The exit code already told them apart; the message did not,
/// and the message is what a person acts on — being told an object does not
/// exist sends them looking for a missing file when what they have is a present
/// one whose history will not load.
///
/// A non-contiguous event log is the realistic way to reach this: it is what a
/// Git merge of two branches that each confirmed a mutation from the same base
/// leaves behind, and it is the exact state #33 records as a concern.
#[test]
fn a_produced_target_that_cannot_be_read_is_not_reported_as_missing() {
    let (_dir, root) = workspace();
    let object = new_object(&root, "a record");
    admit(&root, add(&object, wording("first")));
    let id = item(&root, "topic", "unresolved");

    // Two confirmed mutations claiming the same revision.
    let path = store::events_path(&root, &object);
    let text = std::fs::read_to_string(&path).expect("events");
    let last = text.lines().last().expect("last event").to_owned();
    std::fs::write(&path, format!("{text}{last}\n")).expect("duplicate the rev");

    let error = backlog::record_produced(
        &root,
        &id,
        1,
        Produced::object(format!("obj:{}", compact(&object))),
        &on_section(&root, &id, 1),
    )
    .expect_err("the history does not load");
    assert_eq!(
        error.code,
        engr::EXIT_SCHEMA,
        "malformed authority is a schema failure, not absence"
    );
    assert!(
        !error.message.contains("does not exist"),
        "the object is right there; its history is what is broken: {}",
        error.message
    );
    assert!(
        error.message.contains("cannot be read as authority"),
        "{}",
        error.message
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

/// Put an Object on disk directly, for a fixture that needs one there.
fn save_raw(root: &std::path::Path, object: &engr::model::Object) -> engr::Result<()> {
    write_raw(&engr::store::object_path(root, &object.id), object)
}

/// A number too large for JCS is a fault in the file, not in the command line.
///
/// One traversal, two fault classes. The same walk validates a value a caller
/// just typed and a value found inside a persisted resource, and reporting the
/// second as `usage` sends whoever reads the refusal to fix their command
/// instead of the record.
#[test]
fn a_stored_number_outside_the_shared_domain_is_a_schema_fault() {
    let (_dir, root) = workspace();
    let id = engr::model::new_id();
    let beyond = serde_json::Number::from(engr::proof::MAX_SAFE_INTEGER + 1);
    let item = serde_json::json!({
        "id": id,
        "topic": "outside the domain",
        "next_section_id": beyond,
        "sections": [{
            "id": 1,
            "text": "unresolved",
            "updated_at": "2026-08-25T00:00:00Z",
        }],
    });
    // Written with an ordinary serializer: JCS cannot carry the value at all,
    // which is the whole reason the domain exists.
    std::fs::write(
        backlog::item_path(&root, &id),
        serde_json::to_string(&item).expect("json"),
    )
    .expect("stage");

    let error = backlog::load(&root, &id).expect_err("that number cannot survive JCS");
    assert_eq!(
        error.code,
        engr::EXIT_SCHEMA,
        "a stored file outside the schema is not a usage error: {error}"
    );
    assert!(error.message.contains("safe integer"), "{error}");
}

/// Recording what a point produced is activity, and the protocol says so.
///
/// Bookkeeping moves the field triage sorts by, because learning what a point
/// produced is meaningful to whoever picks it up next. The document is what
/// `engr protocol` prints, so the two cannot disagree — an earlier reading that
/// an outcome does not count survived in the prose after the build stopped
/// implementing it, and that is exactly the shape this asserts against.
#[test]
fn recording_a_produced_outcome_is_activity_and_the_protocol_agrees() {
    let (_dir, root) = workspace();
    let object = new_object(&root, "a record");
    admit(&root, add(&object, wording("confirmed wording")));
    let id = item(&root, "topic", "unresolved");
    let before = backlog::load(&root, &id).expect("load").sections[0]
        .updated_at
        .clone();

    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert!(backlog::record_produced(
        &root,
        &id,
        1,
        Produced::object(format!("obj:{}", compact(&object))),
        &on_section(&root, &id, 1),
    )
    .expect("record"));
    let after = backlog::load(&root, &id).expect("load").sections[0]
        .updated_at
        .clone();
    assert_ne!(
        after, before,
        "bookkeeping is activity on the unresolved work"
    );

    assert!(
        engr::PROTOCOL.contains("recording or forgetting a produced outcome"),
        "the normative document must describe the behaviour this build has"
    );
    assert!(
        !engr::PROTOCOL.contains("neither\ndoes appending a produced outcome"),
        "and must not still carry the reading that was withdrawn"
    );
}

/// A stored Backlog Section is held to everything its writer enforces.
///
/// Backlog is hand-editable by design and has no seal, so loading is the only
/// boundary there is. Two shapes it shares with an Object Section were checked
/// on the way in and not on the way out: a `header` written empty — which the
/// omission rule says is absence spelled a second way — and a `content[]` entry
/// with an invalid type or an empty body. Both loaded cleanly and were then
/// rendered as a point somebody wrote.
#[test]
fn stored_backlog_sections_are_held_to_the_writers_navigation_and_content_rules() {
    let (_dir, root) = workspace();
    let id = item(&root, "stored shapes", "unresolved");
    let path = backlog::item_path(&root, &id);
    let original: serde_json::Value = store::read_json(&path).expect("item");

    for (what, edit, expected) in [
        (
            "an empty header",
            serde_json::json!(""),
            "omits the member rather than carrying an empty one",
        ),
        (
            "a header of only whitespace is still a header",
            serde_json::json!("   "),
            "",
        ),
    ] {
        let mut edited = original.clone();
        edited["sections"][0]["header"] = edit;
        write_raw(&path, &edited).expect("write");
        let outcome = backlog::load(&root, &id);
        if expected.is_empty() {
            outcome.expect(what);
        } else {
            let error = outcome.err().unwrap_or_else(|| panic!("{what}: refused"));
            assert_eq!(error.code, engr::EXIT_SCHEMA, "{what}");
            assert!(error.message.contains(expected), "{what}: {error}");
        }
    }

    for (what, entry, expected) in [
        (
            "an unknown content prefix",
            serde_json::json!({ "type": "text.md", "body": "prose" }),
            "must begin with code. or data.",
        ),
        (
            "a tag that is not a tag",
            serde_json::json!({ "type": "code.NOT-A-TAG", "body": "x" }),
            "content type",
        ),
        (
            "an empty body",
            serde_json::json!({ "type": "code.rs", "body": "" }),
            "a body cannot be empty",
        ),
    ] {
        let mut edited = original.clone();
        edited["sections"][0]["content"] = serde_json::json!([entry]);
        write_raw(&path, &edited).expect("write");
        let error = backlog::load(&root, &id)
            .err()
            .unwrap_or_else(|| panic!("{what}: this must be refused"));
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{what}");
        assert!(error.message.contains(expected), "{what}: {error}");
        assert!(error.message.contains("§1"), "{what}: {error}");
    }

    write_raw(&path, &original).expect("restore");
    backlog::load(&root, &id).expect("and it reads back");
}
