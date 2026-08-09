use crate::require;
use crate::{EngrError, Result, EVENT_SCHEMA_VERSION, EXIT_SCHEMA, EXIT_VERSION, PROTOCOL_VERSION};
use regex::Regex;
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::Path;
use std::sync::OnceLock;
use time::OffsetDateTime;

pub const HANDSHAKE: &str = "engineering-record\tprotocol=1\tevent-schema=1\tstate-schema=1";

fn regex(pattern: &'static str) -> &'static Regex {
    static CACHE: OnceLock<std::collections::HashMap<&'static str, Regex>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| {
        [
            ("work", r"^WI-[0-9]{8}-[0-9]{2}$"),
            ("decision", r"^DR-[0-9]{8}-[0-9]{2}-[A-Z0-9]+(?:-[A-Z0-9]+)*$"),
            ("entity", r"^[A-Z][A-Z0-9-]{0,63}$"),
            ("event_no", r"^E-[0-9]{8}-[0-9]{4}$"),
            ("uuid", r"^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"),
            ("ulid", r"^[0-9A-HJKMNP-TV-Z]{26}$"),
            ("hash", r"^[0-9a-f]{64}$"),
            ("challenge", r"^[23456789ABCDEFGHJKLMNPQRSTUVWXYZ]{6}$"),
            ("rfc3339", r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]+)?(?:Z|[+-][0-9]{2}:[0-9]{2})$"),
        ].into_iter().map(|(name, expression)| (name, Regex::new(expression).unwrap())).collect()
    });
    match pattern {
        "work" | "decision" | "entity" | "event_no" | "uuid" | "ulid" | "hash" | "challenge"
        | "rfc3339" => cache.get(pattern).unwrap(),
        _ => unreachable!("unknown protocol regex"),
    }
}

pub fn stream_kind(stream: &str) -> Result<&'static str> {
    if regex("work").is_match(stream) {
        Ok("work_item")
    } else if regex("decision").is_match(stream) {
        Ok("decision")
    } else {
        Err(EngrError::new(
            EXIT_SCHEMA,
            format!("invalid stream id: {stream}"),
        ))
    }
}

pub fn valid_event_id(value: &str) -> bool {
    regex("uuid").is_match(value) || regex("ulid").is_match(value)
}

pub fn exact_version(value: &Value, supported: i64) -> bool {
    matches!(value, Value::Number(number) if number.as_i64() == Some(supported) && number.is_i64())
}

pub fn object<'a>(value: &'a Value, label: &str) -> Result<&'a Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, format!("{label}: expected object")))
}

pub fn string<'a>(object: &'a Map<String, Value>, key: &str, label: &str) -> Result<&'a str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, format!("{label}.{key}: expected string")))
}

pub fn integer(object: &Map<String, Value>, key: &str, label: &str) -> Result<i64> {
    object
        .get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, format!("{label}.{key}: expected integer")))
}

pub fn require_keys(
    object: &Map<String, Value>,
    required: &[&str],
    optional: &[&str],
    label: &str,
) -> Result<()> {
    let known: BTreeSet<&str> = required.iter().chain(optional).copied().collect();
    let missing: Vec<_> = required
        .iter()
        .filter(|key| !object.contains_key(**key))
        .copied()
        .collect();
    let unknown: Vec<_> = object
        .keys()
        .filter(|key| !known.contains(key.as_str()))
        .cloned()
        .collect();
    require!(
        missing.is_empty(),
        EXIT_SCHEMA,
        "{label}: missing fields {missing:?}"
    );
    require!(
        unknown.is_empty(),
        EXIT_SCHEMA,
        "{label}: unknown fields {unknown:?}"
    );
    Ok(())
}

/// JSON bytes whose object ordering and escaping are independent of serde's map implementation.
pub fn canonical_json(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => {
            serde_json::to_string(value).expect("string serialization cannot fail")
        }
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Object(values) => {
            let mut entries: Vec<_> = values.iter().collect();
            entries.sort_unstable_by(|left, right| left.0.cmp(right.0));
            let body = entries
                .into_iter()
                .map(|(key, value)| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap(),
                        canonical_json(value)
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{body}}}")
        }
    }
}

