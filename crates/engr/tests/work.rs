//! What execution memory is, and — mostly — what it is not.
//!
//! Work is the one domain that can quietly corrupt the value of the record
//! without ever writing to it: an agent that reads a sidecar with every item
//! done and concludes the Object is settled has been misled by something no
//! human confirmed. So the rules pinned here are almost all boundaries —
//! Work owns no authority, changes no Object state, is not addressable, and
//! cannot be mistaken for the record on any screen that prints it.

use engr::model::{Action, Content, Object, Payload};
use engr::semantics::{ObjectType, State as ObjectState};
use engr::work::{self, ItemState, State};
use engr::{gate, ops, store};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn workspace() -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().to_path_buf();
    store::init(&root).expect("init");
    (dir, root)
}

fn admit(root: &Path, payload: Payload) -> Object {
    let prepared = gate::prepare(root, payload).expect("prepare");
    let response = format!("CONFIRM {}", prepared.candidate.challenge);
    gate::confirm(root, &response).expect("confirm").object
}

fn new_object(root: &Path, title: &str) -> String {
    let id = engr::model::new_id();
    admit(
        root,
        Payload {
            action: Action::ObjectCreated,
            object: id.clone(),
            content: Content {
                text: title.to_owned(),
                ..Content::default()
            },
        },
    );
    id
}

fn compact(id: &str) -> String {
    engr::reference::encode_uuid_str(id).expect("compact")
}

/// One named way of hand-editing a stored sidecar into something invalid.
type Corruption = (&'static str, fn(&mut Value));

/// Edit the sidecar the way a text editor would.
fn rewrite(root: &Path, object: &str, edit: impl FnOnce(&mut Value)) {
    let path = work::path(root, object);
    let mut value: Value = store::read_json(&path).expect("read");
    edit(&mut value);
    store::write_json(&path, &value).expect("write");
}

/// A sidecar belongs to an Object, and only to one that exists.
///
/// It is not a resource: nothing addresses it, nothing points at it, and it has
/// no identity of its own beyond the Object it hangs off.
#[test]
fn a_sidecar_belongs_to_an_object_and_is_not_a_resource_itself() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "the object");

    // Absent is the ordinary state, and never an error.
    assert!(work::find(&root, &id).expect("find").is_none());
    let missing = work::load(&root, &id).expect_err("nothing recorded yet");
    assert_eq!(missing.code, engr::EXIT_NOT_FOUND);

    work::start(&root, &id, Some("where things stand")).expect("start");
    assert!(work::find(&root, &id).expect("find").is_some());
    assert_eq!(work::ids(&root).expect("ids"), vec![id.clone()]);

    // Twice is refused: there is one sidecar per Object, changed rather than
    // replaced, or the second `start` would silently discard a handoff.
    let again = work::start(&root, &id, None).expect_err("one sidecar per object");
    assert_eq!(again.code, engr::EXIT_INVARIANT);

    // An Object that does not exist has nothing to keep memory for.
    let absent = engr::model::new_id();
    let error = work::start(&root, &absent, None).expect_err("no such object");
    assert_eq!(error.code, engr::EXIT_NOT_FOUND);

    // And there is no `engr:work:` namespace to address one with.
    for spelling in ["engr:work:0", &format!("engr:work:{}", compact(&id))] {
        assert!(
            engr::reference::EngrRef::parse_standalone(spelling).is_err(),
            "{spelling:?} must not parse: Work is not an addressable resource"
        );
    }

    // The sidecar carries no format or version of its own; `.engr/format.json`
    // is the single schema authority for the workspace.
    let raw: Value = store::read_json(&work::path(&root, &id)).expect("raw");
    assert!(raw.get("format").is_none(), "{raw}");
    assert!(raw.get("version").is_none(), "{raw}");
}

