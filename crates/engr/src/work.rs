//! Bounded execution memory an agent keeps for one Object or one Backlog item.
//!
//! Work answers "where does execution currently stand", and nothing else. It is
//! agent-managed, non-authoritative, and deliberately outside the confirmation
//! gate — an agent writes here directly, the way it writes Backlog, because
//! asking a human to confirm a checkpoint would make checkpoints not worth
//! writing.
//!
//! What keeps that safe is that Work owns no authority. It is a sidecar on its
//! owner, not a resource: there is no `engr:work:` reference, nothing points at
//! it, and finishing every item it lists changes nothing about what its owner
//! says. Stable conclusions reach the record through the applicable Human or
//! reviewed Agent admission path. Work is what the next agent reads first and
//! trusts least.
//!
//! That question — where does execution stand — is the same one whether the
//! execution started from an unresolved Backlog point or from a durable Object,
//! so the owner follows the thing being worked on while authority and identity
//! stay with the owner. #63 is where that was decided. What the second owner
//! kind costs is a lifetime rule, because the two are not alike in one respect:
//! **an Object is never removed, and a Backlog item is removed by being
//! resolved**. See [`Owner`] and the removal guard in
//! [`crate::backlog::consume_section`].

use crate::reference::{canonical_embedded, EngrTarget, ResourceKind};
use crate::rules::Attempt;
use crate::{
    ensure, store, tool_error, Error, Result, EXIT_INVARIANT, EXIT_NOT_FOUND, EXIT_SCHEMA,
    EXIT_USAGE,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DIR: &str = "work";

/// Terse on purpose, and the limits are validation rather than advice.
///
/// Each one is sized to the field's job: a checkpoint is a few short sentences,
/// an item is one action, a result is one outcome, a reason is why one thing is
/// waiting. Text that will not fit is text that belongs somewhere else — the
/// unresolved part in Backlog, the settled part in the Object — and a limit is
/// what makes an agent choose rather than append.
pub const SUMMARY_MAX: usize = 300;
pub const ITEM_TEXT_MAX: usize = 160;
pub const ITEM_RESULT_MAX: usize = 240;
pub const REASON_MAX: usize = 200;

/// What a sidecar belongs to.
///
/// Two kinds, and they are one enum rather than two designs because they differ
/// in exactly one respect: **an Object cannot be removed, and a Backlog item is
/// meant to be**. Everything else about a sidecar — its shape, its limits, its
/// Rule domain, what it may depend on — is the same either way, so the owner is
/// a parameter here, and the difference is a lifetime rule over in
/// [`crate::backlog`].
///
/// The owner is not a member of the stored file. It is the directory the file is
/// in plus the file's own name, which is what keeps Work from acquiring an
/// identity of its own: there is no `engr:work:`, nothing points at a sidecar,
/// and the only way to reach one is to name the thing it belongs to.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Owner {
    Object(String),
    Backlog(String),
}

/// One owner kind: the directory its sidecars live in, and how to name one.
type Kind = (&'static str, fn(String) -> Owner);

/// Both owner kinds, in the order everything that enumerates them uses.
///
/// One list, so a directory cannot join the domain in one place and be missed in
/// another — which is how the released-v1 floor came to cover half of Work.
const OWNERS: &[Kind] = &[("objects", Owner::Object), ("backlog", Owner::Backlog)];

impl Owner {
    pub fn id(&self) -> &str {
        match self {
            Self::Object(id) | Self::Backlog(id) => id,
        }
    }

    pub fn kind(&self) -> ResourceKind {
        match self {
            Self::Object(_) => ResourceKind::Object,
            Self::Backlog(_) => ResourceKind::Backlog,
        }
    }

    pub(crate) fn folder(&self) -> &'static str {
        match self {
            Self::Object(_) => "objects",
            Self::Backlog(_) => "backlog",
        }
    }

    /// What the owner is called in prose, where a reference would read as noise.
    pub fn noun(&self) -> &'static str {
        match self {
            Self::Object(_) => "object",
            Self::Backlog(_) => "backlog item",
        }
    }

    /// The owner in the spelling a caller can retype.
    ///
    /// Diagnostics name owners this way rather than by bare id, because a bare
    /// id no longer says which namespace it is in, and a message that leaves the
    /// reader to guess between two resources is the kind of thing #61 was about.
    pub fn reference(&self) -> String {
        let token = self.kind().token();
        match crate::reference::encode_uuid_str(self.id()) {
            Ok(compact) => format!("engr:{token}:{compact}"),
            // Reachable only for a stored id that is not a UUID at all, which
            // every load path refuses. The raw id beats nothing here, because
            // the message is about that id being wrong.
            Err(_) => format!("engr:{token}:{}", self.id()),
        }
    }
}

