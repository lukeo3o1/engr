//! What a plan is, and what grouping something in one does not mean.
//!
//! Collections are the third non-authoritative domain, and the one furthest
//! from the record: Backlog holds wording nobody confirmed, Work holds progress
//! nobody confirmed, and a Collection holds neither — only the claim that some
//! things belong together and in what order. So the rules pinned here are about
//! that claim staying small: membership changes nothing, priority belongs to the
//! membership rather than the target, completing a plan proves nothing about
//! what is in it, and a consumed member is shown rather than quietly repointed.

mod common;

use engr::backlog::Prepared;
use engr::collection::{self, Level, Priority, Schedule, State};
use engr::{backlog, ops, store};
use serde_json::{json, Value};
use std::path::Path;

use common::{new_object, workspace};

fn object_ref(id: &str) -> String {
    format!(
        "obj:{}",
        engr::reference::encode_uuid_str(id).expect("compact")
    )
}

fn backlog_ref(id: &str) -> String {
    format!(
        "backlog:{}",
        engr::reference::encode_uuid_str(id).expect("compact")
    )
}

/// A plan under a caller-chosen id derived from its title, so a test that makes
/// several plans does not have to invent keys.
fn plan(root: &Path, title: &str) -> collection::Collection {
    plan_id(root, &slug(title), title)
}

fn plan_id(root: &Path, id: &str, title: &str) -> collection::Collection {
    collection::create(root, id, title, None, None, engr::rules::Attempt::FIRST).expect("create")
}

/// The id grammar is `[a-z0-9][a-z0-9-]{0,31}`, so a title becomes one by
/// lowercasing, replacing anything else with a dash, and trimming to fit.
fn slug(title: &str) -> String {
    let mut out: String = title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    out.truncate(32);
    while out.starts_with('-') {
        out.remove(0);
    }
    if out.is_empty() {
        out.push('p');
    }
    out
}

/// Edit a stored plan the way a text editor would.
fn rewrite(root: &Path, id: &str, edit: impl FnOnce(&mut Value)) {
    let path = collection::path(root, id);
    let mut value: Value = store::read_json(&path).expect("read");
    edit(&mut value);
    write_raw(&path, &value).expect("write");
}

/// A plan has an identity of its own, and it says nothing.
///
/// Ten opaque characters: no date, no milestone number, no type. Every one of
/// those would be a fact that can stop being true while the id cannot change,
/// and renaming a plan must not make it a different plan.
#[test]
fn a_collection_id_is_the_callers_stable_key_independent_of_the_title() {
    let (_dir, root) = workspace();
    let item = plan_id(&root, "auth-q3", "Q3 authentication");
    assert_eq!(item.id, "auth-q3", "the caller chose it");
    engr::reference::validate_collection_id(&item.id).expect("the id grammar");
    assert_eq!(item.state, State::Open, "a new plan is being pursued");

    // The canonical reference form resolves, and the id survives a rename.
    engr::reference::EngrRef::parse_standalone(&format!("engr:collection:{}", item.id))
        .expect("a collection reference");
    let renamed = collection::rename(
        &root,
        &item.id,
        "Q4 authentication",
        engr::rules::Attempt::FIRST,
    )
    .expect("rename");
    assert_eq!(renamed.id, item.id);
    assert_eq!(renamed.title, "Q4 authentication");

    // Ids are unique, and a prefix resolves the way object ids do.
    let other = plan_id(&root, "billing", "another plan");
    assert_ne!(other.id, item.id);
    assert_eq!(
        collection::resolve_id(&root, &item.id[..6]).expect("prefix"),
        item.id
    );
    assert_eq!(
        collection::resolve_id(&root, &format!("engr:collection:{}", item.id)).expect("reference"),
        item.id
    );
    let missing = collection::resolve_id(&root, "zzzzzz").expect_err("no such plan");
    assert_eq!(missing.code, engr::EXIT_NOT_FOUND);

    // The filename is the identity; a file that disagrees is two identities.
    rewrite(&root, &item.id, |value| {
        value["id"] = json!(other.id);
    });
    let error = collection::load(&root, &item.id).expect_err("one plan, one identity");
    assert_eq!(error.code, engr::EXIT_SCHEMA);

    // And no `format`/`version` of its own: the workspace answers that once.
    let raw: Value = store::read_json(&collection::path(&root, &other.id)).expect("raw");
    assert!(raw.get("format").is_none(), "{raw}");
    assert!(raw.get("version").is_none(), "{raw}");
}

