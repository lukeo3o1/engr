//! Read surfaces: staleness assessment, `show`, and `ls`.
//!
//! Two rules shape everything here. The confirmed wording and how much it can
//! be trusted appear on the *same* default surface — the previous design hid the
//! confirmed text behind a flag, and a reader given the default output drew the
//! opposite conclusion from the truth. And nothing is truncated: an agent asked
//! to reason from a record needs all of it.

use crate::backlog;
use crate::git;
use crate::model::{Object, Ref, Status};
use crate::{ops, store, Result};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct RefDrift {
    pub object: String,
    pub section: u64,
    pub confirmed_sha256: String,
    pub current_sha256: Option<String>,
    pub lookback: Option<String>,
    /// The target's stored content no longer hashes to the target's own
    /// recorded hash. Comparing hashes cannot see this: an edit that leaves the
    /// stored hash alone leaves `current_sha256` equal to what was pinned, so
    /// the ref looks unmoved while the wording under it was rewritten.
    pub target_tampered: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SectionStatus {
    /// This section's stored content does not hash to its own recorded hash.
    /// Dominates every other signal: a forged section's drift assessment is an
    /// assessment of something nobody confirmed.
    pub tampered: bool,
    pub basis: Option<git::Distance>,
    pub drifted: Vec<RefDrift>,
}

impl SectionStatus {
    pub fn is_ok(&self) -> bool {
        !self.tampered && self.basis.is_none() && self.drifted.is_empty()
    }

    /// A section whose own words are intact, standing on wording that is not
    /// what was confirmed.
    pub fn stands_on_tampered(&self) -> bool {
        self.drifted.iter().any(|drift| drift.target_tampered)
    }

    /// Whether the wording here can be trusted at all — either it was forged,
    /// or what it explicitly leans on was.
    pub fn forged(&self) -> bool {
        self.tampered || self.stands_on_tampered()
    }

    pub fn label(&self) -> &'static str {
        if self.tampered {
            return "TAMPERED";
        }
        if self.stands_on_tampered() {
            return "REF TAMPERED";
        }
        match (self.basis.is_some(), !self.drifted.is_empty()) {
            (false, false) => "ok",
            (true, false) => "basis moved",
            (false, true) => "refs moved",
            (true, true) => "basis and refs moved",
        }
    }

    pub fn key(&self) -> &'static str {
        if self.tampered {
            return "tampered";
        }
        if self.stands_on_tampered() {
            return "ref_tampered";
        }
        match (self.basis.is_some(), !self.drifted.is_empty()) {
            (false, false) => "ok",
            (true, false) => "stale_basis",
            (false, true) => "stale_refs",
            (true, true) => "stale_both",
        }
    }
}

/// Does this section's stored content still hash to its recorded hash?
///
/// A hash that cannot be recomputed counts as a mismatch: the alternative is
/// reporting content we could not check as sound.
fn tampered(section: &crate::model::Section) -> bool {
    section
        .recomputed_sha256()
        .map(|now| now != section.sha256)
        .unwrap_or(true)
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
    let target = ops::effective(root, &reference.object)
        .ok()
        .and_then(|target| target.section(reference.section).ok().cloned());
    let target_tampered = target.as_ref().map(tampered).unwrap_or(false);
    let current = target.map(|section| section.sha256);
    let moved = current.as_deref() != Some(reference.sha256.as_str());
    // Worth offering whenever the target is not what was pinned, whether it was
    // revised through the gate or rewritten behind it.
    let lookback = (current.is_some() && (moved || target_tampered)).then(|| {
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
        target_tampered,
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
                    drift.target_tampered
                        || drift.current_sha256.as_deref() != Some(drift.confirmed_sha256.as_str())
                })
                .collect();
            (
                section.id,
                SectionStatus {
                    tampered: tampered(section),
                    basis,
                    drifted,
                },
            )
        })
        .collect()
}

pub struct Counts {
    pub total: usize,
    pub ok: usize,
    /// Sections whose wording, or whose stated basis, is not what was
    /// confirmed. Counted apart from `attention`: drift is a question for a
    /// human, this is a broken record.
    pub tampered: usize,
    pub attention: usize,
}

