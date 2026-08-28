//! Phase-4 hardening: what holds when several writers arrive at once.
//!
//! The domains each have their own tests, and those run one caller at a time.
//! This file asks the question those cannot: with the workspace lock as the only
//! thing between them, does concurrent work across *different* domains leave
//! every resource valid and every history readable?
//!
//! Nothing here invents persisted design. A finding that would need a frozen
//! contract to change belongs on its owning issue under #32's stop rule.

use engr::model::{Action, Content, Payload};
use engr::rules::Attempt;
use engr::{backlog, collection, gate, integrity, ops, store, work};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use tempfile::TempDir;

fn workspace() -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().to_path_buf();
    store::init(&root).expect("init");
    (dir, root)
}

fn attempt() -> Attempt {
    Attempt::new(1).expect("the first attempt")
}

fn payload(action: Action, object: &str, text: &str) -> Payload {
    Payload {
        action,
        object: object.to_owned(),
        becomes: None,
        content: Content {
            text: text.to_owned(),
            ..Content::default()
        },
    }
}

fn admit(root: &Path, payload: Payload) {
    let prepared = gate::prepare(root, payload).expect("prepare");
    gate::confirm(root, &format!("CONFIRM {}", prepared.candidate.challenge)).expect("confirm");
}

fn new_object(root: &Path, title: &str) -> String {
    let id = engr::model::new_id();
    admit(root, payload(Action::ObjectCreated, &id, title));
    id
}

/// Every admitted Object still verifies, and its history still reads.
fn assert_workspace_sound(root: &Path) {
    for id in store::object_ids(root).expect("ids") {
        let object = store::load_object(root, &id).expect("every Object still loads");
        integrity::check_stored_object_integrity(&object).expect("and still verifies");
        let events = store::load_events(root, &id).expect("and its history still reads");
        let replayed = ops::effective(root, &id).expect("and still projects");
        assert_eq!(
            replayed.rev,
            events.last().map_or(0, |event| event.rev),
            "{id}: the projection and its history disagree about the revision"
        );
    }
}

/// Concurrent admissions on *distinct* Objects all land, and none corrupts another.
///
/// The existing gate test proves two callers cannot both take one predecessor.
/// This is the other half: the lock must serialize without losing work. A writer
/// that silently dropped its admission would pass a contention test that only
/// counts how many callers were refused.
#[test]
fn parallel_admissions_on_distinct_objects_all_land() {
    let (dir, root) = workspace();
    let ids: Vec<String> = (0..6)
        .map(|n| new_object(&root, &format!("object {n}")))
        .collect();

    let barrier = Arc::new(Barrier::new(ids.len()));
    std::thread::scope(|scope| {
        for (n, id) in ids.iter().enumerate() {
            let barrier = Arc::clone(&barrier);
            let root = root.clone();
            let id = id.clone();
            scope.spawn(move || {
                barrier.wait();
                admit(
                    &root,
                    payload(Action::SectionAdded, &id, &format!("wording {n}")),
                );
            });
        }
    });

    for id in &ids {
        let object = store::load_object(&root, id).expect("object");
        assert_eq!(
            object.sections.len(),
            1,
            "{id}: its own admission is the one that landed"
        );
        assert_eq!(object.rev, 2);
    }
    assert_workspace_sound(&root);
    drop(dir);
}

