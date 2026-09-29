//! What a ChangeSet is: several Section mutations of one Object, reviewed once
//! and admitted together — and what it is not, which is a way around anything a
//! lone Agent mutation has to pass.

mod common;

use common::{agent_payload, wording, Act};
use engr::changeset::{self, Applied};
use engr::model::{Content, Payload};
use engr::semantics::Admission;
use engr::{gate, ops, proof, rules, store};
use std::path::Path;
use std::process::Command;

fn object_rule(root: &Path) {
    std::fs::create_dir_all(rules::dir(root)).expect("rules dir");
    std::fs::write(
        rules::dir(root).join("object-policy.md"),
        "---\nid: object-policy\napplies:\n  domains:\n    - object\n---\n\n# Object policy\n\nReview the exact mutation.\n",
    )
    .expect("rule");
}

fn add(object: &str, text: &str) -> Payload {
    agent_payload(Act::Add, object, wording(text))
}

fn revise(object: &str, section: u64, text: &str) -> Payload {
    agent_payload(Act::Revise(section), object, wording(text))
}

fn passed(plan: &gate::ChangeSetPlan) -> gate::ReviewAttestation {
    attest(plan, proof::ReviewResult::Passed)
}

fn attest(plan: &gate::ChangeSetPlan, result: proof::ReviewResult) -> gate::ReviewAttestation {
    gate::ReviewAttestation {
        review_digest: plan.digest.clone(),
        reviewed_rules: plan.rules.clone(),
        attempt: 1,
        result,
        explanation: None,
    }
}

/// One Agent mutation on its own, reviewed and admitted the ordinary way.
fn admit_alone(root: &Path, payload: Payload) {
    let needed = match gate::admit_agent(root, payload.clone(), None).expect("governed") {
        gate::AgentOutcome::NeedsReview(needed) => needed,
        gate::AgentOutcome::Admitted(_) => panic!("a governed mutation waits for its review"),
    };
    let review = gate::ReviewAttestation {
        review_digest: needed.digest.clone(),
        reviewed_rules: needed.rules.clone(),
        attempt: 1,
        result: proof::ReviewResult::Passed,
        explanation: None,
    };
    gate::admit_agent(root, payload, Some(review))
        .expect("reviewed")
        .admitted()
        .expect("admitted");
}

/// A governed Object with one Agent-admitted Section, and a ChangeSet on it.
///
/// Agent-admitted because an Agent may not rewrite wording a person admitted,
/// in a ChangeSet or out of one.
fn governed() -> (tempfile::TempDir, std::path::PathBuf, String, String) {
    let (temp, root) = common::workspace();
    let object = common::new_object(&root, "ChangeSet");
    object_rule(&root);
    admit_alone(&root, add(&object, "Admitted by an agent."));
    let changeset = changeset::create(&root, &object).expect("create").id;
    (temp, root, object, changeset)
}

fn rewrite(root: &Path, changeset: &changeset::ChangeSet) {
    let bytes = proof::stored_bytes(changeset, "changeset").expect("bytes");
    std::fs::write(file(root, &changeset.id), bytes).expect("write");
}

fn file(root: &Path, id: &str) -> std::path::PathBuf {
    changeset::dir(root).join(format!("{id}.json"))
}

