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

/// Tamper with stored authority without resealing it.
///
/// Schema-valid bytes whose Section seal no longer covers its own wording —
/// what an out-of-band edit actually looks like, rather than corruption.
fn tamper(root: &Path, id: &str) {
    let path = store::object_path(root, id);
    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("object bytes"))
            .expect("object json");
    value["sections"][0]["text"] = serde_json::Value::String("wording nobody admitted".to_owned());
    std::fs::write(
        &path,
        proof::canonical_bytes(&value, "tampered object").expect("canonical"),
    )
    .expect("tamper");
}

/// #35 §10's second half: an integrity-invalid Object can be recovered, and
/// only to what admitted history proves.
///
/// The first half — refusing ordinary mutation — was already closed, and on its
/// own it left one hand edit able to freeze a record permanently. The ruling on
/// `5442662072` settles the rest: Human Gate only, recorded as `object_repaired`,
/// restoring exactly the replay-derived projection.
#[test]
fn an_integrity_invalid_object_is_recovered_only_through_explicit_repair() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    store::init(root).expect("init");
    let id = "018f7d58-4ca7-7a2e-98f1-9b3014681848";
    admit_human(root, creation(id));
    admit_human(root, add(id, "the wording that was actually admitted"));
    let admitted = store::load_object(root, id).expect("admitted");

    tamper(root, id);
    integrity::check_stored_object_integrity(&store::load_object(root, id).expect("load"))
        .expect_err("the fixture must really break integrity");

    // Ordinary mutation stays refused — repair does not relax that.
    gate::prepare(root, add(id, "an ordinary change on broken authority"))
        .expect_err("ordinary mutation is refused while integrity fails");

    let prepared = gate::prepare_repair(root, id).expect("repair prepares");
    assert!(matches!(
        prepared.candidate.payload.action,
        engr::model::Action::ObjectRepaired
    ));
    gate::confirm(root, &format!("CONFIRM {}", prepared.candidate.challenge)).expect("confirm");

    let repaired = store::load_object(root, id).expect("repaired");
    integrity::check_stored_object_integrity(&repaired).expect("repair reseals");
    assert_eq!(
        repaired.sections[0].text, admitted.sections[0].text,
        "restored to what history proves, not to the tampered wording"
    );
    assert_eq!(repaired.rev, admitted.rev + 1, "repair is itself an event");

    let events = store::load_events(root, id).expect("history");
    let last = events.last().expect("an event");
    assert!(
        matches!(last.payload.action, engr::model::Action::ObjectRepaired),
        "the repair is visible in immutable history"
    );
    assert_eq!(last.payload.action.label(), "object.repaired");

    // And ordinary work is possible again afterwards.
    admit_human(
        root,
        add(id, "a change admitted the normal way, after repair"),
    );
}

/// Repair is an exceptional boundary, not a general-purpose rewrite.
#[test]
fn repair_is_refused_on_authority_that_still_verifies() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    store::init(root).expect("init");
    let id = "018f7d58-4ca7-7a2e-98f1-9b3014681849";
    admit_human(root, creation(id));

    let error = gate::prepare_repair(root, id).expect_err("nothing to repair");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("nothing to repair"), "{error}");
}

/// Repair fails closed when history cannot rebuild the projection.
///
/// The ruling is explicit that this is a different damage class from a broken
/// current projection and must not be guessed at through the same mechanism.
/// Restoring "something close" would be repair inventing authority, which is
/// the failure the whole boundary exists to prevent.
///
/// The refusal arrives from the history decoder rather than from `provable`'s
/// own check — a log whose revisions no longer start at 1 is not a readable
/// history at all. Same direction, one layer earlier, which is why this pins
/// the refusal and the untouched workspace rather than a particular code path.
#[test]
fn repair_refuses_when_admitted_history_cannot_rebuild_the_object() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    store::init(root).expect("init");
    let id = "018f7d58-4ca7-7a2e-98f1-9b301468184b";
    admit_human(root, creation(id));
    admit_human(root, add(id, "wording whose creation is about to vanish"));
    tamper(root, id);

    // Drop the creation, keeping the rest: history that can no longer say what
    // this Object started as.
    let path = store::events_path(root, id);
    let history = std::fs::read_to_string(&path).expect("history");
    let without_creation: Vec<&str> = history.lines().skip(1).collect();
    std::fs::write(&path, format!("{}\n", without_creation.join("\n"))).expect("truncate");

    let error = gate::prepare_repair(root, id).expect_err("nothing provable to restore");
    assert!(
        error.code == engr::EXIT_SCHEMA || error.code == engr::EXIT_INVARIANT,
        "refused as unusable history, not as something to guess at: {error}"
    );
    assert_eq!(
        std::fs::read_to_string(store::events_path(root, id)).expect("after"),
        format!("{}\n", without_creation.join("\n")),
        "and it wrote nothing while refusing"
    );
}

