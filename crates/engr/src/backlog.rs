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
use crate::rules::Attempt;
use crate::{
    ensure, git, store, tool_error, Error, Result, EXIT_INVARIANT, EXIT_NOT_FOUND, EXIT_SCHEMA,
    EXIT_STALE, EXIT_USAGE,
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
    /// Present only when this point's current wording was admitted **without a
    /// passing review**, because the attempt had passed a project rule's
    /// ceiling.
    ///
    /// Backlog admits it anyway — preserving unresolved engineering intent is
    /// what this domain is for, and refusing would send the thought back to
    /// nowhere. The marker is what stops that from being silent: a reader can
    /// see that this wording went in exhausted, and roughly why.
    ///
    /// Absent is the ordinary case and means what it says — this went in
    /// normally, or no project rule governed it at all. A later successful
    /// mutation clears it; a later exhausted one replaces it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_review: Option<crate::rules::RuleReview>,
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
            //
            // Compared through the same projection equality uses. `dirty` is a
            // note about the moment of writing, not part of which target is
            // meant — so comparing raw bytes here would let one file at one
            // commit sit in the set twice, once clean and once dirty, while
            // every other part of the model insists those are one subject.
            ensure!(
                seen.insert(canonical_json(&subject.identity())?),
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
        // The marker exists only to describe an exhausted review, so a stored
        // value that could not have come from one is not a diagnostic — it is a
        // claim about a review that never happened. Refusing unknown fields
        // while accepting `{attempts: 0, limit: 0}` would make the strictness
        // decorative: this file is hand-editable by design, and the realistic
        // failure is a plausible value, not a corrupt one.
        if let Some(review) = self.rule_review {
            ensure!(
                review.limit >= 1,
                EXIT_SCHEMA,
                "§{}: a review ceiling of 0 is not a limit, it is a rule nothing can pass",
                self.id
            );
            ensure!(
                review.attempts > review.limit,
                EXIT_SCHEMA,
                "§{}: attempt {} is within a ceiling of {}, so it is not exhausted and there is nothing for this marker to record",
                self.id,
                review.attempts,
                review.limit
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
    // Against the commit being pinned, not against HEAD. They are the same
    // question only when the pin is HEAD: choose an older revision from a clean
    // worktree and a status check says "clean" while the file is plainly not
    // what that commit reconstructs — which is exactly the claim this subject
    // is about to make. Answering the easy question would put `dirty: false` on
    // a subject whose baseline never held what the agent read.
    let dirty = git::path_differs_at(root, &commit, path).ok_or_else(|| {
        Error::new(
            EXIT_INVARIANT,
            format!(
                "could not compare {path} against {}; \
                 a subject records whether what was read matches what it pins",
                short(&commit)
            ),
        )
    })?;
    Ok((commit, dirty))
}

// ---------------------------------------------------------------------------
// Mutation preconditions
// ---------------------------------------------------------------------------

/// Exactly what a prepared Backlog mutation was written against.
///
/// There is a gap between reading a point, reviewing a change to it, and
/// applying that change. Whatever happens in that gap must not be applied over:
/// a review of v1 wording must not silently land on v2, and a consume prepared
/// against a point somebody has since sharpened must not destroy the sharpening.
///
/// This replaces the old Backlog-specific `canonical(text, subjects[])`
/// fingerprint, which is gone as a protocol concept. The difference is not the
/// hashing — an implementation may still hash internally — it is the **scope**.
/// The fingerprint covered a hand-picked subset and was therefore blind to any
/// field outside it; these bind the whole predecessor the mutation actually
/// depends on, so a field added later is covered without anyone remembering.
///
/// Each variant binds what that mutation genuinely rests on and deliberately no
/// more. Binding the whole item everywhere would be simpler and wrong: an
/// unrelated sibling Section moving would stale a mutation that never read it,
/// and a model that cries stale for unrelated work teaches people to re-prepare
/// without looking.
#[derive(Clone, Serialize, PartialEq, Eq, Debug)]
#[serde(tag = "precondition", rename_all = "snake_case")]
pub enum Precondition {
    /// Changing the topic: the complete parent item.
    ///
    /// The topic is shared context for every Section under it, so a change to
    /// any of them can change what renaming it means.
    Item { item: Item },
    /// Adding a Section: the parent topic, and that the id is still free.
    ///
    /// Sibling Sections are excluded on purpose — adding a point does not read
    /// them, so their moving says nothing about this add.
    SectionAbsent {
        item: String,
        topic: String,
        section: u64,
    },
    /// Changing or consuming a Section: that whole Section, and the topic.
    ///
    /// The whole Section rather than its wording: `subjects[]`, `produced[]` and
    /// `updated_at` are all things a reviewer may have taken into account, and a
    /// subset would be blind to exactly the field somebody else touched. The
    /// topic comes too, because it is the context the Section is read in.
    Section {
        item: String,
        topic: String,
        section: Section,
    },
    /// Merging: the topic, and every Section the merge consumes or keeps.
    ///
    /// All of them, because a merge is one judgement about several points at
    /// once: if any of them moved, the judgement was about something else.
    Merge {
        item: String,
        topic: String,
        sections: Vec<Section>,
    },
}

impl Precondition {
    /// Read the current predecessor for a topic change.
    pub fn topic(root: &Path, item: &str) -> Result<Self> {
        Ok(Self::of_item(&load(root, item)?))
    }

    /// The same three predecessors, from an item already in hand.
    ///
    /// The read surface has to print the exact values the mutations will
    /// compare against, and re-reading the file to do it would let the two
    /// disagree about a state neither of them saw.
    pub fn of_item(item: &Item) -> Self {
        Self::Item { item: item.clone() }
    }

    pub fn of_add(item: &Item) -> Self {
        Self::SectionAbsent {
            item: item.id.clone(),
            topic: item.topic.clone(),
            section: item.next_section_id,
        }
    }

    pub fn of_section(item: &Item, section: u64) -> Result<Self> {
        Ok(Self::Section {
            item: item.id.clone(),
            topic: item.topic.clone(),
            section: item.section(section)?.clone(),
        })
    }

    /// Read the current predecessor for adding a Section.
    ///
    /// The id bound is the one the item will hand out next, so a concurrent add
    /// that takes it stales this one — two adds must not each believe they are
    /// creating the same point.
    pub fn section_absent(root: &Path, item: &str) -> Result<Self> {
        Ok(Self::of_add(&load(root, item)?))
    }

    /// Read the current predecessor for changing or consuming one Section.
    pub fn section(root: &Path, item: &str, section: u64) -> Result<Self> {
        Self::of_section(&load(root, item)?, section)
    }

    /// Read the current predecessor for a merge over several Sections.
    pub fn merge(root: &Path, item: &str, sections: &[u64]) -> Result<Self> {
        let loaded = load(root, item)?;
        let mut bound = Vec::new();
        for id in sections {
            bound.push(loaded.section(*id)?.clone());
        }
        bound.sort_by_key(|section| section.id);
        Ok(Self::Merge {
            item: loaded.id.clone(),
            topic: loaded.topic.clone(),
            sections: bound,
        })
    }

    /// Fold the predecessors a merge names into the one it binds.
    ///
    /// A merge is a single judgement about several points, so it binds them
    /// together: if any of them moved, the judgement was about something else.
    /// They arrive separately because a caller reads them separately — one token
    /// per point, from the line describing that point.
    pub fn combine(mut bound: Vec<Self>) -> Result<Self> {
        ensure!(
            !bound.is_empty(),
            EXIT_INVARIANT,
            "a mutation binds at least one predecessor"
        );
        if bound.len() == 1 {
            return Ok(bound.remove(0));
        }
        let mut item = None;
        let mut topic = None;
        let mut sections = Vec::new();
        for one in bound {
            let Self::Section {
                item: owner,
                topic: context,
                section,
            } = one
            else {
                return Err(Error::new(
                    EXIT_INVARIANT,
                    "only unresolved points combine into one predecessor".to_owned(),
                ));
            };
            // A merge is within one item by construction; two owners means the
            // caller read points that were never siblings.
            if let Some(first) = &item {
                ensure!(
                    first == &owner && topic.as_ref() == Some(&context),
                    EXIT_INVARIANT,
                    "these points do not belong to the same topic"
                );
            }
            item = Some(owner);
            topic = Some(context);
            sections.push(section);
        }
        sections.sort_by_key(|section| section.id);
        Ok(Self::Merge {
            item: item.expect("checked non-empty"),
            topic: topic.expect("checked non-empty"),
            sections,
        })
    }

    /// A short stand-in for this predecessor, for a caller that cannot hold the
    /// whole thing.
    ///
    /// A command line cannot carry a complete Section, so an agent that read one
    /// needs some way to say *which* state it read. This is that: engr prints it
    /// beside what it describes, and takes it back on the mutation.
    ///
    /// **Not a fingerprint, and never persisted.** §8 removed
    /// `canonical(text, subjects[])` as a protocol concept and allows an
    /// implementation to compare a predecessor internally by hash; this is that
    /// allowance and nothing more. Nothing stores it, no field records it, and
    /// its spelling is engr's own business — the authority is still the whole
    /// predecessor, compared under the lock.
    pub fn token(&self) -> Result<String> {
        use sha2::{Digest, Sha256};
        Ok(format!(
            "{:x}",
            Sha256::digest(canonical_json(self)?.as_bytes())
        ))
    }

    /// The item this precondition is about.
    pub fn item(&self) -> &str {
        match self {
            Self::SectionAbsent { item, .. }
            | Self::Section { item, .. }
            | Self::Merge { item, .. } => item,
            Self::Item { item } => &item.id,
        }
    }

    /// Whether the world still looks the way this mutation was written against.
    ///
    /// Called inside the same lock that performs the write; checking outside it
    /// would leave open the very gap it exists to close.
    pub fn still_holds(&self, root: &Path) -> Result<()> {
        match self {
            Self::Item { item } => {
                let current = load(root, &item.id)?;
                if &current == item {
                    Ok(())
                } else {
                    stale("the backlog item")
                }
            }
            Self::SectionAbsent {
                item,
                topic,
                section,
            } => {
                let current = load(root, item)?;
                if &current.topic != topic {
                    return stale("the topic");
                }
                // The allocation state, not merely whether the id looks free.
                // Absence alone says yes to a slot that was taken and then
                // consumed: the id reads free again while the counter has moved
                // on permanently, so the add would receive a different identity
                // than the one reviewed — under a number earlier subjects may
                // already be aimed at. Comparing the counter asks the question
                // that was actually reserved.
                if current.next_section_id != *section {
                    return stale("the section id this would take");
                }
                Ok(())
            }
            Self::Section {
                item,
                topic,
                section,
            } => {
                let current = load(root, item)?;
                if &current.topic != topic {
                    return stale("the topic");
                }
                match current.section(section.id) {
                    Ok(now) if now == section => Ok(()),
                    _ => stale("that unresolved point"),
                }
            }
            Self::Merge {
                item,
                topic,
                sections,
            } => {
                let current = load(root, item)?;
                if &current.topic != topic {
                    return stale("the topic");
                }
                for section in sections {
                    match current.section(section.id) {
                        Ok(now) if now == section => {}
                        _ => return stale("one of the points being merged"),
                    }
                }
                Ok(())
            }
        }
    }
}

/// Stale is its own outcome, not a failure of the mutation.
///
/// The caller did nothing wrong and the data is not corrupt: the world moved
/// between reading and writing. Saying which part moved is what makes the retry
/// intelligent rather than a reflex.
fn stale(what: &str) -> Result<()> {
    Err(Error::new(
        EXIT_STALE,
        format!(
            "{what} changed since this was prepared, so the change was prepared against something else; read it again and re-prepare"
        ),
    ))
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

/// Everything a Backlog mutation carries besides the change itself.
///
/// One argument rather than two loose parameters on nine functions, because the
/// two travel together: both are established when the mutation is prepared and
/// both are settled at the moment it is applied, inside the same lock. Splitting
/// them invites a caller to pass one and forget the other.
#[derive(Clone, Debug)]
pub struct Prepared {
    /// Which attempt of this review sequence the agent is on.
    ///
    /// Agent-attested process metadata: engr keeps no counter, and a sequence
    /// that is abandoned or lost may honestly begin again at 1.
    pub attempt: Attempt,
    /// What the mutation was written against, when it was prepared separately.
    ///
    /// Absent when there was no gap to protect — a caller that reads and writes
    /// inside one invocation was never exposed to the race this closes, and
    /// making it invent a predecessor would be ceremony rather than safety.
    pub precondition: Option<Precondition>,
}

impl Prepared {
    /// A first attempt, prepared against nothing.
    pub fn first() -> Self {
        Self {
            attempt: Attempt::FIRST,
            precondition: None,
        }
    }

    /// A given attempt of a review sequence.
    pub fn attempt(attempt: Attempt) -> Self {
        Self {
            attempt,
            precondition: None,
        }
    }

    /// Bind the predecessor this mutation was written against.
    pub fn against(mut self, precondition: Precondition) -> Self {
        self.precondition = Some(precondition);
        self
    }
}

/// What Rule Review said about the mutation now being applied.
///
/// Composed inside the lock, immediately before the change lands. Composing it
/// earlier would leave the interval between the verdict and the write open to
/// exactly the rule change that would have altered the verdict.
struct Reviewed {
    marker: Option<crate::rules::RuleReview>,
    /// Whether any project rule governed this mutation at all — and so whether
    /// a review was required, and a predecessor with it.
    governed: bool,
}

impl Reviewed {
    fn compose(root: &Path, attempt: Attempt) -> Result<Self> {
        // One read of policy for both answers. Asking twice lets a rule appear
        // between them and leaves this mutation acting on two pictures at once.
        let (governed, verdict) =
            crate::rules::assess(root, crate::rules::Domain::Backlog, attempt)?;
        match verdict {
            // No applicable rule, or none of them out of attempts. Either way
            // there is no diagnostic to carry.
            crate::rules::Exhaustion::NotReached => Ok(Self {
                marker: None,
                governed,
            }),
            crate::rules::Exhaustion::Exhausted(marker) => Ok(Self {
                marker: Some(marker),
                governed,
            }),
            // The Object verdicts. Reaching one here would mean the composition
            // answered for the wrong domain, and the failure mode — a Backlog
            // mutation quietly refused, or sent to a human — is precisely what
            // #25 says must not happen to unresolved intent.
            other => Err(Error::new(
                EXIT_INVARIANT,
                format!("a backlog mutation was given the {other:?} verdict, which is not its own"),
            )),
        }
    }

    /// Stamp the verdict onto a point this mutation preserved.
    ///
    /// Both directions matter. An exhausted admission gains the marker; an
    /// ordinary one **clears** any marker already there, because the diagnostic
    /// describes how the wording standing now got in, and this mutation is now
    /// the answer to that.
    fn mark(&self, section: &mut Section) {
        section.rule_review = self.marker;
    }

    /// A reviewed mutation must carry what it was reviewed against.
    ///
    /// Here rather than only at the command line, because the library exposes
    /// the same semantic mutation. `Prepared::first()` is a perfectly ordinary
    /// constructor, and without this a direct caller could reword — or
    /// destructively consume — a governed point having reviewed nothing, while
    /// every check that does run passes. Enforcement that lives one layer above
    /// the thing it protects is enforcement of the layer, not of the thing.
    fn needs_predecessor(&self, precondition: Option<&Precondition>) -> Result<()> {
        ensure!(
            !self.governed || precondition.is_some(),
            EXIT_INVARIANT,
            "a project rule governs backlog, so this mutation must carry the predecessor it was reviewed against"
        );
        Ok(())
    }

    /// Refuse a mutation that soft-admission does not cover.
    ///
    /// Soft-admission buys exactly one thing: an unresolved point survives a
    /// review it could not pass, *and* the marker says so. A mutation that
    /// cannot deliver both halves does not get the exception — either because it
    /// preserves nothing, or because there is nowhere for the marker to go.
    /// Nothing is written when this refuses, marker included: no mutation was
    /// admitted, so there is nothing for a diagnostic to describe.
    fn must_have_passed(&self, refusal: &str) -> Result<()> {
        match self.marker {
            None => Ok(()),
            Some(marker) => Err(Error::new(
                EXIT_INVARIANT,
                format!(
                    "this is attempt {} and a project rule allows {}: {refusal}",
                    marker.attempts, marker.limit
                ),
            )),
        }
    }
}

/// What a given mutation is entitled to have been prepared against.
///
/// A precondition that still holds is not the same as a precondition that
/// authorizes *this* change. `still_holds` asks the world whether the thing it
/// names has moved, and answers honestly about a thing nobody is mutating — so
/// without this, a caller holding a perfectly valid predecessor for one item can
/// apply it to another, or bind §1 and consume §5, and the exact-predecessor
/// guarantee is gone precisely where it looks satisfied.
enum Binds<'a> {
    /// The topic, and therefore the complete item.
    Topic,
    /// The Section id an add is about to receive.
    NewSection,
    /// One whole Section, by id.
    Section(u64),
    /// A merge: the destination and every source, and nothing else.
    Merge {
        destination: u64,
        sources: &'a [u64],
    },
}

impl Precondition {
    /// Whether this predecessor is the one *this* mutation rests on.
    ///
    /// Checked before [`Self::still_holds`], because "you prepared against
    /// something else entirely" is a more fundamental answer than "what you
    /// prepared against has moved", and reporting the second for the first would
    /// send a caller off to re-read the wrong thing.
    fn authorizes(&self, item: &str, binds: &Binds) -> Result<()> {
        ensure!(
            self.item() == item,
            EXIT_INVARIANT,
            "this was prepared against backlog {}, and the change is to {}; a predecessor that still holds is not a predecessor for this",
            short(self.item()),
            short(item)
        );
        let matches = match (self, binds) {
            (Self::Item { .. }, Binds::Topic) => true,
            (Self::SectionAbsent { .. }, Binds::NewSection) => true,
            (Self::Section { section, .. }, Binds::Section(target)) => section.id == *target,
            (
                Self::Merge { sections, .. },
                Binds::Merge {
                    destination,
                    sources,
                },
            ) => {
                let bound: BTreeSet<u64> = sections.iter().map(|section| section.id).collect();
                let touched: BTreeSet<u64> = std::iter::once(*destination)
                    .chain(sources.iter().copied())
                    .collect();
                bound == touched
            }
            _ => false,
        };
        ensure!(
            matches,
            EXIT_INVARIANT,
            "this was prepared against {}, which is not what this change touches",
            self.describes()
        );
        Ok(())
    }

    /// What this predecessor covers, for a refusal to name.
    fn describes(&self) -> String {
        match self {
            Self::Item { .. } => "the whole item".to_owned(),
            Self::SectionAbsent { section, .. } => format!("§{section} not existing yet"),
            Self::Section { section, .. } => format!("§{}", section.id),
            Self::Merge { sections, .. } => sections
                .iter()
                .map(|section| format!("§{}", section.id))
                .collect::<Vec<_>>()
                .join(" + "),
        }
    }
}

fn locked<T>(root: &Path, body: impl FnOnce() -> Result<T>) -> Result<T> {
    store::require_current(root)?;
    store::with_lock(root, || {
        store::require_current(root)?;
        body()
    })
}

/// Apply one mutation to one item, under the lock, having earned the right to.
///
/// The order is the contract. The precondition is checked first, because a
/// mutation prepared against something else is not a mutation anyone reviewed;
/// then the verdict is composed against the rules as they stand right now.
fn edit<T>(
    root: &Path,
    id: &str,
    prepared: &Prepared,
    binds: Binds,
    body: impl FnOnce(&mut Item, &Reviewed) -> Result<T>,
) -> Result<T> {
    locked(root, || {
        if let Some(precondition) = &prepared.precondition {
            precondition.authorizes(id, &binds)?;
            precondition.still_holds(root)?;
        }
        let reviewed = Reviewed::compose(root, prepared.attempt)?;
        reviewed.needs_predecessor(prepared.precondition.as_ref())?;
        let mut item = load(root, id)?;
        let outcome = body(&mut item, &reviewed)?;
        item.sections.sort_by_key(|section| section.id);
        save(root, &item)?;
        Ok(outcome)
    })
}

pub fn create(
    root: &Path,
    topic: &str,
    text: &str,
    subjects: Vec<Subject>,
    prepared: &Prepared,
) -> Result<Item> {
    check_topic(topic)?;
    check_text(text)?;
    locked(root, || {
        // engr allocates the identity here, so a creation has no predecessor to
        // bind: whatever id a caller prepared against, the item created is a
        // different one, and checking the first would authorize the second.
        // Refused rather than ignored — silently accepting a precondition that
        // cannot apply is how a caller comes to believe it has a guarantee.
        //
        // And so creation is the one mutation exempt from carrying a
        // predecessor under a governing rule. It has to be: requiring what it
        // cannot express would make creating an unresolved point impossible in
        // exactly the workspaces that have rules about unresolved points, which
        // is the opposite of what a rule is for.
        //
        // Settled rather than provisional: engr mints the id during the create
        // and a caller may not choose one. The alternative — letting a caller
        // propose it, so creation would bind that id's absence — needs
        // reservation state or a token proving the caller was entitled to that
        // id, which is a lifecycle bolted on to protect an identity nobody else
        // can be racing for.
        ensure!(
            prepared.precondition.is_none(),
            EXIT_INVARIANT,
            "a new backlog item takes an id engr allocates, so there is nothing for a precondition to bind"
        );
        let reviewed = Reviewed::compose(root, prepared.attempt)?;
        let mut section = Section {
            id: 1,
            text: text.to_owned(),
            updated_at: now(),
            subjects,
            produced: Vec::new(),
            rule_review: None,
        };
        reviewed.mark(&mut section);
        let item = Item {
            id: new_id(),
            topic: topic.trim().to_owned(),
            next_section_id: 2,
            sections: vec![section],
        };
        save(root, &item)?;
        Ok(item)
    })
}

/// Renaming the topic is not activity on any unresolved point, so it must not
/// refresh Section timestamps — that would make every item look freshly worked.
///
/// An exhausted rename is **refused**, and that is the interesting case. The
/// marker is a Section field, and a rename admits no Section wording: stamping
/// every Section would claim of each one something true of none. But letting the
/// rename through unmarked would make it the one soft-admission nothing records,
/// which is worse — the whole point of the marker is that an exhausted change
/// cannot be silent. What an item-level marker should look like is a persisted
/// representation nobody has settled, so this refuses rather than inventing one.
pub fn rename(root: &Path, id: &str, topic: &str, prepared: &Prepared) -> Result<Item> {
    check_topic(topic)?;
    edit(root, id, prepared, Binds::Topic, |item, reviewed| {
        reviewed.must_have_passed(
            "a topic is not renamed on an exhausted review, because there is nowhere to record that it was: the marker belongs to a point, and this changes none of them",
        )?;
        topic.trim().clone_into(&mut item.topic);
        Ok(item.clone())
    })
}

pub fn add_section(
    root: &Path,
    id: &str,
    text: &str,
    subjects: Vec<Subject>,
    prepared: &Prepared,
) -> Result<u64> {
    check_text(text)?;
    edit(root, id, prepared, Binds::NewSection, |item, reviewed| {
        let section = take_id(item)?;
        let mut added = Section {
            id: section,
            text: text.to_owned(),
            updated_at: now(),
            subjects,
            produced: Vec::new(),
            rule_review: None,
        };
        reviewed.mark(&mut added);
        item.sections.push(added);
        Ok(section)
    })
}

pub fn revise_section(
    root: &Path,
    id: &str,
    section: u64,
    text: &str,
    prepared: &Prepared,
) -> Result<()> {
    check_text(text)?;
    edit(
        root,
        id,
        prepared,
        Binds::Section(section),
        |item, reviewed| {
            item.section(section)?;
            let slot = item
                .sections
                .iter_mut()
                .find(|candidate| candidate.id == section)
                .expect("section presence checked above");
            // Rewriting a section with the wording it already had is not work on
            // it. An idempotent write must not manufacture activity, or a retried
            // command makes an untouched point look like the freshest one.
            //
            // The verdict follows the same test, for the same reason: a write that
            // changed nothing admitted nothing, so it neither earns a marker nor
            // clears one somebody else's write put there.
            if slot.text != text {
                text.clone_into(&mut slot.text);
                slot.updated_at = now();
                reviewed.mark(slot);
            }
            Ok(())
        },
    )
}

pub fn set_subjects(
    root: &Path,
    id: &str,
    section: u64,
    subjects: Vec<Subject>,
    prepared: &Prepared,
) -> Result<()> {
    edit(
        root,
        id,
        prepared,
        Binds::Section(section),
        |item, reviewed| {
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
                reviewed.mark(slot);
            }
            Ok(())
        },
    )
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
pub fn record_produced(
    root: &Path,
    id: &str,
    section: u64,
    outcome: Produced,
    prepared: &Prepared,
) -> Result<bool> {
    outcome.validate()?;
    let (object, target_section) = outcome.target()?;
    edit(
        root,
        id,
        prepared,
        Binds::Section(section),
        |item, reviewed| {
            // Inside the lock, not before it. Existence is checked exactly once, at
            // the moment the claim is made — so it has to be checked at the moment
            // the claim is *written*. Validating first and appending afterwards
            // leaves a gap an Object mutation fits through, and the one check this
            // relationship ever gets would have been against something that no
            // longer existed when the relationship landed.
            let projected = crate::ops::effective(root, &object).map_err(|error| {
                // Absent and unreadable are different answers, and #13 §4 says
                // so: never downgrade invalid authority to "not found". The
                // exit code already distinguished them; the wording did not,
                // and the wording is what a person acts on — "does not exist"
                // sends someone looking for a missing object when what they
                // have is a present one whose history will not load.
                let what = if error.code == EXIT_NOT_FOUND {
                    "does not exist"
                } else {
                    "cannot be read as authority"
                };
                Error::new(
                    error.code,
                    format!(
                        "produced outcome names object {}, which {what}: {}",
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
            // Existing is not the same as sound. This entry asserts that a
            // durably admitted outcome exists, and authority whose wording was
            // changed outside the gate is exactly what that assertion must not
            // be allowed to launder — the claim gets made once and is never
            // re-examined, so the one check it gets has to be the real one.
            crate::ops::sound(root, &projected, target_section).map_err(|error| {
                Error::new(
                    error.code,
                    format!(
                        "produced outcome names authority that is not intact: {}",
                        error.message
                    ),
                )
            })?;
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
            reviewed.mark(slot);
            Ok(true)
        },
    )
}

/// Take an outcome back off a point.
///
/// Mutable bookkeeping, not append-only history: an entry recorded in error is
/// corrected here. What it corrects is the *relationship* — that this point
/// produced that outcome — and never the target, which is why removal asks
/// nothing about whether the target still resolves. Requiring it to would make
/// a mistaken entry uncorrectable exactly when the target has gone.
pub fn forget_produced(
    root: &Path,
    id: &str,
    section: u64,
    outcome: &Produced,
    prepared: &Prepared,
) -> Result<bool> {
    edit(
        root,
        id,
        prepared,
        Binds::Section(section),
        |item, reviewed| {
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
                reviewed.mark(slot);
            }
            Ok(removed)
        },
    )
}

/// Consolidate unresolved points into one, **into an explicit destination**.
///
/// The destination survives with its own id and the merged wording; the sources
/// are removed in the same mutation. It allocates nothing: a merge says these
/// were one unresolved point all along, and minting a third id to say that would
/// leave every subject pointing at the destination suddenly naming something
/// that no longer exists. Source ids are not reused, like any consumed id.
///
/// All or nothing, under one lock. Consolidation is a single judgement, and half
/// of it applied is a state nobody decided on — a destination rewritten with
/// merged wording while the source it merged still sits there unresolved.
///
/// `produced[]` is carried by **set union**. Dropping the sources' outcomes
/// would lose the one thing that stops a later session re-solving work an
/// earlier one already got confirmed: merging says these were the same point,
/// not that the outcomes never happened.
pub fn merge_into(
    root: &Path,
    id: &str,
    destination: u64,
    sources: &[u64],
    text: &str,
    subjects: Vec<Subject>,
    prepared: &Prepared,
) -> Result<()> {
    check_text(text)?;
    let mut unique = sources.to_vec();
    unique.sort_unstable();
    unique.dedup();
    ensure!(
        !sources.is_empty(),
        EXIT_INVARIANT,
        "a merge needs at least one section to merge into the destination"
    );
    ensure!(
        unique.len() == sources.len(),
        EXIT_INVARIANT,
        "a merge cannot take the same section twice"
    );
    ensure!(
        !sources.contains(&destination),
        EXIT_INVARIANT,
        "§{destination} is the destination, so it cannot also be merged into itself"
    );
    edit(
        root,
        id,
        prepared,
        Binds::Merge {
            destination,
            sources,
        },
        |item, reviewed| {
            // A merge removes the sources, and a Section leaves only through a
            // review that passed — a consume, or atomically as the source of a
            // merge. Soft-admission is for mutations that keep the unresolved point
            // available, and the sources' own wording does not survive this one.
            reviewed.must_have_passed(
            "a merge removes its sources, and an unresolved point is not removed on an exhausted review",
        )?;
            // Every participant is checked before anything moves, so a merge naming
            // a section that is not there changes nothing at all.
            let mut produced = item.section(destination)?.produced.clone();
            for source in sources {
                for outcome in &item.section(*source)?.produced {
                    if !produced.contains(outcome) {
                        produced.push(outcome.clone());
                    }
                }
            }
            item.sections
                .retain(|section| !sources.contains(&section.id));
            let slot = item
                .sections
                .iter_mut()
                .find(|section| section.id == destination)
                .expect("destination presence checked above");
            text.clone_into(&mut slot.text);
            slot.subjects = subjects;
            slot.produced = produced;
            // Unambiguously activity: the destination now states something it did
            // not state before, whatever the wording happens to be.
            slot.updated_at = now();
            // Reached only on a review that passed, so this always clears.
            reviewed.mark(slot);
            Ok(())
        },
    )
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
/// Removal is the one mutation an exhausted review does not get. Everywhere
/// else Backlog would rather admit the thought and mark it than lose it; here
/// there would be nothing left to mark. The point stays exactly as it was, and
/// no marker is written, because nothing was admitted for one to describe.
pub fn consume_section(root: &Path, id: &str, section: u64, prepared: &Prepared) -> Result<bool> {
    locked(root, || {
        if let Some(precondition) = &prepared.precondition {
            precondition.authorizes(id, &Binds::Section(section))?;
            precondition.still_holds(root)?;
        }
        let reviewed = Reviewed::compose(root, prepared.attempt)?;
        reviewed.needs_predecessor(prepared.precondition.as_ref())?;
        reviewed.must_have_passed(
            "an unresolved point is not removed on an exhausted review. Revise it or raise the ceiling — it is still here either way",
        )?;
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