/// `blocked` is a reading of the sidecar; `done` does not exist at all.
///
/// Storing either would create a second answer to a question the Object already
/// answers — and the `done` one is the dangerous half, because a completed
/// sidecar must never be mistaken for a settled Object.
#[test]
fn the_only_stored_states_are_active_and_paused() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "state");
    let item = work::start(&root, &id, None).expect("start");
    assert_eq!(item.state, State::Active);
    assert_eq!(item.standing(), "active");
    assert!(!item.is_blocked());

    let item = work::add_blocker(&root, &id, Some("waiting on approval"), None).expect("block");
    assert_eq!(item.state, State::Active, "blocked is not stored");
    assert!(item.is_blocked());
    assert_eq!(item.standing(), "blocked");
    let raw: Value = store::read_json(&work::path(&root, &id)).expect("raw");
    assert_eq!(raw["state"], "active", "{raw}");

    // Paused wins over blocked on the screen: what a human said outranks what
    // the sidecar noticed.
    let item = work::set_state(&root, &id, State::Paused).expect("pause");
    assert_eq!(item.standing(), "paused");

    // Nothing else deserializes into the field.
    for invalid in ["blocked", "done", "closed", ""] {
        rewrite(&root, &id, |value| {
            value["state"] = Value::String(invalid.to_owned());
        });
        assert!(
            work::load(&root, &id).is_err(),
            "{invalid:?} is not a Work state"
        );
    }
}

/// `paused` is a human saying stop, and the rule about it is the agent's.
///
/// #12 makes "an agent MUST NOT delete a paused WorkObject without explicit
/// human direction" a normative agent rule, not a gate mutation — so engr does
/// not refuse the deletion. It cannot tell who asked, refusing would make a
/// human's own "delete that" impossible to carry out directly, and it would
/// stop no agent that is willing to resume first anyway. What it does is report
/// what went with the sidecar, so the signal never disappears in silence.
#[test]
fn deleting_a_paused_sidecar_is_carried_out_and_reported_rather_than_refused() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "pause");
    work::start(&root, &id, None).expect("start");
    work::set_state(&root, &id, State::Paused).expect("pause");

    let removed = work::remove(&root, &id).expect("the rule is the agent's, not the tool's");
    assert!(
        removed.was_paused,
        "and what was discarded is reported rather than swallowed"
    );
    assert!(work::find(&root, &id).expect("find").is_none());

    // Nothing about deleting an active one is different, except that there is
    // no stop signal to mention.
    work::start(&root, &id, None).expect("start again");
    let removed = work::remove(&root, &id).expect("active work may be deleted");
    assert!(!removed.was_paused);

    // Deleting says nothing about the Object, either way.
    let object = ops::effective(&root, &id).expect("object");
    assert_eq!(object.state, ObjectState::Open);
    assert!(object.needs_attention());
}

/// Item ids are allocated once and never handed out again.
///
/// Handoff notes and conversations say "work item 3". If pruning item 3 let a
/// later step take that number, every one of those sentences would quietly
/// start pointing at different work.
#[test]
fn item_ids_are_monotonic_and_never_reused() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "items");
    work::start(&root, &id, None).expect("start");

    assert_eq!(work::add_item(&root, &id, "first").expect("add"), 1);
    assert_eq!(work::add_item(&root, &id, "second").expect("add"), 2);
    work::set_item_state(&root, &id, 1, ItemState::Done).expect("done");
    work::remove_item(&root, &id, 1).expect("prune");

    assert_eq!(
        work::add_item(&root, &id, "third").expect("add"),
        3,
        "the pruned id is gone, not free"
    );
    let item = work::load(&root, &id).expect("load");
    assert_eq!(
        item.items.iter().map(|entry| entry.id).collect::<Vec<_>>(),
        vec![2, 3]
    );
    assert_eq!(item.next_item_id, 4);

    // Stored data is held to it too: an id that was never allocated, or one that
    // appears twice, is not a sidecar this build will read.
    rewrite(&root, &id, |value| {
        value["items"][0]["id"] = json!(99);
    });
    let error = work::load(&root, &id).expect_err("99 was never allocated");
    assert_eq!(error.code, engr::EXIT_SCHEMA);

    rewrite(&root, &id, |value| {
        value["items"][0]["id"] = json!(3);
    });
    let error = work::load(&root, &id).expect_err("3 is already taken");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("never reused"), "{error}");
}

