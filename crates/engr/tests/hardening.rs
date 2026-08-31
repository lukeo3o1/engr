//! Phase-4 hardening: what holds when several writers arrive at once.
//!
//! The domains each have their own tests, and those run one caller at a time.
//! This file asks the question those cannot: with the workspace lock as the only
//! thing between them, does concurrent work across *different* domains leave
//! every resource valid and every history readable?
//!
//! Nothing here invents persisted design. A finding that would need a frozen
//! contract to change belongs on its owning issue under #32's stop rule.

mod common;

use common::{add, admit, new_object, wording, workspace};
use engr::rules::Attempt;
use engr::{backlog, collection, gate, integrity, ops, store, work};
use std::path::Path;
use std::sync::{Arc, Barrier};

fn attempt() -> Attempt {
    Attempt::new(1).expect("the first attempt")
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
                admit(&root, add(&id, wording(&format!("wording {n}"))));
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
    let plan =
        collection::create(&root, "plan", "plan", None, None, attempt()).expect("collection");
    work::start(&root, &obj(&id), None, attempt()).expect("work sidecar");

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
                    0 => {
                        admit(&root, add(&id, wording("admitted under contention")));
                    }
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
                        work::add_item(&root, &obj(&id), "remembered under contention", attempt())
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
        work::load(&root, &obj(&id)).expect("work").items.len(),
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
                    let Ok(prepared) = gate::prepare(&root, add(&id, wording(&format!("w{n}"))))
                    else {
                        return false;
                    };
                    gate::confirm(&root, &format!("CONFIRM {}", prepared.candidate.code())).is_ok()
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

/// The three candidate states are reachable, distinct, and each one behaves.
///
/// Phase 4, scope item 3. Only `Stale` was asserted anywhere before this:
/// `Pending` and `AlreadyApplied` were reached incidentally by tests about
/// something else, so nothing pinned what they mean. They are the states a
/// crash and a race leave behind, which makes them exactly the ones worth
/// stating.
#[test]
fn the_candidate_states_are_reachable_and_each_one_behaves() {
    let (dir, root) = workspace();
    let id = new_object(&root, "candidate states");

    // Pending: freshly prepared, nothing has moved under it.
    let prepared = gate::prepare(&root, add(&id, wording("pending"))).expect("prepare");
    assert!(matches!(
        gate::candidate_state(&root, &prepared.candidate).expect("classify"),
        gate::CandidateState::Pending
    ));

    // AlreadyApplied: the Event landed and the envelope outlived it, which is
    // what a crash between the append and the envelope's removal leaves behind.
    let path = store::challenge_path(&root, prepared.candidate.code()).expect("path");
    let envelope = std::fs::read(&path).expect("candidate bytes");
    gate::confirm(&root, &format!("CONFIRM {}", prepared.candidate.code())).expect("confirm");
    std::fs::write(&path, &envelope).expect("restore what a crash would have left");
    let restored = gate::find(&root, prepared.candidate.code()).expect("it still reads");
    assert!(matches!(
        gate::candidate_state(&root, &restored).expect("classify"),
        gate::CandidateState::AlreadyApplied(_)
    ));

    // And confirming it again is an idempotent retry, not a second admission —
    // the property that makes crash recovery safe to attempt blindly.
    let before = store::load_events(&root, &id).expect("history").len();
    gate::confirm(&root, &format!("CONFIRM {}", restored.code())).expect("idempotent retry");
    assert_eq!(
        store::load_events(&root, &id).expect("history").len(),
        before,
        "a retry admits nothing a second time"
    );

    // Stale: prepared against a predecessor something else then moved past. An
    // Agent rename advances the revision without superseding the envelope, so
    // the candidate is left describing a state that no longer exists.
    let stale = gate::prepare(&root, add(&id, wording("stale"))).expect("prepare");
    gate::admit_agent(&root, common::rename(&id, "an agent got there first"), None)
        .expect("agent rename");
    match gate::candidate_state(&root, &stale.candidate).expect("classify") {
        gate::CandidateState::Stale { current_rev } => assert!(
            current_rev > stale.candidate.expected_rev(),
            "stale means the object moved past what this candidate bound"
        ),
        other => panic!("expected a stale candidate, got {other:?}"),
    }
    gate::confirm(&root, &format!("CONFIRM {}", stale.candidate.code()))
        .expect_err("a stale candidate cannot be admitted");

    // It stays readable, though: a dead candidate must still explain itself.
    gate::find(&root, stale.candidate.code()).expect("and it still reads");
    assert_workspace_sound(&root);
    drop(dir);
}

/// The survey surface warns about untrusted material without moving its columns.
///
/// Phase 4, scope item 8. `ls --sections` is the surface people pipe into
/// `grep`, so the trust alarm goes to stderr and stdout stays byte for byte what
/// it was. That is a deliberate trade with two ways to regress, and neither was
/// pinned: move the warning to stdout and every script that splits on those
/// columns breaks; drop it and a survey presents material no admission path
/// accepted as though it were fine.
#[test]
fn the_survey_warns_on_stderr_and_leaves_its_columns_alone() {
    let (dir, root) = workspace();
    let id = new_object(&root, "surveyed object");
    admit(&root, add(&id, wording("wording that was admitted")));

    let clean = std::process::Command::new(env!("CARGO_BIN_EXE_engr"))
        .arg("--root")
        .arg(&root)
        .args(["ls", "--sections"])
        .output()
        .expect("run engr ls");
    assert!(clean.status.success(), "a sound workspace surveys cleanly");
    let columns_before = String::from_utf8_lossy(&clean.stdout).into_owned();
    assert!(
        String::from_utf8_lossy(&clean.stderr).is_empty(),
        "nothing to warn about yet"
    );

    // Edit the stored wording without resealing: schema-valid bytes that no
    // admission path produced.
    let path = store::object_path(&root, &id);
    let mut stored: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("object bytes"))
            .expect("object json");
    stored["sections"][0]["text"] = serde_json::Value::String("wording nobody admitted".to_owned());
    std::fs::write(
        &path,
        engr::proof::canonical_bytes(&stored, "tampered object").expect("canonical"),
    )
    .expect("tamper");

    let surveyed = std::process::Command::new(env!("CARGO_BIN_EXE_engr"))
        .arg("--root")
        .arg(&root)
        .args(["ls", "--sections"])
        .output()
        .expect("run engr ls");
    let stdout = String::from_utf8_lossy(&surveyed.stdout);
    let stderr = String::from_utf8_lossy(&surveyed.stderr);

    assert!(
        stderr.contains("cannot be trusted") && stderr.contains("engr verify"),
        "the survey says so, and says what to run:\n{stderr}"
    );
    assert!(
        !stdout.contains("cannot be trusted"),
        "the alarm does not reach the surface scripts parse:\n{stdout}"
    );
    assert_eq!(
        stdout.lines().count(),
        columns_before.lines().count(),
        "and the columns did not shift under the reader"
    );
    drop(dir);
}

/// A Work subject from a bare Object id, which is what these tests hold.
fn obj(id: &str) -> work::Subject {
    work::Subject::Object(id.to_owned())
}
