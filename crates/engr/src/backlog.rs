//! Backlog — where unresolved engineering work waits.
//!
//! Nothing here is authority. A Backlog Section is a point somebody has not
//! settled yet: an agent may write it, reword it, and delete it without asking
//! anyone, and git is its entire history. That is the opposite of the record,
//! and the two must never be read as the same kind of statement.
//!
//! The lifecycle is one sentence, and it is deliberately the whole model:
//!
//! ```text
//! a Section exists    = still unresolved
//! a Section is gone   = somebody judged it resolved
//! ```
//!
//! No `status`, no `resolved`, no `promoted`. A field that could disagree with
//! that sentence is a field that lets settled work keep looking pending.

use crate::model::{is_canonical_git_oid, new_id};
use crate::reference::{CanonicalEngrRef, EngrRef, ResourceKind};
use crate::{
    ensure, git, store, tool_error, Error, Result, EXIT_INVARIANT, EXIT_NOT_FOUND, EXIT_SCHEMA,
    EXIT_USAGE,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const DIR: &str = "backlog";

/// A topic is the line `engr backlog ls` prints, so the limit exists for the
/// same reason the Object title's does: a body pasted here degrades the listing
/// for every other item as well as its own.
const TOPIC_MAX: usize = 120;

pub fn dir(root: &Path) -> PathBuf {
    store::engr_dir(root).join(DIR)
}

pub fn item_path(root: &Path, id: &str) -> PathBuf {
    dir(root).join(format!("{id}.json"))
}

/// The structural discriminator from the shared embedded reference form. `kind`
/// is structural; `type` stays reserved for semantic classification.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum EngrKind {
    #[serde(rename = "engr")]
    Engr,
}

/// `{ "kind": "engr", "ref": "obj:<id>" }` — the shared embedded engr target.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct EngrTarget {
    pub kind: EngrKind,
    #[serde(rename = "ref")]
    pub reference: String,
}

impl EngrTarget {
    pub fn new(reference: impl Into<String>) -> Self {
        Self {
            kind: EngrKind::Engr,
            reference: reference.into(),
        }
    }
}

/// What an unresolved point concerns.
///
/// Weaker than the record's `refs[]` on purpose. It carries no dependency, no
/// authority, no ordering, and no claim that the target must change — so a
/// subject that stops resolving is a stale signpost, not a broken record.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Subject {
    Engr {
        #[serde(rename = "ref")]
        reference: String,
    },
    File {
        path: String,
        commit: String,
    },
    Symbol {
        path: String,
        symbol: String,
        commit: String,
    },
}

impl Subject {
    pub fn engr(reference: impl Into<String>) -> Self {
        Self::Engr {
            reference: reference.into(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Subject::Engr { reference } => {
                canonical_embedded(
                    reference,
                    &[ResourceKind::Object, ResourceKind::Backlog],
                    "a subject",
                )?;
            }
            Subject::File { path, commit } => {
                validate_repo_path(path)?;
                validate_pinned_commit(commit)?;
            }
            Subject::Symbol {
                path,
                symbol,
                commit,
            } => {
                validate_repo_path(path)?;
                validate_pinned_commit(commit)?;
                ensure!(
                    !symbol.trim().is_empty() && !symbol.contains('\n'),
                    EXIT_SCHEMA,
                    "a symbol subject needs a single-line symbol name"
                );
            }
        }
        Ok(())
    }

    /// How the subject reads on a screen. Not persisted state.
    pub fn render(&self) -> String {
        match self {
            Subject::Engr { reference } => format!("engr:{reference}"),
            Subject::File { path, commit } => format!("file   {path} @{}", short(commit)),
            Subject::Symbol {
                path,
                symbol,
                commit,
            } => format!("symbol {path} :: {symbol} @{}", short(commit)),
        }
    }
}

/// Authoritative knowledge already created or materially changed while working
/// on an unresolved point.
///
/// It is not a resolution signal. One unresolved point may produce several
/// confirmed outcomes across several sessions and still have work left in it;
/// that is why consumption, and only consumption, means resolved.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Produced {
    pub target: EngrTarget,
}