/// A dependency needs a target; a blocker needs a reason, a target, or both.
///
/// The asymmetry is the point. A dependency is a prerequisite, so it must name
/// one. A blocker is a condition, and real execution is stopped by things that
/// are not engr resources — an approval, an environment, a vendor — so one that
/// could only be written as an edge would not be written at all.
#[test]
fn a_dependency_needs_a_target_and_a_blocker_needs_at_least_something() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "edges");
    let other = new_object(&root, "the prerequisite");
    let target = format!("obj:{}", compact(&other));
    work::start(&root, &id, None).expect("start");

    let item = work::add_dependency(&root, &id, &target, None).expect("bare dependency");
    assert_eq!(item.dependencies[0].target.reference, target);
    assert!(item.dependencies[0].reason.is_none());
    let repeat = work::add_dependency(&root, &id, &target, Some("again")).expect_err("once");
    assert_eq!(repeat.code, engr::EXIT_INVARIANT);

    for blocker in [
        (Some("waiting for customer approval"), None),
        (None, Some(target.as_str())),
        (
            Some("the decision must be settled first"),
            Some(&target[..]),
        ),
    ] {
        work::add_blocker(&root, &id, blocker.0, blocker.1).expect("a blocker with something");
    }
    let empty = work::add_blocker(&root, &id, None, None).expect_err("an empty blocker");
    assert_eq!(empty.code, engr::EXIT_SCHEMA);
    assert!(empty.message.contains("says nothing"), "{empty}");

    // Blockers have no ids because they are conditions, not things: one that
    // cleared is gone, and the rest keep their positions.
    let item = work::remove_blocker(&root, &id, 0).expect("unblock");
    assert_eq!(item.blockers.len(), 2);
    let past_end = work::remove_blocker(&root, &id, 9).expect_err("no such blocker");
    assert_eq!(past_end.code, engr::EXIT_NOT_FOUND);

    // A stored blocker with neither field is refused as authority too.
    rewrite(&root, &id, |value| {
        value["blockers"] = json!([{}]);
    });
    assert!(work::load(&root, &id).is_err(), "an empty stored blocker");
}

/// Work points at whole Objects and Backlog items, and nothing finer.
///
/// A section, a file or a symbol would read like the record's own `refs[]`,
/// which pins wording and carries authority. Operational context that looked
/// like that would eventually be treated like it.
#[test]
fn work_targets_only_whole_objects_and_backlog_items() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "targets");
    let other = new_object(&root, "another");
    let item = engr::backlog::create(&root, "unresolved", "a point", Vec::new()).expect("backlog");
    work::start(&root, &id, None).expect("start");

    for allowed in [
        format!("obj:{}", compact(&other)),
        format!("backlog:{}", compact(&item.id)),
    ] {
        work::add_dependency(&root, &id, &allowed, None).expect("a legal target");
    }

    for refused in [
        format!("obj:{}:1", compact(&other)),
        format!("collection:{}", compact(&other)),
        format!("obj:{}@HEAD", compact(&other)),
        format!("engr:obj:{}", compact(&other)),
        "obj:not-a-compact-id".to_owned(),
    ] {
        let error = work::add_dependency(&root, &id, &refused, None)
            .expect_err("Work does not point there");
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{refused}");
    }
}

/// Every Work text field is bounded, and the limits are validation.
///
/// They exist to keep each field doing its own job. Text that will not fit is
/// text with somewhere better to be: the unresolved part in Backlog, the
/// settled part in the Object.
#[test]
fn every_work_text_field_is_bounded() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "limits");
    let other = new_object(&root, "target");
    let target = format!("obj:{}", compact(&other));
    work::start(&root, &id, None).expect("start");
    let long = |limit: usize| "x".repeat(limit + 1);

    let error = work::set_summary(&root, &id, Some(&long(work::SUMMARY_MAX))).expect_err("summary");
    assert_eq!(error.code, engr::EXIT_USAGE);
    assert!(error.message.contains("backlog"), "{error}");
    work::set_summary(&root, &id, Some(&"x".repeat(work::SUMMARY_MAX))).expect("exactly the limit");

    assert!(work::add_item(&root, &id, &long(work::ITEM_TEXT_MAX)).is_err());
    let entry = work::add_item(&root, &id, "a step").expect("add");
    assert!(work::set_item_result(&root, &id, entry, Some(&long(work::ITEM_RESULT_MAX))).is_err());
    assert!(work::add_dependency(&root, &id, &target, Some(&long(work::REASON_MAX))).is_err());
    assert!(work::add_blocker(&root, &id, Some(&long(work::REASON_MAX)), None).is_err());

    // Empty is not "short", it is missing.
    assert!(work::add_item(&root, &id, "   ").is_err());
    assert!(work::set_summary(&root, &id, Some("")).is_err());

    // Clearing is a different thing from writing nothing, and is allowed.
    let item = work::set_summary(&root, &id, None).expect("clear");
    assert!(item.summary.is_none());
    let item = work::set_item_result(&root, &id, entry, None).expect("clear");
    assert!(item.items[0].result.is_none());
}

