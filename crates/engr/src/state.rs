use crate::protocol::{attach_integrity, object, stream_kind};
use crate::require;
use crate::{
    EngrError, Result, EVENT_SCHEMA_VERSION, EXIT_FORK, EXIT_INVARIANT, EXIT_SCHEMA,
    PROTOCOL_VERSION, STATE_SCHEMA_VERSION,
};
use serde_json::{json, Map, Value};
use std::collections::{BTreeSet, HashMap, HashSet};

fn event_object(event: &Value) -> Result<&Map<String, Value>> {
    object(event, "event")
}
fn event_string(event: &Value, key: &str) -> Result<String> {
    event_object(event)?
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, format!("event.{key}: expected string")))
}
fn event_data(event: &Value) -> Result<&Map<String, Value>> {
    object(event_object(event)?.get("data").unwrap(), "event.data")
}
fn data_string(event: &Value, key: &str) -> Result<String> {
    event_data(event)?
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, format!("event.data.{key}: expected string")))
}
fn status(state: &Value) -> Result<String> {
    object(state, "State")?
        .get("status")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, "State.status: expected string"))
}
fn set_status(state: &mut Value, value: &str) -> Result<()> {
    object_mut(state, "State")?.insert("status".into(), Value::String(value.into()));
    Ok(())
}
fn object_mut<'a>(value: &'a mut Value, label: &str) -> Result<&'a mut Map<String, Value>> {
    value
        .as_object_mut()
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, format!("{label}: expected object")))
}
fn entries_mut<'a>(state: &'a mut Value, collection: &str) -> Result<&'a mut Vec<Value>> {
    object_mut(state, "State")?
        .get_mut(collection)
        .and_then(Value::as_array_mut)
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, format!("State.{collection}: expected array")))
}
fn entries<'a>(state: &'a Value, collection: &str) -> Result<&'a Vec<Value>> {
    object(state, "State")?
        .get(collection)
        .and_then(Value::as_array)
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, format!("State.{collection}: expected array")))
}
fn record_entry(event: &Value, id: &str, status: &str) -> Result<Value> {
    Ok(json!({
        "id": id,
        "status": status,
        "text": event_object(event)?.get("record").and_then(Value::as_object).and_then(|record| record.get("text")).and_then(Value::as_str).unwrap(),
        "introduced_by": event_string(event, "event_id")?,
        "last_event_id": event_string(event, "event_id")?,
        "last_text": event_object(event)?.get("record").and_then(Value::as_object).and_then(|record| record.get("text")).and_then(Value::as_str).unwrap(),
        "provenance": event_object(event)?.get("provenance").unwrap(),
    }))
}
fn simple_record(event: &Value) -> Result<Value> {
    Ok(
        json!({"text": event_object(event)?.get("record").and_then(Value::as_object).and_then(|record| record.get("text")).and_then(Value::as_str).unwrap(), "event_id": event_string(event, "event_id")?, "provenance": event_object(event)?.get("provenance").unwrap()}),
    )
}
fn transition_entry(entry: &mut Value, event: &Value, next_status: &str) -> Result<()> {
    let entry = object_mut(entry, "State entry")?;
    entry.insert("status".into(), Value::String(next_status.into()));
    entry.insert(
        "last_event_id".into(),
        Value::String(event_string(event, "event_id")?),
    );
    entry.insert(
        "last_text".into(),
        event_object(event)?
            .get("record")
            .and_then(Value::as_object)
            .and_then(|record| record.get("text"))
            .unwrap()
            .clone(),
    );
    entry.insert(
        "provenance".into(),
        event_object(event)?.get("provenance").unwrap().clone(),
    );
    Ok(())
}
fn find_entry_mut<'a>(state: &'a mut Value, collection: &str, id: &str) -> Result<&'a mut Value> {
    let field = match collection {
        "decisions" => "decision_id",
        "related_work_items" => "work_item_id",
        _ => "id",
    };
    entries_mut(state, collection)?
        .iter_mut()
        .find(|entry| entry.get(field).and_then(Value::as_str) == Some(id))
        .ok_or_else(|| EngrError::new(EXIT_INVARIANT, format!("{collection}: unknown id {id}")))
}
fn entry_status(entry: &Value) -> Result<&str> {
    entry
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, "State entry.status: expected string"))
}
fn require_active(entry: &Value, expected: &[&str], label: &str) -> Result<()> {
    require!(
        expected.contains(&entry_status(entry)?),
        EXIT_INVARIANT,
        "{label}: invalid transition from {}",
        entry_status(entry)?
    );
    Ok(())
}
fn used_ids(state: &Value) -> Result<HashSet<String>> {
    let mut ids = HashSet::new();
    for collection in [
        "facts",
        "constraints",
        "unknowns",
        "hypotheses",
        "solutions",
        "implementations",
        "verification",
        "findings",
        "risks",
        "blockers",
    ] {
        for entry in entries(state, collection)? {
            if let Some(id) = entry.get("id").and_then(Value::as_str) {
                ids.insert(id.into());
            }
        }
    }
    for field in ["problem", "impact"] {
        if let Some(id) = object(state, "State")?
            .get(field)
            .and_then(Value::as_object)
            .and_then(|entry| entry.get("id"))
            .and_then(Value::as_str)
        {
            ids.insert(id.into());
        }
    }
    if let Some(retired) = object(state, "State")?
        .get("retired")
        .and_then(Value::as_object)
    {
        for collection in ["problems", "impacts"] {
            for entry in retired
                .get(collection)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(id) = entry.get("id").and_then(Value::as_str) {
                    ids.insert(id.into());
                }
            }
        }
    }
    Ok(ids)
}
fn require_new_id(state: &Value, id: &str) -> Result<()> {
    require!(
        !used_ids(state)?.contains(id),
        EXIT_INVARIANT,
        "entity id already used: {id}"
    );
    Ok(())
}