pub fn sha256_json(value: &Value) -> String {
    let digest = Sha256::digest(canonical_json(value).as_bytes());
    format!("{digest:x}")
}

pub fn integrity_value(value: &Value) -> Result<String> {
    let mut object = object(value, "integrity value")?.clone();
    object.remove("integrity");
    Ok(sha256_json(&Value::Object(object)))
}

pub fn attach_integrity(value: &mut Value) -> Result<()> {
    let digest = integrity_value(value)?;
    object_mut(value, "integrity value")?.insert(
        "integrity".to_owned(),
        serde_json::json!({"algorithm": "sha256", "value": digest}),
    );
    Ok(())
}

pub fn verify_integrity(value: &Value, label: &str) -> Result<()> {
    let root = object(value, label)?;
    let integrity = root
        .get("integrity")
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, format!("{label}: missing integrity")))?;
    let integrity = object(integrity, label)?;
    require!(
        integrity.get("algorithm").and_then(Value::as_str) == Some("sha256"),
        EXIT_SCHEMA,
        "{label}: unsupported integrity algorithm"
    );
    let recorded = integrity
        .get("value")
        .and_then(Value::as_str)
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, format!("{label}: invalid integrity value")))?;
    require!(
        regex("hash").is_match(recorded),
        EXIT_SCHEMA,
        "{label}: invalid integrity value"
    );
    let expected = integrity_value(value)?;
    require!(
        recorded == expected,
        EXIT_SCHEMA,
        "{label}: integrity check failed"
    );
    Ok(())
}

pub fn object_mut<'a>(value: &'a mut Value, label: &str) -> Result<&'a mut Map<String, Value>> {
    value
        .as_object_mut()
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, format!("{label}: expected object")))
}

pub fn parse_time(value: &str, label: &str) -> Result<OffsetDateTime> {
    require!(
        regex("rfc3339").is_match(value),
        EXIT_SCHEMA,
        "{label}: expected RFC3339 timestamp"
    );
    OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
        .map_err(|_| EngrError::new(EXIT_SCHEMA, format!("{label}: invalid RFC3339 timestamp")))
}

pub fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("time formatting cannot fail")
}

pub fn json_number(value: i64) -> Value {
    Value::Number(Number::from(value))
}

struct StrictValue;

impl<'de> DeserializeSeed<'de> for StrictValue {
    type Value = Value;
    fn deserialize<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictVisitor)
    }
}

struct StrictVisitor;

impl<'de> Visitor<'de> for StrictVisitor {
    type Value = Value;
    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("strict JSON without duplicate keys or floats")
    }
    fn visit_bool<E>(self, value: bool) -> std::result::Result<Value, E>
    where
        E: de::Error,
    {
        Ok(Value::Bool(value))
    }
    fn visit_i64<E>(self, value: i64) -> std::result::Result<Value, E>
    where
        E: de::Error,
    {
        Ok(Value::Number(value.into()))
    }
    fn visit_u64<E>(self, value: u64) -> std::result::Result<Value, E>
    where
        E: de::Error,
    {
        Ok(Value::Number(value.into()))
    }
    fn visit_f64<E>(self, _: f64) -> std::result::Result<Value, E>
    where
        E: de::Error,
    {
        Err(E::custom("floating-point JSON values are not supported"))
    }
    fn visit_str<E>(self, value: &str) -> std::result::Result<Value, E>
    where
        E: de::Error,
    {
        Ok(Value::String(value.into()))
    }
    fn visit_string<E>(self, value: String) -> std::result::Result<Value, E>
    where
        E: de::Error,
    {
        Ok(Value::String(value))
    }
    fn visit_none<E>(self) -> std::result::Result<Value, E>
    where
        E: de::Error,
    {
        Ok(Value::Null)
    }
    fn visit_unit<E>(self) -> std::result::Result<Value, E>
    where
        E: de::Error,
    {
        Ok(Value::Null)
    }
    fn visit_seq<A>(self, mut sequence: A) -> std::result::Result<Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(StrictValue)? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<A>(self, mut map: A) -> std::result::Result<Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(de::Error::custom(format!("duplicate JSON key: {key}")));
            }
            values.insert(key, map.next_value_seed(StrictValue)?);
        }
        Ok(Value::Object(values))
    }
}