/// Commits are navigation and evidence, never integrity anchors.
///
/// An item can be done with no commit, a commit can exist for work that is not
/// done, and a rebase can make a recorded commit unreachable. None of those is
/// a corrupt sidecar — that is the whole difference from `based_on` and
/// `refs[].commit`, which do anchor.
#[test]
fn commits_are_navigation_and_their_absence_is_never_corruption() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "commits");
    work::start(&root, &id, None).expect("start");
    let done = work::add_item(&root, &id, "research, which produced no commit").expect("add");
    work::set_item_state(&root, &id, done, ItemState::Done).expect("done");
    let item = work::load(&root, &id).expect("load");
    assert!(
        item.items[0].commits.is_empty(),
        "done does not require a commit"
    );

    let pending = work::add_item(&root, &id, "started, and already pushed").expect("add");
    let unreachable = "0".repeat(40);
    work::add_item_commit(&root, &id, pending, &unreachable).expect("a commit no walk will find");
    let repeat = work::add_item_commit(&root, &id, pending, &unreachable).expect_err("once");
    assert_eq!(repeat.code, engr::EXIT_INVARIANT);

    let item = work::load(&root, &id).expect("a dead signpost is not corruption");
    assert_eq!(item.items[1].state, ItemState::Pending);
    assert_eq!(item.items[1].commits, vec![unreachable]);

    // The shape is still held: an abbreviation is not a commit id.
    let error = work::add_item_commit(&root, &id, pending, "0000000").expect_err("abbreviated");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
}

/// Finishing the work settles nothing.
///
/// This is the invariant the whole domain exists under: an agent may complete
/// every item it wrote and the Object is exactly where it was. Only a
/// confirmation moves that, and no amount of operational progress substitutes.
#[test]
fn completing_every_item_changes_nothing_about_the_object() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "the design");
    admit(
        &root,
        Payload {
            action: Action::ObjectClassified {
                object_type: Some(ObjectType::Design),
                state: ObjectState::Draft,
            },
            object: id.clone(),
            content: Content::default(),
        },
    );
    let before = ops::effective(&root, &id).expect("object");

    work::start(&root, &id, Some("implementing it")).expect("start");
    for text in ["write it", "test it", "ship it"] {
        let entry = work::add_item(&root, &id, text).expect("add");
        work::set_item_state(&root, &id, entry, ItemState::Done).expect("done");
        work::set_item_result(&root, &id, entry, Some("done")).expect("result");
    }
    let item = work::load(&root, &id).expect("load");
    assert!(item
        .items
        .iter()
        .all(|entry| entry.state == ItemState::Done));
    assert_eq!(item.state, State::Active, "there is no done to move to");

    let after = ops::effective(&root, &id).expect("object");
    assert_eq!(after.state, before.state);
    assert_eq!(after.object_type, before.object_type);
    assert_eq!(after.rev, before.rev, "and no event was appended");
    assert!(
        after.needs_attention(),
        "a draft design still needs a human"
    );
}

