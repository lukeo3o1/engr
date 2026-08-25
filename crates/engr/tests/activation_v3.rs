use engr::model::{
    Action, Content, Event, HumanConfirmation, Payload, Provenance, TaggedAdmission,
};
use engr::semantics::Admission;
use engr::{gate, integrity, proof, rules, store};
use std::path::Path;
use std::process::Command;

fn creation(id: &str) -> Payload {
    Payload {
        action: Action::ObjectCreated,
        object: id.to_owned(),
        becomes: None,
        content: Content {
            text: "workspace generation three".to_owned(),
            ..Content::default()
        },
    }
}

fn add(id: &str, text: &str) -> Payload {
    Payload {
        action: Action::SectionAdded,
        object: id.to_owned(),
        becomes: None,
        content: Content {
            text: text.to_owned(),
            ..Content::default()
        },
    }
}

fn admit_human(root: &Path, payload: Payload) {
    let prepared = gate::prepare(root, payload).expect("prepare");
    gate::confirm(root, &format!("CONFIRM {}", prepared.candidate.challenge)).expect("confirm");
}

fn object_rule(root: &Path) {
    std::fs::create_dir_all(rules::dir(root)).expect("rules dir");
    std::fs::write(
        rules::dir(root).join("object-policy.md"),
        "---\nid: object-policy\napplies:\n  domains:\n    - object\n---\n\n# Object policy\n\nReview the exact mutation.\n",
    )
    .expect("rule");
}

fn attestation(
    root: &Path,
    payload: &Payload,
    admission: Admission,
    result: proof::ReviewResult,
    explanation: Option<&str>,
) -> gate::ReviewAttestation {
    let before = store::load_object(root, &payload.object).expect("before");
    let mut after = before.clone();
    let event = Event {
        format: engr::model::EVENT_FORMAT.to_owned(),
        version: engr::EVENT_ENVELOPE_VERSION,
        event_id: engr::model::new_id(),
        rev: before.rev + 1,
        time: "2026-08-25T00:00:00Z".to_owned(),
        payload: payload.clone(),
        provenance: Provenance::Tagged {
            admission: TaggedAdmission {
                kind: admission,
                confirmation: (admission == Admission::Human).then(|| HumanConfirmation {
                    challenge: "ABCD-EFGH".to_owned(),
                    candidate_digest: format!("1:{}", "0".repeat(64)),
                }),
                rule_review: None,
            },
        },
    };
    engr::model::project(&mut after, &event).expect("project");
    let mutation = proof::object_review_mutation(&before, &after, payload).expect("mutation");
    let binding = rules::bind_object(root, &mutation, before.rev).expect("binding");
    gate::ReviewAttestation {
        review_digest: binding.digest().expect("digest").to_string(),
        reviewed_rules: binding.rule_ids(),
        attempt: 1,
        result,
        explanation: explanation.map(str::to_owned),
    }
}

#[test]
fn human_gate_emits_candidate_v3_event_v2_and_a_sealed_object() {
    let temp = tempfile::tempdir().expect("temp");
    store::init(temp.path()).expect("init");
    let id = "018f7d58-4ca7-7a2e-98f1-9b3014681848";

    let prepared = gate::prepare(temp.path(), creation(id)).expect("prepare");
    assert_eq!(prepared.candidate.version, 3);
    assert!(prepared.candidate.candidate_digest.starts_with("1:"));
    assert_eq!(prepared.candidate.integrity_sha256.len(), 64);

    let admitted = gate::confirm(
        temp.path(),
        &format!("CONFIRM {}", prepared.candidate.challenge),
    )
    .expect("confirm");

    assert_eq!(admitted.event.version, 2);
    let Provenance::Tagged { admission } = &admitted.event.provenance else {
        panic!("Event v2 must carry tagged provenance");
    };
    assert_eq!(admission.kind, engr::semantics::Admission::Human);
    assert_eq!(
        admission
            .confirmation
            .as_ref()
            .expect("human confirmation")
            .candidate_digest,
        prepared.candidate.candidate_digest
    );
    integrity::check_stored_object_integrity(&admitted.object).expect("object integrity");
    let loaded = store::load_object(temp.path(), id).expect("load");
    assert_eq!(loaded, admitted.object);
}

#[test]
fn agent_review_is_rechecked_and_persisted_by_the_direct_admission_path() {
    let temp = tempfile::tempdir().expect("temp");
    store::init(temp.path()).expect("init");
    let id = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    admit_human(temp.path(), creation(id));
    object_rule(temp.path());
    let payload = add(id, "agent-reviewed knowledge");
    let review = attestation(
        temp.path(),
        &payload,
        Admission::Agent,
        proof::ReviewResult::Passed,
        None,
    );
    let expected = review.review_digest.clone();

    let admitted = gate::admit_agent(temp.path(), payload, Some(review)).expect("agent admit");

    assert_eq!(admitted.object.sections[0].admission, Admission::Agent);
    let Provenance::Tagged { admission } = &admitted.event.provenance else {
        panic!("tagged provenance");
    };
    assert_eq!(admission.kind, Admission::Agent);
    assert!(admission.confirmation.is_none());
    assert_eq!(
        admission
            .rule_review
            .as_ref()
            .expect("durable review")
            .review_digest,
        expected
    );
    integrity::check_stored_object_integrity(&admitted.object).expect("integrity");
}