pub fn strict_json(text: &str, label: &str) -> Result<Value> {
    let mut parser = serde_json::Deserializer::from_str(text);
    let value = StrictValue
        .deserialize(&mut parser)
        .and_then(|value| {
            parser.end()?;
            Ok(value)
        })
        .map_err(|error: serde_json::Error| {
            EngrError::new(
                EXIT_SCHEMA,
                format!("{label}: invalid UTF-8 JSON ({error})"),
            )
        })?;
    Ok(value)
}

pub fn read_json(path: &Path, label: &str) -> Result<Value> {
    let text = fs::read_to_string(path).map_err(|error| {
        EngrError::new(
            if error.kind() == std::io::ErrorKind::NotFound {
                crate::EXIT_NOT_FOUND
            } else {
                crate::EXIT_TOOL
            },
            format!("{label}: {error}"),
        )
    })?;
    strict_json(&text, label)
}

pub fn validate_entity_id(value: &Value, field: &str) -> Result<()> {
    require!(
        value
            .as_str()
            .is_some_and(|item| regex("entity").is_match(item)),
        EXIT_SCHEMA,
        "{field}: invalid entity id"
    );
    Ok(())
}

pub fn validate_artifacts(value: &Value, field: &str) -> Result<()> {
    let values = value
        .as_array()
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, format!("{field}: expected array")))?;
    require!(
        values.iter().all(|item| item
            .as_str()
            .is_some_and(|item| !item.is_empty() && item.len() <= 512)),
        EXIT_SCHEMA,
        "{field}: expected non-empty string references"
    );
    Ok(())
}

pub fn work_events() -> HashSet<&'static str> {
    [
        "work_item.created",
        "problem.revised",
        "impact.revised",
        "fact.added",
        "fact.invalidated",
        "constraint.added",
        "constraint.removed",
        "unknown.added",
        "unknown.resolved",
        "hypothesis.added",
        "hypothesis.invalidated",
        "solution.proposed",
        "solution.selected",
        "solution.rejected",
        "solution.superseded",
        "implementation.started",
        "implementation.completed",
        "verification.criterion_added",
        "verification.result",
        "verification.invalidated",
        "finding.raised",
        "finding.promoted",
        "decision.linked",
        "decision.unlinked",
        "work_item.related",
        "work_item.unrelated",
        "risk.added",
        "risk.accepted",
        "risk.mitigated",
        "work_item.blocked",
        "work_item.unblocked",
        "work_item.deferred",
        "work_item.resumed",
        "work_item.resolved",
        "work_item.reopened",
        "work_item.cancelled",
        "stream.fork_reconciled",
    ]
    .into_iter()
    .collect()
}

pub fn decision_events() -> HashSet<&'static str> {
    [
        "decision.created",
        "decision.revised",
        "decision.accepted",
        "decision.superseded",
        "decision.revoked",
        "stream.fork_reconciled",
    ]
    .into_iter()
    .collect()
}

