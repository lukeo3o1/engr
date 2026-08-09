use assert_cmd::Command;
use engr::store::{
    append_event, confirm_candidate, create_snapshot, init_project, prepare_candidate,
    run_conformance, run_conformance_dir, verify_project,
};
use engr::EXIT_USAGE;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const FIXTURES: &str = "../../conformance/fixtures";

fn run_fixture(name: &str) {
    let temporary = TempDir::new().expect("temporary fixture directory");
    fs::copy(Path::new(FIXTURES).join(name), temporary.path().join(name)).expect("copy fixture");
    run_conformance_dir(temporary.path()).expect("fixture must pass");
}

macro_rules! conformance_case {
    ($test:ident, $fixture:literal) => {
        #[test]
        fn $test() {
            run_fixture($fixture);
        }
    };
}

conformance_case!(s01_linear_resolution, "01-linear-resolution.json");
conformance_case!(s02_long_preservation, "02-long-preservation.json");
conformance_case!(s03_unresolved_fork, "03-unresolved-fork.json");
conformance_case!(
    s04_explicit_reconciliation,
    "04-explicit-reconciliation.json"
);
conformance_case!(s05_solution_pivot, "05-solution-pivot.json");
conformance_case!(s06_problem_refinement, "06-problem-refinement.json");
conformance_case!(s07_fact_invalidation, "07-fact-invalidation.json");
conformance_case!(s08_solution_selection, "08-solution-selection.json");
conformance_case!(s09_verification_gate, "09-verification-gate.json");
conformance_case!(
    s10_verification_invalidation,
    "10-verification-invalidation.json"
);
conformance_case!(s11_decision_supersession, "11-decision-supersession.json");
conformance_case!(s12_work_item_reopen, "12-work-item-reopen.json");
conformance_case!(s13_unknown_resolution, "13-unknown-resolution.json");
conformance_case!(s14_long_horizon_drift, "14-long-horizon-drift.json");
conformance_case!(
    s15_hashless_human_confirmation,
    "15-hashless-human-confirmation.json"
);
conformance_case!(s16_relation_provenance, "16-relation-provenance.json");

#[test]
fn t7_long_horizon_case_is_stable_in_isolation() {
    run_fixture("14-long-horizon-drift.json");
}

#[test]
fn mutated_fixture_is_rejected() {
    let temporary = TempDir::new().expect("temporary fixture directory");
    let source = Path::new(FIXTURES).join("01-linear-resolution.json");
    let mut fixture: Value =
        serde_json::from_slice(&fs::read(source).expect("read fixture")).expect("fixture JSON");
    let streams = fixture["streams"].as_object_mut().expect("streams object");
    let events = streams
        .values_mut()
        .next()
        .and_then(Value::as_array_mut)
        .expect("event array");
    events[0]["event_id"] = json!("019fe0f7-2fde-7ca0-8df4-978056f5ffff");
    fs::write(
        temporary.path().join("mutated.json"),
        serde_json::to_vec_pretty(&fixture).expect("serialize mutation"),
    )
    .expect("write mutated fixture");

    assert!(run_conformance_dir(temporary.path()).is_err());
}

fn test_project() -> TempDir {
    let temporary = TempDir::new().expect("temporary project");
    init_project(temporary.path()).expect("initialize project");
    assert!(temporary.path().join(".engr/FORMAT.md").is_file());
    temporary
}

fn stream_for_today() -> String {
    let date = engr::protocol::now_rfc3339();
    format!("WI-{}-01", date.get(..10).expect("date").replace('-', ""))
}

