//! Read surfaces: staleness assessment, `show`, and `ls`.
//!
//! Two rules shape everything here. The admitted wording and how much it can
//! be trusted appear on the *same* default surface — the previous design hid the
//! authoritative text behind a flag, and a reader given the default output drew the
//! opposite conclusion from the truth. And nothing is truncated: an agent asked
//! to reason from a record needs all of it.

use crate::backlog;
use crate::git;
use crate::model::{Object, Ref};
use crate::semantics::{Relation, Supplement};
use crate::{collection, ops, store, work, Error, Result};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct RefDrift {
    pub object: String,
    pub section: u64,
    pub confirmed_sha256: String,
    pub current_sha256: Option<String>,
    pub lookback: Option<String>,
    /// Current or historical target integrity failed before semantic drift
    /// could be trusted. `integrity_side` keeps the actionable distinction.
    pub target_tampered: bool,
    /// The target could not be read at all — malformed authority, a broken
    /// invariant, a file this build refuses. **Not** the same as the target
    /// being absent, and the difference is the point: absence is a fact a
    /// record can legitimately report, while unreadable authority is a failure
    /// and must never be downgraded into "moved", "gone", or a clean verify.
    pub target_unreadable: bool,
    /// The target is not there at all.
    ///
    /// A failure, by the ruling on review comment 5360513209, and not ordinary
    /// drift: a Section that explicitly stands on another Section which no
    /// longer exists is not merely out of date, it is standing on nothing. That
    /// `verify` already treated it as a failure while this surface called it
    /// drift was one workspace state with two verdicts.
    pub target_missing: bool,
    /// Selected semantic fields that changed for a selective Ref.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub moved_fields: Vec<String>,
    /// Which side of a selective Ref failed integrity. The machine state is
    /// intentionally coarse; this diagnostic keeps the actionable distinction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrity_side: Option<&'static str>,
}

#[derive(Debug, Clone, Default)]
pub struct SectionStatus {
    /// This section's stored content does not hash to its own recorded hash.
    /// Dominates every other signal: a forged section's drift assessment is an
    /// assessment of something no admission path accepted.
    pub tampered: bool,
    pub basis: Option<git::Distance>,
    pub drifted: Vec<RefDrift>,
    /// A `superseded_by` on this Section whose target cannot be established.
    ///
    /// Not drift, and deliberately not one of the `drifted` entries. Drift asks
    /// a person whether this wording still holds; this says the authoritative
    /// forward link out of this Object leads nowhere, so the chain a reader
    /// follows to find current knowledge is broken.
    pub replacement: Option<String>,
}

impl SectionStatus {
    pub fn is_ok(&self) -> bool {
        !self.tampered
            && self.basis.is_none()
            && self.drifted.is_empty()
            && self.replacement.is_none()
    }

    /// A Section standing on semantics that are not what was admitted.
    pub fn stands_on_tampered(&self) -> bool {
        self.drifted.iter().any(|drift| drift.target_tampered)
    }

    /// A section standing on nothing: the target is gone.
    pub fn stands_on_missing(&self) -> bool {
        self.drifted.iter().any(|drift| drift.target_missing)
    }

    /// A section standing on authority nothing could read. Reported apart from
    /// tampering because they are different facts — one says the words were
    /// changed behind the gate, the other says nobody can tell what the words
    /// are — and both are failures rather than drift.
    pub fn stands_on_unreadable(&self) -> bool {
        self.drifted.iter().any(|drift| drift.target_unreadable)
    }

    /// Whether the wording here can be trusted at all — either it was forged,
    /// or what it explicitly leans on was.
    pub fn forged(&self) -> bool {
        self.tampered
            || self.stands_on_tampered()
            || self.stands_on_unreadable()
            || self.stands_on_missing()
            || self.replacement.is_some()
    }