#[test]
fn agent_cli_surfaces_the_review_then_admits_the_same_bound_mutation() {
    let temp = tempfile::tempdir().expect("temp");
    store::init(temp.path()).expect("init");
    let id = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    admit_human(temp.path(), creation(id));
    object_rule(temp.path());
    let payload = add(id, "agent-reviewed through the CLI");

    let first = Command::new(env!("CARGO_BIN_EXE_engr"))
        .arg("--root")
        .arg(temp.path())
        .args([
            "prepare",
            "--object",
            id,
            "--add",
            "--text",
            "agent-reviewed through the CLI",
            "--no-based-on",
            "--agent",
        ])
        .output()
        .expect("first attempt");
    assert_eq!(first.status.code(), Some(engr::EXIT_USAGE));
    let refusal = String::from_utf8_lossy(&first.stderr);
    assert!(refusal.contains("object-policy"), "{refusal}");
    assert!(refusal.contains("review digest"), "{refusal}");
    assert_eq!(
        store::load_object(temp.path(), id).expect("unchanged").rev,
        1
    );

    let review = attestation(
        temp.path(),
        &payload,
        Admission::Agent,
        proof::ReviewResult::Passed,
        None,
    );
    let admitted = Command::new(env!("CARGO_BIN_EXE_engr"))
        .arg("--root")
        .arg(temp.path())
        .args([
            "prepare",
            "--object",
            id,
            "--add",
            "--text",
            "agent-reviewed through the CLI",
            "--no-based-on",
            "--agent",
            "--review",
            &review.review_digest,
            "--reviewed-rule",
            "object-policy",
            "--review-result",
            "passed",
            "--json",
        ])
        .output()
        .expect("admit");
    assert!(
        admitted.status.success(),
        "{}",
        String::from_utf8_lossy(&admitted.stderr)
    );
    let output: serde_json::Value = serde_json::from_slice(&admitted.stdout).expect("json");
    assert_eq!(output["event"]["version"], 2);
    assert_eq!(output["event"]["admission"]["kind"], "agent");
    assert_eq!(output["object"]["sections"][0]["admission"], "agent");
    assert_eq!(
        output["event"]["admission"]["rule_review"]["review_digest"],
        review.review_digest
    );
}

#[test]
fn human_source_cannot_treat_agent_semantics_as_human_authority() {
    let temp = tempfile::tempdir().expect("temp");
    store::init(temp.path()).expect("init");
    let id = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    admit_human(temp.path(), creation(id));
    object_rule(temp.path());
    let agent_payload = add(id, "agent authority");
    let review = attestation(
        temp.path(),
        &agent_payload,
        Admission::Agent,
        proof::ReviewResult::Passed,
        None,
    );
    gate::admit_agent(temp.path(), agent_payload, Some(review)).expect("agent section");

    let reference = engr::dependency::SelectiveRef::stored(
        engr::proof::section_target(id, 1),
        vec![engr::dependency::SemanticField::Text],
        "0".repeat(40),
        format!("1:{}", "0".repeat(64)),
    )
    .expect("stored ref shape");
    let error = gate::prepare(
        temp.path(),
        Payload {
            action: Action::SectionAdded,
            object: id.to_owned(),
            becomes: None,
            content: Content {
                text: "human assertion".to_owned(),
                refs: vec![engr::model::Ref::selective(reference)],
                ..Content::default()
            },
        },
    )
    .expect_err("Human source authority cannot be borrowed from an Agent Section");

    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(
        error.message.contains("human-admitted authority"),
        "{error}"
    );
}

#[test]
fn human_override_binds_the_review_explanation_and_persists_minimal_provenance() {
    let temp = tempfile::tempdir().expect("temp");
    store::init(temp.path()).expect("init");
    let id = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    admit_human(temp.path(), creation(id));
    object_rule(temp.path());
    let payload = add(id, "human accepts this despite the failed review");
    let review = attestation(
        temp.path(),
        &payload,
        Admission::Human,
        proof::ReviewResult::Failed,
        Some("The rule conflicts with an explicit compatibility requirement."),
    );
    let expected = review.review_digest.clone();

    let prepared = gate::prepare_reviewed(temp.path(), payload, gate::Allowance::Normal, review)
        .expect("reviewed candidate");
    assert_eq!(
        prepared
            .candidate
            .context
            .rule_review
            .as_ref()
            .expect("candidate review")
            .explanation
            .as_deref(),
        Some("The rule conflicts with an explicit compatibility requirement.")
    );
    let admitted = gate::confirm(
        temp.path(),
        &format!("CONFIRM {}", prepared.candidate.challenge),
    )
    .expect("confirm override");
    let Provenance::Tagged { admission } = &admitted.event.provenance else {
        panic!("tagged provenance");
    };
    let durable = admission.rule_review.as_ref().expect("durable review");
    assert_eq!(durable.outcome, engr::model::ReviewOutcome::Overridden);
    assert_eq!(durable.review_digest, expected);
    assert_eq!(admitted.object.sections[0].admission, Admission::Human);
}