impl Produced {
    pub fn object(reference: impl Into<String>) -> Self {
        Self {
            target: EngrTarget::new(reference),
        }
    }

    /// Phase 2 outcomes are authoritative Objects and Object Sections only.
    /// Backlog, Collection, file and symbol targets are refused: `produced[]`
    /// answers "what did the record gain", and nothing outside the record can.
    pub fn validate(&self) -> Result<()> {
        self.target().map(|_| ())
    }

    /// The authoritative Object, and the Section where one is named.
    ///
    /// Whether that Object still exists is a separate question, and one this
    /// deliberately does not ask. An outcome recorded when it was real must not
    /// stop the staging around it loading years later because the Object was
    /// since deleted — `produced[]` is a record of what happened, not a
    /// referential-integrity constraint. Admission checks existence; loading
    /// checks shape.
    pub fn target(&self) -> Result<(String, Option<u64>)> {
        let canonical = canonical_embedded(
            &self.target.reference,
            &[ResourceKind::Object],
            "a produced outcome",
        )?;
        Ok((
            crate::reference::decode_uuid(canonical.id())?.to_string(),
            canonical.section(),
        ))
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Section {
    pub id: u64,
    pub text: String,
    /// When the unresolved statement itself last changed. Operational triage
    /// metadata: it is not in the resolution basis, and appending a produced
    /// outcome does not refresh it, because an outcome deliberately does not
    /// change what remains unresolved.
    pub updated_at: String,
    #[serde(default)]
    pub subjects: Vec<Subject>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub produced: Vec<Produced>,
}

impl Section {
    /// The transient compare-and-consume token: `canonical(text, subjects[])`.
    ///
    /// Excludes `produced[]`, `updated_at`, the Section id and the parent topic.
    /// A candidate pins this at prepare so confirming a proposal written against
    /// v1 of an unresolved point cannot quietly consume v2 — but accumulating an
    /// outcome, or renaming the topic, must not invalidate a candidate either.
    pub fn resolution_basis(&self) -> Result<String> {
        resolution_basis(&self.text, &self.subjects)
    }

    fn validate(&self) -> Result<()> {
        ensure!(self.id > 0, EXIT_SCHEMA, "section ids start at 1");
        // What the write path refuses, a stored file may not contain. Two
        // validations that disagree mean the stricter one is decorative: the
        // shape only has to survive one hand-edit to stop being true.
        ensure!(
            !self.text.trim().is_empty(),
            EXIT_SCHEMA,
            "§{}: a backlog section needs text",
            self.id
        );
        ensure!(
            time::OffsetDateTime::parse(
                &self.updated_at,
                &time::format_description::well_known::Rfc3339
            )
            .is_ok(),
            EXIT_SCHEMA,
            "§{}: updated_at {:?} is not an RFC3339 timestamp",
            self.id,
            self.updated_at
        );
        let mut seen = BTreeSet::new();
        for subject in &self.subjects {
            subject.validate()?;
            // subjects[] is a set: two identical entries carry no more meaning
            // than one, and permitting them would make "equivalent subjects"
            // ambiguous for the resolution basis.
            ensure!(
                seen.insert(canonical_json(subject)?),
                EXIT_SCHEMA,
                "§{} lists the same subject twice",
                self.id
            );
        }
        let mut outcomes = BTreeSet::new();
        for produced in &self.produced {
            produced.validate()?;
            ensure!(
                outcomes.insert(canonical_json(produced)?),
                EXIT_SCHEMA,
                "§{} lists the same produced outcome twice",
                self.id
            );
        }
        Ok(())
    }
}

/// One unresolved topic. Sections are the unresolved work units inside it,
/// because a topic commonly holds several independent concerns and forcing them
/// to move together makes partial resolution impossible.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Item {
    pub id: String,
    pub topic: String,
    /// Monotonic and never reset, for the same reason Object sections are:
    /// `max(existing) + 1` would hand out the id of a consumed Section, and
    /// every subject pointing at it would silently mean something else.
    pub next_section_id: u64,
    pub sections: Vec<Section>,
}

impl Item {
    pub fn section(&self, id: u64) -> Result<&Section> {
        self.sections
            .iter()
            .find(|section| section.id == id)
            .ok_or_else(|| {
                Error::new(
                    EXIT_NOT_FOUND,
                    format!("backlog section §{id} does not exist"),
                )
            })
    }