#[test]
fn a_current_collection_refuses_an_explicit_null_optional_member() {
    let (_dir, root) = workspace();
    let collection = plan(&root, "one spelling");
    rewrite(&root, &collection.id, |value| {
        value["description"] = Value::Null;
    });

    let error = collection::load(&root, &collection.id)
        .expect_err("the writer omits an absent description");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("exact shape"), "{error}");
}

/// Planning state is declared, never inferred — and it is only about the plan.
///
/// `completed` does not claim its members are resolved. A milestone can be
/// finished with work deliberately deferred or moved out of scope, and a plan
/// that could only be completed once everything in it was would be a plan
/// nobody could ever close honestly.
#[test]
fn completing_a_plan_declares_nothing_about_its_members() {
    let (_dir, root) = workspace();
    let object = new_object(&root, "still being worked on");
    let item = plan(&root, "Q3");
    collection::add_member(
        &root,
        &item.id,
        &object_ref(&object),
        None,
        None,
        engr::rules::Attempt::FIRST,
    )
    .expect("add");

    let before = ops::effective(&root, &object).expect("object");
    let completed = collection::set_state(
        &root,
        &item.id,
        State::Completed,
        engr::rules::Attempt::FIRST,
    )
    .expect("complete");
    assert_eq!(completed.state, State::Completed);

    let after = ops::effective(&root, &object).expect("object");
    assert_eq!(after.state, before.state, "the member did not move");
    assert_eq!(after.rev, before.rev, "and no event was appended");
    assert!(
        after.needs_attention(),
        "a completed plan can hold work that still needs attention"
    );

    // Cancelled is a different fact from completed, and both are storable.
    let cancelled = collection::set_state(
        &root,
        &item.id,
        State::Cancelled,
        engr::rules::Attempt::FIRST,
    )
    .expect("cancel");
    assert_eq!(cancelled.state, State::Cancelled);
    for invalid in ["archived", "closed", "open ", ""] {
        rewrite(&root, &item.id, |value| {
            value["state"] = json!(invalid);
        });
        assert!(
            collection::load(&root, &item.id).is_err(),
            "{invalid:?} is not a planning state"
        );
    }
}

/// Membership is a set, and a rank is a position only one member can hold.
#[test]
fn one_plan_holds_a_target_once_and_a_rank_once() {
    let (_dir, root) = workspace();
    let first = new_object(&root, "first");
    let second = new_object(&root, "second");
    let item = plan(&root, "ordering");

    collection::add_member(
        &root,
        &item.id,
        &object_ref(&first),
        Some(10),
        None,
        engr::rules::Attempt::FIRST,
    )
    .expect("add");
    collection::add_member(
        &root,
        &item.id,
        &object_ref(&second),
        Some(20),
        None,
        engr::rules::Attempt::FIRST,
    )
    .expect("add");

    let twice = collection::add_member(
        &root,
        &item.id,
        &object_ref(&first),
        None,
        None,
        engr::rules::Attempt::FIRST,
    )
    .expect_err("one plan holds it once");
    assert_eq!(twice.code, engr::EXIT_INVARIANT);

    let clash = collection::set_order(
        &root,
        &item.id,
        &object_ref(&second),
        Some(10),
        engr::rules::Attempt::FIRST,
    )
    .expect_err("two members cannot both be tenth");
    assert_eq!(clash.code, engr::EXIT_SCHEMA);
    assert!(
        clash.message.contains("says nothing at that point"),
        "{clash}"
    );

    // Unranked is a real answer, and any number of members may share it.
    let third = new_object(&root, "third");
    collection::add_member(
        &root,
        &item.id,
        &object_ref(&third),
        None,
        None,
        engr::rules::Attempt::FIRST,
    )
    .expect("add");
    let unranked = collection::set_order(
        &root,
        &item.id,
        &object_ref(&second),
        None,
        engr::rules::Attempt::FIRST,
    )
    .expect("unranking is allowed");
    assert_eq!(
        unranked
            .members
            .iter()
            .filter(|member| member.order.is_none())
            .count(),
        2
    );

    // Reading order is by rank, then the unranked — never array position.
    let planned: Vec<Option<i64>> = unranked
        .planned()
        .iter()
        .map(|member| member.order)
        .collect();
    assert_eq!(planned, vec![Some(10), None, None]);

    // Stored data is held to both rules too.
    rewrite(&root, &item.id, |value| {
        value["members"][1]["order"] = json!(10);
    });
    assert!(collection::load(&root, &item.id).is_err(), "a shared rank");
}