pub fn initial_state(event: &Value) -> Result<Value> {
    let stream = event_string(event, "stream")?;
    let head = json!({"event_id": event_string(event, "event_id")?, "rev": event_object(event)?.get("rev").unwrap()});
    if stream_kind(&stream)? == "work_item" {
        Ok(
            json!({"format":"engineering-eventstore-state","protocol_version":PROTOCOL_VERSION,"event_schema_version":EVENT_SCHEMA_VERSION,"state_schema_version":STATE_SCHEMA_VERSION,"stream":stream,"kind":"work_item","status":"discovered","head":head,"title":simple_record(event)?,"problem":null,"impact":null,"retired":{"problems":[],"impacts":[]},"facts":[],"constraints":[],"unknowns":[],"hypotheses":[],"solutions":[],"selected_solution":null,"implementations":[],"verification":[],"findings":[],"risks":[],"blockers":[],"decisions":[],"related_work_items":[]}),
        )
    } else {
        Ok(
            json!({"format":"engineering-eventstore-state","protocol_version":PROTOCOL_VERSION,"event_schema_version":EVENT_SCHEMA_VERSION,"state_schema_version":STATE_SCHEMA_VERSION,"stream":stream,"kind":"decision","status":"proposed","head":head,"topic":simple_record(event)?,"decision":null,"superseded_by":null}),
        )
    }
}

fn resolution_gaps(state: &Value) -> Result<Vec<String>> {
    let mut gaps = Vec::new();
    let verification = entries(state, "verification")?;
    if !verification
        .iter()
        .any(|entry| entry.get("required").and_then(Value::as_bool) == Some(true))
    {
        gaps.push("no active required verification criterion exists".into());
    }
    for entry in verification {
        if entry.get("required").and_then(Value::as_bool) == Some(true)
            && entry.get("result").and_then(Value::as_str) != Some("passed")
        {
            gaps.push(format!(
                "required verification {} is not passed",
                entry.get("id").and_then(Value::as_str).unwrap_or("?")
            ));
        }
    }
    for entry in entries(state, "blockers")? {
        if entry_status(entry)? == "active" {
            gaps.push(format!(
                "blocker {} is active",
                entry.get("id").and_then(Value::as_str).unwrap_or("?")
            ));
        }
    }
    for entry in entries(state, "risks")? {
        if !["accepted", "mitigated"].contains(&entry_status(entry)?) {
            gaps.push(format!(
                "risk {} is not accepted or mitigated",
                entry.get("id").and_then(Value::as_str).unwrap_or("?")
            ));
        }
    }
    let selected = object(state, "State")?
        .get("selected_solution")
        .and_then(Value::as_str);
    for entry in entries(state, "implementations")? {
        if entry_status(entry)? == "superseded" {
            continue;
        }
        if entry_status(entry)? != "completed" {
            gaps.push(format!(
                "implementation {} is not completed",
                entry.get("id").and_then(Value::as_str).unwrap_or("?")
            ));
        }
        if let Some(solution) = entry.get("solution_id").and_then(Value::as_str) {
            if selected.is_none() {
                gaps.push(format!(
                    "implementation {} names a solution but none is selected",
                    entry.get("id").and_then(Value::as_str).unwrap_or("?")
                ));
            } else if selected != Some(solution) {
                gaps.push(format!(
                    "implementation {} contradicts selected solution {}",
                    entry.get("id").and_then(Value::as_str).unwrap_or("?"),
                    selected.unwrap()
                ));
            }
        }
    }
    Ok(gaps)
}