#[test]
fn several_steps_are_reviewed_once_and_admitted_as_one_record_each() {
    let (_temp, root, object, id) = governed();
    let before = store::load_object(&root, &object).expect("before");

    changeset::add(&root, &id, add(&object, "One Object per ChangeSet.")).expect("step 1");
    changeset::add(&root, &id, add(&object, "Kept on this machine.")).expect("step 2");
    let (_, plan) = changeset::add(
        &root,
        &id,
        revise(&object, 1, "Revised by the same review."),
    )
    .expect("step 3");
    assert_eq!(plan.steps.len(), 3);
    assert_eq!(plan.rules, vec!["object-policy".to_owned()]);

    // Unreviewed, it is the subject and nothing else.
    match changeset::apply(&root, &id, None, &[]).expect("apply") {
        Applied::NeedsReview(needed) => assert_eq!(needed.digest, plan.digest),
        other => panic!("an unreviewed apply writes nothing: {other:?}"),
    }
    assert_eq!(
        store::load_object(&root, &object).expect("unchanged"),
        before
    );
    assert_eq!(store::load_events(&root, &object).expect("events").len(), 2);

    let (events, admitted) =
        match changeset::apply(&root, &id, Some(passed(&plan)), &[]).expect("apply") {
            Applied::Admitted { events, object, .. } => (events, object),
            other => panic!("a passing review admits: {other:?}"),
        };
    assert_eq!(
        events.len(),
        3,
        "one record per step, not one record for the batch"
    );
    assert_eq!(
        events.iter().map(|event| event.rev).collect::<Vec<_>>(),
        vec![before.rev + 1, before.rev + 2, before.rev + 3]
    );
    for event in &events {
        let admission = &event.metadata.admitted;
        assert_eq!(admission.by, Admission::Agent);
        let review = admission
            .review
            .as_ref()
            .expect("the review is recorded on every step");
        assert_eq!(review.result, proof::ReviewResult::Passed);
        assert_eq!(review.attempts, 1);
        assert_eq!(
            admission.at, events[0].metadata.admitted.at,
            "one act, one moment"
        );
    }
    assert_eq!(admitted.rev, before.rev + 3);
    assert_eq!(admitted.sections.len(), 3);
    assert_eq!(
        admitted.section(1).expect("§1").text,
        "Revised by the same review."
    );
    assert_eq!(
        store::load_object(&root, &object).expect("stored"),
        admitted
    );
    assert!(ops::verify(&root, &object).expect("verify").passed());
    assert!(!file(&root, &id).exists(), "an applied ChangeSet is gone");
}

#[test]
fn the_digest_names_every_step_and_their_order() {
    let (_temp, root, object, first) = governed();
    let second = changeset::create(&root, &object).expect("second").id;
    changeset::add(&root, &first, add(&object, "Alpha.")).expect("add");
    let (_, forward) = changeset::add(&root, &first, add(&object, "Beta.")).expect("add");
    changeset::add(&root, &second, add(&object, "Beta.")).expect("add");
    let (_, backward) = changeset::add(&root, &second, add(&object, "Alpha.")).expect("add");
    assert_ne!(
        forward.digest, backward.digest,
        "order is part of what was reviewed"
    );

    // A review of one sequence does not admit another.
    let error =
        changeset::apply(&root, &second, Some(passed(&forward)), &[]).expect_err("other subject");
    assert_eq!(error.code, engr::EXIT_INVARIANT);

    // Nor does it survive a step being taken out.
    let shorter = changeset::remove(&root, &first, 2).expect("remove");
    assert_eq!(shorter.steps.len(), 1);
    let error =
        changeset::apply(&root, &first, Some(passed(&forward)), &[]).expect_err("fewer steps");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert_eq!(store::load_events(&root, &object).expect("events").len(), 2);
}

#[test]
fn an_object_that_moved_after_the_review_refuses_the_whole_changeset() {
    let (_temp, root, object, id) = governed();
    changeset::add(&root, &id, add(&object, "Reviewed against rev 2.")).expect("add");
    let (_, plan) = changeset::add(&root, &id, add(&object, "Also against rev 2.")).expect("add");

    // Somebody else admits in between.
    admit_alone(&root, add(&object, "Admitted meanwhile."));
    let moved = store::load_object(&root, &object).expect("moved");

    let error = changeset::apply(&root, &id, Some(passed(&plan)), &[]).expect_err("stale review");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert_eq!(
        store::load_object(&root, &object).expect("unchanged"),
        moved
    );
    assert!(file(&root, &id).exists(), "the draft survives a refusal");

    // What it needs now is a review of what it would do now.
    let (_, fresh) = changeset::plan(&root, &id).expect("replan");
    assert_ne!(fresh.digest, plan.digest);
    assert!(matches!(
        changeset::apply(&root, &id, Some(passed(&fresh)), &[]).expect("apply"),
        Applied::Admitted { .. }
    ));
}