impl std::fmt::Display for Owner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.reference())
    }
}

/// The whole domain, both owner kinds.
///
/// Separate from [`dir`] because a caller asking "does this workspace hold any
/// Work at all" must not be handed one half of the answer. The released-v1
/// migration floor asks exactly that.
pub fn root_dir(root: &Path) -> PathBuf {
    store::engr_dir(root).join(DIR)
}

pub fn dir(root: &Path, owner: &Owner) -> PathBuf {
    root_dir(root).join(owner.folder())
}

/// Every directory this domain stores sidecars in.
pub fn dirs(root: &Path) -> Vec<PathBuf> {
    OWNERS
        .iter()
        .map(|(folder, _)| root_dir(root).join(folder))
        .collect()
}

pub fn path(root: &Path, owner: &Owner) -> PathBuf {
    dir(root, owner).join(format!("{}.json", owner.id()))
}

/// Whether a sidecar file is there at all, without asking whether it loads.
///
/// The question a removal guard has to ask: an unreadable sidecar is still a
/// sidecar, and letting a broken one through would make corruption the way past
/// the invariant. `symlink_metadata`, so a dangling link counts as present for
/// the same reason it does in the migration floor.
///
/// Every "is there one" in this module goes through here, so a sidecar cannot be
/// absent to one caller and present to another — which is what `exists()` and
/// `symlink_metadata` would otherwise disagree about for a broken link.
pub fn exists(root: &Path, owner: &Owner) -> bool {
    std::fs::symlink_metadata(path(root, owner)).is_ok()
}

/// Whether agents may keep going on their own.
///
/// Two states, and the second one is not an observation. See [`Work::state`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Active,
    Paused,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
        }
    }
}

/// How far one step has got.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum ItemState {
    Pending,
    Active,
    Done,
}

impl ItemState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Done => "done",
        }
    }
}

/// Something this execution relies on.
///
/// The target is required because a dependency without one says nothing an
/// agent can act on. The reason is optional and should say why the target
/// matters *here* — repeating the target's title is worse than leaving it out.
///
/// A dependency is operational, and stays operational. If work establishes that
/// it is a stable engineering fact, that conclusion enters the Object through
/// the confirmation path; nothing here is ever promoted automatically.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    pub target: EngrTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// A condition currently preventing useful progress.
///
/// Both fields are optional and at least one must be present, which is the
/// difference from a dependency: real execution is blocked by things that are
/// not engr resources — an approval, an environment, a vendor — and a blocker
/// that could only be written as a graph edge would not be written at all.
///
/// Kept separate from `dependencies` deliberately. The same target can be both,
/// and when the blocking condition clears the dependency remains true.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Blocker {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<EngrTarget>,
}

/// One step of the current decomposition.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub id: u64,
    pub text: String,
    pub state: ItemState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// Full Git object ids, as navigation and evidence — never as an integrity
    /// anchor. A commit that a rebase made unreachable is a dead signpost, not a
    /// corrupt sidecar, and an item can be done with no commit at all.
    ///
    /// Required, and may be empty, for the same reason the lists above are.
    pub commits: Vec<String>,
}

