//! Planning metadata: which work is grouped together, and in what order.
//!
//! A collection groups Objects and Backlog items so a plan can be read in one
//! place. Like Backlog and Work it is agent-managed and outside the gate; unlike
//! either, it is a resource of its own with a stable id that other things can
//! name.
//!
//! It changes nothing about what it contains. Moving an Object between
//! collections, ranking it, or calling a plan complete is planning activity —
//! the Object means exactly what its admitted Sections say either way. That is
//! the whole trust boundary, and every rule here exists to keep it: membership
//! carries no authority, priority belongs to the membership rather than to the
//! target, and completing a plan is a declaration rather than a proof.

use crate::reference::{canonical_embedded, EngrTarget, ResourceKind};
use crate::rules::Attempt;
use crate::{
    ensure, store, tool_error, Error, Result, EXIT_INVARIANT, EXIT_NOT_FOUND, EXIT_SCHEMA,
    EXIT_USAGE,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DIR: &str = "collections";

pub fn dir(root: &Path) -> PathBuf {
    store::engr_dir(root).join(DIR)
}

pub fn path(root: &Path, id: &str) -> PathBuf {
    dir(root).join(format!("{id}.json"))
}

/// Where a plan stands, as its planner declares it.
///
/// Never inferred — not from dates, not from what the members are doing.
/// `completed` and `cancelled` are deliberately distinct: one plan reached the
/// end it was aiming at, the other stopped being pursued, and flattening them
/// would lose the only thing anyone wants to know later.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Open,
    Completed,
    Cancelled,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// How important a member is **to this plan**.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Low,
    Normal,
    High,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::High => "high",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Priority {
    pub level: Level,
    /// Why this member has this priority *in this collection*. Planning
    /// rationale, never engineering rationale — that belongs in the Object,
    /// through the gate. Optional, and absence does not make a priority
    /// incomplete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Optional time context for a plan.
///
/// Generic on purpose: with no collection type there is nothing to make the
/// shape depend on. Nothing here changes any state anywhere — "overdue" is a
/// question a reader asks, not a value engr stores.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    /// The point the plan is aiming at, which need not sit between the other
    /// two — a target before the end is an intention, not a contradiction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

impl Schedule {
    fn is_empty(&self) -> bool {
        self.start.is_none() && self.end.is_none() && self.target.is_none()
    }

    fn validate(&self, code: i32) -> Result<()> {
        ensure!(
            !self.is_empty(),
            code,
            "a schedule needs at least one of start, end or target; an empty one says nothing"
        );
        for (what, value) in [
            ("start", &self.start),
            ("end", &self.end),
            ("target", &self.target),
        ] {
            if let Some(value) = value {
                check_date(code, what, value)?;
            }
        }
        if let (Some(start), Some(end)) = (&self.start, &self.end) {
            ensure!(
                date(start)? <= date(end)?,
                code,
                "a schedule that starts on {start} cannot end on {end}"
            );
        }
        Ok(())
    }
}

/// One thing this plan covers.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub target: EngrTarget,
    /// Intended sequencing within this collection. Absent means **unranked**,
    /// which is a real answer rather than a gap: a partly ordered plan is the
    /// normal state of a plan, and array position is not an answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
}

/// The plan itself.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Collection {
    /// Ten Crockford Base32 characters, stable and independent of the name.
    /// Renaming a plan does not make it a different plan, and nothing about the
    /// id says what the plan is — no milestone number, no date, no type.
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub state: State,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<Schedule>,
    /// Required, and may be empty. Not `#[serde(default)]`: an omitted list and
    /// an empty one would be one plan written two ways, and a stored shape the
    /// write path cannot produce is a second schema waiting to be depended on.
    pub members: Vec<Member>,
}

impl Collection {
    /// Members in the order a plan is meant to be read: ranked ones first by
    /// rank, then the unranked, which stay explicitly unranked rather than
    /// being given a position by where they happen to sit in the file.
    pub fn planned(&self) -> Vec<&Member> {
        let mut ranked: Vec<&Member> = self
            .members
            .iter()
            .filter(|member| member.order.is_some())
            .collect();
        ranked.sort_by_key(|member| member.order);
        ranked.extend(self.members.iter().filter(|member| member.order.is_none()));
        ranked
    }