    /// The newest Section activity. Derived rather than stored: an item-level
    /// timestamp that can disagree with its own Sections is a field that will.
    ///
    /// Compared as instants, not as text. RFC3339 permits an offset, and
    /// `2026-08-17T01:00:00+08:00` sorts after `2026-08-16T20:00:00Z` as a
    /// string while being an hour earlier in fact. The value returned is still
    /// the one stored, so nothing here reinterprets what a Section recorded.
    pub fn updated_at(&self) -> &str {
        self.sections
            .iter()
            .max_by_key(|section| instant(&section.updated_at))
            .map(|section| section.updated_at.as_str())
            .unwrap_or("")
    }

    pub fn validate(&self) -> Result<()> {
        let id = crate::model::canonical_object_id(&self.id).map_err(|_| {
            Error::new(
                EXIT_SCHEMA,
                format!("backlog id {:?} must be a canonical UUIDv7", self.id),
            )
        })?;
        ensure!(
            id == self.id,
            EXIT_SCHEMA,
            "backlog id {:?} must be a canonical UUIDv7",
            self.id
        );
        ensure!(
            !self.topic.trim().is_empty(),
            EXIT_SCHEMA,
            "{}: a backlog item needs a topic",
            self.id
        );
        ensure!(
            !self.topic.contains('\n'),
            EXIT_SCHEMA,
            "{}: a stored topic cannot span lines",
            self.id
        );
        ensure!(
            self.topic.chars().count() <= TOPIC_MAX,
            EXIT_SCHEMA,
            "{}: a stored topic cannot exceed {TOPIC_MAX} characters",
            self.id
        );
        ensure!(
            self.next_section_id > 0,
            EXIT_SCHEMA,
            "{}: next_section_id must start at 1",
            self.id
        );
        // An item with no unresolved Sections has nothing unresolved in it, and
        // the lifecycle says that is not a state — it is a removal.
        ensure!(
            !self.sections.is_empty(),
            EXIT_SCHEMA,
            "{}: a backlog item with no sections must be removed, not stored",
            self.id
        );
        let mut ids = BTreeSet::new();
        for section in &self.sections {
            section.validate()?;
            ensure!(
                ids.insert(section.id),
                EXIT_SCHEMA,
                "{}: section §{} appears more than once",
                self.id,
                section.id
            );
            ensure!(
                section.id < self.next_section_id,
                EXIT_SCHEMA,
                "{}: next_section_id {} would reuse live section §{}",
                self.id,
                self.next_section_id,
                section.id
            );
        }
        Ok(())
    }
}

/// `canonical(text, subjects[])`, with subjects compared as an unordered set.
///
/// Two Sections whose subjects differ only in array order state the same
/// unresolved thing, so they must fingerprint the same — otherwise reordering a
/// list nobody reads as ordered would silently kill a prepared candidate.
pub fn resolution_basis(text: &str, subjects: &[Subject]) -> Result<String> {
    let mut canonical: Vec<String> = subjects.iter().map(canonical_json).collect::<Result<_>>()?;
    canonical.sort();
    let mut basis = serde_json::Map::new();
    basis.insert(
        "text".to_owned(),
        serde_json::Value::String(text.to_owned()),
    );
    basis.insert(
        "subjects".to_owned(),
        serde_json::Value::Array(
            canonical
                .into_iter()
                .map(serde_json::Value::String)
                .collect(),
        ),
    );
    crate::confirmation::fingerprint(&serde_json::Value::Object(basis))
}

/// `serde_json::Map` is a `BTreeMap`, so this is key-sorted and stable.
fn canonical_json<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("canonical form: {error}")))
}