/// The sidecar itself.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Work {
    /// `active` or `paused`, and nothing else.
    ///
    /// There is no `blocked`: that is derived from `blockers`, and storing it
    /// would let the two disagree. There is no `done` either, and that absence
    /// is load-bearing — a completed sidecar must not become a second answer to
    /// "is this settled" alongside its owner's own state.
    ///
    /// `paused` is a **human-directed** stop signal. An agent must not set it
    /// because a session is ending, because nothing is currently actionable, or
    /// because it judged the work should wait; must not clear it without being
    /// told to; and must not delete a paused sidecar without being told to.
    ///
    /// All of that is normative on the agent and **none of it is enforced**.
    /// engr cannot tell an agent from a human, so this is the same kind of rule
    /// as the gate itself, and it fails the same way — quietly. What the tool
    /// does is refuse to let the signal disappear unremarked: see [`remove`].
    pub state: State,
    /// The shortest useful checkpoint for whoever picks this up next.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub updated_at: String,
    /// Item ids are allocated from here and never reused, so "work item 3" in a
    /// handoff note or a conversation keeps meaning the same step even after
    /// item 3 is pruned.
    pub next_item_id: u64,
    /// Required, and may be empty. Not `#[serde(default)]`: an omitted list and
    /// an empty one would be the same sidecar written two ways, and a stored
    /// shape the write path can never produce is a shadow schema that only ever
    /// gets discovered by something depending on it.
    pub dependencies: Vec<Dependency>,
    pub blockers: Vec<Blocker>,
    pub items: Vec<Item>,
}

impl Work {
    fn new() -> Self {
        Self {
            state: State::Active,
            summary: None,
            updated_at: now(),
            next_item_id: 1,
            dependencies: Vec::new(),
            blockers: Vec::new(),
            items: Vec::new(),
        }
    }

    /// Blocked is a reading of the sidecar, not a field in it.
    pub fn is_blocked(&self) -> bool {
        self.state == State::Active && !self.blockers.is_empty()
    }