    pub fn member(&self, target: &str) -> Result<&Member> {
        self.members
            .iter()
            .find(|member| member.target.reference == target)
            .ok_or_else(|| {
                Error::new(
                    EXIT_NOT_FOUND,
                    format!("{target} is not a member of this collection"),
                )
            })
    }

    fn member_mut(&mut self, target: &str) -> Result<&mut Member> {
        self.members
            .iter_mut()
            .find(|member| member.target.reference == target)
            .ok_or_else(|| {
                Error::new(
                    EXIT_NOT_FOUND,
                    format!("{target} is not a member of this collection"),
                )
            })
    }

    /// Held to the same rules whether it was just written or read off disk.
    ///
    /// Every fault here is a fault in *stored* data, so they read as schema
    /// rather than usage: a caller who typed something invalid is refused
    /// earlier by the same rule with the other exit code, and a file nobody
    /// currently running a command wrote is not their mistake.
    pub fn validate(&self) -> Result<()> {
        let value = serde_json::to_value(self)
            .map_err(|error| Error::new(EXIT_SCHEMA, format!("collection: {error}")))?;
        crate::proof::stored_within_safe_integers(&value, "collection")?;
        crate::reference::canonical_embedded(
            &format!("collection:{}", self.id),
            &[ResourceKind::Collection],
            "a collection id",
        )?;
        check_stored_name(&self.name)?;
        if let Some(description) = &self.description {
            ensure!(
                !description.trim().is_empty(),
                EXIT_SCHEMA,
                "a description that is present says something; leave it out instead"
            );
        }
        if let Some(schedule) = &self.schedule {
            schedule.validate(EXIT_SCHEMA)?;
        }
        let mut targets = std::collections::BTreeSet::new();
        let mut ranks = std::collections::BTreeSet::new();
        for member in &self.members {
            check_target("a collection member", &member.target.reference)?;
            // One resource is in a plan once. Twice would be two answers to
            // "where does this sit here", with no way to tell which is meant.
            ensure!(
                targets.insert(member.target.reference.as_str()),
                EXIT_SCHEMA,
                "{} is a member twice, and one plan holds it once",
                member.target.reference
            );
            if let Some(order) = member.order {
                // Unranked members may share their absence; a *rank* cannot be
                // shared, or the sequence it exists to express has a tie it
                // cannot break.
                ensure!(
                    ranks.insert(order),
                    EXIT_SCHEMA,
                    "two members are both ranked {order}, so the order says nothing at that point"
                );
            }
            if let Some(priority) = &member.priority {
                if let Some(reason) = &priority.reason {
                    ensure!(
                        !reason.trim().is_empty(),
                        EXIT_SCHEMA,
                        "a priority reason that is present says something; leave it out instead"
                    );
                }
            }
        }
        Ok(())
    }
}