/// Priority belongs to the membership, not to the thing.
///
/// The same Object can be urgent in one plan and incidental in another, and a
/// priority stored on the Object would make those two plans argue. `reason` is
/// planning rationale — why it matters *here* — and never engineering
/// rationale, which has one home and a gate in front of it.
#[test]
fn priority_belongs_to_the_membership_and_not_to_the_target() {
    let (_dir, root) = workspace();
    let object = new_object(&root, "shared between plans");
    let urgent = plan(&root, "this quarter");
    let later = plan(&root, "someday");
    let target = object_ref(&object);

    collection::add_member(
        &root,
        &urgent.id,
        &target,
        Some(1),
        Some(Priority {
            level: Level::High,
            reason: Some("Blocks the rest of the milestone.".to_owned()),
        }),
        engr::rules::Attempt::FIRST,
    )
    .expect("add");
    collection::add_member(
        &root,
        &later.id,
        &target,
        None,
        Some(Priority {
            level: Level::Low,
            reason: None,
        }),
        engr::rules::Attempt::FIRST,
    )
    .expect("add");

    assert_eq!(
        collection::load(&root, &urgent.id).expect("load").members[0]
            .priority
            .as_ref()
            .expect("priority")
            .level,
        Level::High
    );
    assert_eq!(
        collection::load(&root, &later.id).expect("load").members[0]
            .priority
            .as_ref()
            .expect("priority")
            .level,
        Level::Low
    );

    // Nothing about either landed on the Object.
    let raw: Value = store::read_json(&store::object_path(&root, &object)).expect("raw");
    let text = raw.to_string();
    assert!(!text.contains("priority"), "{raw}");
    assert!(!text.contains("collection"), "{raw}");

    // A reason without a level is not a priority, and clearing works.
    let cleared = collection::set_priority(
        &root,
        &urgent.id,
        &target,
        None,
        engr::rules::Attempt::FIRST,
    )
    .expect("clear");
    assert!(cleared.members[0].priority.is_none());
    rewrite(&root, &urgent.id, |value| {
        value["members"][0]["priority"] = json!({"reason": "no level"});
    });
    assert!(
        collection::load(&root, &urgent.id).is_err(),
        "a priority is its level"
    );
}

/// A schedule is planning context, not a scheduler.
#[test]
fn a_schedule_is_calendar_dates_and_never_a_timestamp() {
    let (_dir, root) = workspace();
    let dated = collection::create(
        &root,
        "dated",
        "with dates",
        None,
        Some(Schedule {
            start: Some("2026-07-01".to_owned()),
            end: Some("2026-09-30".to_owned()),
            target: Some("2026-09-15".to_owned()),
        }),
        engr::rules::Attempt::FIRST,
    )
    .expect("create");
    let schedule = dated.schedule.as_ref().expect("schedule");
    assert_eq!(schedule.start.as_deref(), Some("2026-07-01"));

    // Each field is independent, and one is enough.
    for single in [
        Schedule {
            start: Some("2026-07-01".to_owned()),
            ..Schedule::default()
        },
        Schedule {
            end: Some("2026-07-01".to_owned()),
            ..Schedule::default()
        },
        Schedule {
            target: Some("2026-07-01".to_owned()),
            ..Schedule::default()
        },
    ] {
        collection::set_schedule(&root, &dated.id, Some(single), engr::rules::Attempt::FIRST)
            .expect("one date is a schedule");
    }
    // A target need not sit between start and end: it is an intention.
    collection::set_schedule(
        &root,
        &dated.id,
        Some(Schedule {
            start: Some("2026-07-01".to_owned()),
            end: Some("2026-09-30".to_owned()),
            target: Some("2026-12-01".to_owned()),
        }),
        engr::rules::Attempt::FIRST,
    )
    .expect("a target outside the window is not a contradiction");

    // Absent is valid; present-and-empty is not.
    let cleared = collection::set_schedule(&root, &dated.id, None, engr::rules::Attempt::FIRST)
        .expect("clear");
    assert!(cleared.schedule.is_none());
    let empty = collection::set_schedule(
        &root,
        &dated.id,
        Some(Schedule::default()),
        engr::rules::Attempt::FIRST,
    )
    .expect_err("an empty schedule");
    assert_eq!(empty.code, engr::EXIT_USAGE);

    for (what, schedule) in [
        (
            "a start after its end",
            Schedule {
                start: Some("2026-09-30".to_owned()),
                end: Some("2026-07-01".to_owned()),
                target: None,
            },
        ),
        (
            "a timestamp",
            Schedule {
                start: Some("2026-07-01T00:00:00Z".to_owned()),
                ..Schedule::default()
            },
        ),
        (
            "an unpadded date",
            Schedule {
                start: Some("2026-7-1".to_owned()),
                ..Schedule::default()
            },
        ),
        (
            "a date that does not exist",
            Schedule {
                end: Some("2026-02-30".to_owned()),
                ..Schedule::default()
            },
        ),
    ] {
        let error = collection::set_schedule(
            &root,
            &dated.id,
            Some(schedule),
            engr::rules::Attempt::FIRST,
        )
        .expect_err(what);
        assert_eq!(error.code, engr::EXIT_USAGE, "{what}");
    }
}