/// The moment a stored timestamp names, or `None` if it is not one.
///
/// Loading validates this, so `None` should be unreachable for a Section that
/// came off disk — but every comparison here goes through it rather than
/// through the text, because the text of an RFC3339 value does not order.
pub fn instant(timestamp: &str) -> Option<time::OffsetDateTime> {
    time::OffsetDateTime::parse(timestamp, &time::format_description::well_known::Rfc3339).ok()
}

/// Whether two subject lists say the same thing.
///
/// `subjects[]` is a set, so order is not content. Reordering one leaves the
/// resolution basis identical by design; activity has to agree, or triage
/// reports work on a point nobody touched.
fn same_subjects(left: &[Subject], right: &[Subject]) -> Result<bool> {
    let mut left: Vec<String> = left.iter().map(canonical_json).collect::<Result<_>>()?;
    let mut right: Vec<String> = right.iter().map(canonical_json).collect::<Result<_>>()?;
    left.sort();
    right.sort();
    Ok(left == right)
}

fn short(value: &str) -> &str {
    &value[..8.min(value.len())]
}

/// Parse an embedded engr reference, restrict it to the kinds this field may
/// name, and require the stored spelling to already be canonical.
fn canonical_embedded(
    reference: &str,
    allowed: &[ResourceKind],
    what: &str,
) -> Result<CanonicalEngrRef> {
    let parsed = EngrRef::parse_embedded(reference)?;
    ensure!(
        parsed.snapshot_selector().is_none(),
        EXIT_SCHEMA,
        "{what} names a current resource, so it cannot carry a Git snapshot selector"
    );
    ensure!(
        allowed.contains(&parsed.kind()),
        EXIT_SCHEMA,
        "{what} cannot target {reference:?}"
    );
    let canonical = parsed.canonicalize(|_| None)?;
    ensure!(
        canonical.embedded() == reference,
        EXIT_SCHEMA,
        "{what} {reference:?} is not canonical; write it as {:?}",
        canonical.embedded()
    );
    Ok(canonical)
}

fn validate_repo_path(path: &str) -> Result<()> {
    ensure!(
        !path.trim().is_empty(),
        EXIT_SCHEMA,
        "a file or symbol subject needs a path"
    );
    ensure!(
        !path.contains('\\') && !path.starts_with('/') && !path.contains(':'),
        EXIT_SCHEMA,
        "subject path {path:?} must be repository-relative with forward slashes"
    );
    ensure!(
        !path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".."),
        EXIT_SCHEMA,
        "subject path {path:?} must be a normalized repository path"
    );
    Ok(())
}

fn validate_pinned_commit(commit: &str) -> Result<()> {
    ensure!(
        is_canonical_git_oid(commit),
        EXIT_SCHEMA,
        "a file or symbol subject must pin a full resolved Git object id"
    );
    Ok(())
}

fn check_topic(topic: &str) -> Result<()> {
    ensure!(
        !topic.trim().is_empty(),
        EXIT_USAGE,
        "a backlog item needs a topic"
    );
    ensure!(
        !topic.contains('\n'),
        EXIT_USAGE,
        "--topic names what the unresolved work is about, so it cannot span lines. \
         Put the detail in a section."
    );
    let length = topic.chars().count();
    ensure!(
        length <= TOPIC_MAX,
        EXIT_USAGE,
        "--topic names what the unresolved work is about, not the work itself \
         ({length} characters, limit {TOPIC_MAX}). Put the detail in a section."
    );
    Ok(())
}

fn check_text(text: &str) -> Result<()> {
    ensure!(
        !text.trim().is_empty(),
        EXIT_USAGE,
        "a backlog section needs text"
    );
    Ok(())
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("formatting a timestamp cannot fail")
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

pub fn ids(root: &Path) -> Result<Vec<String>> {
    let dir = dir(root);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|error| tool_error(dir.display(), error))? {
        let entry = entry.map_err(|error| tool_error(dir.display(), error))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(id) = name.strip_suffix(".json") {
            found.push(id.to_owned());
        }
    }
    found.sort();
    Ok(found)
}