#[test]
fn confirmation_gate_seals_the_candidate_and_accepts_only_the_exact_response() {
    let temporary = test_project();
    let stream = stream_for_today();
    let first = prepare_candidate(
        temporary.path(),
        &stream,
        "work_item.created",
        "Rotate signing keys",
        json!({}),
    )
    .expect("prepare human candidate");
    let code = first["challenge"].as_str().expect("challenge");
    let invalid = confirm_candidate(temporary.path(), &format!("CONFIRM {code} now"))
        .expect_err("qualified response must be rejected");
    assert_eq!(invalid.code, EXIT_USAGE);

    let candidate = prepare_candidate(
        temporary.path(),
        &stream,
        "work_item.created",
        "Rotate signing keys",
        json!({}),
    )
    .expect("prepare replacement candidate");
    let code = candidate["challenge"].as_str().expect("challenge");
    assert_eq!(candidate["candidate_sha256"].as_str().unwrap().len(), 64);
    let (event, state, _) = confirm_candidate(temporary.path(), &format!("CONFIRM {code}"))
        .expect("exact confirmation appends event");
    assert_eq!(event["provenance"]["initiator"], "human");
    assert_eq!(
        event["provenance"]["confirmation"]["candidate_sha256"],
        candidate["candidate_sha256"]
    );
    assert_eq!(state["head"]["event_id"], event["event_id"]);
    verify_project(temporary.path(), None).expect("accepted event project verification");
}

#[test]
fn confirmation_recovers_when_event_append_precedes_receipt_archival() {
    let temporary = test_project();
    let stream = stream_for_today();
    let candidate = prepare_candidate(
        temporary.path(),
        &stream,
        "work_item.created",
        "Recover receipt after durable event",
        json!({}),
    )
    .expect("prepare candidate");
    let challenge = candidate["challenge"].as_str().unwrap();
    let provenance = json!({
        "initiator": "human",
        "basis": "human_confirmation",
        "confirmation": {
            "challenge": challenge,
            "candidate_sha256": candidate["candidate_sha256"],
        }
    });
    let (appended, _, _) = append_event(
        temporary.path(),
        &stream,
        "work_item.created",
        candidate["record"]["text"].as_str().unwrap(),
        candidate["data"].clone(),
        provenance,
        candidate["expected_parent"].clone(),
    )
    .expect("simulate durable event before receipt archival");

    let (recovered, _, _) = confirm_candidate(temporary.path(), &format!("CONFIRM {challenge}"))
        .expect("confirmation recovers the matching event");
    assert_eq!(recovered["event_id"], appended["event_id"]);
    assert!(temporary
        .path()
        .join(format!(
            ".engr/artifacts/confirmations/accepted/{}.json",
            appended["event_id"].as_str().unwrap()
        ))
        .is_file());
    verify_project(temporary.path(), None).expect("recovered event project verification");
}

#[test]
fn tampered_snapshot_is_detected_by_verification() {
    let temporary = test_project();
    let stream = stream_for_today();
    let candidate = prepare_candidate(
        temporary.path(),
        &stream,
        "work_item.created",
        "Snapshot verification subject",
        json!({}),
    )
    .expect("prepare candidate");
    confirm_candidate(
        temporary.path(),
        &format!("CONFIRM {}", candidate["challenge"].as_str().unwrap()),
    )
    .expect("confirm candidate");
    let (snapshot_path, _) =
        create_snapshot(temporary.path(), &stream, Some("checkpoint")).expect("create snapshot");
    let mut snapshot: Value =
        serde_json::from_slice(&fs::read(&snapshot_path).expect("read snapshot"))
            .expect("snapshot JSON");
    snapshot["state"]["status"] = json!("resolved");
    fs::write(
        &snapshot_path,
        serde_json::to_vec_pretty(&snapshot).expect("serialize tampering"),
    )
    .expect("write tampered snapshot");

    assert!(verify_project(temporary.path(), Some(&stream)).is_err());
}

#[test]
fn native_cli_reports_the_protocol_handshake() {
    let mut command = Command::cargo_bin("engr").expect("engr binary");
    command
        .args(["version", "--handshake"])
        .assert()
        .success()
        .stdout(format!("{}\n", engr::protocol::HANDSHAKE));
}

#[test]
fn initialized_project_embeds_the_complete_native_conformance_corpus() {
    let temporary = test_project();
    assert!(!temporary.path().join(".engr/.gitattributes").exists());
    let report = run_conformance(temporary.path()).expect("bundled conformance corpus");
    assert_eq!(report["fixtures"].as_array().unwrap().len(), 16);
}