    /// What `engr work ls` prints in the state column.
    pub fn standing(&self) -> &'static str {
        match (self.state, self.is_blocked()) {
            (State::Paused, _) => "paused",
            (State::Active, true) => "blocked",
            (State::Active, false) => "active",
        }
    }

    pub fn item(&self, id: u64) -> Result<&Item> {
        self.items
            .iter()
            .find(|item| item.id == id)
            .ok_or_else(|| Error::new(EXIT_NOT_FOUND, format!("work item {id} does not exist")))
    }

    fn item_mut(&mut self, id: u64) -> Result<&mut Item> {
        self.items
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or_else(|| Error::new(EXIT_NOT_FOUND, format!("work item {id} does not exist")))
    }

    fn take_id(&mut self) -> Result<u64> {
        let id = self.next_item_id;
        ensure!(
            id < crate::proof::MAX_SAFE_INTEGER,
            EXIT_USAGE,
            "this sidecar has no remaining item ids in the shared safe-integer domain"
        );
        self.next_item_id = self
            .next_item_id
            .checked_add(1)
            .ok_or_else(|| Error::new(EXIT_INVARIANT, "this sidecar has no remaining item ids"))?;
        Ok(id)
    }

    /// Held to the same rules whether it was just written or read off disk.
    ///
    /// A check that only runs on the way in stops being true after one hand
    /// edit, and these files are meant to be read and diffed like any other
    /// tracked file.
    pub fn validate(&self) -> Result<()> {
        let value = serde_json::to_value(self)
            .map_err(|error| Error::new(EXIT_SCHEMA, format!("work: {error}")))?;
        crate::proof::stored_within_safe_integers(&value, "work")?;
        ensure!(
            self.next_item_id >= 1,
            EXIT_SCHEMA,
            "next_item_id must be positive"
        );
        // Every fault here is a fault in *stored* data, so they all read as
        // schema rather than usage. A caller who typed something too long is
        // refused earlier, by the same rule with the other exit code — a file
        // this build cannot accept is not the current caller's mistake.
        ensure!(
            crate::backlog::instant(&self.updated_at).is_some(),
            EXIT_SCHEMA,
            "updated_at {:?} is not an RFC3339 timestamp",
            self.updated_at
        );
        if let Some(summary) = &self.summary {
            bounded(EXIT_SCHEMA, "summary", summary, SUMMARY_MAX)?;
        }
        let mut targets = std::collections::BTreeSet::new();
        for dependency in &self.dependencies {
            check_target("a work dependency", &dependency.target.reference)?;
            // The write path refuses a second dependency on the same target, so
            // a stored pair of them is a shape it could not have produced.
            ensure!(
                targets.insert(dependency.target.reference.as_str()),
                EXIT_SCHEMA,
                "{} is a dependency twice, and one prerequisite is one dependency",
                dependency.target.reference
            );
            if let Some(reason) = &dependency.reason {
                bounded(EXIT_SCHEMA, "a dependency reason", reason, REASON_MAX)?;
            }
        }
        for blocker in &self.blockers {
            ensure!(
                blocker.reason.is_some() || blocker.target.is_some(),
                EXIT_SCHEMA,
                "a blocker needs a reason, a target, or both; an empty one says nothing"
            );
            if let Some(target) = &blocker.target {
                check_target("a work blocker", &target.reference)?;
            }
            if let Some(reason) = &blocker.reason {
                bounded(EXIT_SCHEMA, "a blocker reason", reason, REASON_MAX)?;
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for item in &self.items {
            ensure!(item.id >= 1, EXIT_SCHEMA, "work item ids must be positive");
            bounded(EXIT_SCHEMA, "a work item", &item.text, ITEM_TEXT_MAX)?;
            if let Some(result) = &item.result {
                bounded(EXIT_SCHEMA, "a work item result", result, ITEM_RESULT_MAX)?;
            }
            let mut commits = std::collections::BTreeSet::new();
            for commit in &item.commits {
                crate::semantics::validate_pinned_commit("a work item commit", commit)?;
                ensure!(
                    commits.insert(commit.as_str()),
                    EXIT_SCHEMA,
                    "work item {} records {commit} twice",
                    item.id
                );
            }
            ensure!(
                item.id < self.next_item_id,
                EXIT_SCHEMA,
                "work item {} was never allocated: next_item_id is {}",
                item.id,
                self.next_item_id
            );
            ensure!(
                seen.insert(item.id),
                EXIT_SCHEMA,
                "work item {} appears twice, and an id is never reused",
                item.id
            );
        }
        Ok(())
    }

    /// When this was last touched, as an instant rather than as text.
    ///
    /// Two valid RFC3339 values written in different offsets do not compare
    /// correctly as strings, so anything that orders sidecars has to parse them.
    /// Validation guarantees this parses; the stored spelling is what gets
    /// displayed, so nothing here rewrites what an earlier writer chose.
    pub fn updated_at(&self) -> time::OffsetDateTime {
        crate::backlog::instant(&self.updated_at)
            .expect("loading validates the timestamp, so a loaded Work always parses")
    }
}

/// What a dependency or a blocker is allowed to point at.
///
/// Whole Objects and whole Backlog items. Not a section, not a file, not a
/// symbol, not a Collection, and not another sidecar — the finer the target,
/// the more it reads like the record's own `refs[]`, which pins wording and
/// carries authority. Operational context that looked like that would end up
/// being treated like it.
pub fn check_target(what: &str, reference: &str) -> Result<()> {
    let canonical = canonical_embedded(
        reference,
        &[ResourceKind::Object, ResourceKind::Backlog],
        what,
    )?;
    ensure!(
        canonical.section().is_none(),
        EXIT_SCHEMA,
        "{what} names a whole Object or Backlog item; a section is the record's kind of \
         dependency, not this one"
    );
    Ok(())
}

/// Every Work text field lands here, so one rule covers all of them.
///
/// The exit code is the caller's, because the same rule answers two different
/// questions. A caller writing something too long made a usage mistake; a file
/// on disk holding something too long is a schema fault, and reporting that as
/// usage would blame whoever happened to run the next command.
fn bounded(code: i32, what: &str, text: &str, limit: usize) -> Result<()> {
    ensure!(!text.trim().is_empty(), code, "{what} needs text");
    let length = text.chars().count();
    ensure!(
        length <= limit,
        code,
        "{what} is a handoff note, not the work itself ({length} characters, limit \
         {limit}). Keep the part the next agent needs; unresolved reasoning belongs \
         in `engr backlog`, and settled knowledge belongs in the Object."
    );
    Ok(())
}

/// The same rule, at the moment a caller supplies the text.
fn check_text(what: &str, text: &str, limit: usize) -> Result<()> {
    bounded(EXIT_USAGE, what, text, limit)
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("formatting a timestamp cannot fail")
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Every owner that currently has a sidecar.
///
/// Deterministic across both kinds: the kinds in [`OWNERS`] order, each sorted.
/// Migration plans this sequence into a manifest, so "whatever the filesystem
/// hands back" is not good enough.
pub fn ids(root: &Path) -> Result<Vec<Owner>> {
    let mut found = Vec::new();
    for (folder, make) in OWNERS {
        let dir = root_dir(root).join(folder);
        if !dir.is_dir() {
            continue;
        }
        let mut here = Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(|error| tool_error(dir.display(), error))? {
            let entry = entry.map_err(|error| tool_error(dir.display(), error))?;
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(id) = name.strip_suffix(".json") {
                here.push(id.to_owned());
            }
        }
        here.sort();
        found.extend(here.into_iter().map(make));
    }
    Ok(found)
}

pub fn load(root: &Path, owner: &Owner) -> Result<Work> {
    let path = path(root, owner);
    ensure!(
        exists(root, owner),
        EXIT_NOT_FOUND,
        "no work recorded for {} {}",
        owner.noun(),
        owner
    );
    let work: Work = store::read_resource(root, &path)?;
    work.validate()?;
    if store::validate_format(root)? == store::WorkspaceFormat::Current {
        check_canonical_work(&path, &work)?;
    }
    // The owner invariant, held on the way *out* as well as on the way in. A
    // sidecar names its owner in its path, so a copied file can name one that
    // never existed — and a check that only runs on the write path would let
    // this build read, list and hand back operational memory for nothing.
    // `is_ok()` used to answer this, which collapsed "the owner is not there"
    // and "the owner will not load" into one sentence — and the sentence it
    // chose was the wrong one, sending a reader to create a record that is
    // already on disk while hiding the fault that actually needs looking at.
    // Unreadable authority is not absence, on this path as on every other.
    //
    // Reachable for a Backlog owner only through a fault: the removal guard in
    // `backlog::consume_section` is what keeps an ordinary resolution from
    // producing one. This is the second half of that invariant, and it is the
    // half that catches a hand-edited workspace.
    if let Err(error) = require_owner(root, owner) {
        return Err(if error.code == EXIT_NOT_FOUND {
            Error::new(
                EXIT_SCHEMA,
                format!(
                    "{} belongs to nothing: a sidecar is owned by the resource it names, and {} {owner} does not exist",
                    path.display(),
                    owner.noun()
                ),
            )
        } else {
            Error::new(
                error.code,
                format!(
                    "{} belongs to {} {owner}, which cannot be read: {}",
                    path.display(),
                    owner.noun(),
                    error.message
                ),
            )
        });
    }
    Ok(work)
}

/// Decode predecessor bytes already captured by coordinated migration.
pub(crate) fn decode_for_migration(path: &Path, owner: &Owner, text: &str) -> Result<Work> {
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))?;
    crate::proof::stored_within_safe_integers(&value, &path.display().to_string())?;
    let work: Work = serde_json::from_value(value)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))?;
    work.validate()?;
    // The path is the sidecar owner. `Work` does not duplicate that id, so the
    // caller supplies it and validates ownership against its plan. Both owner
    // kinds are UUIDv7, so one validator answers for both.
    crate::model::validate_object_id(owner.id()).map_err(|error| {
        Error::new(
            EXIT_SCHEMA,
            format!(
                "{}: work sidecar owner {:?} is invalid: {}",
                path.display(),
                owner.id(),
                error.message
            ),
        )
    })?;
    Ok(work)
}