/// A consumed Backlog member is shown, never silently repointed.
///
/// Backlog resolution is not one-to-one: a point can be settled by two Objects,
/// by none, or by something nobody recorded. Retargeting the member at whatever
/// the work became would change what the plan says while nobody was looking.
#[test]
fn a_member_that_stops_existing_is_surfaced_rather_than_retargeted() {
    let (_dir, root) = workspace();
    let item = backlog::create(
        &root,
        "refresh strategy",
        "offline may invalidate it",
        Vec::new(),
        &Prepared::first(),
    )
    .expect("backlog");
    let plan = plan(&root, "with a backlog member");
    let target = backlog_ref(&item.id);
    collection::add_member(
        &root,
        &plan.id,
        &target,
        Some(10),
        None,
        engr::rules::Attempt::FIRST,
    )
    .expect("add");

    // The point is settled and removed from staging.
    backlog::consume_section(
        &root,
        &item.id,
        1,
        &Prepared::first()
            .against(backlog::Precondition::section(&root, &item.id, 1).expect("observe")),
    )
    .expect("consume");
    assert!(backlog::load(&root, &item.id).is_err());

    // The plan still says what it said. Nothing was rewritten.
    let after = collection::load(&root, &plan.id).expect("a dangling member is not corruption");
    assert_eq!(after.members.len(), 1);
    assert_eq!(after.members[0].target.reference, target);

    // Removing it is an explicit act, not something that happened by itself.
    let emptied = collection::remove_member(&root, &plan.id, &target, engr::rules::Attempt::FIRST)
        .expect("remove");
    assert!(emptied.members.is_empty());
}

/// A plan groups whole resources, and only ones that exist.
#[test]
fn members_are_whole_objects_and_backlog_items() {
    let (_dir, root) = workspace();
    let object = new_object(&root, "a member");
    let item = plan(&root, "targets");
    let compact = engr::reference::encode_uuid_str(&object).expect("compact");

    collection::add_member(
        &root,
        &item.id,
        &object_ref(&object),
        None,
        None,
        engr::rules::Attempt::FIRST,
    )
    .expect("legal");

    for refused in [
        format!("obj:{compact}:1"),
        format!("collection:{}", item.id),
        format!("obj:{compact}@HEAD"),
        format!("engr:obj:{compact}"),
        "obj:not-a-compact-id".to_owned(),
    ] {
        let error = collection::add_member(
            &root,
            &item.id,
            &refused,
            None,
            None,
            engr::rules::Attempt::FIRST,
        )
        .expect_err("a plan does not group that");
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{refused}");
    }
}