pub fn load(root: &Path, id: &str) -> Result<Item> {
    let path = item_path(root, id);
    let item: Item = store::read_json(&path)?;
    item.validate()?;
    ensure!(
        item.id == id,
        EXIT_SCHEMA,
        "{}: backlog id {:?} does not match its filename",
        path.display(),
        item.id
    );
    Ok(item)
}

/// Write one item. Callers must already hold the workspace lock: every mutation
/// here is read-modify-write, and the compare-and-consume path additionally
/// needs its fingerprint check and its write to be one step.
fn save(root: &Path, item: &Item) -> Result<()> {
    store::require_current(root)?;
    item.validate()?;
    store::write_json(&item_path(root, &item.id), item)
}

fn remove(root: &Path, id: &str) -> Result<()> {
    let path = item_path(root, id);
    std::fs::remove_file(&path).map_err(|error| tool_error(path.display(), error))
}

pub fn all(root: &Path) -> Result<Vec<Item>> {
    ids(root).and_then(|ids| ids.iter().map(|id| load(root, id)).collect())
}

/// Resolve a unique id prefix or an `engr:backlog:<id>` reference, the way
/// Objects are addressed.
pub fn resolve_id(root: &Path, prefix: &str) -> Result<String> {
    if prefix.starts_with("engr:") {
        let reference = EngrRef::parse_standalone(prefix)?;
        ensure!(
            reference.kind() == ResourceKind::Backlog
                && reference.section().is_none()
                && reference.snapshot_selector().is_none(),
            EXIT_NOT_FOUND,
            "{prefix:?} is not a whole Backlog item reference"
        );
        return Ok(crate::reference::decode_uuid(reference.id())?.to_string());
    }
    if prefix.len() == 26 {
        if let Ok(id) = crate::reference::decode_uuid(prefix) {
            let id = id.to_string();
            if item_path(root, &id).exists() {
                return Ok(id);
            }
        }
    }
    let ids = ids(root)?;
    if ids.iter().any(|id| id == prefix) {
        return Ok(prefix.to_owned());
    }
    let matches: Vec<_> = ids.iter().filter(|id| id.starts_with(prefix)).collect();
    match matches.len() {
        1 => Ok(matches[0].clone()),
        0 => Err(Error::new(
            EXIT_NOT_FOUND,
            format!("no backlog item matches {prefix:?}"),
        )),
        count => Err(Error::new(
            EXIT_NOT_FOUND,
            format!("{prefix:?} matches {count} backlog items; use more characters"),
        )),
    }
}

// ---------------------------------------------------------------------------
// Git provenance for file and symbol subjects
// ---------------------------------------------------------------------------

/// Resolve the commit a file or symbol subject pins.
///
/// Backlog is non-authoritative, but it must not knowingly persist a false
/// snapshot: with the path dirty, HEAD does not describe what the agent
/// actually read. So an omitted revision defaults to HEAD only while the path
/// is clean, and the path has to exist in whatever commit is pinned.
pub fn pin(root: &Path, path: &str, revision: Option<&str>) -> Result<String> {
    validate_repo_path(path)?;
    let commit = match revision {
        Some(revision) => git::resolve(root, revision).ok_or_else(|| {
            Error::new(
                EXIT_INVARIANT,
                format!("{revision} is not a commit in this repository"),
            )
        })?,
        None => match git::path_dirty(root, path) {
            Some(false) => git::head(root).ok_or_else(|| {
                Error::new(
                    EXIT_INVARIANT,
                    format!(
                        "there is no repository HEAD to pin {path} at; choose a committed revision"
                    ),
                )
            })?,
            Some(true) => {
                return Err(Error::new(
                    EXIT_INVARIANT,
                    format!(
                        "{path} has uncommitted changes, so HEAD would not describe what was read; \
                         commit it first, or choose another committed revision"
                    ),
                ))
            }
            None => {
                return Err(Error::new(
                    EXIT_INVARIANT,
                    format!(
                        "could not determine whether {path} is clean; \
                         commit it first, or choose another committed revision"
                    ),
                ))
            }
        },
    };
    ensure!(
        git::path_at(root, &commit, path),
        EXIT_INVARIANT,
        "{path} does not exist at commit {}; a subject cannot pin a snapshot that never held it",
        short(&commit)
    );
    Ok(commit)
}