/// Validate a staged Work artifact as a current resource before publication.
pub(crate) fn decode_current_staged(path: &Path, owner: &Owner, text: &str) -> Result<Work> {
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))?;
    store::check_canonical_bytes(path, text, &value)?;
    let work = decode_for_migration(path, owner, text)?;
    store::check_current_resource_shape(path, text, &work)?;
    check_canonical_work(path, &work)?;
    Ok(work)
}

/// The current writer's one representation for each Work collection.
pub(crate) fn canonicalize_work(work: &mut Work) -> Result<()> {
    crate::proof::canonical_set(&mut work.dependencies, "work dependency")?;
    work.items.sort_by_key(|item| item.id);
    for item in &mut work.items {
        crate::proof::canonical_set(&mut item.commits, "work item commit")?;
    }
    Ok(())
}

fn check_canonical_work(path: &Path, work: &Work) -> Result<()> {
    let mut canonical = work.clone();
    canonicalize_work(&mut canonical)?;
    ensure!(
        canonical == *work,
        EXIT_SCHEMA,
        "{}: Work collections are stored in their canonical order",
        path.display()
    );
    Ok(())
}

/// The sidecar if there is one, and no complaint if there is not.
///
/// Absence means only that engr holds no operational memory for this owner —
/// the ordinary state of most owners, and never an error.
pub fn find(root: &Path, owner: &Owner) -> Result<Option<Work>> {
    if !exists(root, owner) {
        return Ok(None);
    }
    load(root, owner).map(Some)
}