/// The recovery path is reachable from the command line, not only the library.
#[test]
fn repair_is_available_as_a_supported_command() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    store::init(root).expect("init");
    let id = "018f7d58-4ca7-7a2e-98f1-9b301468184c";
    admit_human(root, creation(id));
    admit_human(root, add(id, "wording admitted before the edit"));
    tamper(root, id);

    let output = Command::new(env!("CARGO_BIN_EXE_engr"))
        .arg("--root")
        .arg(root)
        .args(["repair", id, "--json"])
        .output()
        .expect("run engr repair");
    assert!(
        output.status.success(),
        "repair must be reachable without hand-editing .engr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).expect("repair json");
    let candidate = &document["candidate"];
    assert_eq!(candidate["action"], "object_repaired");

    // The 3B comparison travels with it: what is restored, what is on disk, and
    // that the stored record does not verify.
    assert_eq!(document["repair"]["stored_verifies"], false);
    assert_eq!(
        document["repair"]["restores"]["sections"][0]["text"],
        "wording admitted before the edit"
    );
    assert_eq!(
        document["repair"]["stored"]["sections"][0]["text"],
        "wording nobody admitted"
    );

    let challenge = candidate["challenge"].as_str().expect("challenge");
    let confirmed = Command::new(env!("CARGO_BIN_EXE_engr"))
        .arg("--root")
        .arg(root)
        .args(["confirm", &format!("CONFIRM {challenge}")])
        .output()
        .expect("run engr confirm");
    assert!(
        confirmed.status.success(),
        "confirm: {}",
        String::from_utf8_lossy(&confirmed.stderr)
    );
    integrity::check_stored_object_integrity(&store::load_object(root, id).expect("repaired"))
        .expect("the workspace verifies again");
}

/// Repair is Human-only as a property of the schema, not of one caller.
///
/// The CLI reaching repair only through `prepare_repair` is not the invariant;
/// it is one caller behaving. An Agent asking for it directly must be refused,
/// and so must a stored Event-v2 record that claims an Agent repair happened —
/// otherwise a hand-written log could establish one after the fact, which is
/// the whole thing the Human Gate is here to prevent.
#[test]
fn an_agent_cannot_repair_through_the_api_or_through_a_stored_event() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    store::init(root).expect("init");
    let id = "018f7d58-4ca7-7a2e-98f1-9b301468184d";
    admit_human(root, creation(id));
    admit_human(root, add(id, "wording admitted through the gate"));

    let repair = Payload {
        action: engr::model::Action::ObjectRepaired,
        object: id.to_owned(),
        becomes: None,
        content: Content::default(),
    };
    let error = gate::admit_agent(root, repair.clone(), None).expect_err("no agent repair");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("human gate only"), "{error}");

    // And the same refusal when it arrives as durable history rather than as a
    // call: append an Event-v2 record tagged `agent` carrying the action.
    let path = store::events_path(root, id);
    let history = std::fs::read_to_string(&path).expect("history");
    let forged = Event {
        format: engr::model::EVENT_FORMAT.to_owned(),
        version: engr::EVENT_ENVELOPE_VERSION,
        event_id: engr::model::new_id(),
        rev: store::load_object(root, id).expect("object").rev + 1,
        time: "2026-08-27T00:00:00Z".to_owned(),
        payload: repair,
        provenance: Provenance::Tagged {
            admission: TaggedAdmission {
                kind: Admission::Agent,
                confirmation: None,
                // A passing review, so the forgery gets past the check that an
                // Agent event carries one and reaches the authority rule this
                // test is actually about.
                rule_review: Some(engr::model::ReviewProvenance {
                    outcome: engr::model::ReviewOutcome::Passed,
                    review_digest: format!("1:{}", "0".repeat(64)),
                }),
            },
        },
    };
    let line = proof::canonical_bytes(&forged, "forged repair").expect("canonical");
    std::fs::write(&path, format!("{history}{line}\n")).expect("append");

    let error = store::load_events(root, id).expect_err("a stored agent repair is not history");
    assert!(
        error.message.contains("human admission"),
        "refused as an authority violation: {error}"
    );
}

