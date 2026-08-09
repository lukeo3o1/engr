use crate::{EngrError, Result, EXIT_SCHEMA};
use serde_json::Value;

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, format!("State.{key}: expected string")))
}
fn format_item(entry: &Value, provenance: bool) -> Result<String> {
    let mut line = format!("{} — {}", string(entry, "id")?, string(entry, "text")?);
    if provenance {
        let detail = entry
            .get("provenance")
            .and_then(Value::as_object)
            .ok_or_else(|| EngrError::new(EXIT_SCHEMA, "State entry provenance missing"))?;
        line.push_str(&format!(
            " [{}; event {}; {}/{}]",
            string(entry, "status")?,
            string(entry, "last_event_id")?,
            detail
                .get("initiator")
                .and_then(Value::as_str)
                .unwrap_or("?"),
            detail.get("basis").and_then(Value::as_str).unwrap_or("?")
        ));
    }
    Ok(line)
}
fn format_link(entry: &Value, label: &str) -> Result<String> {
    let provenance = entry
        .get("provenance")
        .and_then(Value::as_object)
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, "State link provenance missing"))?;
    Ok(format!(
        "{label} — {} [{}; event {}; {}/{}]",
        string(entry, "last_text")?,
        string(entry, "status")?,
        string(entry, "last_event_id")?,
        provenance
            .get("initiator")
            .and_then(Value::as_str)
            .unwrap_or("?"),
        provenance
            .get("basis")
            .and_then(Value::as_str)
            .unwrap_or("?")
    ))
}

pub fn render_state(state: &Value, provenance: bool, markdown: bool) -> Result<String> {
    let heading = if markdown { "# " } else { "" };
    let sub = if markdown { "## " } else { "" };
    let head = state
        .get("head")
        .and_then(Value::as_object)
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, "State.head missing"))?;
    let mut lines = vec![
        format!(
            "{heading}{} — {}",
            string(state, "stream")?,
            string(state, "status")?
        ),
        format!(
            "State through: rev {} ({})",
            head.get("rev")
                .and_then(Value::as_i64)
                .ok_or_else(|| EngrError::new(EXIT_SCHEMA, "State.head.rev missing"))?,
            head.get("event_id").and_then(Value::as_str).unwrap_or("?")
        ),
    ];
    if state.get("kind").and_then(Value::as_str) == Some("decision") {
        lines.extend([
            String::new(),
            format!("{sub}Topic"),
            state["topic"]["text"].as_str().unwrap_or("?").to_owned(),
        ]);
        if let Some(decision) = state.get("decision").and_then(Value::as_object) {
            lines.extend([
                String::new(),
                format!("{sub}Decision"),
                decision
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("?")
                    .to_owned(),
            ]);
        }
        if let Some(target) = state.get("superseded_by").and_then(Value::as_str) {
            lines.extend([String::new(), format!("Superseded by: {target}")]);
        }
        return Ok(lines.join("\n") + "\n");
    }
    for (title, key) in [("Problem", "problem"), ("Impact", "impact")] {
        if let Some(entry) = state.get(key).filter(|entry| !entry.is_null()) {
            lines.extend([
                String::new(),
                format!("{sub}{title}"),
                if provenance {
                    format_item(entry, true)?
                } else {
                    entry
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("?")
                        .to_owned()
                },
            ]);
        }
    }
    let sections: [(&str, &str, &[&str]); 10] = [
        ("Facts", "facts", &["active"]),
        ("Constraints", "constraints", &["active"]),
        ("Unknowns", "unknowns", &["unresolved"]),
        ("Hypotheses", "hypotheses", &["active"]),
        ("Solutions", "solutions", &["proposed", "selected"]),
        (
            "Implementations",
            "implementations",
            &["in_progress", "completed"],
        ),
        (
            "Verification",
            "verification",
            &["pending", "passed", "failed", "inconclusive"],
        ),
        ("Findings", "findings", &["active"]),
        ("Risks", "risks", &["open", "accepted", "mitigated"]),
        ("Blockers", "blockers", &["active"]),
    ];
    for (title, key, statuses) in sections {
        let entries = state
            .get(key)
            .and_then(Value::as_array)
            .ok_or_else(|| EngrError::new(EXIT_SCHEMA, format!("State.{key}: expected array")))?;
        let selected: Vec<_> = entries
            .iter()
            .filter(|entry| {
                provenance
                    || entry
                        .get("status")
                        .and_then(Value::as_str)
                        .is_some_and(|status| statuses.contains(&status))
            })
            .collect();
        if selected.is_empty() {
            continue;
        }
        lines.extend([String::new(), format!("{sub}{title}")]);
        for entry in selected {
            let mut text = format_item(entry, provenance)?;
            if key == "verification" {
                text.push_str(&format!(
                    " [required={}, result={}]",
                    entry
                        .get("required")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    entry
                        .get("result")
                        .and_then(Value::as_str)
                        .unwrap_or("pending")
                ));
            }
            lines.push(format!("- {text}"));
        }
    }
    lines.extend([
        String::new(),
        format!(
            "Selected solution: {}",
            state
                .get("selected_solution")
                .and_then(Value::as_str)
                .unwrap_or("none")
        ),
    ]);
    if !provenance {
        if let Some(items) = state.get("decisions").and_then(Value::as_array) {
            let decisions: Vec<_> = items
                .iter()
                .filter(|entry| entry.get("status").and_then(Value::as_str) == Some("linked"))
                .filter_map(|entry| entry.get("decision_id").and_then(Value::as_str))
                .collect();
            if !decisions.is_empty() {
                lines.push(format!("Decisions: {}", decisions.join(", ")));
            }
        }
    }
    if provenance {
        if let Some(entries) = state
            .get("decisions")
            .and_then(Value::as_array)
            .filter(|entries| !entries.is_empty())
        {
            lines.extend([String::new(), format!("{sub}Decisions")]);
            for entry in entries {
                lines.push(format!(
                    "- {}",
                    format_link(
                        entry,
                        entry
                            .get("decision_id")
                            .and_then(Value::as_str)
                            .unwrap_or("?")
                    )?
                ));
            }
        }
        if let Some(entries) = state
            .get("related_work_items")
            .and_then(Value::as_array)
            .filter(|entries| !entries.is_empty())
        {
            lines.extend([String::new(), format!("{sub}Related work items")]);
            for entry in entries {
                let label = format!(
                    "{} ({})",
                    entry
                        .get("work_item_id")
                        .and_then(Value::as_str)
                        .unwrap_or("?"),
                    entry.get("relation").and_then(Value::as_str).unwrap_or("?")
                );
                lines.push(format!("- {}", format_link(entry, &label)?));
            }
        }
        if let Some(retired) = state.get("retired").and_then(Value::as_object) {
            for (kind, entries) in retired {
                if let Some(entries) = entries.as_array().filter(|entries| !entries.is_empty()) {
                    lines.extend([String::new(), format!("{sub}Retired {kind}")]);
                    for entry in entries {
                        lines.push(format!("- {}", format_item(entry, true)?));
                    }
                }
            }
        }
    }
    Ok(lines.join("\n") + "\n")
}
