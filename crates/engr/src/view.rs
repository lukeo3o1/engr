//! Read surfaces: staleness assessment, `show`, and `ls`.
//!
//! Two rules shape everything here. The confirmed wording and how much it can
//! be trusted appear on the *same* default surface — the previous design hid the
//! confirmed text behind a flag, and a reader given the default output drew the
//! opposite conclusion from the truth. And nothing is truncated: an agent asked
//! to reason from a record needs all of it.

use crate::git;
use crate::model::{Object, Ref, Status};
use crate::{store, Result};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct RefDrift {
    pub object: String,
    pub section: u64,
    pub confirmed_sha256: String,
    pub current_sha256: Option<String>,
    pub lookback: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SectionStatus {
    pub basis: Option<git::Distance>,
    pub drifted: Vec<RefDrift>,
}

impl SectionStatus {
    pub fn is_ok(&self) -> bool {
        self.basis.is_none() && self.drifted.is_empty()
    }

    pub fn label(&self) -> &'static str {
        match (self.basis.is_some(), !self.drifted.is_empty()) {
            (false, false) => "ok",
            (true, false) => "地基已變動",
            (false, true) => "依據已變動",
            (true, true) => "地基與依據都變動",
        }
    }

    pub fn key(&self) -> &'static str {
        match (self.basis.is_some(), !self.drifted.is_empty()) {
            (false, false) => "ok",
            (true, false) => "stale_basis",
            (false, true) => "stale_refs",
            (true, true) => "stale_both",
        }
    }
}

/// For commit ids and content hashes, which are random throughout.
fn short(value: &str) -> &str {
    &value[..8.min(value.len())]
}

/// For object ids, whose leading characters are a timestamp. `len` comes from
/// [`store::abbrev_len`] over the whole set.
fn abbrev(id: &str, len: usize) -> &str {
    &id[..len.min(id.len())]
}

/// The abbreviation width to use across one command's output.
pub fn width(root: &Path) -> usize {
    store::object_ids(root)
        .map(|ids| store::abbrev_len(&ids))
        .unwrap_or(8)
}

fn drift_for(root: &Path, reference: &Ref) -> RefDrift {
    let current = store::load_object(root, &reference.object)
        .ok()
        .and_then(|target| target.section(reference.section).ok().cloned())
        .map(|section| section.sha256);
    let lookback = current
        .as_ref()
        .filter(|now| **now != reference.sha256)
        .map(|_| {
            format!(
                "git show {}:{}/objects/{}.json",
                short(&reference.commit),
                store::DIR,
                reference.object
            )
        });
    RefDrift {
        object: reference.object.clone(),
        section: reference.section,
        confirmed_sha256: reference.sha256.clone(),
        current_sha256: current,
        lookback,
    }
}

/// Assess every section. Status is computed, never stored — a stored verdict
/// would be wrong the moment HEAD moved.
pub fn assess(root: &Path, object: &Object) -> Vec<(u64, SectionStatus)> {
    object
        .sections
        .iter()
        .map(|section| {
            let basis = section
                .based_on
                .as_deref()
                .and_then(|commit| git::distance(root, commit))
                .filter(git::Distance::moved);
            let drifted = section
                .refs
                .iter()
                .map(|reference| drift_for(root, reference))
                .filter(|drift| {
                    drift.current_sha256.as_deref() != Some(drift.confirmed_sha256.as_str())
                })
                .collect();
            (section.id, SectionStatus { basis, drifted })
        })
        .collect()
}

pub struct Counts {
    pub total: usize,
    pub ok: usize,
    pub attention: usize,
}

pub fn counts(assessment: &[(u64, SectionStatus)]) -> Counts {
    let ok = assessment
        .iter()
        .filter(|(_, status)| status.is_ok())
        .count();
    Counts {
        total: assessment.len(),
        ok,
        attention: assessment.len() - ok,
    }
}

pub fn render_show(root: &Path, object: &Object) -> String {
    let assessment = assess(root, object);
    let tally = counts(&assessment);
    let w = width(root);
    let mut out = String::new();
    out.push_str(&format!(
        "{}  {}  {}\n",
        abbrev(&object.id, w),
        object.status.as_str(),
        object.title
    ));
    out.push_str(&format!("{} sections   {} ok", tally.total, tally.ok));
    if tally.attention > 0 {
        out.push_str(&format!("   {} 需注意", tally.attention));
    }
    out.push_str(&format!("   rev {}\n", object.rev));
    if let Some(commit) = &object.last_projection_commit {
        out.push_str(&format!("last_projection_commit  {}\n", short(commit)));
    }

    for section in &object.sections {
        let status = assessment
            .iter()
            .find(|(id, _)| *id == section.id)
            .map(|(_, status)| status.clone())
            .unwrap_or_default();
        out.push_str(&format!("\n── §{} ── {}\n", section.id, status.label()));
        out.push_str(section.text.trim_end());
        out.push('\n');
        if let Some(commit) = &section.based_on {
            out.push_str(&format!(
                "    based_on {}   confirmed {}\n",
                short(commit),
                section.confirmed_at
            ));
        } else {
            out.push_str(&format!("    confirmed {}\n", section.confirmed_at));
        }
        for reference in &section.refs {
            out.push_str(&format!(
                "    refs     {} §{}\n",
                abbrev(&reference.object, w),
                reference.section
            ));
        }
        // Say what to do about it, rather than making the reader work it out.
        if let Some(distance) = &status.basis {
            out.push_str(&format!(
                "    建議     自 {} 起 {} 個 commit、{} 個檔案變動;確認本段是否仍成立\n",
                short(section.based_on.as_deref().unwrap_or("")),
                distance.commits,
                distance.files.len()
            ));
        }
        for drift in &status.drifted {
            match (&drift.current_sha256, &drift.lookback) {
                (Some(current), Some(lookback)) => {
                    out.push_str(&format!(
                        "    建議     {} §{} 確認時 {} → 目前 {}\n             {}\n",
                        abbrev(&drift.object, w),
                        drift.section,
                        short(&drift.confirmed_sha256),
                        short(current),
                        lookback
                    ));
                }
                _ => {
                    out.push_str(&format!(
                        "    建議     {} §{} 已不存在;本段的依據消失了\n",
                        abbrev(&drift.object, w),
                        drift.section
                    ));
                }
            }
        }
    }
    out
}

