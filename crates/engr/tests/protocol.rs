use engr::protocol::{
    attach_integrity, candidate_hash, canonical_json, strict_json, verify_integrity,
};
use serde_json::json;

#[test]
fn canonical_json_orders_object_keys_without_rewriting_unicode() {
    let value = json!({"z": 1, "a": ["繁體", {"b": true, "a": null}]});
    assert_eq!(
        canonical_json(&value),
        r#"{"a":["繁體",{"a":null,"b":true}],"z":1}"#
    );
}

#[test]
fn integrity_excludes_only_the_integrity_envelope() {
    let mut value = json!({"stream": "WI-20260809-01", "head": {"rev": 1}});
    attach_integrity(&mut value).unwrap();
    verify_integrity(&value, "test state").unwrap();
    value["head"]["rev"] = json!(2);
    assert!(verify_integrity(&value, "test state").is_err());
}

#[test]
fn candidate_hash_is_stable_for_equivalent_object_order() {
    let first = candidate_hash(
        "WI-20260809-01",
        "work_item.created",
        &json!({"text":"Start"}),
        &json!({"b":2,"a":1}),
        &json!(null),
    );
    let second = candidate_hash(
        "WI-20260809-01",
        "work_item.created",
        &json!({"text":"Start"}),
        &json!({"a":1,"b":2}),
        &json!(null),
    );
    assert_eq!(first, second);
}

#[test]
fn strict_json_rejects_float_values() {
    assert!(strict_json(r#"{"version": 1.0}"#, "test").is_err());
}