/// Deleting a plan is the agent's rule to follow, and engr says what went.
///
/// #10 makes "an agent MUST NOT delete a Collection unless explicitly directed
/// by a human" normative agent behaviour, and says a stronger technical guard
/// can be added later if real use shows one is needed. So this does not refuse:
/// engr cannot tell who asked, and inventing the guard now is exactly what the
/// issue deferred. What it does is report the planning context that went with
/// it.
#[test]
fn deleting_a_plan_is_carried_out_and_reported_rather_than_refused() {
    let (_dir, root) = workspace();
    let object = new_object(&root, "a member");
    let item = collection::create(
        &root,
        "auth-q3",
        "Q3 authentication",
        None,
        None,
        engr::rules::Attempt::FIRST,
    )
    .expect("create");
    collection::add_member(
        &root,
        &item.id,
        &object_ref(&object),
        None,
        None,
        engr::rules::Attempt::FIRST,
    )
    .expect("add");

    let removed = collection::remove(&root, &item.id, engr::rules::Attempt::FIRST)
        .expect("the rule is the agent's");
    assert_eq!(removed.title, "Q3 authentication");
    assert_eq!(removed.members, 1, "and what went with it is reported");
    assert!(collection::load(&root, &item.id).is_err());

    // The member is exactly where it was.
    let after = ops::effective(&root, &object).expect("object");
    assert_eq!(after.rev, 1);
    assert!(after.needs_attention());
}