pub fn validate_event_data(event_type: &str, data: &Value) -> Result<()> {
    let data = object(data, &format!("{event_type}.data"))?;
    let empty = [
        "work_item.created",
        "work_item.deferred",
        "work_item.resumed",
        "work_item.resolved",
        "work_item.reopened",
        "work_item.cancelled",
        "decision.created",
        "decision.revised",
        "decision.accepted",
        "decision.revoked",
    ];
    if empty.contains(&event_type) {
        return require_keys(data, &[], &[], &format!("{event_type}.data"));
    }
    let single = match event_type {
        "fact.added" | "fact.invalidated" => Some("fact_id"),
        "constraint.added" | "constraint.removed" => Some("constraint_id"),
        "unknown.added" | "unknown.resolved" => Some("unknown_id"),
        "hypothesis.added" | "hypothesis.invalidated" => Some("hypothesis_id"),
        "solution.proposed" | "solution.selected" | "solution.rejected" => Some("solution_id"),
        "verification.invalidated" => Some("verification_id"),
        "finding.raised" => Some("finding_id"),
        "risk.added" | "risk.accepted" | "risk.mitigated" => Some("risk_id"),
        "work_item.blocked" | "work_item.unblocked" => Some("blocker_id"),
        "decision.linked" | "decision.unlinked" | "decision.superseded" => {
            Some(if event_type == "decision.superseded" {
                "by_decision_id"
            } else {
                "decision_id"
            })
        }
        _ => None,
    };
    if let Some(field) = single {
        require_keys(data, &[field], &[], &format!("{event_type}.data"))?;
        if field == "decision_id" || field == "by_decision_id" {
            require!(
                data.get(field)
                    .and_then(Value::as_str)
                    .is_some_and(|value| regex("decision").is_match(value)),
                EXIT_SCHEMA,
                "{field}: invalid Decision Record id"
            );
        } else {
            validate_entity_id(data.get(field).unwrap(), field)?;
        }
        return Ok(());
    }
    match event_type {
        "problem.revised" | "impact.revised" => {
            let field = if event_type == "problem.revised" {
                "problem_id"
            } else {
                "impact_id"
            };
            require_keys(
                data,
                &[field],
                &["supersedes"],
                &format!("{event_type}.data"),
            )?;
            validate_entity_id(data.get(field).unwrap(), field)?;
            if let Some(value) = data.get("supersedes") {
                validate_entity_id(value, "supersedes")?;
            }
        }
        "solution.superseded" => {
            require_keys(
                data,
                &["solution_id"],
                &["by_solution_id"],
                "solution.superseded.data",
            )?;
            validate_entity_id(data.get("solution_id").unwrap(), "solution_id")?;
            if let Some(value) = data.get("by_solution_id") {
                validate_entity_id(value, "by_solution_id")?;
            }
        }
        "implementation.started" | "implementation.completed" => {
            let optional = if event_type == "implementation.completed" {
                vec!["solution_id", "artifacts"]
            } else {
                vec!["solution_id"]
            };
            require_keys(
                data,
                &["implementation_id"],
                &optional,
                &format!("{event_type}.data"),
            )?;
            validate_entity_id(data.get("implementation_id").unwrap(), "implementation_id")?;
            if let Some(value) = data.get("solution_id") {
                validate_entity_id(value, "solution_id")?;
            }
            if let Some(value) = data.get("artifacts") {
                validate_artifacts(value, "artifacts")?;
            }
        }
        "verification.criterion_added" => {
            require_keys(
                data,
                &["verification_id", "required"],
                &[],
                "verification.criterion_added.data",
            )?;
            validate_entity_id(data.get("verification_id").unwrap(), "verification_id")?;
            require!(
                data.get("required").is_some_and(Value::is_boolean),
                EXIT_SCHEMA,
                "required: expected Boolean"
            );
        }
        "verification.result" => {
            require_keys(
                data,
                &["verification_id", "result"],
                &["artifacts"],
                "verification.result.data",
            )?;
            validate_entity_id(data.get("verification_id").unwrap(), "verification_id")?;
            require!(
                matches!(
                    data.get("result").and_then(Value::as_str),
                    Some("passed" | "failed" | "inconclusive")
                ),
                EXIT_SCHEMA,
                "result: invalid verification result"
            );
            if let Some(value) = data.get("artifacts") {
                validate_artifacts(value, "artifacts")?;
            }
        }
        "finding.promoted" => {
            require_keys(
                data,
                &["finding_id", "work_item_id"],
                &[],
                "finding.promoted.data",
            )?;
            validate_entity_id(data.get("finding_id").unwrap(), "finding_id")?;
            require!(
                data.get("work_item_id")
                    .and_then(Value::as_str)
                    .is_some_and(|value| regex("work").is_match(value)),
                EXIT_SCHEMA,
                "work_item_id: invalid Work Item id"
            );
        }
        "work_item.related" | "work_item.unrelated" => {
            require_keys(
                data,
                &["work_item_id", "relation"],
                &[],
                &format!("{event_type}.data"),
            )?;
            require!(
                data.get("work_item_id")
                    .and_then(Value::as_str)
                    .is_some_and(|value| regex("work").is_match(value)),
                EXIT_SCHEMA,
                "work_item_id: invalid Work Item id"
            );
            require!(
                matches!(
                    data.get("relation").and_then(Value::as_str),
                    Some("relates_to" | "depends_on" | "blocks" | "duplicates" | "parent_of")
                ),
                EXIT_SCHEMA,
                "relation: invalid relationship"
            );
        }
        "stream.fork_reconciled" => {
            require_keys(
                data,
                &["fork_parent", "accepted_root", "rejected_roots"],
                &[],
                "stream.fork_reconciled.data",
            )?;
            for field in ["fork_parent", "accepted_root"] {
                require!(
                    data.get(field)
                        .and_then(Value::as_str)
                        .is_some_and(valid_event_id),
                    EXIT_SCHEMA,
                    "{field}: invalid event id"
                );
            }
            let rejected = data
                .get("rejected_roots")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    EngrError::new(EXIT_SCHEMA, "rejected_roots: expected non-empty array")
                })?;
            require!(
                !rejected.is_empty()
                    && rejected
                        .iter()
                        .all(|item| item.as_str().is_some_and(valid_event_id)),
                EXIT_SCHEMA,
                "rejected_roots: invalid event id"
            );
            let names: HashSet<_> = rejected.iter().filter_map(Value::as_str).collect();
            require!(
                names.len() == rejected.len(),
                EXIT_SCHEMA,
                "rejected_roots: duplicates"
            );
        }
        _ => {
            return Err(EngrError::new(
                EXIT_SCHEMA,
                format!("no data schema for event type: {event_type}"),
            ))
        }
    }
    Ok(())
}