// ---------------------------------------------------------------------------
// Ordinary editing — agent-managed, no confirmation
// ---------------------------------------------------------------------------

fn take_id(item: &mut Item) -> Result<u64> {
    let id = item.next_section_id;
    item.next_section_id = item.next_section_id.checked_add(1).ok_or_else(|| {
        Error::new(
            EXIT_INVARIANT,
            format!("{} has no remaining section ids", item.id),
        )
    })?;
    Ok(id)
}

fn locked<T>(root: &Path, body: impl FnOnce() -> Result<T>) -> Result<T> {
    store::require_current(root)?;
    store::with_lock(root, || {
        store::require_current(root)?;
        body()
    })
}

fn edit<T>(root: &Path, id: &str, body: impl FnOnce(&mut Item) -> Result<T>) -> Result<T> {
    locked(root, || {
        let mut item = load(root, id)?;
        let outcome = body(&mut item)?;
        item.sections.sort_by_key(|section| section.id);
        save(root, &item)?;
        Ok(outcome)
    })
}

pub fn create(root: &Path, topic: &str, text: &str, subjects: Vec<Subject>) -> Result<Item> {
    check_topic(topic)?;
    check_text(text)?;
    locked(root, || {
        let item = Item {
            id: new_id(),
            topic: topic.trim().to_owned(),
            next_section_id: 2,
            sections: vec![Section {
                id: 1,
                text: text.to_owned(),
                updated_at: now(),
                subjects,
                produced: Vec::new(),
            }],
        };
        save(root, &item)?;
        Ok(item)
    })
}

/// Renaming the topic is not activity on any unresolved point, so it must not
/// refresh Section timestamps — that would make every item look freshly worked.
pub fn rename(root: &Path, id: &str, topic: &str) -> Result<Item> {
    check_topic(topic)?;
    edit(root, id, |item| {
        topic.trim().clone_into(&mut item.topic);
        Ok(item.clone())
    })
}

pub fn add_section(root: &Path, id: &str, text: &str, subjects: Vec<Subject>) -> Result<u64> {
    check_text(text)?;
    edit(root, id, |item| {
        let section = take_id(item)?;
        item.sections.push(Section {
            id: section,
            text: text.to_owned(),
            updated_at: now(),
            subjects,
            produced: Vec::new(),
        });
        Ok(section)
    })
}

pub fn revise_section(root: &Path, id: &str, section: u64, text: &str) -> Result<()> {
    check_text(text)?;
    edit(root, id, |item| {
        item.section(section)?;
        let slot = item
            .sections
            .iter_mut()
            .find(|candidate| candidate.id == section)
            .expect("section presence checked above");
        // Rewriting a section with the wording it already had is not work on
        // it. An idempotent write must not manufacture activity, or a retried
        // command makes an untouched point look like the freshest one.
        if slot.text != text {
            text.clone_into(&mut slot.text);
            slot.updated_at = now();
        }
        Ok(())
    })
}

pub fn set_subjects(root: &Path, id: &str, section: u64, subjects: Vec<Subject>) -> Result<()> {
    edit(root, id, |item| {
        item.section(section)?;
        let slot = item
            .sections
            .iter_mut()
            .find(|candidate| candidate.id == section)
            .expect("section presence checked above");
        // The caller's order is persisted, because no canonical order is
        // required — but reordering a set is not a change to what is
        // unresolved, and the resolution basis already says so.
        let changed = !same_subjects(&slot.subjects, &subjects)?;
        slot.subjects = subjects;
        if changed {
            slot.updated_at = now();
        }
        Ok(())
    })
}

