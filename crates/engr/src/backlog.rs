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
//!
//! Which is why every persisted shape here refuses a field it does not know.
//! Backlog is hand-editable on purpose, so the realistic failure is not a
//! corrupt file but a plausible one. Accepting `"status": "resolved"` and
//! ignoring it is worse than refusing the file: engr would call the workspace
//! valid, then drop the field on the next ordinary rewrite, having silently
//! edited data it told the reader it understood. `.engr/format.json` is the
//! only schema authority, so a resource carrying its own `format`/`version` is
//! refused for that same reason.

use crate::model::new_id;
use crate::reference::{canonical_embedded, EngrRef, ResourceKind};
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

/// The shared embedded engr target, defined once in [`crate::reference`] and
/// re-exported here because Backlog was its first user. Work is its second, and
/// two domains reaching for the same shape is exactly what "shared" was meant
/// to mean — but only the syntax is shared, never the semantics.
pub use crate::reference::{EngrKind, EngrTarget};

/// What an unresolved point concerns.
///
/// Weaker than the record's `refs[]` on purpose. It carries no dependency, no
/// authority, no ordering, and no claim that the target must change — so a
/// subject that stops resolving is a stale signpost, not a broken record.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Subject {
    Engr {
        #[serde(rename = "ref")]
        reference: String,
    },
    File {
        path: String,
        commit: String,
        /// The observed target carried changes the pinned commit does not hold.
        ///
        /// **Target-local, and only that.** It does not say the repository is
        /// dirty, and it has nothing to do with `git worktree`. It says: when
        /// this subject was written, what the agent actually read is not what
        /// `commit` reconstructs. The commit remains a recoverable baseline; the
        /// extra context may be gone for good, and a later reader deserves to
        /// know that rather than trust a snapshot that was never exact.
        ///
        /// Absent when clean, so a clean subject is byte-for-byte what it was.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        dirty: bool,
    },
    Symbol {
        path: String,
        symbol: String,
        commit: String,
        /// The **containing file** had uncommitted changes. See
        /// [`Subject::File::dirty`].
        ///
        /// Deliberately not a claim about the symbol itself. Proving that a diff
        /// touches one symbol's own range needs language parsing, AST mapping
        /// and symbol-aware diffing, and the protocol refuses to require any of
        /// that for a piece of context metadata. So this is the conservative
        /// answer, and readers must not sharpen it into one about the symbol.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        dirty: bool,
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
            Subject::File { path, commit, .. } => {
                validate_repo_path(path)?;
                validate_pinned_commit(commit)?;
            }
            Subject::Symbol {
                path,
                symbol,
                commit,
                ..
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

    /// What this subject *concerns*, with observation detail stripped.
    ///
    /// `dirty` is deliberately absent. It is a note about the moment of writing,
    /// not part of which target is meant, so two subjects naming the same file
    /// at the same commit are the same subject whether or not one of them was
    /// observed against a modified worktree.
    ///
    /// Kept as its own method rather than a hand-written `PartialEq`: structural
    /// equality still has to mean structural equality, and a comparison that
    /// silently ignored a field would be a trap for the next person who
    /// compares two subjects for a different reason.
    fn identity(&self) -> Subject {
        match self {
            Subject::Engr { .. } => self.clone(),
            Subject::File { path, commit, .. } => Subject::File {
                path: path.clone(),
                commit: commit.clone(),
                dirty: false,
            },
            Subject::Symbol {
                path,
                symbol,
                commit,
                ..
            } => Subject::Symbol {
                path: path.clone(),
                symbol: symbol.clone(),
                commit: commit.clone(),
                dirty: false,
            },
        }
    }

    /// How the subject reads on a screen. Not persisted state.
    pub fn render(&self) -> String {
        match self {
            Subject::Engr { reference } => format!("engr:{reference}"),
            Subject::File {
                path,
                commit,
                dirty,
            } => format!("file   {path} @{}{}", short(commit), inexact(*dirty)),
            Subject::Symbol {
                path,
                symbol,
                commit,
                dirty,
            } => format!(
                "symbol {path} :: {symbol} @{}{}",
                short(commit),
                inexact(*dirty)
            ),
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
#[serde(deny_unknown_fields)]
pub struct Section {
    pub id: u64,
    pub text: String,
    /// When the unresolved statement itself last changed. Operational triage
    /// metadata: last meaningful activity on the unresolved work, which includes
    /// a change to `produced[]` as much as to the wording. Not a concurrency
    /// token -- staleness is decided by the mutation precondition, not by this.
    pub updated_at: String,
    #[serde(default)]
    pub subjects: Vec<Subject>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub produced: Vec<Produced>,
}

impl Section {
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
            // ambiguous when the set is compared.
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
#[serde(deny_unknown_fields)]
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
/// same unresolved thing by design; activity has to agree, or triage
/// reports work on a point nobody touched.
/// Compared on identity, which **excludes `dirty`**. That flag records how the
/// target looked when it was observed, not which target is meant, so a subject
/// re-observed against a dirty worktree still concerns the same thing and must
/// not read as fresh work.
fn same_subjects(left: &[Subject], right: &[Subject]) -> Result<bool> {
    let mut left: Vec<String> = left
        .iter()
        .map(|subject| canonical_json(&subject.identity()))
        .collect::<Result<_>>()?;
    let mut right: Vec<String> = right
        .iter()
        .map(|subject| canonical_json(&subject.identity()))
        .collect::<Result<_>>()?;
    left.sort();
    right.sort();
    Ok(left == right)
}

/// How a dirty subject reads. Said on the surface, not left in the JSON, because
/// the person deciding whether to trust the pinned baseline is the one reading
/// this line.
fn inexact(dirty: bool) -> &'static str {
    if dirty {
        "  (observed with uncommitted changes)"
    } else {
        ""
    }
}

fn short(value: &str) -> &str {
    &value[..8.min(value.len())]
}

/// Backlog says "subject" where the record says "target", so the refusals name
/// the field the caller actually wrote.
fn validate_repo_path(path: &str) -> Result<()> {
    crate::semantics::validate_repo_path("a file or symbol subject", path)
}

fn validate_pinned_commit(commit: &str) -> Result<()> {
    crate::semantics::validate_pinned_commit("a file or symbol subject", commit)
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
/// here is read-modify-write, and a destructive path additionally needs its
/// precondition check and its write to be one step.
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

/// Resolve the commit a subject pins, and whether what was read matched it.
///
/// A dirty path used to be refused outright: HEAD would not describe what the
/// agent actually read, so the subject was rejected as a false snapshot. That
/// was the wrong trade. Refusing loses the context entirely — the agent had
/// genuinely read something and now cannot say so — while the honest answer is
/// available for nothing: pin the baseline **and record that it is inexact**.
///
/// So this returns both, and `dirty` is asked in every branch. Naming an
/// explicit revision does not make the working file match it either; an agent
/// reading a modified file and pinning an older commit has exactly the same gap.
/// What is refused is not knowing — if git cannot say whether the path is clean,
/// there is no honest answer to record.
///
/// The path must exist in whatever commit is pinned, dirty or not: a baseline
/// that never held the file reconstructs nothing.
pub fn pin(root: &Path, path: &str, revision: Option<&str>) -> Result<(String, bool)> {
    validate_repo_path(path)?;
    // Asked in every branch, because pinning an explicit revision does not make
    // the working file match it either — an agent reading a modified file and
    // naming an older commit has the same gap, and the marker is about what was
    // read rather than about which commit was chosen.
    let dirty = git::path_dirty(root, path).ok_or_else(|| {
        Error::new(
            EXIT_INVARIANT,
            format!(
                "could not determine whether {path} is clean; \
                 a subject records whether what was read matches what it pins"
            ),
        )
    })?;
    let commit = match revision {
        Some(revision) => git::resolve(root, revision).ok_or_else(|| {
            Error::new(
                EXIT_INVARIANT,
                format!("{revision} is not a commit in this repository"),
            )
        })?,
        None => git::head(root).ok_or_else(|| {
            Error::new(
                EXIT_INVARIANT,
                format!(
                    "there is no repository HEAD to pin {path} at; choose a committed revision"
                ),
            )
        })?,
    };
    ensure!(
        git::path_at(root, &commit, path),
        EXIT_INVARIANT,
        "{path} does not exist at commit {}; a subject cannot pin a snapshot that never held it",
        short(&commit)
    );
    Ok((commit, dirty))
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
        // unresolved, and the set comparison already says so.
        let changed = !same_subjects(&slot.subjects, &subjects)?;
        slot.subjects = subjects;
        if changed {
            slot.updated_at = now();
        }
        Ok(())
    })
}

/// Record that working on this point produced durable knowledge.
///
/// **The second of two independent operations.** Admitting the Object does not
/// reach in here, so an agent that produced something records it afterwards, as
/// an ordinary staging edit. Forgetting leaves the bookkeeping stale and the
/// admitted record perfectly valid — which is the trade #8 chose over an
/// inferred link that would eventually consume a point nobody meant to resolve.
///
/// Existence is checked **here and only here**, and only in this direction. A
/// target must exist to be claimed; afterwards it may be superseded, deleted or
/// absorbed by a merge, and the entry becomes an unavailable historical pointer
/// rather than corruption. It is never a reverse constraint: no Object operation
/// consults `produced[]`, and nothing retargets an entry to a replacement,
/// because that would rewrite what was actually produced.
pub fn record_produced(root: &Path, id: &str, section: u64, outcome: Produced) -> Result<bool> {
    outcome.validate()?;
    let (object, target_section) = outcome.target()?;
    let projected = crate::ops::effective(root, &object).map_err(|error| {
        Error::new(
            error.code,
            format!(
                "produced outcome names object {}, which does not exist: {}",
                short(&object),
                error.message
            ),
        )
    })?;
    if let Some(target_section) = target_section {
        projected.section(target_section).map_err(|_| {
            Error::new(
                EXIT_NOT_FOUND,
                format!(
                    "produced outcome names {} §{target_section}, which does not exist",
                    short(&object)
                ),
            )
        })?;
    }
    edit(root, id, |item| {
        item.section(section)?;
        let slot = item
            .sections
            .iter_mut()
            .find(|candidate| candidate.id == section)
            .expect("section presence checked above");
        // A set: claiming the same outcome twice carries no more than claiming
        // it once, so a repeated call is not an error and not a duplicate.
        if slot.produced.contains(&outcome) {
            return Ok(false);
        }
        slot.produced.push(outcome.clone());
        // Bookkeeping *is* activity on the unresolved work, even though the
        // wording did not move: `updated_at` means last meaningful activity, and
        // learning what a point produced is meaningful to whoever picks it up.
        slot.updated_at = now();
        Ok(true)
    })
}

/// Take an outcome back off a point.
///
/// Mutable bookkeeping, not append-only history: an entry recorded in error is
/// corrected here. What it corrects is the *relationship* — that this point
/// produced that outcome — and never the target, which is why removal asks
/// nothing about whether the target still resolves. Requiring it to would make
/// a mistaken entry uncorrectable exactly when the target has gone.
pub fn forget_produced(root: &Path, id: &str, section: u64, outcome: &Produced) -> Result<bool> {
    edit(root, id, |item| {
        item.section(section)?;
        let slot = item
            .sections
            .iter_mut()
            .find(|candidate| candidate.id == section)
            .expect("section presence checked above");
        let before = slot.produced.len();
        slot.produced.retain(|entry| entry != outcome);
        let removed = slot.produced.len() != before;
        if removed {
            slot.updated_at = now();
        }
        Ok(removed)
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

/// Consume one unresolved point: judge it resolved and take it out.
///
/// **The only way a Section leaves.** There is no separate delete or discard,
/// and that is deliberate rather than a naming preference: two removal verbs
/// would mean a reader of the history cannot tell whether a point was resolved,
/// abandoned, or swept away by mistake. Consume covers all of those, including
/// the cases with nothing to show for them — a duplicate, an obsolete concern, a
/// decision not to pursue — because what authorizes removal is the judgement
/// that it no longer needs to be pending, not evidence of an outcome.
///
/// If it was the last point, the item goes with it: an item is a topic that
/// still has unresolved work in it, and an empty one is not a canonical state.
/// That removal is mechanical structural cleanup inside this same mutation, not
/// a second deletion anyone has to ask for.
pub fn consume_section(root: &Path, id: &str, section: u64) -> Result<bool> {
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