pub fn validate_provenance(value: &Value) -> Result<()> {
    let value = object(value, "provenance")?;
    let initiator = string(value, "initiator", "provenance")?;
    let basis = string(value, "basis", "provenance")?;
    let allowed = match initiator {
        "human" => ["human_confirmation"].as_slice(),
        "agent" => [
            "observation",
            "inference",
            "agent_proposal",
            "implementation",
            "verification",
            "dependency_change",
        ]
        .as_slice(),
        "system" => ["observation", "verification", "dependency_change"].as_slice(),
        _ => {
            return Err(EngrError::new(
                EXIT_SCHEMA,
                "provenance.initiator: invalid value",
            ))
        }
    };
    require!(
        allowed.contains(&basis),
        EXIT_SCHEMA,
        "provenance.basis: incompatible value"
    );
    if initiator == "human" {
        require_keys(
            value,
            &["initiator", "basis", "confirmation"],
            &[],
            "provenance",
        )?;
        let confirmation = object(
            value.get("confirmation").unwrap(),
            "provenance.confirmation",
        )?;
        require_keys(
            confirmation,
            &["challenge"],
            &["candidate_sha256"],
            "provenance.confirmation",
        )?;
        require!(
            confirmation
                .get("challenge")
                .and_then(Value::as_str)
                .is_some_and(|item| regex("challenge").is_match(item)),
            EXIT_SCHEMA,
            "confirmation.challenge: invalid"
        );
        if let Some(hash) = confirmation.get("candidate_sha256") {
            require!(
                hash.as_str()
                    .is_some_and(|item| regex("hash").is_match(item)),
                EXIT_SCHEMA,
                "confirmation.candidate_sha256: invalid"
            );
        }
    } else {
        require_keys(value, &["initiator", "basis"], &[], "provenance")?;
    }
    Ok(())
}

pub fn candidate_material(
    stream: &str,
    event_type: &str,
    record: &Value,
    data: &Value,
    expected_parent: &Value,
) -> Value {
    serde_json::json!({"stream": stream, "event": event_type, "record": record, "data": data, "expected_parent": expected_parent})
}