/// Consolidate unresolved points into one, taking a new id.
///
/// The absorbed Sections' `produced[]` carry forward, deduplicated. Dropping
/// them would lose the one thing that stops a later session re-solving work an
/// earlier one already got confirmed — and merging says these were the same
/// unresolved point, not that the outcomes never happened.
pub fn merge_sections(
    root: &Path,
    id: &str,
    absorbs: &[u64],
    text: &str,
    subjects: Vec<Subject>,
) -> Result<u64> {
    check_text(text)?;
    let mut unique = absorbs.to_vec();
    unique.sort_unstable();
    unique.dedup();
    ensure!(
        absorbs.len() >= 2,
        EXIT_INVARIANT,
        "a merge must absorb at least two sections"
    );
    ensure!(
        unique.len() == absorbs.len(),
        EXIT_INVARIANT,
        "a merge cannot absorb the same section twice"
    );
    edit(root, id, |item| {
        let mut produced: Vec<Produced> = Vec::new();
        for absorbed in absorbs {
            for outcome in &item.section(*absorbed)?.produced {
                if !produced.contains(outcome) {
                    produced.push(outcome.clone());
                }
            }
        }
        let section = take_id(item)?;
        item.sections.retain(|item| !absorbs.contains(&item.id));
        item.sections.push(Section {
            id: section,
            text: text.to_owned(),
            updated_at: now(),
            subjects,
            produced,
        });
        Ok(section)
    })
}

/// Remove one unresolved point. If it was the last, the item goes with it:
/// an item is a topic that still has unresolved work in it.
pub fn delete_section(root: &Path, id: &str, section: u64) -> Result<bool> {
    locked(root, || {
        let mut item = load(root, id)?;
        item.section(section)?;
        item.sections.retain(|candidate| candidate.id != section);
        if item.sections.is_empty() {
            remove(root, id)?;
            return Ok(true);
        }
        save(root, &item)?;
        Ok(false)
    })
}

pub fn delete_item(root: &Path, id: &str) -> Result<()> {
    locked(root, || {
        load(root, id)?;
        remove(root, id)
    })
}

// ---------------------------------------------------------------------------
// Candidate-driven reconciliation
// ---------------------------------------------------------------------------

/// A Backlog Section a candidate declares it was derived from, with what the
/// candidate says confirming it means for that source.
///
/// Nothing here is inferred. An Object changing is not evidence that any
/// unresolved point was worked on, so the relationship has to be stated.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub item: String,
    pub section: u64,
    /// The source's resolution basis as it stood when the candidate was
    /// prepared.
    pub basis_sha256: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub produced: Vec<Produced>,
    /// Whether successful confirmation declares this source resolved.
    pub resolves: bool,
}