/// An ISO calendar date, and nothing else.
///
/// No time, no offset. A collection carries planning context, not a schedule
/// somebody executes — accepting a timestamp here would invite exactly the
/// precision this field must not claim to have.
const DATE: &[time::format_description::FormatItem<'static>] =
    time::macros::format_description!("[year]-[month]-[day]");

fn date(value: &str) -> Result<time::Date> {
    time::Date::parse(value, DATE).map_err(|_| {
        Error::new(
            EXIT_SCHEMA,
            format!("{value:?} is not a calendar date as YYYY-MM-DD"),
        )
    })
}

fn check_date(code: i32, what: &str, value: &str) -> Result<()> {
    let parsed = date(value).map_err(|error| Error::new(code, error.message))?;
    // Round-tripped, so `2026-8-1` and `2026-08-01` cannot both be stored for
    // the same day: one spelling per date, or comparing them means parsing them
    // everywhere forever.
    let canonical = parsed.format(DATE).unwrap_or_else(|_| value.to_owned());
    ensure!(
        canonical == value,
        code,
        "a schedule {what} is a calendar date as YYYY-MM-DD, so write it as {canonical}"
    );
    Ok(())
}

/// What a plan is called, on the one line a listing gives it.
///
/// #10 sets no length bound for collection text and none is invented here. What
/// is refused is a name that cannot do its job: an empty one, or one carrying a
/// line break, which would break every other row in the listing as well as its
/// own. Detail belongs in `description`, which is unbounded.
fn check_name(code: i32, name: &str) -> Result<()> {
    ensure!(!name.trim().is_empty(), code, "a collection needs a name");
    ensure!(
        !name.contains('\n'),
        code,
        "a collection name is the line a listing prints, so it cannot span lines. \
         Put the detail in --description."
    );
    Ok(())
}

/// The same rule, plus what the write path guarantees about stored names.
///
/// `create` and `rename` trim before storing, so a stored name never carries
/// leading or trailing whitespace — and a reader that accepted one anyway would
/// be accepting a spelling the API cannot produce. Two spellings of `Q3` that
/// only a listing's alignment can tell apart is exactly the shadow schema the
/// other domains were closed against.
fn check_stored_name(name: &str) -> Result<()> {
    check_name(EXIT_SCHEMA, name)?;
    ensure!(
        name.trim() == name,
        EXIT_SCHEMA,
        "a stored collection name carries no surrounding whitespace, so this is not \
         one this build wrote: {name:?}"
    );
    Ok(())
}

/// What a member is allowed to point at.
///
/// Whole Objects and whole Backlog items. Not sections, files, symbols or other
/// collections: a plan groups work, and Phase 1 has no hierarchy.
pub fn check_target(what: &str, reference: &str) -> Result<()> {
    let canonical = canonical_embedded(
        reference,
        &[ResourceKind::Object, ResourceKind::Backlog],
        what,
    )?;
    ensure!(
        canonical.section().is_none(),
        EXIT_SCHEMA,
        "{what} names a whole Object or Backlog item; a plan groups work, not parts of it"
    );
    Ok(())
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

/// The shared canonical order for `members[]`.
///
/// A plan's own text says the order of members in the file is never the plan's
/// order — `--order` is, and it is a field. So `members[]` is a set, and a set
/// has one persisted spelling. Applied on the way out, so a caller adding
/// members in whatever order they thought of them is not refused for it.
pub(crate) fn canonicalize_members(collection: &mut Collection) -> Result<()> {
    crate::proof::canonical_set(&mut collection.members, "collection member")
}

fn check_canonical_members(path: &Path, collection: &Collection) -> Result<()> {
    let mut canonical = collection.clone();
    canonicalize_members(&mut canonical)?;
    ensure!(
        canonical == *collection,
        EXIT_SCHEMA,
        "{}: members are stored in canonical set order",
        path.display()
    );
    Ok(())
}

pub fn load(root: &Path, id: &str) -> Result<Collection> {
    let path = path(root, id);
    ensure!(path.exists(), EXIT_NOT_FOUND, "no collection {id}");
    let collection: Collection = store::read_resource(root, &path)?;
    collection.validate()?;
    // Current generation only: a predecessor workspace kept whatever order its
    // writer produced, and migration is where those bytes come forward.
    if store::validate_format(root)? == store::WorkspaceFormat::Current {
        check_canonical_members(&path, &collection)?;
    }
    // The filename is the identity, so a file whose contents disagree with it
    // is two identities for one plan — and every reference names one of them.
    ensure!(
        collection.id == id,
        EXIT_SCHEMA,
        "{} says it is collection {}, and a plan has one identity",
        path.display(),
        collection.id
    );
    Ok(collection)
}

/// Decode predecessor bytes already captured by coordinated migration.
pub(crate) fn decode_for_migration(path: &Path, id: &str, text: &str) -> Result<Collection> {
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))?;
    crate::proof::stored_within_safe_integers(&value, &path.display().to_string())?;
    let collection: Collection = serde_json::from_value(value)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))?;
    collection.validate()?;
    ensure!(
        collection.id == id,
        EXIT_SCHEMA,
        "{} says it is collection {}, and a plan has one identity",
        path.display(),
        collection.id
    );
    Ok(collection)
}

/// Validate a staged Collection artifact as a current resource before publication.
pub(crate) fn decode_current_staged(path: &Path, id: &str, text: &str) -> Result<Collection> {
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))?;
    store::check_canonical_bytes(path, text, &value)?;
    let collection = decode_for_migration(path, id, text)?;
    store::check_current_resource_shape(path, text, &collection)?;
    check_canonical_members(path, &collection)?;
    Ok(collection)
}

fn save(root: &Path, collection: &Collection) -> Result<()> {
    collection.validate()?;
    let mut collection = collection.clone();
    canonicalize_members(&mut collection)?;
    store::write_json(&path(root, &collection.id), &collection)
}