fn save(root: &Path, owner: &Owner, work: &Work) -> Result<()> {
    work.validate()?;
    let mut work = work.clone();
    canonicalize_work(&mut work)?;
    store::write_json(&path(root, owner), &work)
}

/// Every Work mutation, under the lock, past the applicable Rule set.
///
/// The Rule check is inside the lock and before the read, so what it was
/// established against is what gets written. Work has no prepared candidate to
/// bind, so the attempt is the whole of what a caller attests.
fn locked<T>(root: &Path, attempt: Attempt, body: impl FnOnce() -> Result<T>) -> Result<T> {
    store::require_current(root)?;
    store::with_lock(root, || {
        store::require_current(root)?;
        crate::rules::direct(root, crate::rules::Domain::Work, attempt)?;
        body()
    })
}

/// A sidecar belongs to an owner that exists.
///
/// Checked on every write rather than only at creation: a hand-copied file can
/// name anything, and a sidecar for nothing is operational memory nobody will
/// ever read.
///
/// The two owner kinds reach the same question through different doors. An
/// Object's existence is its admitted history — a missing projection is a
/// repairable fault and not an absence, which is why this asks
/// [`crate::ops::effective`] rather than the objects directory. A Backlog item
/// has no history behind it: the file is the item, so its absence is the whole
/// answer.
fn require_owner(root: &Path, owner: &Owner) -> Result<()> {
    match owner {
        Owner::Object(id) => crate::ops::effective(root, id).map(|_| ()),
        Owner::Backlog(id) => crate::backlog::load(root, id).map(|_| ()),
    }
}

/// A dependency or blocker may name only something that exists **now**.
///
/// Checked here rather than at the command line, so the rule holds whichever
/// door a caller comes through. It is a write-time rule and only a write-time
/// rule: a Backlog item gets consumed, a rebase strands an Object, and Work
/// deliberately keeps the dangling note rather than rewriting history nobody
/// asked it to rewrite. What it will not do is admit a target that was already
/// gone when it was written, which is a note pointing at nothing from birth.
fn require_target(root: &Path, target: &str) -> Result<()> {
    let parsed = crate::reference::EngrRef::parse_embedded(target)?;
    let uuid = crate::reference::decode_uuid(parsed.id())?.to_string();
    let outcome = match parsed.kind() {
        ResourceKind::Backlog => crate::backlog::load(root, &uuid).map(|_| ()),
        _ => crate::ops::effective(root, &uuid).map(|_| ()),
    };
    // Absence and unreadable authority stay apart. "Does not exist" would send
    // someone to create what is already there and hide the fault that needs
    // looking at.
    match outcome {
        Ok(()) => Ok(()),
        Err(error) if error.code == EXIT_NOT_FOUND => Err(Error::new(
            EXIT_NOT_FOUND,
            format!("{target} does not exist"),
        )),
        Err(error) => Err(Error::new(
            error.code,
            format!("{target} cannot be read: {}", error.message),
        )),
    }
}

/// Read, change, stamp, write — under the lock, with the invariants checked on
/// both sides of the change.
fn edit<T>(
    root: &Path,
    owner: &Owner,
    attempt: Attempt,
    body: impl FnOnce(&mut Work) -> Result<T>,
) -> Result<T> {
    locked(root, attempt, || {
        require_owner(root, owner)?;
        let mut work = load(root, owner)?;
        let outcome = body(&mut work)?;
        work.updated_at = now();
        save(root, owner, &work)?;
        Ok(outcome)
    })
}