impl Source {
    pub fn validate(&self) -> Result<()> {
        crate::model::validate_object_id(&self.item).map_err(|_| {
            Error::new(
                EXIT_SCHEMA,
                format!(
                    "backlog source id {:?} must be a canonical UUIDv7",
                    self.item
                ),
            )
        })?;
        ensure!(
            self.section > 0,
            EXIT_SCHEMA,
            "backlog source section ids start at 1"
        );
        ensure!(
            self.basis_sha256.len() == 64
                && self
                    .basis_sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            EXIT_SCHEMA,
            "backlog source must pin a resolution basis"
        );
        for produced in &self.produced {
            produced.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reconciliation {
    /// The declared outcomes were appended. `added` counts the ones that were
    /// not already listed, so a retry reports zero rather than duplicating.
    Recorded {
        added: usize,
    },
    Consumed {
        item_removed: bool,
    },
    /// The source moved after the candidate was prepared. The Object mutation
    /// still stands; this unresolved point needs a human or agent decision.
    SourceChanged,
    /// Already consumed, or edited away. Nothing left to reconcile.
    SourceGone,
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub item: String,
    pub section: u64,
    pub result: Reconciliation,
}

impl Outcome {
    pub fn needs_attention(&self) -> bool {
        matches!(
            self.result,
            Reconciliation::SourceChanged | Reconciliation::SourceGone
        )
    }
}

/// Apply what a successful confirmation said about its Backlog sources.
///
/// Must run while the caller holds the workspace lock, and only after the
/// authoritative mutation is durable. Two rules do the work:
///
/// - a source whose basis moved since prepare is left entirely alone, because a
///   candidate written against v1 must never touch v2 — and is reported, because
///   the reconciliation still has to happen somewhere;
/// - appending an outcome already listed is a no-op, which is what makes the
///   retry after a crash between the projection and the candidate's deletion
///   apply nothing twice.
///
/// A source that changed is not a failure of the Object mutation. That was
/// confirmed by a human and is already in the record.
///
/// Crate-visible deliberately. Its precondition — the caller already holds the
/// workspace lock — cannot be expressed in the signature, and a lock a caller
/// has to remember is one a caller will forget. Inside the crate there is
/// exactly one caller and it is the confirmation path.
pub(crate) fn reconcile(root: &Path, sources: &[Source]) -> Result<Vec<Outcome>> {
    let mut outcomes = Vec::new();
    for source in sources {
        let mut item = match load(root, &source.item) {
            Ok(item) => item,
            Err(error) if error.code == EXIT_NOT_FOUND => {
                outcomes.push(Outcome {
                    item: source.item.clone(),
                    section: source.section,
                    result: Reconciliation::SourceGone,
                });
                continue;
            }
            Err(error) => return Err(error),
        };
        let Some(current) = item
            .sections
            .iter()
            .find(|section| section.id == source.section)
        else {
            outcomes.push(Outcome {
                item: source.item.clone(),
                section: source.section,
                result: Reconciliation::SourceGone,
            });
            continue;
        };
        if current.resolution_basis()? != source.basis_sha256 {
            outcomes.push(Outcome {
                item: source.item.clone(),
                section: source.section,
                result: Reconciliation::SourceChanged,
            });
            continue;
        }

        let result = if source.resolves {
            item.sections.retain(|section| section.id != source.section);
            if item.sections.is_empty() {
                remove(root, &source.item)?;
                Reconciliation::Consumed { item_removed: true }
            } else {
                save(root, &item)?;
                Reconciliation::Consumed {
                    item_removed: false,
                }
            }
        } else {
            let slot = item
                .sections
                .iter_mut()
                .find(|section| section.id == source.section)
                .expect("section presence checked above");
            let mut added = 0;
            for outcome in &source.produced {
                if !slot.produced.contains(outcome) {
                    slot.produced.push(outcome.clone());
                    added += 1;
                }
            }
            if added > 0 {
                save(root, &item)?;
            }
            Reconciliation::Recorded { added }
        };
        outcomes.push(Outcome {
            item: source.item.clone(),
            section: source.section,
            result,
        });
    }
    Ok(outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subject_pair() -> (Subject, Subject) {
        (
            Subject::engr("obj:01h47kwz2mfk0v47mffcnstqva:3"),
            Subject::File {
                path: "src/auth/session.rs".to_owned(),
                commit: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            },
        )
    }

    #[test]
    fn the_resolution_basis_reads_subjects_as_a_set() {
        let (first, second) = subject_pair();
        assert_eq!(
            resolution_basis("unresolved", &[first.clone(), second.clone()]).expect("basis"),
            resolution_basis("unresolved", &[second, first]).expect("basis"),
            "equivalent subject sets in a different order state the same thing"
        );
    }

    #[test]
    fn the_resolution_basis_follows_text_and_subjects_only() {
        let (first, second) = subject_pair();
        let one = std::slice::from_ref(&first);
        let base = resolution_basis("unresolved", one).expect("basis");
        assert_ne!(base, resolution_basis("reworded", one).expect("basis"));
        assert_ne!(
            base,
            resolution_basis("unresolved", &[first.clone(), second]).expect("basis")
        );
    }
}