/// Every Collection mutation, under the lock, past the applicable Rule set.
///
/// The Rule check is inside the lock and before the read, so what it was
/// established against is what gets written. Collection has no prepared
/// candidate to bind, so the attempt is the whole of what a caller attests.
fn locked<T>(root: &Path, attempt: Attempt, body: impl FnOnce() -> Result<T>) -> Result<T> {
    store::require_current(root)?;
    store::with_lock(root, || {
        store::require_current(root)?;
        crate::rules::direct(root, crate::rules::Domain::Collection, attempt)?;
        body()
    })
}

fn edit<T>(
    root: &Path,
    id: &str,
    attempt: Attempt,
    body: impl FnOnce(&mut Collection) -> Result<T>,
) -> Result<T> {
    locked(root, attempt, || {
        let mut collection = load(root, id)?;
        let outcome = body(&mut collection)?;
        save(root, &collection)?;
        Ok(outcome)
    })
}

/// Ten Crockford characters of workspace-scoped identity.
///
/// Checked against what is already on disk rather than trusted to be unique:
/// fifty random bits make a clash vanishingly unlikely and not impossible, and
/// the cost of checking once at creation is nothing next to two plans sharing
/// an id that every reference to either then resolves ambiguously.
fn mint(root: &Path) -> Result<String> {
    let taken = ids(root)?;
    for _ in 0..64 {
        let id = crate::reference::random_collection_id();
        if !taken.contains(&id) {
            return Ok(id);
        }
    }
    Err(Error::new(
        EXIT_INVARIANT,
        "could not find an unused collection id".to_owned(),
    ))
}

/// Resolve any unique id prefix, the way objects and backlog items resolve.
pub fn resolve_id(root: &Path, spec: &str) -> Result<String> {
    let spec = spec.strip_prefix("engr:collection:").unwrap_or(spec);
    let found: Vec<String> = ids(root)?
        .into_iter()
        .filter(|id| id.starts_with(spec))
        .collect();
    match found.len() {
        1 => Ok(found.into_iter().next().expect("length checked")),
        0 => Err(Error::new(
            EXIT_NOT_FOUND,
            format!("no collection matching {spec:?}"),
        )),
        count => Err(Error::new(
            EXIT_USAGE,
            format!("{spec:?} matches {count} collections; use more characters"),
        )),
    }
}

pub fn create(
    root: &Path,
    name: &str,
    description: Option<&str>,
    schedule: Option<Schedule>,
    attempt: Attempt,
) -> Result<Collection> {
    check_name(EXIT_USAGE, name)?;
    if let Some(schedule) = &schedule {
        schedule.validate(EXIT_USAGE)?;
    }
    locked(root, attempt, || {
        let collection = Collection {
            id: mint(root)?,
            name: name.trim().to_owned(),
            description: description.map(str::to_owned),
            state: State::Open,
            schedule,
            members: Vec::new(),
        };
        save(root, &collection)?;
        Ok(collection)
    })
}

/// Stop keeping a plan.
///
/// #10 makes "an agent MUST NOT delete a Collection unless explicitly directed
/// by a human" a **normative agent rule**, and explicitly not a use of the
/// confirmation gate — it even says a stronger technical guard can be added
/// later if dogfooding shows agents ignoring it. So this does not refuse: engr
/// cannot tell an agent from a human, refusing would make a human's own
/// instruction impossible to carry out directly, and inventing the guard now is
/// exactly what the issue deferred.
///
/// What it does is report what the plan held, so removing planning context is
/// never silent. Deleting a collection changes nothing about its members.
pub fn remove(root: &Path, id: &str, attempt: Attempt) -> Result<Removed> {
    locked(root, attempt, || {
        let collection = load(root, id)?;
        let path = path(root, id);
        std::fs::remove_file(&path).map_err(|error| tool_error(path.display(), error))?;
        Ok(Removed {
            name: collection.name,
            members: collection.members.len(),
        })
    })
}

/// What deleting a plan threw away, so a caller can say so.
#[derive(Debug)]
pub struct Removed {
    pub name: String,
    pub members: usize,
}

pub fn rename(root: &Path, id: &str, name: &str, attempt: Attempt) -> Result<Collection> {
    check_name(EXIT_USAGE, name)?;
    edit(root, id, attempt, |collection| {
        collection.name = name.trim().to_owned();
        Ok(())
    })?;
    load(root, id)
}