/// Stored sidecars are held to exactly what the write path enforces.
///
/// The same rule Backlog and the Phase 3 types live under: a check that only
/// runs on the way in stops being true after one hand edit, and these files are
/// meant to be read and diffed like everything else that is tracked.
#[test]
fn a_hand_edited_sidecar_outside_the_schema_is_refused_rather_than_repaired() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "stored");
    let other = new_object(&root, "target");
    work::start(&root, &id, Some("a checkpoint")).expect("start");
    let entry = work::add_item(&root, &id, "a step").expect("add");
    work::add_dependency(&root, &id, &format!("obj:{}", compact(&other)), Some("why"))
        .expect("depend");
    work::add_blocker(&root, &id, Some("waiting"), None).expect("block");
    work::set_item_result(&root, &id, entry, Some("an outcome")).expect("result");

    let corruptions: [Corruption; 9] = [
        ("an unknown top-level field", |value| {
            value["owner"] = json!("someone");
        }),
        ("an unknown item field", |value| {
            value["items"][0]["estimate"] = json!("2d");
        }),
        ("an unknown dependency field", |value| {
            value["dependencies"][0]["weight"] = json!(3);
        }),
        ("an unknown blocker field", |value| {
            value["blockers"][0]["severity"] = json!("high");
        }),
        ("an unknown target field", |value| {
            value["dependencies"][0]["target"]["alias"] = json!("auth");
        }),
        ("an item state outside the vocabulary", |value| {
            value["items"][0]["state"] = json!("blocked");
        }),
        ("a target kind Work does not accept", |value| {
            value["dependencies"][0]["target"]["ref"] =
                json!("collection:01a01a0000000000000000000");
        }),
        ("a summary past the limit", |value| {
            value["summary"] = json!("x".repeat(engr::work::SUMMARY_MAX + 1));
        }),
        ("a commit that is not a full object id", |value| {
            value["items"][0]["commits"] = json!(["abc1234"]);
        }),
    ];

    let sound = store::read_json::<Value>(&work::path(&root, &id)).expect("read");
    for (what, corrupt) in corruptions {
        store::write_json(&work::path(&root, &id), &sound).expect("restore");
        rewrite(&root, &id, corrupt);
        let error = work::load(&root, &id).expect_err(what);
        // Schema, not usage, and not "either". Accepting both would hide the
        // boundary this test exists to hold: nothing about a file on disk is
        // the current caller's mistake.
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{what}: {error}");
    }
}

/// A sidecar that belongs to no Object is invalid Work, not a row with a hole.
///
/// The owner invariant has to hold on the way out as well as the way in. A
/// sidecar names its Object in its filename, so a copied file can name one that
/// never existed — and a check that only ran on the write path would let this
/// build read, list and hand back operational memory for nothing.
#[test]
fn an_orphan_sidecar_is_refused_rather_than_listed() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "the object");
    work::start(&root, &id, Some("a checkpoint")).expect("start");

    // Copied to a filename naming an Object that was never created.
    let orphan = engr::model::new_id();
    let sound: Value = store::read_json(&work::path(&root, &id)).expect("read");
    store::write_json(&work::path(&root, &orphan), &sound).expect("write");

    let error = work::load(&root, &orphan).expect_err("a sidecar for nothing");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("belongs to no object"), "{error}");
    assert!(work::find(&root, &orphan).is_err(), "and find agrees");

    // The real one is untouched by its neighbour being wrong.
    work::load(&root, &id).expect("the valid sidecar still loads");
}