pub fn counts(assessment: &[(u64, SectionStatus)]) -> Counts {
    let ok = assessment
        .iter()
        .filter(|(_, status)| status.is_ok())
        .count();
    let tampered = assessment
        .iter()
        .filter(|(_, status)| status.forged())
        .count();
    Counts {
        total: assessment.len(),
        ok,
        tampered,
        attention: assessment.len() - ok - tampered,
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
        object.state.as_str(),
        object.title
    ));
    out.push_str(&format!("{} sections   {} ok", tally.total, tally.ok));
    // Lower case here even though the section label shouts: this line is a
    // tally, and a tally that shouts makes glancing at a healthy record feel
    // like an alarm.
    if tally.tampered > 0 {
        out.push_str(&format!("   {} tampered", tally.tampered));
    }
    if tally.attention > 0 {
        out.push_str(&format!("   {} stale", tally.attention));
    }
    out.push_str(&format!("   rev {}\n", object.rev));
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
        // The hash sits in the same file as the text it covers, so this catches
        // a careless edit, not a careful one. Hand over the git command either
        // way: committed history is the anchor, and the reader needs to be
        // pointed at it in the moment they learn something is wrong.
        if status.tampered {
            out.push_str(&format!(
                "    !!       content does not match the hash confirmed at {}\n",
                section.confirmed_at
            ));
            match git::last_commit_for(root, &store::object_path(root, &object.id)) {
                Some(commit) => out.push_str(&format!(
                    "    !!       git show {}:{}/objects/{}.json\n",
                    short(&commit),
                    store::DIR,
                    object.id
                )),
                None => out.push_str(
                    "    !!       this object was never committed, so there is nothing to compare against\n",
                ),
            }
        }
        // Every `!!` before every `advice`: corruption under a section outranks
        // the question of whether that section drifted, and a reader who stops
        // after the first line has to have stopped on the worse news.
        for drift in status.drifted.iter().filter(|drift| drift.target_tampered) {
            out.push_str(&format!(
                "    !!       {} §{} does not match its own hash; what this section stands on is not what was confirmed\n",
                abbrev(&drift.object, w),
                drift.section
            ));
            if let Some(lookback) = &drift.lookback {
                out.push_str(&format!("             {lookback}\n"));
            }
        }
        // Say what to do about it, rather than making the reader work it out.
        if let Some(distance) = &status.basis {
            out.push_str(&format!(
                "    advice   {} commits and {} files have changed since {}; check this still holds\n",
                distance.commits,
                distance.files.len(),
                short(section.based_on.as_deref().unwrap_or("")),
            ));
        }
        for drift in &status.drifted {
            if drift.target_tampered {
                continue;
            }
            match (&drift.current_sha256, &drift.lookback) {
                (Some(current), Some(lookback)) => {
                    out.push_str(&format!(
                        "    advice   {} §{} was {} when confirmed, now {}\n             {}\n",
                        abbrev(&drift.object, w),
                        drift.section,
                        short(&drift.confirmed_sha256),
                        short(current),
                        lookback
                    ));
                }
                _ => {
                    out.push_str(&format!(
                        "    advice   {} §{} no longer exists; what this section stood on is gone\n",
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
    tampered: usize,
    attention: usize,
}

#[derive(Serialize)]
struct JsonObject<'a> {
    id: &'a str,
    title: &'a str,
    state: &'static str,
    rev: u64,
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
        state: object.state.as_str(),
        rev: object.rev,
        summary: JsonSummary {
            sections: tally.total,
            ok: tally.ok,
            tampered: tally.tampered,
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
            _ if tally.tampered > 0 => format!("{} tampered", tally.tampered),
            _ if tally.attention > 0 => format!("{} stale", tally.attention),
            _ => "ok".to_owned(),
        };
        out.push_str(&format!(
            "{}  {:<6}  {:>2} sections  {:<12}  {}\n",
            abbrev(&object.id, w),
            object.state.as_str(),
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

/// Sections whose stored content no longer hashes to its recorded hash.
///
/// `render_ls_sections` keeps its columns byte for byte — scripts split on
/// them, and a marker column would shift every field along — so the caller
/// warns on stderr instead, which survives the pipe into `grep`.
pub fn tampered_count(objects: &[Object]) -> usize {
    objects
        .iter()
        .flat_map(|object| &object.sections)
        .filter(|section| tampered(section))
        .count()
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
                object.state.as_str(),
                section.text.replace('\n', " ").trim()
            ));
        }
    }
    out
}

/// Every Backlog surface opens with this line.
///
/// The record's whole claim is that a human read these words and agreed to
/// them. Nothing here was read by anyone, and the two are one `engr` command
/// apart — so the boundary is stated where the reader already is, rather than
/// left to be inferred from which subcommand they happened to type.
pub const STAGING_BANNER: &str = "UNCONFIRMED STAGING — nothing here was confirmed by a human\n";

pub fn backlog_width(root: &Path) -> usize {
    backlog::ids(root)
        .map(|ids| store::abbrev_len(&ids))
        .unwrap_or(8)
}

/// Whether a subject still resolves, so a signpost pointing nowhere says so.
/// A failure is never an error here: subjects are navigation, and staging that
/// refused to load because a file moved would be a referential-integrity
/// database, which is the thing Backlog is explicitly not.
fn subject_note(root: &Path, subject: &backlog::Subject) -> Option<&'static str> {
    match subject {
        backlog::Subject::Engr { reference } => {
            let parsed = crate::reference::EngrRef::parse_embedded(reference).ok()?;
            let id = crate::reference::decode_uuid(parsed.id()).ok()?.to_string();
            let found = match parsed.kind() {
                crate::reference::ResourceKind::Backlog => backlog::load(root, &id)
                    .map(|item| {
                        parsed
                            .section()
                            .map(|section| item.section(section).is_ok())
                            .unwrap_or(true)
                    })
                    .unwrap_or(false),
                _ => ops::effective(root, &id)
                    .map(|object| {
                        parsed
                            .section()
                            .map(|section| object.section(section).is_ok())
                            .unwrap_or(true)
                    })
                    .unwrap_or(false),
            };
            (!found).then_some("not found")
        }
        backlog::Subject::File { path, commit } | backlog::Subject::Symbol { path, commit, .. } => {
            (!git::path_at(root, commit, path)).then_some("snapshot unavailable")
        }
    }
}

pub fn render_backlog_ls(root: &Path, items: &[backlog::Item], keyword: Option<&str>) -> String {
    let w = backlog_width(root);
    let mut out = String::from(STAGING_BANNER);
    for item in items {
        if let Some(needle) = keyword {
            let needle = needle.to_lowercase();
            let hit = item.topic.to_lowercase().contains(&needle)
                || item
                    .sections
                    .iter()
                    .any(|section| section.text.to_lowercase().contains(&needle));
            if !hit {
                continue;
            }
        }
        let produced: usize = item
            .sections
            .iter()
            .map(|section| section.produced.len())
            .sum();
        let note = if produced > 0 {
            format!("{produced} produced")
        } else {
            "-".to_owned()
        };
        out.push_str(&format!(
            "{}  {:>2} unresolved  {:<12}  {}  {}\n",
            abbrev(&item.id, w),
            item.sections.len(),
            note,
            item.updated_at(),
            item.topic
        ));
    }
    out
}

/// One unresolved topic in full.
///
/// A resumed agent needs all four of text, subjects, produced outcomes and
/// activity to decide what is left — and needs none of it to read like
/// confirmed wording, which is what the banner and the section marker are for.
pub fn render_backlog_show(root: &Path, item: &backlog::Item) -> String {
    let w = backlog_width(root);
    let mut out = String::from(STAGING_BANNER);
    out.push_str(&format!("{}  {}\n", abbrev(&item.id, w), item.topic));
    out.push_str(&format!(
        "{} unresolved   updated {}\n",
        item.sections.len(),
        item.updated_at()
    ));
    for section in &item.sections {
        out.push_str(&format!("\n── §{} ── unresolved\n", section.id));
        out.push_str(section.text.trim_end());
        out.push('\n');
        out.push_str(&format!("    updated  {}\n", section.updated_at));
        for subject in &section.subjects {
            match subject_note(root, subject) {
                Some(note) => {
                    out.push_str(&format!("    concerns {}  ({note})\n", subject.render()))
                }
                None => out.push_str(&format!("    concerns {}\n", subject.render())),
            }
        }
        for produced in &section.produced {
            out.push_str(&format!(
                "    produced engr:{}\n",
                produced.target.reference
            ));
        }
        // Said once per Section that has outcomes, because this is the exact
        // place a resuming agent is most likely to conclude the opposite.
        if !section.produced.is_empty() {
            out.push_str("             (already confirmed; this point is still unresolved)\n");
        }
    }
    out
}

#[derive(Serialize)]
struct JsonBacklogItem<'a> {
    id: &'a str,
    topic: &'a str,
    authority: &'static str,
    next_section_id: u64,
    updated_at: &'a str,
    sections: &'a [backlog::Section],
}

pub fn render_backlog_json(item: &backlog::Item) -> Result<String> {
    serde_json::to_string_pretty(&JsonBacklogItem {
        id: &item.id,
        topic: &item.topic,
        // Structured output travels furthest from the banner, so the boundary
        // has to be a field rather than a line somebody printed once.
        authority: "unconfirmed_staging",
        next_section_id: item.next_section_id,
        updated_at: item.updated_at(),
        sections: &item.sections,
    })
    .map_err(|error| crate::Error::new(crate::EXIT_SCHEMA, format!("json: {error}")))
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
            let closed = object.state == Status::Closed;
            let marker = if closed { "⚠" } else { "·" };
            let tail = if closed {
                " — nobody is looking at this one"
            } else {
                ""
            };
            out.push_str(&format!(
                "{} {}  {:<6}  §{}  {}{}\n",
                marker,
                abbrev(&object.id, w),
                object.state.as_str(),
                id,
                status.label(),
                tail
            ));
        }
    }
    out
}