/// Begin keeping execution memory for an Object or a Backlog item.
pub fn start(root: &Path, owner: &Owner, summary: Option<&str>, attempt: Attempt) -> Result<Work> {
    if let Some(summary) = summary {
        check_text("summary", summary, SUMMARY_MAX)?;
    }
    locked(root, attempt, || {
        require_owner(root, owner)?;
        ensure!(
            !exists(root, owner),
            EXIT_INVARIANT,
            "{} {owner} already has work recorded; change it rather than starting again",
            owner.noun()
        );
        let mut work = Work::new();
        if let Some(summary) = summary {
            work.summary = Some(summary.to_owned());
        }
        save(root, owner, &work)?;
        Ok(work)
    })
}

/// Forget the execution memory for one owner.
///
/// Deleting says nothing about the owner; the record is untouched. It is also
/// the only way to clear the way for a Backlog item to be resolved, which is
/// what makes this the deliberate step the invariant asks for rather than a
/// cascade nobody sees.
///
/// A `paused` sidecar is deleted too, and that is deliberate. #12 makes "an
/// agent MUST NOT delete a paused WorkObject without explicit human direction" a
/// **normative agent rule**, not a confirmation-gate mutation — and refusing
/// here would be neither. It would not stop a non-compliant agent, which can
/// call [`set_state`] and then this; it would make a human's own "delete that"
/// impossible to carry out directly; and it would invent a persisted
/// `paused -> active` step whose only purpose is to satisfy the refusal, with a
/// window where a crash has already discarded the stop signal.
///
/// So the rule stays where #12 put it, on the agent, stated in the protocol and
/// the Skill. Whether human direction should have a mechanical representation at
/// all is a real question and an open one; see `## What v0 does not solve`.
/// [`Removed`] reports what was discarded so a caller can say so.
pub fn remove(root: &Path, owner: &Owner, attempt: Attempt) -> Result<Removed> {
    locked(root, attempt, || {
        let work = load(root, owner)?;
        let path = path(root, owner);
        std::fs::remove_file(&path).map_err(|error| tool_error(path.display(), error))?;
        Ok(Removed {
            was_paused: work.state == State::Paused,
        })
    })
}

/// What deleting a sidecar threw away that a reader should hear about.
#[derive(Debug)]
pub struct Removed {
    /// A human had stopped this. Deletion is still carried out — engr cannot
    /// tell who asked — but a screen that said nothing would let the signal
    /// disappear silently, which is the part worth reporting.
    pub was_paused: bool,
}

/// Both directions of the human-directed stop signal.
///
/// Named for what a human says rather than for a field assignment, because the
/// rule an agent has to follow is about who decided, not about which value is
/// stored. engr cannot check that; the Skill is where it is stated, and the
/// refusal in [`remove`] is where it bites.
pub fn set_state(root: &Path, owner: &Owner, state: State, attempt: Attempt) -> Result<Work> {
    edit(root, owner, attempt, |work| {
        work.state = state;
        Ok(())
    })?;
    load(root, owner)
}

pub fn set_summary(
    root: &Path,
    owner: &Owner,
    summary: Option<&str>,
    attempt: Attempt,
) -> Result<Work> {
    if let Some(summary) = summary {
        check_text("summary", summary, SUMMARY_MAX)?;
    }
    edit(root, owner, attempt, |work| {
        work.summary = summary.map(str::to_owned);
        Ok(())
    })?;
    load(root, owner)
}

pub fn add_dependency(
    root: &Path,
    owner: &Owner,
    target: &str,
    reason: Option<&str>,
    attempt: Attempt,
) -> Result<Work> {
    if let Some(reason) = reason {
        check_text("a dependency reason", reason, REASON_MAX)?;
    }
    check_target("a dependency target", target)?;
    edit(root, owner, attempt, |work| {
        // Inside the lock, because the check is what defines admission. It used
        // to live only in the CLI, so what a sidecar could name depended on
        // which door it came through — the same split that was fixed for
        // collection membership. A target that goes missing *afterwards* is
        // still expected and still reported on read; one that was never there
        // is a different thing, and only a shared critical section tells them
        // apart.
        require_target(root, target)?;
        let dependency = Dependency {
            target: EngrTarget::new(target),
            reason: reason.map(str::to_owned),
        };
        ensure!(
            !work
                .dependencies
                .iter()
                .any(|held| held.target == dependency.target),
            EXIT_INVARIANT,
            "{target} is already a dependency of this work"
        );
        work.dependencies.push(dependency);
        Ok(())
    })?;
    load(root, owner)
}