/// Authoritative admission and non-authoritative staging, at the same time.
///
/// Backlog, Collection and Work are admitted by nobody, but they share the
/// workspace lock with the gate. Nothing else exercises them arriving together:
/// each domain's own tests run alone, so a lock held for the wrong span would
/// only show up here — as a torn resource, a lost write, or an Object whose
/// projection stops matching its history.
#[test]
fn staging_and_admission_can_run_at_the_same_time() {
    let (dir, root) = workspace();
    let id = new_object(&root, "concurrent domains");
    let item = backlog::create(
        &root,
        "unresolved",
        "unresolved while the gate is busy",
        Vec::new(),
        &backlog::Prepared::attempt(attempt()),
    )
    .expect("backlog item");
    let plan = collection::create(&root, "plan", None, None, attempt()).expect("collection");
    work::start(&root, &id, None, attempt()).expect("work sidecar");

    let barrier = Arc::new(Barrier::new(4));
    std::thread::scope(|scope| {
        for task in 0..4 {
            let barrier = Arc::clone(&barrier);
            let root = root.clone();
            let id = id.clone();
            let item = item.id.clone();
            let plan = plan.id.clone();
            scope.spawn(move || {
                barrier.wait();
                match task {
                    0 => admit(
                        &root,
                        payload(Action::SectionAdded, &id, "admitted under contention"),
                    ),
                    1 => {
                        let prepared = backlog::Prepared::attempt(attempt()).against(
                            backlog::Precondition::section_absent(&root, &item)
                                .expect("observe the point"),
                        );
                        backlog::add_section(
                            &root,
                            &item,
                            "staged under contention",
                            Vec::new(),
                            &prepared,
                        )
                        .expect("backlog section");
                    }
                    2 => {
                        collection::set_state(
                            &root,
                            &plan,
                            engr::collection::State::Completed,
                            attempt(),
                        )
                        .expect("collection state");
                    }
                    _ => {
                        work::add_item(&root, &id, "remembered under contention", attempt())
                            .expect("work item");
                    }
                }
            });
        }
    });

    // Each domain kept its own write, and none of them is the others' business.
    let object = store::load_object(&root, &id).expect("object");
    assert_eq!(object.sections.len(), 1, "the admission landed");
    assert_eq!(
        backlog::load(&root, &item.id)
            .expect("backlog item")
            .sections
            .len(),
        2,
        "the point was created with a section, and the staged one joined it"
    );
    assert_eq!(
        collection::load(&root, &plan.id).expect("collection").state,
        engr::collection::State::Completed,
        "the collection state landed"
    );
    assert_eq!(
        work::load(&root, &id).expect("work").items.len(),
        1,
        "the work item landed"
    );
    assert_workspace_sound(&root);
    drop(dir);
}

/// Repeated contention on one Object leaves one coherent history.
///
/// Each caller prepares against whatever it finds and confirms; some lose the
/// race and are refused, which is correct. What must hold afterwards is that the
/// revisions are contiguous, every seal verifies, and the number of Sections
/// equals the number of admissions that actually succeeded — no gap, no
/// duplicate, no Section admitted twice under one revision.
#[test]
fn contention_on_one_object_leaves_a_contiguous_history() {
    let (dir, root) = workspace();
    let id = new_object(&root, "contended object");

    let barrier = Arc::new(Barrier::new(8));
    let landed: Vec<bool> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|n| {
                let barrier = Arc::clone(&barrier);
                let root = root.clone();
                let id = id.clone();
                scope.spawn(move || {
                    barrier.wait();
                    let Ok(prepared) =
                        gate::prepare(&root, payload(Action::SectionAdded, &id, &format!("w{n}")))
                    else {
                        return false;
                    };
                    gate::confirm(&root, &format!("CONFIRM {}", prepared.candidate.challenge))
                        .is_ok()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("thread"))
            .collect()
    });

    let admitted = landed.iter().filter(|ok| **ok).count();
    assert!(admitted >= 1, "at least one caller must get through");
    let events = store::load_events(&root, &id).expect("history");
    for (index, event) in events.iter().enumerate() {
        assert_eq!(
            event.rev,
            index as u64 + 1,
            "revisions are contiguous from 1, with no gap a crash would have left"
        );
    }
    let object = store::load_object(&root, &id).expect("object");
    assert_eq!(
        object.sections.len(),
        admitted,
        "one Section per admission that reported success, and no others"
    );
    assert_eq!(object.rev, events.len() as u64);
    assert_workspace_sound(&root);
    drop(dir);
}