    pub fn label(&self) -> &'static str {
        if self.tampered {
            return "TAMPERED";
        }
        if self.stands_on_tampered() {
            return "REF TAMPERED";
        }
        if self.stands_on_unreadable() {
            return "REF UNREADABLE";
        }
        if self.stands_on_missing() {
            return "REF MISSING";
        }
        if self.replacement.is_some() {
            return "REPLACEMENT UNAVAILABLE";
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
        if self.stands_on_unreadable() {
            return "ref_unreadable";
        }
        if self.stands_on_missing() {
            return "ref_missing";
        }
        if self.replacement.is_some() {
            return "replacement_unavailable";
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
fn section_tampered(object: &Object, section: &crate::model::Section) -> bool {
    if object.sha256.is_some() {
        return crate::integrity::check_section_seal(section, &section.sha256).is_err();
    }
    section
        .recomputed_sha256()
        .map(|now| now != section.sha256)
        .unwrap_or(true)
}

fn object_tampered(object: &Object) -> bool {
    object
        .sha256
        .as_deref()
        .is_some_and(|seal| crate::integrity::check_object_integrity(object, seal).is_err())
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

/// The canonical reference for a resource, as every reference-taking flag
/// wants it written.
///
/// Falls back to the raw id if the compact encoding somehow fails, because a
/// read surface refusing to print because one field could not be derived is
/// worse than printing the rest — and the encoding cannot fail for a stored id.
fn canonical_reference(id: &str) -> String {
    crate::reference::encode_uuid_str(id)
        .map(|compact| format!("engr:obj:{compact}"))
        .unwrap_or_else(|_| id.to_owned())
}

/// The abbreviation width to use across one command's output.
pub fn width(root: &Path) -> usize {
    ops::object_ids(root)
        .map(|ids| store::abbrev_len(&ids))
        .unwrap_or(8)
}

fn drift_for(root: &Path, reference: &Ref) -> RefDrift {
    let (object, section) = match reference.target_identity() {
        Ok(identity) => identity,
        Err(_) => {
            return RefDrift {
                object: "invalid reference target".to_owned(),
                section: 0,
                confirmed_sha256: String::new(),
                current_sha256: None,
                lookback: None,
                target_tampered: false,
                target_unreadable: true,
                target_missing: false,
                moved_fields: Vec::new(),
                integrity_side: None,
            }
        }
    };
    let loaded = ops::effective(root, &object);
    // Absent and unreadable are different answers, and flattening them here is
    // what would let a corrupt dependency read as ordinary drift on the one
    // surface whose job is to say how far wording can be trusted.
    let mut target_unreadable = loaded
        .as_ref()
        .err()
        .is_some_and(|error| error.code != crate::EXIT_NOT_FOUND);
    let target_object = loaded.ok();
    let target = target_object
        .as_ref()
        .and_then(|target| target.section(section).ok().cloned());
    let target_missing = target.is_none() && !target_unreadable;

    if let Ref::Selective(selective) = reference {
        let current_integrity_failed = target_object.as_ref().is_some_and(|target| {
            target.sha256.as_deref().map_or(true, |seal| {
                crate::integrity::check_object_integrity(target, seal).is_err()
            })
        });
        let evaluated = target_object
            .as_ref()
            .and_then(|target| target.sha256.as_deref().map(|seal| (target, seal)))
            .map(|(target, seal)| crate::dependency::evaluate(root, target, seal, selective));
        if evaluated.as_ref().is_some_and(Result::is_err) {
            target_unreadable = true;
        }
        let state = evaluated.and_then(Result::ok);
        let moved_fields = match &state {
            Some(crate::dependency::Dependency::Drifted { fields }) => fields
                .iter()
                .map(|field| field.as_str().to_owned())
                .collect(),
            _ => Vec::new(),
        };
        let target_tampered = matches!(
            state,
            Some(crate::dependency::Dependency::TargetIntegrityFailure)
        );
        let target_missing =
            matches!(state, Some(crate::dependency::Dependency::TargetMissing)) || target_missing;
        let target_unreadable = target_unreadable
            || matches!(
                state,
                Some(
                    crate::dependency::Dependency::ProvenanceUnavailable
                        | crate::dependency::Dependency::SchemaMismatch
                        | crate::dependency::Dependency::DigestInvalid
                )
            );
        let lookback =
            (!matches!(state, Some(crate::dependency::Dependency::Unchanged))).then(|| {
                format!(
                    "git show {}:{}/objects/{}.json",
                    short(selective.commit()),
                    store::DIR,
                    object
                )
            });
        return RefDrift {
            object,
            section,
            confirmed_sha256: selective.digest().to_owned(),
            current_sha256: matches!(state, Some(crate::dependency::Dependency::Unchanged))
                .then(|| selective.digest().to_owned()),
            lookback,
            target_tampered,
            target_unreadable,
            target_missing,
            moved_fields,
            integrity_side: target_tampered.then_some(if current_integrity_failed {
                "current"
            } else {
                "historical"
            }),
        };
    }
    let legacy = reference
        .as_legacy()
        .expect("the selective case returned above");
    let target_tampered = target_object
        .as_ref()
        .zip(target.as_ref())
        .is_some_and(|(object, section)| section_tampered(object, section));
    // Recomputed from the target's actual content, not read off its stored
    // seal. The seal is a claim about what was admitted; it is not the
    // content, and a file rewritten outside admission keeps the old seal while
    // saying something else. Reporting the seal here made the two verdicts
    // agree by accident — "was X, now X" for a section whose wording had
    // changed — because the value being compared was the very value that had
    // not moved. Content identity decides drift; the seal decides tampering.
    let current = target.and_then(|section| section.recomputed_sha256().ok());
    let moved = current.as_deref() != Some(legacy.sha256.as_str());
    // Worth offering whenever the target is not what was pinned, whether it was
    // revised through admission or rewritten behind it.
    let lookback = (current.is_some() && (moved || target_tampered)).then(|| {
        format!(
            "git show {}:{}/objects/{}.json",
            short(&legacy.commit),
            store::DIR,
            legacy.object
        )
    });
    RefDrift {
        object: legacy.object.clone(),
        section: legacy.section,
        confirmed_sha256: legacy.sha256.clone(),
        current_sha256: current,
        lookback,
        target_tampered,
        target_unreadable,
        target_missing,
        moved_fields: Vec::new(),
        integrity_side: None,
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
                    tampered: section_tampered(object, section),
                    basis,
                    drifted,
                    replacement: crate::ops::broken_replacements_in(root, section)
                        .into_iter()
                        .next()
                        .map(|broken| format!("{}: {}", broken.target, broken.reason)),
                },
            )
        })
        .collect()
}

pub struct Counts {
    pub total: usize,
    pub ok: usize,
    /// Sections whose persisted state, or whose stated basis, is not what was
    /// admitted. Counted apart from `attention`: drift is a question for a
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

/// `type` and `state` in one column, because the state vocabulary only means
/// anything against the type it belongs to: `accepted` on a risk and `accepted`
/// on a decision are different facts, and a column that showed only the state
/// would invite reading them as the same one.
pub fn classification(object: &Object) -> String {
    match object.object_type {
        Some(object_type) => format!("{}/{}", object_type.as_str(), object.state.as_str()),
        None => object.state.as_str().to_owned(),
    }
}

/// The supplementary entries, verbatim.
///
/// Never truncated and never re-indented: the body is literal content somebody
/// admitted, and an agent reading it back has to get the bytes that were
/// hashed, not a prettier arrangement of them.
fn render_content(out: &mut String, content: &[Supplement]) {
    for (index, entry) in content.iter().enumerate() {
        out.push_str(&format!("    content  [{index}] {}\n", entry.content_type));
        for line in entry.body.split('\n') {
            out.push_str(&format!("             {line}\n"));
        }
    }
}

fn render_relations(out: &mut String, relations: &[Relation]) {
    for relation in relations {
        out.push_str(&format!(
            "    relation {}\n",
            relation.render(|commit| short(commit).to_owned())
        ));
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
        classification(object),
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
    if object_tampered(object) {
        out.push_str("!!         Object integrity failed; current authority changed outside a supported transition\n");
        match git::last_commit_for(root, &store::object_path(root, &object.id)) {
            Some(commit) => out.push_str(&format!(
                "!!         git show {}:{}/objects/{}.json\n",
                short(&commit),
                store::DIR,
                object.id
            )),
            None => out.push_str(
                "!!         this Object was never committed, so there is nothing to compare against\n",
            ),
        }
    }
    // The canonical reference, on the screen you land on when you want to name
    // this object to something else. Every reference-taking flag wants this
    // exact string, and until it was printed the only way to produce one was to
    // implement Crockford Base32 outside engr.
    out.push_str(&format!("{}\n", canonical_reference(&object.id)));
    for section in &object.sections {
        let status = assessment
            .iter()
            .find(|(id, _)| *id == section.id)
            .map(|(_, status)| status.clone())
            .unwrap_or_default();
        match section.role {
            Some(role) => out.push_str(&format!(
                "\n── §{} [{}] ── {}\n",
                section.id,
                role.as_str(),
                status.label()
            )),
            None => out.push_str(&format!("\n── §{} ── {}\n", section.id, status.label())),
        }
        out.push_str(section.text.trim_end());
        out.push('\n');
        render_content(&mut out, &section.content);
        render_relations(&mut out, &section.relations);
        if let Some(commit) = &section.based_on {
            out.push_str(&format!(
                "    based_on {}   admitted {}\n",
                short(commit),
                section.admitted_at
            ));
        } else {
            out.push_str(&format!("    admitted {}\n", section.admitted_at));
        }
        for reference in &section.refs {
            let (target, target_section) = reference
                .target_identity()
                .unwrap_or_else(|_| ("invalid".to_owned(), 0));
            out.push_str(&format!(
                "    refs     {} §{}\n",
                abbrev(&target, w),
                target_section
            ));
        }
        // The hash sits in the same file as the text it covers, so this catches
        // a careless edit, not a careful one. Hand over the git command either
        // way: committed history is the anchor, and the reader needs to be
        // pointed at it in the moment they learn something is wrong.
        if status.tampered {
            out.push_str(&format!(
                "    !!       persisted Section does not match the seal admitted at {}\n",
                section.admitted_at
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
                "    !!       {} §{} {} integrity failed; what this section stands on is not what was admitted\n",
                abbrev(&drift.object, w),
                drift.section,
                drift.integrity_side.unwrap_or("current")
            ));
            if let Some(lookback) = &drift.lookback {
                out.push_str(&format!("             {lookback}\n"));
            }
        }
        // Before the advice lines, with the other `!!`: this is not a question
        // for the reader to weigh, it is a forward link that leads nowhere.
        if let Some(detail) = &status.replacement {
            out.push_str(&format!("    !!       superseded by {detail}\n"));
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
                (_, Some(lookback)) if !drift.moved_fields.is_empty() => {
                    out.push_str(&format!(
                        "    advice   {} §{} changed selected fields: {}\n             {}\n",
                        abbrev(&drift.object, w),
                        drift.section,
                        drift.moved_fields.join(", "),
                        lookback
                    ));
                }
                (Some(current), Some(lookback)) => {
                    out.push_str(&format!(
                        "    advice   {} §{} was {} when admitted, now {}\n             {}\n",
                        abbrev(&drift.object, w),
                        drift.section,
                        short(&drift.confirmed_sha256),
                        short(current),
                        lookback
                    ));
                }
                // Absence and unreadable authority get different advice,
                // because they need different work. The header already
                // separates them; this line is what a reader acts on, and
                // "gone" would send someone to recreate a file that is right
                // there. The protocol names those words as the ones malformed
                // authority must never be reported in.
                _ if drift.target_missing => {
                    out.push_str(&format!(
                        "    advice   {} §{} no longer exists; what this section stood on is gone
",
                        abbrev(&drift.object, w),
                        drift.section
                    ));
                }
                _ if drift.target_unreadable => {
                    out.push_str(&format!(
                        "    advice   {} §{} is there and will not load; what this section stands on cannot be read\n",
                        abbrev(&drift.object, w),
                        drift.section
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
    /// The section's own canonical reference, for `--ref` and `--subject`.
    reference: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<&'static str>,
    text: &'a str,
    #[serde(skip_serializing_if = "<[Supplement]>::is_empty")]
    content: &'a [Supplement],
    status: &'static str,
    based_on: Option<&'a str>,
    refs: &'a [Ref],
    #[serde(skip_serializing_if = "<[Relation]>::is_empty")]
    relations: &'a [Relation],
    sha256: &'a str,
    admitted_at: &'a str,
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
    /// The canonical form every reference-taking flag wants — `--subject`,
    /// `--ref`, `work depend --on`, `collection add --target`. Without it an
    /// agent holding this document cannot name this object to engr without
    /// implementing Crockford Base32 itself, which is a workflow that leaves
    /// the tool for no reason.
    reference: String,
    title: &'a str,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    object_type: Option<&'static str>,
    state: &'static str,
    /// Derived here and nowhere on disk. It is in the read surface because a
    /// planning agent needs the answer without reimplementing the table, and it
    /// is absent from storage because a stored copy would be a second truth.
    attention: bool,
    rev: u64,
    integrity: &'static str,
    sha256: Option<&'a str>,
    summary: JsonSummary,
    sections: Vec<JsonSection<'a>>,
}

pub fn render_show_json(root: &Path, object: &Object) -> Result<String> {
    let compact = canonical_reference(&object.id);
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
                reference: format!("{compact}:{}", section.id),
                role: section.role.map(|role| role.as_str()),
                text: &section.text,
                content: &section.content,
                status: status.key(),
                based_on: section.based_on.as_deref(),
                refs: &section.refs,
                relations: &section.relations,
                sha256: &section.sha256,
                admitted_at: &section.admitted_at,
                basis_commits_behind: status.basis.as_ref().map(|item| item.commits),
                basis_files_changed: status.basis.as_ref().map(|item| item.files.len()),
                stale: status.drifted.clone(),
            }
        })
        .collect();
    let value = JsonObject {
        id: &object.id,
        reference: compact.clone(),
        title: &object.title,
        object_type: object.object_type.map(|value| value.as_str()),
        state: object.state.as_str(),
        attention: object.needs_attention(),
        rev: object.rev,
        integrity: if object_tampered(object) {
            "tampered"
        } else {
            "ok"
        },
        sha256: object.sha256.as_deref(),
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
            _ if object_tampered(object) => "object tampered".to_owned(),
            _ if tally.tampered > 0 => format!("{} tampered", tally.tampered),
            _ if tally.attention > 0 => format!("{} stale", tally.attention),
            _ => "ok".to_owned(),
        };
        out.push_str(&format!(
            "{}  {:<20}  {:>2} sections  {:<12}  {}\n",
            abbrev(&object.id, w),
            classification(object),
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
        .map(|object| {
            usize::from(object_tampered(object))
                + object
                    .sections
                    .iter()
                    .filter(|section| section_tampered(object, section))
                    .count()
        })
        .sum()
}

/// One line per section, so grep can reach the text.
///
/// Every row carries its own trust status, immediately before the wording.
/// This is the surface the Skill recommends for searching before a decision, so
/// a hit here is something an agent is about to act on — and wording that was
/// forged, or that stands on authority which was, must not arrive looking
/// exactly like wording a human confirmed. A count on stderr did not do that
/// job: it says how many, never which, and it is the first thing a pipe drops.
///
/// The status sits before the text rather than after it because the text is the
/// only field that can contain spaces; anything following it is not a column.
pub fn render_ls_sections(root: &Path, objects: &[Object]) -> String {
    let w = width(root);
    let mut out = String::new();
    for object in objects {
        let assessment: Vec<(u64, SectionStatus)> = assess(root, object);
        let aggregate_tampered = object_tampered(object);
        for section in &object.sections {
            let status = if aggregate_tampered {
                "object_tampered"
            } else {
                assessment
                    .iter()
                    .find(|(id, _)| *id == section.id)
                    .map(|(_, status)| status.key())
                    .unwrap_or("ok")
            };
            out.push_str(&format!(
                "{} §{:<3} {:<20}  {:<14}  {}\n",
                abbrev(&object.id, w),
                section.id,
                classification(object),
                status,
                section.text.replace('\n', " ").trim()
            ));
        }
    }
    out
}

/// Which sections cannot be trusted, named rather than counted.
///
/// "3 sections do not match their hashes" tells a reader that something is
/// wrong and nothing about where, which turns a warning into a chore. The
/// listing above already marks each row; this is for the caller that wants to
/// say so once, loudly, on the stream a pipe does not swallow.
pub fn untrusted_sections(root: &Path, objects: &[Object]) -> Vec<String> {
    let w = width(root);
    let mut found = Vec::new();
    for object in objects {
        for (id, status) in assess(root, object) {
            if status.forged() {
                found.push(format!(
                    "{} §{id} {}",
                    abbrev(&object.id, w),
                    status.label()
                ));
            }
        }
    }
    found
}

/// Every Backlog surface opens with this line.
///
/// The record's whole claim is that a human read these words and agreed to
/// them. Nothing here was read by anyone, and the two are one `engr` command
/// apart — so the boundary is stated where the reader already is, rather than
/// left to be inferred from which subcommand they happened to type.
pub const STAGING_BANNER: &str = "UNCONFIRMED STAGING — nothing here is admitted to the record\n";

/// Activity to the second, in UTC.
///
/// The stored value keeps whatever precision the clock gave it, but nine
/// fractional digits in a listing column push the topic — the thing being
/// scanned for — off to the right for no gain: triage asks which points were
/// touched recently, not which nanosecond.
///
/// Shortened by parsing the instant and formatting it again, never by cutting
/// the string. RFC3339 allows an offset, so trimming at the `.` and appending
/// `Z` turns `2026-08-17T10:00:00.123+08:00` into a time eight hours away from
/// the one recorded. Normalizing to UTC leaves the instant untouched and makes
/// the column comparable down the page, which is the only thing it is for.
fn to_the_second(timestamp: &str) -> String {
    const SECONDS: &[time::format_description::FormatItem<'static>] =
        time::macros::format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]Z");
    backlog::instant(timestamp)
        .map(|instant| instant.to_offset(time::UtcOffset::UTC))
        .and_then(|instant| instant.format(SECONDS).ok())
        // Loading validates the format, so this is unreachable for a stored
        // section — and showing the raw value beats inventing one.
        .unwrap_or_else(|| timestamp.to_owned())
}

/// The canonical reference for one unresolved point.
fn backlog_reference(id: &str) -> String {
    crate::reference::encode_uuid_str(id)
        .map(|compact| format!("engr:backlog:{compact}"))
        .unwrap_or_else(|_| id.to_owned())
}

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
            // Absent and unreadable are different answers even here, where
            // neither is authority: "not found" invites removing the signpost,
            // "unreadable" invites looking at why the file will not load.
            let outcome = match parsed.kind() {
                crate::reference::ResourceKind::Backlog => backlog::load(root, &id).map(|item| {
                    parsed
                        .section()
                        .map(|section| item.section(section).is_ok())
                        .unwrap_or(true)
                }),
                _ => ops::effective(root, &id).map(|object| {
                    parsed
                        .section()
                        .map(|section| object.section(section).is_ok())
                        .unwrap_or(true)
                }),
            };
            match outcome {
                Ok(true) => None,
                Ok(false) => Some("not found"),
                Err(error) if error.code == crate::EXIT_NOT_FOUND => Some("not found"),
                Err(_) => Some("unreadable"),
            }
        }
        backlog::Subject::File { path, commit, .. }
        | backlog::Subject::Symbol { path, commit, .. } => {
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
            to_the_second(item.updated_at()),
            item.topic
        ));
    }
    out
}

/// One unresolved topic in full.
///
/// A resumed agent needs all four of text, subjects, produced outcomes and
/// activity to decide what is left — and needs none of it to read like
/// admitted wording, which is what the banner and the section marker are for.
pub fn render_backlog_show(root: &Path, item: &backlog::Item) -> String {
    let w = backlog_width(root);
    let mut out = String::from(STAGING_BANNER);
    out.push_str(&format!("{}  {}\n", abbrev(&item.id, w), item.topic));
    out.push_str(&format!(
        "{} unresolved   updated {}\n",
        item.sections.len(),
        to_the_second(item.updated_at())
    ));
    // The same line the record's `show` carries, for the same reason: this is
    // where you land when you want to name this item to another command.
    out.push_str(&format!("{}\n", backlog_reference(&item.id)));
    for section in &item.sections {
        out.push_str(&format!("\n── §{} ── unresolved\n", section.id));
        out.push_str(section.text.trim_end());
        out.push('\n');
        out.push_str(&format!(
            "    updated  {}\n",
            to_the_second(&section.updated_at)
        ));
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
            out.push_str("             (already admitted; this point is still unresolved)\n");
        }
    }
    out
}

#[derive(Serialize)]
struct JsonBacklogSection<'a> {
    /// The section's own canonical reference. An addressable entity exposes
    /// one on a machine-readable path; a `--subject` naming a backlog section
    /// is exactly what this is for.
    ///
    /// `id` is deliberately not repeated here. The flattened section already
    /// carries one, and emitting it twice makes a document that a strict
    /// parser rejects and a typed deserializer — `serde_json` included —
    /// refuses outright. A machine-readable contract that only permissive
    /// readers can read is not one.
    reference: String,
    /// What to hand back to `--expect` when changing or consuming this point.
    ///
    /// The predecessor an agent read, in a form a command line can carry. It
    /// covers the whole Section and the parent topic, so anything either of them
    /// does between reading and applying changes it.
    expect: String,
    #[serde(flatten)]
    section: &'a backlog::Section,
}

#[derive(Serialize)]
struct JsonBacklogItem<'a> {
    id: &'a str,
    /// The canonical form `--subject`, `work depend --on` and
    /// `collection add --target` all want, for the same reason the object
    /// surface carries one.
    reference: String,
    topic: &'a str,
    authority: &'static str,
    next_section_id: u64,
    updated_at: &'a str,
    /// The two item-level predecessors, for the two mutations that do not name
    /// an existing point: renaming the topic, and adding a point.
    ///
    /// Separate values because they bind different things. A rename rests on the
    /// complete item, so any point moving stales it; an add rests only on the
    /// topic and the id it will receive, so a sibling being reworded does not.
    expect: JsonBacklogExpect,
    sections: Vec<JsonBacklogSection<'a>>,
}