#[derive(Serialize)]
struct JsonSection<'a> {
    id: u64,
    text: &'a str,
    status: &'static str,
    based_on: Option<&'a str>,
    refs: &'a [Ref],
    sha256: &'a str,
    confirmed_at: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    basis_commits_behind: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    basis_files_changed: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    stale: Vec<RefDrift>,
}

#[derive(Serialize)]
struct JsonSummary {
    sections: usize,
    ok: usize,
    attention: usize,
}

#[derive(Serialize)]
struct JsonObject<'a> {
    id: &'a str,
    title: &'a str,
    status: &'static str,
    rev: u64,
    last_projection_commit: Option<&'a str>,
    summary: JsonSummary,
    sections: Vec<JsonSection<'a>>,
}

pub fn render_show_json(root: &Path, object: &Object) -> Result<String> {
    let assessment = assess(root, object);
    let tally = counts(&assessment);
    let sections = object
        .sections
        .iter()
        .map(|section| {
            let status = assessment
                .iter()
                .find(|(id, _)| *id == section.id)
                .map(|(_, status)| status.clone())
                .unwrap_or_default();
            JsonSection {
                id: section.id,
                text: &section.text,
                status: status.key(),
                based_on: section.based_on.as_deref(),
                refs: &section.refs,
                sha256: &section.sha256,
                confirmed_at: &section.confirmed_at,
                basis_commits_behind: status.basis.as_ref().map(|item| item.commits),
                basis_files_changed: status.basis.as_ref().map(|item| item.files.len()),
                stale: status.drifted.clone(),
            }
        })
        .collect();
    let value = JsonObject {
        id: &object.id,
        title: &object.title,
        status: object.status.as_str(),
        rev: object.rev,
        last_projection_commit: object.last_projection_commit.as_deref(),
        summary: JsonSummary {
            sections: tally.total,
            ok: tally.ok,
            attention: tally.attention,
        },
        sections,
    };
    serde_json::to_string_pretty(&value)
        .map_err(|error| crate::Error::new(crate::EXIT_SCHEMA, format!("json: {error}")))
}

/// One line per object, stable columns, never wrapped — so `grep`, `awk` and
/// `fzf` all compose with it.
pub fn render_ls(root: &Path, objects: &[Object], keyword: Option<&str>) -> String {
    let w = width(root);
    let mut out = String::new();
    for object in objects {
        let assessment = assess(root, object);
        let tally = counts(&assessment);
        let hits = keyword.map(|needle| matching_sections(object, needle));
        if let Some(hits) = &hits {
            if hits.is_empty()
                && !object
                    .title
                    .to_lowercase()
                    .contains(&keyword.unwrap().to_lowercase())
            {
                continue;
            }
        }
        let note = match &hits {
            Some(hits) if !hits.is_empty() => hits
                .iter()
                .map(|id| format!("§{id}"))
                .collect::<Vec<_>>()
                .join(" "),
            _ if tally.attention > 0 => format!("{} 需注意", tally.attention),
            _ => "ok".to_owned(),
        };
        out.push_str(&format!(
            "{}  {:<6}  {:>2} sections  {:<12}  {}\n",
            abbrev(&object.id, w),
            object.status.as_str(),
            tally.total,
            note,
            object.title
        ));
    }
    out
}

fn matching_sections(object: &Object, needle: &str) -> Vec<u64> {
    let needle = needle.to_lowercase();
    object
        .sections
        .iter()
        .filter(|section| section.text.to_lowercase().contains(&needle))
        .map(|section| section.id)
        .collect()
}

/// One line per section, so grep can reach the text.
pub fn render_ls_sections(root: &Path, objects: &[Object]) -> String {
    let w = width(root);
    let mut out = String::new();
    for object in objects {
        for section in &object.sections {
            out.push_str(&format!(
                "{} §{:<3} {:<6}  {}\n",
                abbrev(&object.id, w),
                section.id,
                object.status.as_str(),
                section.text.replace('\n', " ").trim()
            ));
        }
    }
    out
}

/// The one case worth interrupting for by default: an object someone declared
/// finished, whose basis has since moved. Closed means nobody is looking, which
/// is exactly when drift goes unnoticed.
pub fn render_stale(root: &Path, objects: &[Object]) -> String {
    let w = width(root);
    let mut out = String::new();
    for object in objects {
        for (id, status) in assess(root, object) {
            if status.is_ok() {
                continue;
            }
            let closed = object.status == Status::Closed;
            let marker = if closed { "⚠" } else { "·" };
            let tail = if closed {
                " —— 沒有人在看的物件"
            } else {
                ""
            };
            out.push_str(&format!(
                "{} {}  {:<6}  §{}  {}{}\n",
                marker,
                abbrev(&object.id, w),
                object.status.as_str(),
                id,
                status.label(),
                tail
            ));
        }
    }
    out
}