/// Break integrity through one non-text member of the stored Section.
fn tamper_member(root: &Path, id: &str, member: &str, value: serde_json::Value) {
    let path = store::object_path(root, id);
    let mut stored: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("object bytes"))
            .expect("object json");
    stored["sections"][0][member] = value;
    std::fs::write(
        &path,
        proof::canonical_bytes(&stored, "tampered object").expect("canonical"),
    )
    .expect("tamper");
}

/// The confirmation screen shows every discarded member, not only wording.
///
/// An out-of-band edit to a Section's `role` fails integrity and prepares a
/// repair exactly like an edit to its text — but the first version of this
/// screen compared only `title`, lifecycle `state` and Section `text`, so it
/// asked for `CONFIRM` while showing no differing field at all. #35's 3B ruling
/// is that the Human sees the invalid state, the provable state and their
/// difference; a comparison that silently omits the difference does not satisfy
/// it.
#[test]
fn the_repair_screen_shows_a_discarded_role_and_not_only_text() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    store::init(root).expect("init");
    let id = "018f7d58-4ca7-7a2e-98f1-9b301468184e";
    admit_human(root, creation(id));
    admit_human(root, add(id, "wording that is not what changed"));
    tamper_member(
        root,
        id,
        "role",
        serde_json::Value::String("decision".to_owned()),
    );

    let stored = store::load_object(root, id).expect("stored");
    integrity::check_stored_object_integrity(&stored).expect_err("the edit breaks integrity");
    let provable = engr::ops::provable(root, id).expect("provable");
    let differences = engr::view::repair_differences(&stored, &provable).expect("compare");
    assert!(
        differences
            .iter()
            .any(|difference| difference.at == "§1.role"),
        "the discarded role must be on the screen: {:?}",
        differences.iter().map(|d| &d.at).collect::<Vec<_>>()
    );

    // And it reaches the actual confirmation surface, not just the helper.
    let output = Command::new(env!("CARGO_BIN_EXE_engr"))
        .arg("--root")
        .arg(root)
        .args(["repair", id])
        .output()
        .expect("run engr repair");
    assert!(output.status.success(), "repair prepares");
    let screen = String::from_utf8_lossy(&output.stdout);
    assert!(
        screen.contains("§1.role") && screen.contains("decision"),
        "the human-facing screen names what is being discarded:\n{screen}"
    );
}

/// The same, for a member the comparison was never written to know about.
///
/// The point is structural rather than field-by-field: the comparison walks the
/// canonical projection, so `refs[]`, `relations[]`, `based_on`, `admission`,
/// `admitted_at` and anything added later are covered by construction rather
/// than by remembering to add them.
#[test]
fn the_repair_screen_covers_sealed_members_it_was_never_taught() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    store::init(root).expect("init");
    let id = "018f7d58-4ca7-7a2e-98f1-9b301468184f";
    admit_human(root, creation(id));
    admit_human(root, add(id, "wording that is not what changed"));
    tamper_member(
        root,
        id,
        "content",
        serde_json::json!([{ "type": "code.rs", "body": "fn nobody_admitted_this() {}" }]),
    );

    let stored = store::load_object(root, id).expect("stored");
    integrity::check_stored_object_integrity(&stored).expect_err("the edit breaks integrity");
    let provable = engr::ops::provable(root, id).expect("provable");
    let differences = engr::view::repair_differences(&stored, &provable).expect("compare");
    let named: Vec<&str> = differences
        .iter()
        .map(|difference| difference.at.as_str())
        .collect();
    assert!(
        named.contains(&"§1.content"),
        "supplementary content is sealed material too: {named:?}"
    );
    assert!(
        !named.iter().any(|at| at.contains("sha256")),
        "seals are how the difference was found, not the difference: {named:?}"
    );
}