pub fn apply_work_item(state: &mut Value, event: &Value) -> Result<()> {
    let event_type = event_string(event, "event")?;
    let current_status = status(state)?;
    if ["resolved", "cancelled"].contains(&current_status.as_str())
        && !["work_item.reopened", "stream.fork_reconciled"].contains(&event_type.as_str())
    {
        return Err(EngrError::new(
            EXIT_INVARIANT,
            format!("{event_type}: reopen terminal Work Item first"),
        ));
    }
    if current_status == "deferred"
        && ![
            "work_item.resumed",
            "work_item.cancelled",
            "stream.fork_reconciled",
        ]
        .contains(&event_type.as_str())
    {
        return Err(EngrError::new(
            EXIT_INVARIANT,
            format!("{event_type}: resume deferred Work Item first"),
        ));
    }
    if event_type == "stream.fork_reconciled" {
        return Ok(());
    }
    if event_type == "work_item.created" {
        return Err(EngrError::new(
            EXIT_INVARIANT,
            "work_item.created may only be the stream root",
        ));
    }
    if event_type == "problem.revised" || event_type == "impact.revised" {
        let field = if event_type == "problem.revised" {
            "problem"
        } else {
            "impact"
        };
        let id_field = if field == "problem" {
            "problem_id"
        } else {
            "impact_id"
        };
        let current = object(state, "State")?
            .get(field)
            .cloned()
            .unwrap_or(Value::Null);
        let data = event_data(event)?;
        if current.is_null() {
            require!(
                !data.contains_key("supersedes"),
                EXIT_INVARIANT,
                "{event_type}: first record cannot supersede"
            );
        } else {
            require!(
                data.get("supersedes").and_then(Value::as_str)
                    == current.get("id").and_then(Value::as_str),
                EXIT_INVARIANT,
                "{event_type}: supersedes must name current {field}"
            );
            let mut retired = current.clone();
            transition_entry(&mut retired, event, "superseded")?;
            object_mut(state, "State")?
                .get_mut("retired")
                .and_then(Value::as_object_mut)
                .unwrap()
                .get_mut(&format!("{field}s"))
                .and_then(Value::as_array_mut)
                .unwrap()
                .push(retired);
        }
        let id = data_string(event, id_field)?;
        require_new_id(state, &id)?;
        object_mut(state, "State")?.insert(field.into(), record_entry(event, &id, "active")?);
        if status(state)? == "discovered" {
            set_status(state, "clarifying")?;
        }
        return Ok(());
    }
    match event_type.as_str() {
        "fact.added" => {
            require!(
                !matches!(
                    event_object(event)?
                        .get("provenance")
                        .and_then(Value::as_object)
                        .and_then(|item| item.get("basis"))
                        .and_then(Value::as_str),
                    Some("inference" | "agent_proposal")
                ),
                EXIT_INVARIANT,
                "fact.added cannot use inference/proposal basis"
            );
            let id = data_string(event, "fact_id")?;
            require_new_id(state, &id)?;
            entries_mut(state, "facts")?.push(record_entry(event, &id, "active")?);
            if ["discovered", "clarifying"].contains(&status(state)?.as_str()) {
                set_status(state, "investigating")?;
            }
        }
        "fact.invalidated" => {
            let id = data_string(event, "fact_id")?;
            let entry = find_entry_mut(state, "facts", &id)?;
            require_active(entry, &["active"], "fact.invalidated")?;
            transition_entry(entry, event, "invalidated")?;
        }
        "constraint.added" => {
            let id = data_string(event, "constraint_id")?;
            require_new_id(state, &id)?;
            entries_mut(state, "constraints")?.push(record_entry(event, &id, "active")?);
        }
        "constraint.removed" => {
            let id = data_string(event, "constraint_id")?;
            let entry = find_entry_mut(state, "constraints", &id)?;
            require_active(entry, &["active"], "constraint.removed")?;
            transition_entry(entry, event, "removed")?;
        }
        "unknown.added" => {
            let id = data_string(event, "unknown_id")?;
            require_new_id(state, &id)?;
            entries_mut(state, "unknowns")?.push(record_entry(event, &id, "unresolved")?);
        }
        "unknown.resolved" => {
            let id = data_string(event, "unknown_id")?;
            let entry = find_entry_mut(state, "unknowns", &id)?;
            require_active(entry, &["unresolved"], "unknown.resolved")?;
            transition_entry(entry, event, "resolved")?;
        }
        "hypothesis.added" => {
            let id = data_string(event, "hypothesis_id")?;
            require_new_id(state, &id)?;
            entries_mut(state, "hypotheses")?.push(record_entry(event, &id, "active")?);
        }
        "hypothesis.invalidated" => {
            let id = data_string(event, "hypothesis_id")?;
            let entry = find_entry_mut(state, "hypotheses", &id)?;
            require_active(entry, &["active"], "hypothesis.invalidated")?;
            transition_entry(entry, event, "invalidated")?;
        }
        "solution.proposed" => {
            let id = data_string(event, "solution_id")?;
            require_new_id(state, &id)?;
            entries_mut(state, "solutions")?.push(record_entry(event, &id, "proposed")?);
        }
        "solution.selected" => {
            let id = data_string(event, "solution_id")?;
            require!(
                object(state, "State")?
                    .get("selected_solution")
                    .is_some_and(Value::is_null),
                EXIT_INVARIANT,
                "solution.selected: supersede current selection first"
            );
            let entry = find_entry_mut(state, "solutions", &id)?;
            require_active(entry, &["proposed"], "solution.selected")?;
            transition_entry(entry, event, "selected")?;
            object_mut(state, "State")?.insert("selected_solution".into(), Value::String(id));
            if !["implementing", "verifying"].contains(&status(state)?.as_str()) {
                set_status(state, "solution_ready")?;
            }
        }
        "solution.rejected" => {
            let id = data_string(event, "solution_id")?;
            let entry = find_entry_mut(state, "solutions", &id)?;
            require_active(entry, &["proposed"], "solution.rejected")?;
            transition_entry(entry, event, "rejected")?;
        }
        "solution.superseded" => {
            let id = data_string(event, "solution_id")?;
            let replacement = data_string(event, "by_solution_id").ok();
            let was_selected = object(state, "State")?
                .get("selected_solution")
                .and_then(Value::as_str)
                == Some(&id);
            {
                let entry = find_entry_mut(state, "solutions", &id)?;
                require_active(entry, &["proposed", "selected"], "solution.superseded")?;
                transition_entry(entry, event, "superseded")?;
                object_mut(entry, "solution")?.insert(
                    "superseded_by".into(),
                    replacement
                        .clone()
                        .map(Value::String)
                        .unwrap_or(Value::Null),
                );
            }
            if let Some(ref replacement_id) = replacement {
                let entry = find_entry_mut(state, "solutions", replacement_id)?;
                require!(
                    entry.get("id").and_then(Value::as_str) != Some(&id),
                    EXIT_INVARIANT,
                    "solution cannot supersede itself"
                );
                require_active(entry, &["proposed"], "solution.superseded replacement")?;
            }
            if was_selected {
                if let Some(ref replacement_id) = replacement {
                    let entry = find_entry_mut(state, "solutions", replacement_id)?;
                    transition_entry(entry, event, "selected")?;
                }
                object_mut(state, "State")?.insert(
                    "selected_solution".into(),
                    replacement
                        .clone()
                        .map(Value::String)
                        .unwrap_or(Value::Null),
                );
                for implementation in entries_mut(state, "implementations")? {
                    if implementation.get("solution_id").and_then(Value::as_str) == Some(&id)
                        && ["in_progress", "completed"].contains(&entry_status(implementation)?)
                    {
                        transition_entry(implementation, event, "superseded")?;
                    }
                }
                for criterion in entries_mut(state, "verification")? {
                    if !criterion.get("result").is_some_and(Value::is_null) {
                        transition_entry(criterion, event, "pending")?;
                        object_mut(criterion, "verification")?.insert("result".into(), Value::Null);
                        object_mut(criterion, "verification")?
                            .insert("artifacts".into(), json!([]));
                    }
                }
                let items = entries(state, "implementations")?;
                if items
                    .iter()
                    .any(|item| entry_status(item).is_ok_and(|value| value == "in_progress"))
                {
                    set_status(state, "implementing")?;
                } else if items
                    .iter()
                    .any(|item| entry_status(item).is_ok_and(|value| value == "completed"))
                {
                    set_status(state, "verifying")?;
                } else if object(state, "State")?
                    .get("selected_solution")
                    .and_then(Value::as_str)
                    .is_some()
                {
                    set_status(state, "solution_ready")?;
                } else {
                    set_status(state, "investigating")?;
                }
            }
        }
        "implementation.started" => {
            let id = data_string(event, "implementation_id")?;
            require_new_id(state, &id)?;
            let solution = event_data(event)?
                .get("solution_id")
                .and_then(Value::as_str);
            if let Some(solution) = solution {
                require!(
                    object(state, "State")?
                        .get("selected_solution")
                        .and_then(Value::as_str)
                        == Some(solution),
                    EXIT_INVARIANT,
                    "implementation solution must match selection"
                );
            }
            let mut entry = record_entry(event, &id, "in_progress")?;
            object_mut(&mut entry, "implementation")?.insert(
                "solution_id".into(),
                solution
                    .map(|item| Value::String(item.into()))
                    .unwrap_or(Value::Null),
            );
            object_mut(&mut entry, "implementation")?.insert("artifacts".into(), json!([]));
            entries_mut(state, "implementations")?.push(entry);
            set_status(state, "implementing")?;
        }
        "implementation.completed" => {
            let id = data_string(event, "implementation_id")?;
            let solution = event_data(event)?.get("solution_id").cloned();
            if let Some(solution) = solution.as_ref() {
                require!(
                    solution
                        == find_entry_mut(state, "implementations", &id)?
                            .get("solution_id")
                            .unwrap(),
                    EXIT_INVARIANT,
                    "implementation solution changed"
                );
                require!(
                    solution == object(state, "State")?.get("selected_solution").unwrap(),
                    EXIT_INVARIANT,
                    "implementation contradicts current selection"
                );
            }
            let entry = find_entry_mut(state, "implementations", &id)?;
            require_active(entry, &["in_progress"], "implementation.completed")?;
            transition_entry(entry, event, "completed")?;
            object_mut(entry, "implementation")?.insert(
                "artifacts".into(),
                event_data(event)?
                    .get("artifacts")
                    .cloned()
                    .unwrap_or(json!([])),
            );
            set_status(state, "verifying")?;
        }
        "verification.criterion_added" => {
            let id = data_string(event, "verification_id")?;
            require_new_id(state, &id)?;
            let mut entry = record_entry(event, &id, "pending")?;
            let map = object_mut(&mut entry, "verification")?;
            map.insert(
                "required".into(),
                event_data(event)?.get("required").unwrap().clone(),
            );
            map.insert("result".into(), Value::Null);
            map.insert("artifacts".into(), json!([]));
            entries_mut(state, "verification")?.push(entry);
        }
        "verification.result" => {
            let id = data_string(event, "verification_id")?;
            let result = data_string(event, "result")?;
            let entry = find_entry_mut(state, "verification", &id)?;
            require_active(
                entry,
                &["pending", "passed", "failed", "inconclusive"],
                "verification.result",
            )?;
            transition_entry(entry, event, &result)?;
            let map = object_mut(entry, "verification")?;
            map.insert("result".into(), Value::String(result));
            map.insert(
                "artifacts".into(),
                event_data(event)?
                    .get("artifacts")
                    .cloned()
                    .unwrap_or(json!([])),
            );
            set_status(state, "verifying")?;
        }
        "verification.invalidated" => {
            let id = data_string(event, "verification_id")?;
            let entry = find_entry_mut(state, "verification", &id)?;
            require_active(
                entry,
                &["passed", "failed", "inconclusive"],
                "verification.invalidated",
            )?;
            transition_entry(entry, event, "pending")?;
            let map = object_mut(entry, "verification")?;
            map.insert("result".into(), Value::Null);
            map.insert("artifacts".into(), json!([]));
            set_status(state, "verifying")?;
        }
        "finding.raised" => {
            let id = data_string(event, "finding_id")?;
            require_new_id(state, &id)?;
            entries_mut(state, "findings")?.push(record_entry(event, &id, "active")?);
        }
        "finding.promoted" => {
            let id = data_string(event, "finding_id")?;
            let target = data_string(event, "work_item_id")?;
            let stream = event_string(event, "stream")?;
            require!(
                target != stream,
                EXIT_INVARIANT,
                "finding cannot be promoted to its own Work Item"
            );
            let entry = find_entry_mut(state, "findings", &id)?;
            require_active(entry, &["active"], "finding.promoted")?;
            transition_entry(entry, event, "promoted")?;
            object_mut(entry, "finding")?.insert("work_item_id".into(), Value::String(target));
        }
        "decision.linked" | "decision.unlinked" => {
            let id = data_string(event, "decision_id")?;
            let existing = entries(state, "decisions")?
                .iter()
                .position(|entry| entry.get("decision_id").and_then(Value::as_str) == Some(&id));
            if event_type == "decision.linked" {
                if let Some(position) = existing {
                    let entry = &mut entries_mut(state, "decisions")?[position];
                    require!(
                        entry_status(entry)? == "unlinked",
                        EXIT_INVARIANT,
                        "decision already linked"
                    );
                    transition_entry(entry, event, "linked")?;
                } else {
                    let mut entry = json!({"decision_id":id,"status":"linked","introduced_by":event_string(event,"event_id")?});
                    transition_entry(&mut entry, event, "linked")?;
                    entries_mut(state, "decisions")?.push(entry);
                }
            } else {
                let position = existing
                    .ok_or_else(|| EngrError::new(EXIT_INVARIANT, "decision is not linked"))?;
                let entry = &mut entries_mut(state, "decisions")?[position];
                require!(
                    entry_status(entry)? == "linked",
                    EXIT_INVARIANT,
                    "decision is not linked"
                );
                transition_entry(entry, event, "unlinked")?;
            }
        }
        "work_item.related" | "work_item.unrelated" => {
            let target = data_string(event, "work_item_id")?;
            let relation = data_string(event, "relation")?;
            let stream = event_string(event, "stream")?;
            require!(
                target != stream,
                EXIT_INVARIANT,
                "Work Item cannot relate to itself"
            );
            let existing = entries(state, "related_work_items")?
                .iter()
                .position(|entry| {
                    entry.get("work_item_id").and_then(Value::as_str) == Some(&target)
                        && entry.get("relation").and_then(Value::as_str) == Some(&relation)
                });
            if event_type == "work_item.related" {
                if let Some(position) = existing {
                    let entry = &mut entries_mut(state, "related_work_items")?[position];
                    require!(
                        entry_status(entry)? == "removed",
                        EXIT_INVARIANT,
                        "relationship already active"
                    );
                    transition_entry(entry, event, "active")?;
                } else {
                    let mut entry = json!({"work_item_id":target,"relation":relation,"status":"active","introduced_by":event_string(event,"event_id")?});
                    transition_entry(&mut entry, event, "active")?;
                    entries_mut(state, "related_work_items")?.push(entry);
                }
            } else {
                let position = existing
                    .ok_or_else(|| EngrError::new(EXIT_INVARIANT, "relationship is not active"))?;
                let entry = &mut entries_mut(state, "related_work_items")?[position];
                require!(
                    entry_status(entry)? == "active",
                    EXIT_INVARIANT,
                    "relationship is not active"
                );
                transition_entry(entry, event, "removed")?;
            }
        }
        "risk.added" => {
            let id = data_string(event, "risk_id")?;
            require_new_id(state, &id)?;
            entries_mut(state, "risks")?.push(record_entry(event, &id, "open")?);
        }
        "risk.accepted" | "risk.mitigated" => {
            let id = data_string(event, "risk_id")?;
            let entry = find_entry_mut(state, "risks", &id)?;
            require_active(entry, &["open"], &event_type)?;
            transition_entry(
                entry,
                event,
                if event_type.ends_with("accepted") {
                    "accepted"
                } else {
                    "mitigated"
                },
            )?;
        }
        "work_item.blocked" => {
            let id = data_string(event, "blocker_id")?;
            require_new_id(state, &id)?;
            entries_mut(state, "blockers")?.push(record_entry(event, &id, "active")?);
            set_status(state, "blocked")?;
        }
        "work_item.unblocked" => {
            let id = data_string(event, "blocker_id")?;
            let entry = find_entry_mut(state, "blockers", &id)?;
            require_active(entry, &["active"], "work_item.unblocked")?;
            transition_entry(entry, event, "cleared")?;
            if !entries(state, "blockers")?
                .iter()
                .any(|entry| entry_status(entry).is_ok_and(|value| value == "active"))
            {
                set_status(state, "reopened")?;
            }
        }
        "work_item.deferred" => set_status(state, "deferred")?,
        "work_item.resumed" => {
            require!(
                status(state)? == "deferred",
                EXIT_INVARIANT,
                "work_item.resumed requires deferred status"
            );
            set_status(state, "reopened")?;
        }
        "work_item.resolved" => {
            let gaps = resolution_gaps(state)?;
            require!(
                gaps.is_empty(),
                EXIT_INVARIANT,
                "work_item.resolved gate failed: {}",
                gaps.join("; ")
            );
            set_status(state, "resolved")?;
        }
        "work_item.reopened" => {
            require!(
                ["resolved", "cancelled"].contains(&status(state)?.as_str()),
                EXIT_INVARIANT,
                "work_item.reopened requires terminal status"
            );
            set_status(state, "reopened")?;
        }
        "work_item.cancelled" => set_status(state, "cancelled")?,
        _ => {
            return Err(EngrError::new(
                EXIT_SCHEMA,
                format!("unimplemented Work Item event: {event_type}"),
            ))
        }
    }
    Ok(())
}