pub fn candidate_hash(
    stream: &str,
    event_type: &str,
    record: &Value,
    data: &Value,
    expected_parent: &Value,
) -> String {
    sha256_json(&candidate_material(
        stream,
        event_type,
        record,
        data,
        expected_parent,
    ))
}

pub fn validate_event(
    event: &Value,
    expected_stream: Option<&str>,
    path_date: Option<&str>,
) -> Result<()> {
    let event = object(event, "event")?;
    require_keys(
        event,
        &[
            "format",
            "protocol_version",
            "event_schema_version",
            "event_id",
            "time",
            "stream",
            "rev",
            "parent",
            "event",
            "provenance",
            "record",
            "data",
        ],
        &["event_no"],
        "event",
    )?;
    require!(
        event.get("format").and_then(Value::as_str) == Some("engineering-event"),
        EXIT_VERSION,
        "event.format: unsupported"
    );
    require!(
        event
            .get("protocol_version")
            .is_some_and(|value| exact_version(value, PROTOCOL_VERSION)),
        EXIT_VERSION,
        "event.protocol_version: unsupported"
    );
    require!(
        event
            .get("event_schema_version")
            .is_some_and(|value| exact_version(value, EVENT_SCHEMA_VERSION)),
        EXIT_VERSION,
        "event.event_schema_version: unsupported"
    );
    let event_id = string(event, "event_id", "event")?;
    require!(
        valid_event_id(event_id),
        EXIT_SCHEMA,
        "event_id: expected UUIDv7 or legacy ULID"
    );
    if let Some(event_no) = event.get("event_no") {
        require!(
            event_no
                .as_str()
                .is_some_and(|item| regex("event_no").is_match(item)),
            EXIT_SCHEMA,
            "event_no: invalid"
        );
    }
    let parsed = parse_time(string(event, "time", "event")?, "time")?;
    let stream = string(event, "stream", "event")?;
    let kind = stream_kind(stream)?;
    if let Some(expected) = expected_stream {
        require!(
            stream == expected,
            EXIT_SCHEMA,
            "event stream does not match filename"
        );
    }
    if let Some(date) = path_date {
        let expected = parsed
            .format(&time::macros::format_description!("[year]/[month]/[day]"))
            .unwrap();
        require!(
            expected == date,
            EXIT_SCHEMA,
            "event time does not match date partition"
        );
    }
    require!(
        integer(event, "rev", "event")? > 0,
        EXIT_SCHEMA,
        "rev: expected positive integer"
    );
    require!(
        event
            .get("parent")
            .is_some_and(|parent| parent.is_null() || parent.as_str().is_some_and(valid_event_id)),
        EXIT_SCHEMA,
        "parent: invalid event id"
    );
    let event_type = string(event, "event", "event")?;
    let allowed = if kind == "work_item" {
        work_events()
    } else {
        decision_events()
    };
    require!(
        allowed.contains(event_type),
        EXIT_SCHEMA,
        "event type {event_type} is invalid for {kind}"
    );
    validate_provenance(event.get("provenance").unwrap())?;
    let record = object(event.get("record").unwrap(), "record")?;
    require_keys(record, &["text"], &[], "record")?;
    require!(
        record
            .get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.is_empty()),
        EXIT_SCHEMA,
        "record.text: expected non-empty string"
    );
    validate_event_data(event_type, event.get("data").unwrap())?;
    if let Some(confirmation) = event
        .get("provenance")
        .and_then(Value::as_object)
        .and_then(|value| value.get("confirmation"))
        .and_then(Value::as_object)
    {
        if let Some(hash) = confirmation.get("candidate_sha256").and_then(Value::as_str) {
            let expected = candidate_hash(
                stream,
                event_type,
                event.get("record").unwrap(),
                event.get("data").unwrap(),
                event.get("parent").unwrap(),
            );
            require!(
                hash == expected,
                EXIT_SCHEMA,
                "confirmation.candidate_sha256 does not derive from this event's sealed candidate"
            );
        }
    }
    Ok(())
}