#[test]
fn a_step_that_would_be_refused_alone_is_refused_by_its_number() {
    let (_temp, root, object, id) = governed();
    changeset::add(&root, &id, add(&object, "Fine.")).expect("add");
    let error = changeset::add(&root, &id, revise(&object, 99, "No such Section."))
        .expect_err("nothing to revise");
    assert!(error.message.starts_with("step 2: "), "{}", error.message);
    // And it is not kept: a draft that cannot be applied is not a draft.
    assert_eq!(changeset::load(&root, &id).expect("load").steps.len(), 1);

    // The same size ceiling a lone mutation has.
    let long = "word ".repeat(5_000);
    let error = changeset::add(&root, &id, add(&object, &long)).expect_err("oversize");
    assert!(error.message.starts_with("step 2: "), "{}", error.message);

    // And a step that changes nothing is not a step.
    let error = changeset::add(&root, &id, revise(&object, 1, "Admitted by an agent."))
        .expect_err("no change");
    assert!(error.message.starts_with("step 2: "), "{}", error.message);
}

#[test]
fn a_changeset_carries_section_work_only() {
    let (_temp, root, object, id) = governed();
    let title = agent_payload(Act::Rename, &object, wording("Another title"));
    let close = agent_payload(Act::Close, &object, Content::default());
    for payload in [title, close] {
        let error = changeset::add(&root, &id, payload).expect_err("not section work");
        assert_eq!(error.code, engr::EXIT_USAGE);
    }
    let lifecycle = common::becoming(
        add(&object, "And classify it too."),
        engr::model::Destination {
            object_type: None,
            state: engr::semantics::State::Closed,
        },
    );
    let error = changeset::add(&root, &id, lifecycle).expect_err("no lifecycle move");
    assert_eq!(error.code, engr::EXIT_USAGE);

    let other = engr::model::new_id();
    let error = changeset::add(&root, &id, add(&other, "Elsewhere.")).expect_err("one Object");
    assert_eq!(error.code, engr::EXIT_USAGE);
    assert!(changeset::load(&root, &id).expect("load").steps.is_empty());
}

#[test]
fn a_review_passes_or_fails_each_step_and_says_which() {
    let (_temp, root, object, id) = governed();
    let (_, plan) = changeset::add(&root, &id, add(&object, "Under review.")).expect("add");
    let cases = [
        // A verdict that contradicts itself is a usage error, not a guess.
        (
            proof::ReviewResult::Passed,
            vec![1],
            engr::EXIT_USAGE,
            "names no failed step",
        ),
        (
            proof::ReviewResult::Failed,
            vec![],
            engr::EXIT_USAGE,
            "--failed-step",
        ),
        (
            proof::ReviewResult::Failed,
            vec![2],
            engr::EXIT_USAGE,
            "cannot have failed step 2",
        ),
        (
            proof::ReviewResult::Failed,
            vec![1, 1],
            engr::EXIT_USAGE,
            "twice",
        ),
        // And an exhausted step leaves by the single path, count and all.
        (
            proof::ReviewResult::Exhausted,
            vec![],
            engr::EXIT_INVARIANT,
            "does not start its count again",
        ),
    ];
    for (result, failed, code, says) in cases {
        let error = changeset::apply(&root, &id, Some(attest(&plan, result)), &failed)
            .expect_err("refused");
        assert_eq!(error.code, code, "{result:?} {failed:?}: {}", error.message);
        assert!(error.message.contains(says), "{}", error.message);
    }
    assert_eq!(store::load_events(&root, &object).expect("events").len(), 2);
    assert!(file(&root, &id).exists());
}