pub fn apply_decision(state: &mut Value, event: &Value) -> Result<()> {
    let event_type = event_string(event, "event")?;
    if event_type == "stream.fork_reconciled" {
        return Ok(());
    }
    match event_type.as_str() {
        "decision.created" => {
            return Err(EngrError::new(
                EXIT_INVARIANT,
                "decision.created may only be the stream root",
            ))
        }
        "decision.revised" => {
            require!(
                status(state)? == "proposed",
                EXIT_INVARIANT,
                "accepted decisions cannot be revised"
            );
            object_mut(state, "State")?.insert("topic".into(), simple_record(event)?);
        }
        "decision.accepted" => {
            require!(
                status(state)? == "proposed",
                EXIT_INVARIANT,
                "decision.accepted requires proposed status"
            );
            object_mut(state, "State")?.insert("decision".into(), simple_record(event)?);
            set_status(state, "accepted")?;
        }
        "decision.superseded" => {
            require!(
                status(state)? == "accepted",
                EXIT_INVARIANT,
                "decision.superseded requires accepted status"
            );
            let target = data_string(event, "by_decision_id")?;
            require!(
                target != event_string(event, "stream")?,
                EXIT_INVARIANT,
                "decision.superseded cannot target its own stream"
            );
            object_mut(state, "State")?.insert("superseded_by".into(), Value::String(target));
            set_status(state, "superseded")?;
        }
        "decision.revoked" => {
            require!(
                status(state)? == "accepted",
                EXIT_INVARIANT,
                "decision.revoked requires accepted status"
            );
            set_status(state, "revoked")?;
        }
        _ => {
            return Err(EngrError::new(
                EXIT_SCHEMA,
                format!("unimplemented Decision event: {event_type}"),
            ))
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct CanonicalChain {
    pub chain: Vec<Value>,
    pub rejected: BTreeSet<String>,
    pub resolutions: Vec<Value>,
}

fn descendants(root: &str, children: &HashMap<String, Vec<String>>) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut stack = vec![root.to_owned()];
    while let Some(current) = stack.pop() {
        if found.insert(current.clone()) {
            if let Some(items) = children.get(&current) {
                stack.extend(items.iter().cloned());
            }
        }
    }
    found
}

pub fn canonical_chain(events: &[Value], stream: &str) -> Result<CanonicalChain> {
    require!(
        !events.is_empty(),
        crate::EXIT_NOT_FOUND,
        "stream not found: {stream}"
    );
    let mut by_id: HashMap<String, &Value> = HashMap::new();
    for event in events {
        let id = event_string(event, "event_id")?;
        require!(
            by_id.insert(id.clone(), event).is_none(),
            EXIT_INVARIANT,
            "duplicate event id in stream {stream}"
        );
    }
    let roots: Vec<_> = events
        .iter()
        .filter(|event| event.get("parent").is_some_and(Value::is_null))
        .collect();
    require!(
        roots.len() == 1,
        EXIT_INVARIANT,
        "stream {stream}: expected exactly one root"
    );
    let root = roots[0];
    let expected = if stream_kind(stream)? == "work_item" {
        "work_item.created"
    } else {
        "decision.created"
    };
    require!(
        event_string(root, "event")? == expected,
        EXIT_INVARIANT,
        "stream {stream}: invalid root event"
    );
    require!(
        root.get("rev").and_then(Value::as_i64) == Some(1),
        EXIT_INVARIANT,
        "stream {stream}: root revision must be 1"
    );
    let mut children: HashMap<String, Vec<String>> =
        by_id.keys().map(|id| (id.clone(), Vec::new())).collect();
    for event in events {
        if let Some(parent) = event.get("parent").and_then(Value::as_str) {
            let parent_event = by_id.get(parent).ok_or_else(|| {
                EngrError::new(
                    EXIT_INVARIANT,
                    format!("stream {stream}: missing parent {parent}"),
                )
            })?;
            require!(
                event.get("rev").and_then(Value::as_i64)
                    == parent_event
                        .get("rev")
                        .and_then(Value::as_i64)
                        .map(|value| value + 1),
                EXIT_INVARIANT,
                "stream {stream}: revision does not follow parent"
            );
            children
                .get_mut(parent)
                .unwrap()
                .push(event_string(event, "event_id")?);
        }
    }
    let root_id = event_string(root, "event_id")?;
    let mut reached = HashSet::new();
    let mut stack = vec![root_id.clone()];
    while let Some(id) = stack.pop() {
        require!(
            reached.insert(id.clone()),
            EXIT_INVARIANT,
            "stream {stream}: cycle detected"
        );
        stack.extend(children.get(&id).unwrap().iter().cloned());
    }
    require!(
        reached.len() == by_id.len(),
        EXIT_INVARIANT,
        "stream {stream}: disconnected event graph"
    );
    let mut chain = Vec::new();
    let mut rejected = BTreeSet::new();
    let mut resolutions = Vec::new();
    let mut current = root_id;
    let mut seen = HashSet::new();
    loop {
        require!(
            seen.insert(current.clone()),
            EXIT_INVARIANT,
            "stream {stream}: canonical cycle"
        );
        chain.push((*by_id[&current]).clone());
        let kids: Vec<_> = children[&current]
            .iter()
            .filter(|id| !rejected.contains(*id))
            .cloned()
            .collect();
        if kids.is_empty() {
            break;
        }
        if kids.len() == 1 {
            current = kids[0].clone();
            continue;
        }
        let candidates: Vec<_> = events
            .iter()
            .filter(|event| {
                event.get("event").and_then(Value::as_str) == Some("stream.fork_reconciled")
                    && event
                        .get("data")
                        .and_then(Value::as_object)
                        .and_then(|data| data.get("fork_parent"))
                        .and_then(Value::as_str)
                        == Some(&current)
            })
            .collect();
        require!(
            candidates.len() == 1,
            EXIT_FORK,
            "stream {stream}: unresolved or multiply reconciled fork at {current}"
        );
        let marker = candidates[0];
        let data = event_data(marker)?;
        let accepted = data.get("accepted_root").and_then(Value::as_str).unwrap();
        let rejected_roots = data
            .get("rejected_roots")
            .and_then(Value::as_array)
            .unwrap();
        let direct: HashSet<_> = kids.iter().map(String::as_str).collect();
        require!(
            direct.contains(accepted),
            EXIT_FORK,
            "fork reconciliation accepted_root is not a direct child"
        );
        let rejected_set: HashSet<_> = rejected_roots.iter().filter_map(Value::as_str).collect();
        require!(
            rejected_set
                == direct
                    .iter()
                    .filter(|id| **id != accepted)
                    .copied()
                    .collect(),
            EXIT_FORK,
            "fork reconciliation must reject every other direct child"
        );
        let marker_parent = marker.get("parent").and_then(Value::as_str).unwrap();
        let mut node = accepted.to_owned();
        while node != marker_parent {
            let node_kids = &children[&node];
            require!(
                node_kids.len() == 1,
                EXIT_FORK,
                "fork reconciliation accepted branch is not linear"
            );
            let next = &node_kids[0];
            require!(
                by_id[next].get("event").and_then(Value::as_str) != Some("stream.fork_reconciled"),
                EXIT_FORK,
                "nested reconciliation on accepted path"
            );
            node = next.clone();
        }
        require!(
            children[&node].len() == 1 && children[&node][0] == event_string(marker, "event_id")?,
            EXIT_FORK,
            "fork reconciliation marker must be next after accepted head"
        );
        for root in rejected_roots.iter().filter_map(Value::as_str) {
            rejected.extend(descendants(root, &children));
        }
        resolutions.push((*marker).clone());
        current = accepted.to_owned();
    }
    let categorized: HashSet<String> = chain
        .iter()
        .filter_map(|event| {
            event
                .get("event_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .chain(rejected.iter().cloned())
        .collect();
    require!(
        categorized.len() == by_id.len(),
        EXIT_FORK,
        "stream {stream}: uncategorized fork history"
    );
    Ok(CanonicalChain {
        chain,
        rejected,
        resolutions,
    })
}

/// Finds the current head of the accepted branch while a same-parent fork is unresolved.
/// Only `stream.fork_reconciled` may use this path; normal appends must reject the fork.
pub fn reconciliation_parent(events: &[Value], stream: &str, data: &Value) -> Result<String> {
    require!(
        !events.is_empty(),
        crate::EXIT_NOT_FOUND,
        "stream not found: {stream}"
    );
    let data = object(data, "stream.fork_reconciled.data")?;
    let fork_parent = data
        .get("fork_parent")
        .and_then(Value::as_str)
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, "fork_parent: invalid event id"))?;
    let accepted = data
        .get("accepted_root")
        .and_then(Value::as_str)
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, "accepted_root: invalid event id"))?;
    let rejected = data
        .get("rejected_roots")
        .and_then(Value::as_array)
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, "rejected_roots: invalid"))?;
    let mut by_id: HashMap<String, &Value> = HashMap::new();
    let mut children: HashMap<String, Vec<String>> = HashMap::new();
    for event in events {
        let id = event_string(event, "event_id")?;
        by_id.insert(id.clone(), event);
        children.entry(id).or_default();
    }
    require!(
        by_id.contains_key(fork_parent),
        EXIT_FORK,
        "fork_parent not found"
    );
    for event in events {
        if let Some(parent) = event.get("parent").and_then(Value::as_str) {
            if let Some(items) = children.get_mut(parent) {
                items.push(event_string(event, "event_id")?);
            }
        }
    }
    let direct = children.get(fork_parent).unwrap();
    require!(direct.len() > 1, EXIT_FORK, "fork_parent has no fork");
    require!(
        direct.iter().any(|id| id == accepted),
        EXIT_FORK,
        "accepted_root is not a fork child"
    );
    let rejected_set: HashSet<_> = rejected.iter().filter_map(Value::as_str).collect();
    let expected: HashSet<_> = direct
        .iter()
        .filter(|id| id.as_str() != accepted)
        .map(String::as_str)
        .collect();
    require!(
        rejected_set == expected,
        EXIT_FORK,
        "rejected_roots must list all other fork children"
    );
    let mut node = accepted.to_owned();
    while !children[&node].is_empty() {
        require!(
            children[&node].len() == 1,
            EXIT_FORK,
            "accepted branch contains another fork"
        );
        node = children[&node][0].clone();
        require!(
            by_id[&node].get("event").and_then(Value::as_str) != Some("stream.fork_reconciled"),
            EXIT_FORK,
            "fork already reconciled"
        );
    }
    Ok(node)
}