/// Stored Work is held to exactly what the write path can produce.
///
/// A shadow schema — shapes the API refuses but the reader accepts — is only
/// ever discovered by something that came to depend on it. These are the four
/// places the two could have drifted apart.
#[test]
fn the_persisted_schema_is_not_looser_than_the_write_path() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "schema");
    let other = new_object(&root, "target");
    let target = format!("obj:{}", compact(&other));
    work::start(&root, &id, None).expect("start");
    let entry = work::add_item(&root, &id, "a step").expect("add");
    work::add_item_commit(&root, &id, entry, &"a".repeat(40)).expect("commit");
    work::add_dependency(&root, &id, &target, None).expect("depend");
    let sound: Value = store::read_json(&work::path(&root, &id)).expect("read");
    let restore = || store::write_json(&work::path(&root, &id), &sound).expect("restore");

    // `dependencies`, `blockers`, `items` and `items[].commits` are required and
    // may be empty. Omitted is a third spelling the write path never produces.
    for field in ["dependencies", "blockers", "items"] {
        restore();
        rewrite(&root, &id, |value| {
            value.as_object_mut().expect("object").remove(field);
        });
        let error = work::load(&root, &id).expect_err(field);
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{field}");
    }
    restore();
    rewrite(&root, &id, |value| {
        value["items"][0]
            .as_object_mut()
            .expect("item")
            .remove("commits");
    });
    assert!(work::load(&root, &id).is_err(), "commits is required too");

    // The write path refuses a duplicate dependency and a duplicate commit, so
    // stored ones are shapes it could not have written.
    restore();
    rewrite(&root, &id, |value| {
        let held = value["dependencies"][0].clone();
        value["dependencies"] = json!([held.clone(), held]);
    });
    let error = work::load(&root, &id).expect_err("one prerequisite is one dependency");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("twice"), "{error}");

    restore();
    rewrite(&root, &id, |value| {
        let held = value["items"][0]["commits"][0].clone();
        value["items"][0]["commits"] = json!([held.clone(), held]);
    });
    let error = work::load(&root, &id).expect_err("a commit recorded twice");
    assert_eq!(error.code, engr::EXIT_SCHEMA);

    // A stored fault is a schema fault. Reporting it as usage would blame
    // whoever happened to run the next command for a file they never wrote.
    let stored_faults: [Corruption; 2] = [
        ("an over-long stored summary", |value| {
            value["summary"] = json!("x".repeat(engr::work::SUMMARY_MAX + 1));
        }),
        ("an empty stored item", |value| {
            value["items"][0]["text"] = json!("   ");
        }),
    ];
    for (what, corrupt) in stored_faults {
        restore();
        rewrite(&root, &id, corrupt);
        let error = work::load(&root, &id).expect_err(what);
        assert_eq!(
            error.code,
            engr::EXIT_SCHEMA,
            "{what} is stored data, not a caller mistake: {error}"
        );
    }

    // The same limits are still usage errors when a caller supplies them.
    restore();
    let error = work::set_summary(&root, &id, Some(&"x".repeat(engr::work::SUMMARY_MAX + 1)))
        .expect_err("too long to write");
    assert_eq!(error.code, engr::EXIT_USAGE);
}

/// `updated_at` is an instant, and is held to being one.
///
/// Two valid RFC3339 values written in different offsets do not compare
/// correctly as strings — the same class of bug already fixed for Backlog — and
/// the most recently touched sidecar is exactly what an ordering by it is for.
#[test]
fn updated_at_is_validated_and_compared_as_an_instant() {
    let (_dir, root) = workspace();
    let earlier = new_object(&root, "earlier");
    let later = new_object(&root, "later");
    work::start(&root, &earlier, None).expect("start");
    work::start(&root, &later, None).expect("start");

    // The same moment, written two ways: as text the offset one sorts lower,
    // as an instant it is two hours after.
    rewrite(&root, &earlier, |value| {
        value["updated_at"] = json!("2026-08-19T10:00:00Z");
    });
    rewrite(&root, &later, |value| {
        value["updated_at"] = json!("2026-08-19T09:00:00-03:00");
    });
    let earlier_at = work::load(&root, &earlier).expect("load").updated_at();
    let later_at = work::load(&root, &later).expect("load").updated_at();
    assert!(
        later_at > earlier_at,
        "12:00Z is after 10:00Z however it is spelled"
    );
    assert!(
        "2026-08-19T09:00:00-03:00" < "2026-08-19T10:00:00Z",
        "and comparing the text would have said the opposite"
    );

    // The stored spelling is preserved, not rewritten to a canonical one.
    assert_eq!(
        work::load(&root, &later).expect("load").updated_at,
        "2026-08-19T09:00:00-03:00"
    );

    for invalid in ["", "yesterday", "2026-08-19", "1755610000"] {
        rewrite(&root, &later, |value| {
            value["updated_at"] = json!(invalid);
        });
        let error = work::load(&root, &later).expect_err(invalid);
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{invalid:?}");
        assert!(error.message.contains("RFC3339"), "{error}");
    }
}