#[test]
fn the_steps_a_review_passed_are_admitted_and_the_ones_it_failed_stay() {
    let (_temp, root, object, id) = governed();
    let before = store::load_object(&root, &object).expect("before");
    changeset::add(&root, &id, add(&object, "First, passed.")).expect("add");
    changeset::add(&root, &id, add(&object, "Second, failed.")).expect("add");
    let (_, plan) = changeset::add(&root, &id, add(&object, "Third, passed.")).expect("add");

    let (events, admitted, remaining) = match changeset::apply(
        &root,
        &id,
        Some(attest(&plan, proof::ReviewResult::Failed)),
        &[2],
    )
    .expect("apply")
    {
        Applied::Admitted {
            events,
            object,
            remaining,
        } => (events, object, remaining),
        other => panic!("the steps that passed are admitted: {other:?}"),
    };
    assert_eq!(events.len(), 2);
    assert_eq!(remaining, 1);
    // The third step's number moved up when the second was left out, and its
    // review still holds: a creation's number is engr's to allocate.
    let texts: Vec<&str> = admitted.sections.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(
        texts,
        ["Admitted by an agent.", "First, passed.", "Third, passed."]
    );
    for event in &events {
        let review = event.metadata.admitted.review.as_ref().expect("review");
        assert_eq!(
            review.result,
            proof::ReviewResult::Passed,
            "what was admitted passed"
        );
    }
    assert_eq!(admitted.rev, before.rev + 2);
    assert!(ops::verify(&root, &object).expect("verify").passed());

    // What failed is still here, alone, against the Object as it now stands.
    let kept = changeset::load(&root, &id).expect("kept");
    assert_eq!(kept.steps.len(), 1);
    changeset::remove(&root, &id, 1).expect("fix it");
    let (_, again) = changeset::add(&root, &id, add(&object, "Second, fixed.")).expect("again");
    let mut second = passed(&again);
    second.attempt = 2;
    match changeset::apply(&root, &id, Some(second), &[]).expect("apply") {
        Applied::Admitted {
            events, remaining, ..
        } => {
            assert_eq!(events.len(), 1);
            assert_eq!(remaining, 0);
            assert_eq!(
                events[0]
                    .metadata
                    .admitted
                    .review
                    .as_ref()
                    .expect("review")
                    .attempts,
                2
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(!file(&root, &id).exists());
}

#[test]
fn a_review_that_failed_every_step_admits_nothing() {
    let (_temp, root, object, id) = governed();
    changeset::add(&root, &id, add(&object, "One.")).expect("add");
    let (_, plan) = changeset::add(&root, &id, add(&object, "Two.")).expect("add");
    match changeset::apply(
        &root,
        &id,
        Some(attest(&plan, proof::ReviewResult::Failed)),
        &[1, 2],
    )
    .expect("apply")
    {
        Applied::NoneAdmitted { remaining } => assert_eq!(remaining, 2),
        other => panic!("{other:?}"),
    }
    assert_eq!(store::load_events(&root, &object).expect("events").len(), 2);
    assert_eq!(changeset::load(&root, &id).expect("kept").steps.len(), 2);
}

#[test]
fn a_passed_step_that_needs_a_failed_one_is_not_admitted_without_it() {
    let (_temp, root, object, id) = governed();
    // Step 2 revises the Section step 1 adds, so it cannot stand without it.
    let (_, first) = changeset::add(&root, &id, add(&object, "Added here.")).expect("add");
    let added = first.after().sections.last().expect("added").id;
    let (_, plan) =
        changeset::add(&root, &id, revise(&object, added, "Then rewritten.")).expect("revise");
    let before = store::load_object(&root, &object).expect("before");
    let error = changeset::apply(
        &root,
        &id,
        Some(attest(&plan, proof::ReviewResult::Failed)),
        &[1],
    )
    .expect_err("depends on the failed step");
    assert!(error.message.starts_with("step 2: "), "{}", error.message);
    assert_eq!(
        store::load_object(&root, &object).expect("unchanged"),
        before
    );
    assert_eq!(changeset::load(&root, &id).expect("kept").steps.len(), 2);
}

#[test]
fn an_ungoverned_changeset_is_not_admitted() {
    let (_temp, root) = common::workspace();
    let object = common::object_with_section(&root, "No Rule", "Nothing governs this.");
    let id = changeset::create(&root, &object).expect("create").id;
    let (_, plan) = changeset::add(&root, &id, add(&object, "Unreviewable.")).expect("add");
    assert!(plan.rules.is_empty());
    let error = changeset::apply(&root, &id, None, &[]).expect_err("no Rule");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert_eq!(store::load_events(&root, &object).expect("events").len(), 2);
}

#[test]
fn a_step_may_not_reference_a_section_the_same_changeset_changes() {
    let (_temp, root) = common::repository();
    let object = common::new_object(&root, "Referenced");
    object_rule(&root);
    admit_alone(&root, add(&object, "The target."));
    common::git(&root, &["add", "-A"]);
    common::git(&root, &["commit", "-qm", "object"]);
    let head = common::head(&root);
    let reference = common::text_ref(&root, &object, 1, &head);
    let leaning = |text: &str| {
        let mut content = wording(text);
        content.refs = vec![reference.clone()];
        agent_payload(Act::Add, &object, content)
    };

    // Untouched by the ChangeSet, it is an ordinary reference.
    let id = changeset::create(&root, &object).expect("create").id;
    changeset::add(&root, &id, leaning("Stands on §1.")).expect("untouched target");

    // Once an earlier step rewrites it, the pin would be to wording already gone.
    let id = changeset::create(&root, &object).expect("create").id;
    changeset::add(&root, &id, revise(&object, 1, "The target, rewritten.")).expect("revise");
    let error = changeset::add(&root, &id, leaning("Stands on §1.")).expect_err("touched target");
    assert!(error.message.contains("step 2"), "{}", error.message);
    assert!(error.message.contains("§1"), "{}", error.message);
}

#[test]
fn the_draft_is_local_and_is_never_committed() {
    let (_temp, root) = common::repository();
    let object = common::object_with_section(&root, "Drafted", "Committed wording.");
    object_rule(&root);
    common::git(&root, &["add", "-A"]);
    common::git(&root, &["commit", "-qm", "object"]);
    let id = changeset::create(&root, &object).expect("create").id;
    changeset::add(&root, &id, add(&object, "Not a record yet.")).expect("add");

    assert!(file(&root, &id).starts_with(store::local_dir(&root)));
    assert_eq!(
        common::git(&root, &["status", "--porcelain"]),
        "",
        "a ChangeSet is intent nobody admitted; nothing about it travels with the repository"
    );
    assert!(ops::verify(&root, &object).expect("verify").passed());
}

#[test]
fn an_interrupted_apply_is_found_admitted_or_not_and_never_admitted_twice() {
    let (_temp, root, object, id) = governed();
    let (_, plan) = changeset::add(&root, &id, add(&object, "Published once.")).expect("add");
    let draft = std::fs::read(file(&root, &id)).expect("draft");
    let events = match changeset::apply(&root, &id, Some(passed(&plan)), &[]).expect("apply") {
        Applied::Admitted { events, .. } => events,
        other => panic!("{other:?}"),
    };
    let admitted = store::load_object(&root, &object).expect("admitted");

    // The crash after the records were published and before the draft was
    // removed: the draft is back, holding the ids it was about to publish.
    std::fs::write(file(&root, &id), draft).expect("restore");
    let mut interrupted = changeset::load(&root, &id).expect("load");
    interrupted.committing = Some(changeset::Committing {
        events: events.iter().map(|event| event.id.clone()).collect(),
        steps: vec![0],
    });
    rewrite(&root, &interrupted);

    for refused in [
        changeset::add(&root, &id, add(&object, "More.")).map(|_| ()),
        changeset::remove(&root, &id, 1).map(|_| ()),
        changeset::plan(&root, &id).map(|_| ()),
        changeset::discard(&root, &id).map(|_| ()),
    ] {
        let error = refused.expect_err("an interrupted apply is settled first");
        assert!(
            error.message.contains("changeset apply"),
            "{}",
            error.message
        );
    }

    match changeset::apply(&root, &id, Some(passed(&plan)), &[]).expect("resume") {
        Applied::AlreadyAdmitted {
            events: found,
            remaining,
        } => {
            assert_eq!(found.len(), 1);
            assert_eq!(remaining, 0);
        }
        other => panic!("it was admitted, and must not be admitted again: {other:?}"),
    }
    assert_eq!(store::load_object(&root, &object).expect("once"), admitted);
    assert!(!file(&root, &id).exists());

    // The crash before anything was published: the ids are nowhere, so the
    // attempt did not happen and the ChangeSet goes the ordinary way.
    let id = changeset::create(&root, &object).expect("create").id;
    let (_, plan) = changeset::add(&root, &id, add(&object, "Never published.")).expect("add");
    let mut interrupted = changeset::load(&root, &id).expect("load");
    interrupted.committing = Some(changeset::Committing {
        events: vec![engr::model::new_id()],
        steps: vec![0],
    });
    rewrite(&root, &interrupted);
    assert!(matches!(
        changeset::apply(&root, &id, Some(passed(&plan)), &[]).expect("apply"),
        Applied::Admitted { .. }
    ));

    // Some of them and not the others is not a crash this could have left.
    let id = changeset::create(&root, &object).expect("create").id;
    changeset::add(&root, &id, add(&object, "Half there.")).expect("add");
    let mut interrupted = changeset::load(&root, &id).expect("load");
    interrupted.committing = Some(changeset::Committing {
        events: vec![events[0].id.clone(), engr::model::new_id()],
        steps: vec![0],
    });
    rewrite(&root, &interrupted);
    let error = changeset::apply(&root, &id, None, &[]).expect_err("partial");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
}

#[test]
fn discarding_a_changeset_changes_nothing_else() {
    let (_temp, root, object, id) = governed();
    changeset::add(&root, &id, add(&object, "Thrown away.")).expect("add");
    let before = store::load_object(&root, &object).expect("before");
    changeset::discard(&root, &id).expect("discard");
    assert!(!file(&root, &id).exists());
    assert!(changeset::list(&root).expect("list").is_empty());
    assert_eq!(store::load_object(&root, &object).expect("after"), before);
}

#[test]
fn a_changeset_is_named_by_any_unique_prefix() {
    let (_temp, root, object, id) = governed();
    let other = changeset::create(&root, &object).expect("other").id;
    let shared = id
        .chars()
        .zip(other.chars())
        .take_while(|(left, right)| left == right)
        .count();
    assert_eq!(
        changeset::resolve(&root, &id[..=shared]).expect("unique"),
        id
    );
    let error = changeset::resolve(&root, &id[..shared]).expect_err("ambiguous");
    assert_eq!(error.code, engr::EXIT_USAGE);
    let error = changeset::resolve(&root, "ffffffff").expect_err("none");
    assert_eq!(error.code, engr::EXIT_NOT_FOUND);
}

fn engr(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_engr"))
        .current_dir(root)
        .args(args)
        .output()
        .expect("engr")
}

#[test]
fn the_command_line_shows_the_subject_and_admits_it_once() {
    let (_temp, root) = common::repository();
    let object = common::object_with_section(&root, "Command line", "Committed wording.");
    object_rule(&root);
    common::git(&root, &["add", "-A"]);
    common::git(&root, &["commit", "-qm", "object"]);

    let created = engr(&root, &["changeset", "new", "--object", &object]);
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let id = changeset::list(&root).expect("list")[0].id.clone();

    // A step borrows prepare's vocabulary, not its admission flags.
    for refused in [
        vec!["--object", object.as_str()],
        vec!["--review", "1:00"],
        vec!["--oversize"],
    ] {
        let mut args = vec![
            "changeset",
            "add",
            id.as_str(),
            "--add",
            "--text",
            "x",
            "--no-based-on",
        ];
        args.extend(refused.iter().copied());
        let output = engr(&root, &args);
        assert_eq!(output.status.code(), Some(engr::EXIT_USAGE), "{args:?}");
    }

    for text in ["First decision.", "Second decision."] {
        let output = engr(
            &root,
            &[
                "changeset",
                "add",
                &id[..20],
                "--add",
                "--header",
                "Decision",
                "--text",
                text,
                "--no-based-on",
            ],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let shown = engr(&root, &["changeset", "show", &id]);
    assert!(shown.status.success());
    let screen = String::from_utf8_lossy(&shown.stdout);
    assert!(
        screen.contains("First decision.") && screen.contains("Second decision."),
        "{screen}"
    );
    assert!(screen.contains("NEEDS REVIEW"), "{screen}");
    let digest = screen
        .split_whitespace()
        .skip_while(|word| *word != "--review")
        .nth(1)
        .expect("the screen names the digest")
        .to_owned();

    // Applying unreviewed shows the same subject and says nothing was written.
    let refused = engr(&root, &["changeset", "apply", &id]);
    assert_eq!(refused.status.code(), Some(engr::EXIT_USAGE));
    assert!(String::from_utf8_lossy(&refused.stdout).contains(&digest));

    let applied = engr(
        &root,
        &[
            "changeset",
            "apply",
            &id,
            "--review",
            &digest,
            "--reviewed-rule",
            "object-policy",
            "--review-result",
            "passed",
        ],
    );
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    let admitted = String::from_utf8_lossy(&applied.stdout);
    assert_eq!(admitted.matches("ADMITTED").count(), 2, "{admitted}");
    assert_eq!(
        store::load_object(&root, &object)
            .expect("object")
            .sections
            .len(),
        3
    );
    assert!(changeset::list(&root).expect("list").is_empty());
}

#[test]
fn an_interrupted_partial_apply_keeps_exactly_the_steps_it_did_not_admit() {
    let (_temp, root, object, id) = governed();
    changeset::add(&root, &id, add(&object, "Passed.")).expect("add");
    let (_, plan) = changeset::add(&root, &id, add(&object, "Failed.")).expect("add");
    let draft = std::fs::read(file(&root, &id)).expect("draft");
    let events = match changeset::apply(
        &root,
        &id,
        Some(attest(&plan, proof::ReviewResult::Failed)),
        &[2],
    )
    .expect("apply")
    {
        Applied::Admitted { events, .. } => events,
        other => panic!("{other:?}"),
    };

    // The crash after publishing step 1 and before the ChangeSet was rewritten.
    std::fs::write(file(&root, &id), draft).expect("restore");
    let mut interrupted = changeset::load(&root, &id).expect("load");
    interrupted.committing = Some(changeset::Committing {
        events: events.iter().map(|event| event.id.clone()).collect(),
        steps: vec![0],
    });
    rewrite(&root, &interrupted);

    match changeset::apply(&root, &id, None, &[]).expect("resume") {
        Applied::AlreadyAdmitted { remaining, .. } => assert_eq!(remaining, 1),
        other => panic!("{other:?}"),
    }
    let kept = changeset::load(&root, &id).expect("kept");
    assert_eq!(kept.steps.len(), 1);
    assert!(kept.committing.is_none());
    assert_eq!(store::load_events(&root, &object).expect("events").len(), 3);
}

#[test]
fn the_command_line_admits_what_passed_and_says_what_attempt_comes_next() {
    let (_temp, root) = common::repository();
    let object = common::object_with_section(&root, "Command line", "Committed wording.");
    object_rule(&root);
    common::git(&root, &["add", "-A"]);
    common::git(&root, &["commit", "-qm", "object"]);
    let id = changeset::create(&root, &object).expect("create").id;
    for text in ["Passes.", "Fails."] {
        let output = engr(
            &root,
            &[
                "changeset",
                "add",
                &id,
                "--add",
                "--text",
                text,
                "--no-based-on",
            ],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let (_, plan) = changeset::plan(&root, &id).expect("plan");
    let applied = engr(
        &root,
        &[
            "changeset",
            "apply",
            &id,
            "--review",
            &plan.digest,
            "--reviewed-rule",
            "object-policy",
            "--review-result",
            "failed",
            "--failed-step",
            "2",
        ],
    );
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    let said = String::from_utf8_lossy(&applied.stdout);
    assert_eq!(said.matches("ADMITTED").count(), 1, "{said}");
    assert!(
        said.contains("KEPT") && said.contains("attempt 2"),
        "{said}"
    );
    assert_eq!(changeset::load(&root, &id).expect("kept").steps.len(), 1);
}

#[test]
fn taking_a_step_out_says_no_earlier_review_covers_what_remains() {
    let (_temp, root) = common::repository();
    let object = common::object_with_section(&root, "Command line", "Committed wording.");
    object_rule(&root);
    common::git(&root, &["add", "-A"]);
    common::git(&root, &["commit", "-qm", "object"]);
    let id = changeset::create(&root, &object).expect("create").id;
    for text in ["Stays.", "Taken out."] {
        let output = engr(
            &root,
            &[
                "changeset",
                "add",
                &id,
                "--add",
                "--text",
                text,
                "--no-based-on",
            ],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let (_, reviewed) = changeset::plan(&root, &id).expect("plan");

    let removed = engr(&root, &["changeset", "rm", &id, "--step", "2"]);
    assert!(removed.status.success());
    let said = String::from_utf8_lossy(&removed.stdout);
    assert!(
        said.contains("no earlier review covers what remains") && said.contains("--failed-step"),
        "{said}"
    );
    let (_, remaining) = changeset::plan(&root, &id).expect("plan");
    assert_ne!(remaining.digest, reviewed.digest);

    // With nothing left there is no digest to warn about.
    let emptied = engr(&root, &["changeset", "rm", &id, "--step", "1"]);
    assert!(emptied.status.success());
    assert!(!String::from_utf8_lossy(&emptied.stdout).contains("note"));
}