pub fn describe(
    root: &Path,
    id: &str,
    description: Option<&str>,
    attempt: Attempt,
) -> Result<Collection> {
    if let Some(description) = description {
        ensure!(
            !description.trim().is_empty(),
            EXIT_USAGE,
            "a description that is present says something; omit --text to clear it"
        );
    }
    edit(root, id, attempt, |collection| {
        collection.description = description.map(str::to_owned);
        Ok(())
    })?;
    load(root, id)
}

/// Declare where the plan stands. Never inferred from members or dates.
pub fn set_state(root: &Path, id: &str, state: State, attempt: Attempt) -> Result<Collection> {
    edit(root, id, attempt, |collection| {
        collection.state = state;
        Ok(())
    })?;
    load(root, id)
}

pub fn set_schedule(
    root: &Path,
    id: &str,
    schedule: Option<Schedule>,
    attempt: Attempt,
) -> Result<Collection> {
    if let Some(schedule) = &schedule {
        schedule.validate(EXIT_USAGE)?;
    }
    edit(root, id, attempt, |collection| {
        collection.schedule = schedule;
        Ok(())
    })?;
    load(root, id)
}

/// Whether a target names something that exists **right now**.
///
/// Checked when a member is added and never again, and the difference is the
/// whole point. A member added by typo is a plan silently covering nothing, and
/// nothing later will tell anyone. A member whose target is consumed *after* it
/// was added is legitimate planning context — the plan really did cover that,
/// and saying so is more honest than repairing it — so this must not become a
/// rule that stored data is held to.
fn require_target(root: &Path, target: &str) -> Result<()> {
    let parsed = crate::reference::EngrRef::parse_embedded(target)?;
    let uuid = crate::reference::decode_uuid(parsed.id())?.to_string();
    let outcome = match parsed.kind() {
        ResourceKind::Backlog => crate::backlog::load(root, &uuid).map(|_| ()),
        _ => crate::ops::effective(root, &uuid).map(|_| ()),
    };
    // Absence and unreadable authority are different refusals. Reporting a
    // malformed file as "does not exist" would send someone to create what is
    // already there, and hide the fault that actually needs looking at.
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

pub fn add_member(
    root: &Path,
    id: &str,
    target: &str,
    order: Option<i64>,
    priority: Option<Priority>,
    attempt: Attempt,
) -> Result<Collection> {
    // Shape first, so a malformed reference is refused as malformed rather than
    // as missing — `decode_uuid` on nonsense would otherwise answer the wrong
    // question. This one needs no lock: it reads the argument, not the
    // workspace.
    check_target("a collection member", target)?;
    edit(root, id, attempt, |collection| {
        // Existence is checked **inside** the lock, because the check is what
        // defines admission. Backlog consumption takes the same workspace lock,
        // so a check outside it can observe a target, lose the race, and then
        // persist a membership that was already dangling at the instant it was
        // admitted. A member that goes dangling *afterwards* is intended and
        // left alone; one that was never admissible is a different thing, and
        // only a shared critical section can tell them apart.
        require_target(root, target)?;
        ensure!(
            collection.member(target).is_err(),
            EXIT_INVARIANT,
            "{target} is already in this collection"
        );
        collection.members.push(Member {
            target: EngrTarget::new(target),
            order,
            priority,
        });
        Ok(())
    })?;
    load(root, id)
}

pub fn remove_member(root: &Path, id: &str, target: &str, attempt: Attempt) -> Result<Collection> {
    edit(root, id, attempt, |collection| {
        collection.member(target)?;
        collection
            .members
            .retain(|member| member.target.reference != target);
        Ok(())
    })?;
    load(root, id)
}

/// Rank a member, or unrank it with `None`.
pub fn set_order(
    root: &Path,
    id: &str,
    target: &str,
    order: Option<i64>,
    attempt: Attempt,
) -> Result<Collection> {
    edit(root, id, attempt, |collection| {
        collection.member_mut(target)?.order = order;
        Ok(())
    })?;
    load(root, id)
}

pub fn set_priority(
    root: &Path,
    id: &str,
    target: &str,
    priority: Option<Priority>,
    attempt: Attempt,
) -> Result<Collection> {
    if let Some(priority) = &priority {
        if let Some(reason) = &priority.reason {
            ensure!(
                !reason.trim().is_empty(),
                EXIT_USAGE,
                "a priority reason that is present says something; omit --reason instead"
            );
        }
    }
    edit(root, id, attempt, |collection| {
        collection.member_mut(target)?.priority = priority;
        Ok(())
    })?;
    load(root, id)
}