pub fn reduce_chain(chain: &[Value], base: Option<&Value>) -> Result<Value> {
    require!(
        !chain.is_empty(),
        EXIT_INVARIANT,
        "cannot reduce empty chain"
    );
    let mut state;
    let start;
    if let Some(base) = base {
        let base_head = base
            .get("head")
            .and_then(Value::as_object)
            .and_then(|head| head.get("event_id"))
            .and_then(Value::as_str)
            .ok_or_else(|| EngrError::new(EXIT_INVARIANT, "replay base has invalid head"))?;
        start = chain
            .iter()
            .position(|event| event.get("event_id").and_then(Value::as_str) == Some(base_head))
            .ok_or_else(|| {
                EngrError::new(
                    EXIT_INVARIANT,
                    "replay base is not an ancestor of the current head",
                )
            })?
            + 1;
        state = base.clone();
        object_mut(&mut state, "State")?.remove("integrity");
    } else {
        state = initial_state(&chain[0])?;
        start = 1;
    }
    for event in &chain[start..] {
        if state.get("kind").and_then(Value::as_str) == Some("work_item") {
            apply_work_item(&mut state, event)?;
        } else {
            apply_decision(&mut state, event)?;
        }
        object_mut(&mut state, "State")?.insert(
            "head".into(),
            json!({"event_id":event_string(event,"event_id")?,"rev":event.get("rev").unwrap()}),
        );
    }
    attach_integrity(&mut state)?;
    Ok(state)
}