pub fn remove_dependency(
    root: &Path,
    owner: &Owner,
    target: &str,
    attempt: Attempt,
) -> Result<Work> {
    edit(root, owner, attempt, |work| {
        let before = work.dependencies.len();
        work.dependencies
            .retain(|held| held.target.reference != target);
        ensure!(
            work.dependencies.len() != before,
            EXIT_NOT_FOUND,
            "{target} is not a dependency of this work"
        );
        Ok(())
    })?;
    load(root, owner)
}

pub fn add_blocker(
    root: &Path,
    owner: &Owner,
    reason: Option<&str>,
    target: Option<&str>,
    attempt: Attempt,
) -> Result<Work> {
    if let Some(reason) = reason {
        check_text("a blocker reason", reason, REASON_MAX)?;
    }
    if let Some(target) = target {
        check_target("a blocker target", target)?;
    }
    edit(root, owner, attempt, |work| {
        if let Some(target) = target {
            require_target(root, target)?;
        }
        work.blockers.push(Blocker {
            reason: reason.map(str::to_owned),
            target: target.map(EngrTarget::new),
        });
        Ok(())
    })?;
    load(root, owner)
}

/// Blockers are addressed by position, because they have no ids — they are
/// conditions rather than things, and a condition that cleared is simply gone.
pub fn remove_blocker(root: &Path, owner: &Owner, index: usize, attempt: Attempt) -> Result<Work> {
    edit(root, owner, attempt, |work| {
        ensure!(
            index < work.blockers.len(),
            EXIT_NOT_FOUND,
            "there is no blocker {index}; this work has {}",
            work.blockers.len()
        );
        work.blockers.remove(index);
        Ok(())
    })?;
    load(root, owner)
}

pub fn add_item(root: &Path, owner: &Owner, text: &str, attempt: Attempt) -> Result<u64> {
    check_text("a work item", text, ITEM_TEXT_MAX)?;
    edit(root, owner, attempt, |work| {
        let id = work.take_id()?;
        work.items.push(Item {
            id,
            text: text.to_owned(),
            state: ItemState::Pending,
            result: None,
            commits: Vec::new(),
        });
        Ok(id)
    })
}

pub fn set_item_state(
    root: &Path,
    owner: &Owner,
    id: u64,
    state: ItemState,
    attempt: Attempt,
) -> Result<Work> {
    edit(root, owner, attempt, |work| {
        work.item_mut(id)?.state = state;
        Ok(())
    })?;
    load(root, owner)
}

pub fn set_item_text(
    root: &Path,
    owner: &Owner,
    id: u64,
    text: &str,
    attempt: Attempt,
) -> Result<Work> {
    check_text("a work item", text, ITEM_TEXT_MAX)?;
    edit(root, owner, attempt, |work| {
        work.item_mut(id)?.text = text.to_owned();
        Ok(())
    })?;
    load(root, owner)
}

pub fn set_item_result(
    root: &Path,
    owner: &Owner,
    id: u64,
    result: Option<&str>,
    attempt: Attempt,
) -> Result<Work> {
    if let Some(result) = result {
        check_text("a work item result", result, ITEM_RESULT_MAX)?;
    }
    edit(root, owner, attempt, |work| {
        work.item_mut(id)?.result = result.map(str::to_owned);
        Ok(())
    })?;
    load(root, owner)
}

pub fn add_item_commit(
    root: &Path,
    owner: &Owner,
    id: u64,
    commit: &str,
    attempt: Attempt,
) -> Result<Work> {
    edit(root, owner, attempt, |work| {
        let item = work.item_mut(id)?;
        ensure!(
            !item.commits.iter().any(|held| held == commit),
            EXIT_INVARIANT,
            "work item {id} already records {commit}"
        );
        item.commits.push(commit.to_owned());
        Ok(())
    })?;
    load(root, owner)
}

/// Prune one item. Its id is not reclaimed, and nothing is archived: git holds
/// what the sidecar used to say.
pub fn remove_item(root: &Path, owner: &Owner, id: u64, attempt: Attempt) -> Result<Work> {
    edit(root, owner, attempt, |work| {
        work.item(id)?;
        work.items.retain(|item| item.id != id);
        Ok(())
    })?;
    load(root, owner)
}