#[derive(Serialize)]
struct JsonBacklogExpect {
    rename: String,
    add: String,
}

pub fn render_backlog_json(item: &backlog::Item) -> Result<String> {
    serde_json::to_string_pretty(&JsonBacklogItem {
        id: &item.id,
        reference: backlog_reference(&item.id),
        topic: &item.topic,
        // Structured output travels furthest from the banner, so the boundary
        // has to be a field rather than a line somebody printed once.
        authority: "unconfirmed_staging",
        next_section_id: item.next_section_id,
        updated_at: item.updated_at(),
        expect: JsonBacklogExpect {
            rename: backlog::Precondition::of_item(item).token()?,
            add: backlog::Precondition::of_add(item).token()?,
        },
        sections: item
            .sections
            .iter()
            .map(|section| {
                Ok(JsonBacklogSection {
                    reference: format!("{}:{}", backlog_reference(&item.id), section.id),
                    expect: backlog::Precondition::of_section(item, section.id)?.token()?,
                    section,
                })
            })
            .collect::<Result<Vec<_>>>()?,
    })
    .map_err(|error| crate::Error::new(crate::EXIT_SCHEMA, format!("json: {error}")))
}

/// The one case worth interrupting for by default: an object nobody is looking
/// at, whose basis has since moved. Outside the attention set is exactly where
/// drift goes unnoticed — which is why this reads the derived class rather than
/// `open`/`closed`, and so covers an accepted design and a mitigated risk too.
pub fn render_stale(root: &Path, objects: &[Object]) -> String {
    let w = width(root);
    let mut out = String::new();
    for object in objects {
        for (id, status) in assess(root, object) {
            if status.is_ok() {
                continue;
            }
            let unwatched = !object.needs_attention();
            let marker = if unwatched { "⚠" } else { "·" };
            let tail = if unwatched {
                " — nobody is looking at this one"
            } else {
                ""
            };
            out.push_str(&format!(
                "{} {}  {:<20}  §{}  {}{}\n",
                marker,
                abbrev(&object.id, w),
                classification(object),
                id,
                status.label(),
                tail
            ));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Work
// ---------------------------------------------------------------------------

/// Louder than the Backlog banner, and for a different reason.
///
/// Backlog says "nobody confirmed this". Work says that *and* that finishing it
/// settles nothing — the failure mode worth preventing is an agent reading a
/// sidecar with every item done and concluding the Object is decided.
pub const WORK_BANNER: &str =
    "EXECUTION MEMORY — agent-managed, admitted by nobody, and not what the record says\n";

/// Whether a Work target still resolves.
///
/// Never an error, exactly like a Backlog subject: these are signposts for the
/// next agent, and a sidecar that refused to load because a Backlog item was
/// consumed would make operational memory more fragile than the record it is
/// not part of.
fn work_target_note(root: &Path, target: &crate::reference::EngrTarget) -> Option<&'static str> {
    let parsed = crate::reference::EngrRef::parse_embedded(&target.reference).ok()?;
    let id = crate::reference::decode_uuid(parsed.id()).ok()?.to_string();
    let outcome = match parsed.kind() {
        crate::reference::ResourceKind::Backlog => backlog::load(root, &id).map(|_| ()),
        _ => ops::effective(root, &id).map(|_| ()),
    };
    match outcome {
        Ok(()) => None,
        Err(error) if error.code == crate::EXIT_NOT_FOUND => Some("  (not found)"),
        Err(_) => Some("  (unreadable)"),
    }
}

fn work_target(root: &Path, target: &crate::reference::EngrTarget) -> String {
    format!(
        "engr:{}{}",
        target.reference,
        work_target_note(root, target).unwrap_or("")
    )
}

/// One line per owner that has execution memory.
///
/// The state column shows the derived standing rather than the stored field,
/// because `blocked` is the answer someone scanning this actually wants and
/// `active` on a sidecar with three blockers would be true and useless.
///
/// The owner kind gets a column of its own rather than being left to the id,
/// because the two namespaces look identical in an abbreviation and a reader
/// scanning this has to know which resource a row is about before the title
/// tells them anything.
pub fn render_work_ls(root: &Path, entries: &[(work::Owner, work::Work)]) -> String {
    // Over the ids actually listed, rather than either namespace's own width.
    // A listing that spans two namespaces cannot borrow one of them to shorten
    // the other: an Object-derived prefix says nothing about whether two Backlog
    // ids in this table still differ at that length.
    let listed: Vec<String> = entries
        .iter()
        .map(|(owner, _)| owner.id().to_owned())
        .collect();
    let w = store::abbrev_len(&listed);
    let mut out = String::from(WORK_BANNER);
    if entries.is_empty() {
        out.push_str("no execution memory\n");
        return out;
    }
    for (owner, item) in entries {
        let open = item
            .items
            .iter()
            .filter(|entry| entry.state != work::ItemState::Done)
            .count();
        out.push_str(&format!(
            "{}  {:<8}  {:<8}  {:>2} open  {}  {}\n",
            abbrev(owner.id(), w),
            owner.kind().token(),
            item.standing(),
            open,
            to_the_second(&item.updated_at),
            owner_title(root, owner)
        ));
    }
    out
}

/// What the owner is called, in whichever way its kind says who it is.
///
/// Loading a sidecar holds the owner invariant, so by the time a row is rendered
/// the owner exists. No "(not found)" fallback: an orphan is invalid Work and is
/// refused as such, not drawn as a row.
fn owner_title(root: &Path, owner: &work::Owner) -> String {
    match owner {
        work::Owner::Object(id) => ops::effective(root, id)
            .map(|object| object.title)
            .unwrap_or_default(),
        work::Owner::Backlog(id) => backlog::load(root, id)
            .map(|item| item.topic)
            .unwrap_or_default(),
    }
}

/// The whole sidecar, in the order a resuming agent reads it: where things
/// stand, what is stopping them, what is left, what was already done.
pub fn render_work_show(root: &Path, owner: &work::Owner, item: &work::Work) -> String {
    // Each namespace's own abbreviation, because a prefix means "unambiguous
    // among these ids" and the two sets are not one set.
    let w = match owner {
        work::Owner::Object(_) => width(root),
        work::Owner::Backlog(_) => backlog_width(root),
    };
    let mut out = String::from(WORK_BANNER);
    // The owner line names the kind, because "Object" over a Backlog item's
    // sidecar was the whole confusion this domain must not cause.
    out.push_str(&format!(
        "{:<10} {}\nState      {}\nUpdated    {}\n",
        match owner {
            work::Owner::Object(_) => "Object",
            work::Owner::Backlog(_) => "Backlog",
        },
        abbrev(owner.id(), w),
        item.standing(),
        to_the_second(&item.updated_at)
    ));
    // The same line the record's and Backlog's `show` carry, and it earns its
    // place here for one more reason: naming this owner to another command is
    // the one thing an abbreviation cannot do now that a bare id no longer says
    // which namespace it is in.
    out.push_str(&format!("{owner}\n"));
    if item.state == work::State::Paused {
        out.push_str("           a human stopped this; do not resume it on your own\n");
    }
    if let Some(summary) = &item.summary {
        out.push('\n');
        out.push_str(summary.trim_end());
        out.push('\n');
    }
    if !item.dependencies.is_empty() {
        out.push_str("\nDepends on\n");
        for dependency in &item.dependencies {
            out.push_str(&format!("  {}\n", work_target(root, &dependency.target)));
            if let Some(reason) = &dependency.reason {
                out.push_str(&format!("    {reason}\n"));
            }
        }
    }
    if !item.blockers.is_empty() {
        out.push_str("\nBlocked by\n");
        for (index, blocker) in item.blockers.iter().enumerate() {
            let head = match (&blocker.reason, &blocker.target) {
                (Some(reason), _) => reason.clone(),
                (None, Some(target)) => work_target(root, target),
                (None, None) => "(nothing stated)".to_owned(),
            };
            out.push_str(&format!("  [{index}] {head}\n"));
            if let (Some(_), Some(target)) = (&blocker.reason, &blocker.target) {
                out.push_str(&format!("      {}\n", work_target(root, target)));
            }
        }
    }
    if !item.items.is_empty() {
        out.push('\n');
        for entry in &item.items {
            out.push_str(&format!(
                "{:>3}. {:<7}  {}\n",
                entry.id,
                entry.state.as_str(),
                entry.text
            ));
            if let Some(result) = &entry.result {
                out.push_str(&format!("     -> {result}\n"));
            }
            for commit in &entry.commits {
                out.push_str(&format!("     {}\n", &commit[..8.min(commit.len())]));
            }
        }
    }
    out
}

pub fn render_work_json(owner: &work::Owner, item: &work::Work) -> Result<String> {
    let value = serde_json::json!({
        // One `owner` object rather than the bare `object` id this used to
        // carry. A sidecar's owner is now a kind *and* an id, and a consumer
        // that read the id alone would have no way to tell an Object's
        // execution memory from a Backlog item's — the two namespaces mint the
        // same shape of identity. `reference` is the spelling that goes back
        // into any engr command that takes one.
        "owner": {
            "kind": owner.kind().token(),
            "id": owner.id(),
            "reference": owner.reference(),
        },
        // Structured output is the surface that travels furthest from any
        // banner — straight into another tool, with no screen in between — so
        // the boundary has to be a field rather than a line somebody printed.
        // `{"state": "active"}` on its own is indistinguishable from an owner's
        // own state, which is exactly the confusion this domain must not cause.
        "authority": "execution_memory",
        "state": item.state.as_str(),
        "standing": item.standing(),
        "blocked": item.is_blocked(),
        "summary": item.summary,
        "updated_at": item.updated_at,
        "next_item_id": item.next_item_id,
        "dependencies": item.dependencies,
        "blockers": item.blockers,
        "items": item.items,
    });
    serde_json::to_string_pretty(&value)
        .map_err(|error| crate::Error::new(crate::EXIT_SCHEMA, format!("json: {error}")))
}

// ---------------------------------------------------------------------------
// Collections
// ---------------------------------------------------------------------------

/// Planning, and never the thing being planned.
///
/// The confusion worth preventing here is different again from Backlog's and
/// Work's: nothing in a collection is even *about* what a record says, so the
/// banner has to stop a reader concluding anything at all about the members
/// from where they sit in a plan.
pub const PLANNING_BANNER: &str =
    "PLANNING — agent-managed, admitted by nobody, and says nothing about what its members mean\n";

pub fn collection_width(root: &Path) -> usize {
    collection::ids(root)
        .map(|ids| store::abbrev_len(&ids))
        .unwrap_or(4)
}

/// What a member currently is, from the domain that owns it.
///
/// Never an error, and never a state a collection stores. A consumed backlog
/// item is a member pointing at nothing, which is a fact worth showing and
/// never a reason to silently retarget it at whatever the work became.
fn member_note(root: &Path, target: &crate::reference::EngrTarget) -> String {
    let Ok(parsed) = crate::reference::EngrRef::parse_embedded(&target.reference) else {
        return "unreadable".to_owned();
    };
    let Ok(uuid) = crate::reference::decode_uuid(parsed.id()) else {
        return "unreadable".to_owned();
    };
    let id = uuid.to_string();
    match parsed.kind() {
        crate::reference::ResourceKind::Backlog => match backlog::load(root, &id) {
            Ok(item) => format!("unresolved  {}", item.topic),
            Err(error) if error.code == crate::EXIT_NOT_FOUND => {
                "gone (consumed or removed)".to_owned()
            }
            Err(_) => "unreadable (the file is there and will not load)".to_owned(),
        },
        _ => match ops::effective(root, &id) {
            // Derived attention, not `open`: a typed Object has no such state,
            // and a plan that spoke in those terms would be describing a
            // vocabulary half its members do not have.
            Ok(object) => format!(
                "{:<10}  {}",
                if object.needs_attention() {
                    "attention"
                } else {
                    "settled"
                },
                object.title
            ),
            Err(error) if error.code == crate::EXIT_NOT_FOUND => "gone".to_owned(),
            Err(_) => "unreadable (the file is there and will not load)".to_owned(),
        },
    }
}

pub fn render_collection_ls(root: &Path, collections: &[collection::Collection]) -> String {
    let w = collection_width(root);
    let mut out = String::from(PLANNING_BANNER);
    if collections.is_empty() {
        out.push_str("no collections\n");
        return out;
    }
    for item in collections {
        let attention = item
            .members
            .iter()
            .filter(|member| needs_attention(root, &member.target))
            .count();
        out.push_str(&format!(
            "{}  {:<9}  {:>2} members  {:>2} need attention  {}\n",
            abbrev(&item.id, w),
            item.state.as_str(),
            item.members.len(),
            attention,
            item.name
        ));
    }
    out
}

/// Whether one member is something somebody still has to look at.
fn needs_attention(root: &Path, target: &crate::reference::EngrTarget) -> bool {
    let Ok(parsed) = crate::reference::EngrRef::parse_embedded(&target.reference) else {
        return false;
    };
    let Ok(uuid) = crate::reference::decode_uuid(parsed.id()) else {
        return false;
    };
    let id = uuid.to_string();
    match parsed.kind() {
        // An unresolved point is by definition unresolved, so a plan holding one
        // is holding something outstanding.
        crate::reference::ResourceKind::Backlog => backlog::load(root, &id).is_ok(),
        _ => ops::effective(root, &id)
            .map(|object| object.needs_attention())
            .unwrap_or(false),
    }
}

pub fn render_collection_show(root: &Path, item: &collection::Collection) -> String {
    let mut out = String::from(PLANNING_BANNER);
    out.push_str(&format!(
        "Collection {}\nReference  engr:collection:{}\nState      {}\nName       {}\n",
        item.id,
        item.id,
        item.state.as_str(),
        item.name
    ));
    if let Some(schedule) = &item.schedule {
        let mut parts = Vec::new();
        for (label, value) in [
            ("start", &schedule.start),
            ("end", &schedule.end),
            ("target", &schedule.target),
        ] {
            if let Some(value) = value {
                parts.push(format!("{label} {value}"));
            }
        }
        out.push_str(&format!("Schedule   {}\n", parts.join("  ")));
    }
    if let Some(description) = &item.description {
        out.push('\n');
        out.push_str(description.trim_end());
        out.push('\n');
    }
    if item.members.is_empty() {
        out.push_str("\nno members\n");
        return out;
    }
    out.push('\n');
    for member in item.planned() {
        let rank = match member.order {
            Some(order) => format!("{order:>4}"),
            None => "  --".to_owned(),
        };
        let priority = match &member.priority {
            Some(priority) => format!("[{}] ", priority.level.as_str()),
            None => String::new(),
        };
        out.push_str(&format!(
            "{rank}  engr:{}\n      {priority}{}\n",
            member.target.reference,
            member_note(root, &member.target)
        ));
        if let Some(reason) = member.priority.as_ref().and_then(|it| it.reason.as_deref()) {
            out.push_str(&format!("      {reason}\n"));
        }
    }
    out
}

pub fn render_collection_json(item: &collection::Collection) -> Result<String> {
    let value = serde_json::json!({
        "id": item.id,
        // Collection is addressable — `engr:collection:<id>` — so its
        // machine-readable read path exposes the reference like every other
        // addressable entity. Identity and addressing stay distinct fields.
        "reference": format!("engr:collection:{}", item.id),
        // The same field Backlog and Work carry, for the same reason: structured
        // output leaves the screen that would otherwise have said so.
        "authority": "planning",
        "name": item.name,
        "description": item.description,
        "state": item.state.as_str(),
        "schedule": item.schedule,
        "members": item.members,
    });
    serde_json::to_string_pretty(&value)
        .map_err(|error| crate::Error::new(crate::EXIT_SCHEMA, format!("json: {error}")))
}

/// One persisted difference between stored bytes and what history proves.
pub struct RepairDifference {
    /// Where it is, e.g. `title` or `§3.role`.
    pub at: String,
    /// What the integrity-invalid file holds.
    pub stored: String,
    /// What admitted history derives, and what repair will restore.
    pub restore: String,
}

/// Every persisted difference between an integrity-invalid Object and the
/// projection a repair would restore.
///
/// Derived from the canonical projection of both, deliberately, rather than
/// from a list of fields. The first version of this comparison enumerated
/// `title`, lifecycle `state` and Section `text`, and therefore showed *nothing
/// at all* for an out-of-band edit to a Section's `role`, `refs` or any other
/// sealed member — the case where a person most needs to see what they are
/// about to discard. Walking the representation instead means a member this
/// code has never heard of is still reported, and a member added later is
/// covered the day it is added.
///
/// Seals are excluded at both levels. They are not the difference; they are how
/// the difference was detected, and the repair's own seals are computed after it
/// is admitted. Listing them would put a guaranteed, meaningless line at the top
/// of every comparison, which is how a screen teaches people to skim.
pub fn repair_differences(stored: &Object, restore: &Object) -> Result<Vec<RepairDifference>> {
    let mut found = Vec::new();
    walk_repair_difference(
        "",
        &comparable_projection(stored)?,
        &comparable_projection(restore)?,
        &mut found,
    );
    Ok(found)
}

/// The projection to compare, with Sections keyed by id rather than by position.
///
/// Position would make a deleted Section read as a change to every Section after
/// it. Ids are what a person and every reference actually name.
fn comparable_projection(object: &Object) -> Result<serde_json::Value> {
    let mut value = serde_json::to_value(object)
        .map_err(|error| Error::new(crate::EXIT_SCHEMA, format!("comparing: {error}")))?;
    if let Some(map) = value.as_object_mut() {
        map.remove("sha256");
        if let Some(sections) = map.get_mut("sections").and_then(|s| s.as_array_mut()) {
            let mut keyed = serde_json::Map::new();
            for mut section in sections.drain(..) {
                let id = section.get("id").and_then(serde_json::Value::as_u64);
                if let Some(entry) = section.as_object_mut() {
                    entry.remove("sha256");
                    entry.remove("id");
                }
                let at = id
                    .map(|id| format!("§{id}"))
                    .unwrap_or_else(|| "§?".to_owned());
                keyed.insert(at, section);
            }
            map.insert("sections".to_owned(), serde_json::Value::Object(keyed));
        }
    }
    Ok(value)
}

fn walk_repair_difference(
    at: &str,
    stored: &serde_json::Value,
    restore: &serde_json::Value,
    found: &mut Vec<RepairDifference>,
) {
    if stored == restore {
        return;
    }
    if let (serde_json::Value::Object(left), serde_json::Value::Object(right)) = (stored, restore) {
        let mut members: Vec<&str> = left
            .keys()
            .chain(right.keys())
            .map(String::as_str)
            .collect();
        members.sort_unstable();
        members.dedup();
        for member in members {
            let nested = if at.is_empty() {
                member.to_owned()
            } else {
                format!("{at}.{member}")
            };
            walk_repair_difference(
                &nested,
                left.get(member).unwrap_or(&serde_json::Value::Null),
                right.get(member).unwrap_or(&serde_json::Value::Null),
                found,
            );
        }
        return;
    }
    found.push(RepairDifference {
        at: at.trim_start_matches("sections.").to_owned(),
        stored: render_repair_value(stored),
        restore: render_repair_value(restore),
    });
}

fn render_repair_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "(absent)".to_owned(),
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}