/// Stored plans are held to what the write path can produce.
#[test]
fn a_hand_edited_plan_outside_the_schema_is_refused_rather_than_repaired() {
    let (_dir, root) = workspace();
    let object = new_object(&root, "a member");
    let item = collection::create(
        &root,
        "schema",
        "schema",
        Some("what this plan covers"),
        Some(Schedule {
            start: Some("2026-07-01".to_owned()),
            ..Schedule::default()
        }),
        engr::rules::Attempt::FIRST,
    )
    .expect("create");
    collection::add_member(
        &root,
        &item.id,
        &object_ref(&object),
        Some(10),
        Some(Priority {
            level: Level::High,
            reason: Some("first".to_owned()),
        }),
        engr::rules::Attempt::FIRST,
    )
    .expect("add");

    type Corruption = (&'static str, fn(&mut Value));
    let corruptions: [Corruption; 8] = [
        ("an unknown top-level field", |value| {
            value["owner"] = json!("someone");
        }),
        ("an unknown member field", |value| {
            value["members"][0]["estimate"] = json!("2d");
        }),
        ("an unknown priority field", |value| {
            value["members"][0]["priority"]["weight"] = json!(3);
        }),
        ("an unknown schedule field", |value| {
            value["schedule"]["due"] = json!("2026-07-01");
        }),
        ("a priority level outside the vocabulary", |value| {
            value["members"][0]["priority"]["level"] = json!("urgent");
        }),
        ("a member kind a plan does not group", |value| {
            value["members"][0]["target"]["ref"] = json!("collection:0123456789");
        }),
        ("an omitted members list", |value| {
            value.as_object_mut().expect("object").remove("members");
        }),
        ("a name that is only whitespace", |value| {
            value["name"] = json!("   ");
        }),
    ];

    let sound: Value = store::read_json(&collection::path(&root, &item.id)).expect("read");
    for (what, corrupt) in corruptions {
        write_raw(&collection::path(&root, &item.id), &sound).expect("restore");
        rewrite(&root, &item.id, corrupt);
        let error = collection::load(&root, &item.id).expect_err(what);
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{what}: {error}");
    }
}

/// A member that never existed is a plan silently covering nothing.
///
/// The distinction the check has to keep is between *never existed* and *stopped
/// existing*. A typo at add time is a mistake nothing later will report; a
/// consumed backlog item is legitimate planning context. So existence is
/// required when a member is added and never asked again — and it is required by
/// the domain, not by whichever caller happened to be used.
#[test]
fn a_member_must_exist_when_it_is_added_whichever_door_it_comes_through() {
    let (_dir, root) = workspace();
    let item = plan(&root, "targets");

    // Well-formed, canonical, and naming a UUID nothing ever created.
    let absent = object_ref(&engr::model::new_id());
    let error = collection::add_member(
        &root,
        &item.id,
        &absent,
        None,
        None,
        engr::rules::Attempt::FIRST,
    )
    .expect_err("a plan cannot cover something that was never there");
    assert_eq!(error.code, engr::EXIT_NOT_FOUND);
    assert!(error.message.contains("does not exist"), "{error}");

    let absent_item = backlog_ref(&engr::model::new_id());
    let error = collection::add_member(
        &root,
        &item.id,
        &absent_item,
        None,
        None,
        engr::rules::Attempt::FIRST,
    )
    .expect_err("the same for staging");
    assert_eq!(error.code, engr::EXIT_NOT_FOUND);

    assert!(
        collection::load(&root, &item.id)
            .expect("load")
            .members
            .is_empty(),
        "and nothing was written"
    );

    // Malformed is still refused as malformed, not as missing: the shape check
    // runs first, so the error names the real problem.
    let error = collection::add_member(
        &root,
        &item.id,
        "obj:not-compact",
        None,
        None,
        engr::rules::Attempt::FIRST,
    )
    .expect_err("a malformed reference");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
}

/// A stored name is exactly what the write path emits.
///
/// `create` and `rename` trim, so a stored name never carries surrounding
/// whitespace — and a reader accepting one would be accepting a spelling the API
/// cannot produce. Two names a listing can only tell apart by alignment is the
/// shadow schema the other domains were already closed against.
#[test]
fn a_stored_collection_title_carries_no_surrounding_whitespace() {
    let (_dir, root) = workspace();
    let item = collection::create(
        &root,
        "auth-q3",
        "  Q3 authentication  ",
        None,
        None,
        engr::rules::Attempt::FIRST,
    )
    .expect("create");
    assert_eq!(
        item.title, "Q3 authentication",
        "the write path trims, and that is the existing behaviour"
    );
    let renamed =
        collection::rename(&root, &item.id, "  Q4  ", engr::rules::Attempt::FIRST).expect("rename");
    assert_eq!(renamed.title, "Q4");

    for stored in ["  Q3 ", "Q3 ", " Q3", "Q3\t"] {
        rewrite(&root, &item.id, |value| {
            value["title"] = json!(stored);
        });
        let error = collection::load(&root, &item.id).expect_err(stored);
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{stored:?}");
        assert!(error.message.contains("surrounding whitespace"), "{error}");
    }

    // The trimmed spelling of the same title is fine, which is the point: one
    // title, one way of storing it.
    rewrite(&root, &item.id, |value| {
        value["title"] = json!("Q3");
    });
    assert_eq!(collection::load(&root, &item.id).expect("load").title, "Q3");
}

/// Admission is decided inside the workspace lock, not before it.
///
/// A member that goes dangling *after* it was admitted is intended, and the test
/// above pins that. This is the other case: a target that stopped existing
/// *before* the membership was written. Backlog consumption takes the same
/// workspace lock a collection edit takes, so an existence check outside that
/// lock can observe a live target, lose the race, and then persist a membership
/// that was never admissible.
///
/// Made deterministic by the lock itself rather than by timing. The target is
/// deleted while this thread holds the lock, so whenever `add_member` acquires
/// it the target is already gone — no interleaving exists in which the check,
/// done inside, can see it. The sleep only makes the *pre-fix* failure reliable
/// by giving the outside check time to run first; the assertion below does not
/// depend on it.
#[test]
fn a_target_consumed_before_the_membership_is_written_is_not_admitted() {
    let (_dir, root) = workspace();
    let item = backlog::create(
        &root,
        "still staging",
        "for now",
        Vec::new(),
        &Prepared::first(),
    )
    .expect("backlog");
    let plan = plan(&root, "racing a consumer");
    let target = backlog_ref(&item.id);
    let path = backlog::item_path(&root, &item.id);

    let outcome = store::with_lock(&root, || {
        let racer = std::thread::spawn({
            let root = root.clone();
            let plan = plan.id.clone();
            let target = target.clone();
            move || {
                collection::add_member(
                    &root,
                    &plan,
                    &target,
                    None,
                    None,
                    engr::rules::Attempt::FIRST,
                )
            }
        });
        std::thread::sleep(std::time::Duration::from_millis(200));
        std::fs::remove_file(&path).expect("consume the target");
        Ok(racer)
    })
    .expect("locked")
    .join()
    .expect("joined");

    let error = outcome.expect_err("the target was gone before the membership was written");
    assert_eq!(error.code, engr::EXIT_NOT_FOUND);
    assert!(
        collection::load(&root, &plan.id)
            .expect("plan")
            .members
            .is_empty(),
        "nothing that was never admissible may be persisted"
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
