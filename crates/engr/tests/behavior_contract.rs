use serde_json::Value;

const BEHAVIOR_EVALS: &str = include_str!("../../../skill/evals/evals.json");
const TRIGGER_EVALS: &str = include_str!("../../../skill/evals/trigger-evals.json");
const S07_ARTIFACT: &str = include_str!("../../../skill/evals/fixtures/v3-bootstrap-test.log");

fn text(value: &Value) -> String {
    serde_json::to_string(value).expect("evaluation JSON serializes")
}

#[test]
fn s01_through_s16_are_complete_native_engagement_contracts() {
    let document: Value = serde_json::from_str(BEHAVIOR_EVALS).expect("valid behavior eval JSON");
    assert_eq!(document["skill_name"], "engr");
    let cases = document["evals"].as_array().expect("eval cases array");
    assert_eq!(cases.len(), 16);
    for (offset, case) in cases.iter().enumerate() {
        assert_eq!(case["id"].as_u64(), Some((offset + 1) as u64));
        assert!(case["prompt"]
            .as_str()
            .is_some_and(|value| !value.is_empty()));
        assert!(case["expected_output"]
            .as_str()
            .is_some_and(|value| !value.is_empty()));
        assert_eq!(case["expectations"].as_array().map(Vec::len), Some(4));
    }

    let all_text = text(&document);
    let legacy_project_directory = [".", "engineering"].concat();
    assert!(!all_text.contains(&legacy_project_directory));
    assert!(!all_text.contains(&["Power", "Shell"].concat()));
    assert!(!all_text.contains(&["Py", "thon"].concat()));
    assert!(all_text.contains("engr doctor"));
    assert!(all_text.contains("approved Engr release"));
    assert!(!S07_ARTIFACT.trim().is_empty());
}

#[test]
fn trigger_corpus_is_valid_and_has_no_legacy_project_path() {
    let document: Value = serde_json::from_str(TRIGGER_EVALS).expect("valid trigger eval JSON");
    let cases = document.as_array().expect("trigger case array");
    assert_eq!(cases.len(), 18);
    assert!(cases.iter().any(|case| case["should_trigger"] == true));
    assert!(cases.iter().any(|case| case["should_trigger"] == false));
    let legacy_project_directory = [".", "engineering"].concat();
    assert!(!text(&document).contains(&legacy_project_directory));
}
