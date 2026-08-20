//! The object/section model, the confirmed payload, and projection.
//!
//! An object is an aggregate of sections. A section's `text` is always its
//! current wording, because wording only ever changes through a confirmed
//! action. Nothing is derived at read time except staleness, which lives in
//! [`crate::git`].

use crate::semantics::{
    needs_attention, validate_state, ObjectType, Relation, RelationType, Role, State, Supplement,
};
use crate::LEGACY_OBJECT_VERSION_V0;
use crate::{ensure, Error, Result, EXIT_INVARIANT, EXIT_NOT_FOUND, EXIT_SCHEMA, EXIT_USAGE};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// uuidv7: time-ordered, and with no date welded into a human-facing id — the
/// previous scheme put one there and could not represent anything backdated, nor
/// more than a hundred records a day.
pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// Turn input into the stable Object identity stored in records and filenames.
/// The compact reference codec is deliberately separate; the record itself
/// keeps standard UUID text so a file can be identified without a resolver.
pub fn canonical_object_id(value: &str) -> Result<String> {
    let id = uuid::Uuid::parse_str(value)
        .map_err(|_| Error::new(EXIT_SCHEMA, format!("object id {value:?} is not a UUID")))?;
    ensure!(
        id.get_version() == Some(uuid::Version::SortRand),
        EXIT_SCHEMA,
        "object id {value:?} must be UUIDv7"
    );
    Ok(id.to_string())
}

/// Stored ids are not just UUID-shaped: they use the one canonical UUIDv7
/// spelling. Gate input may be normalized first, but loaded authority may not
/// quietly preserve an alternative spelling.
pub fn validate_object_id(value: &str) -> Result<()> {
    let canonical = canonical_object_id(value)?;
    ensure!(
        canonical == value,
        EXIT_SCHEMA,
        "object id {value:?} must be a canonical UUIDv7"
    );
    Ok(())
}

/// Git resolves selectors such as `HEAD` for input, but confirmed records pin
/// the immutable object id it produced. Accept SHA-1 and SHA-256 repositories.
pub fn is_canonical_git_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_git_oid(field: &str, value: &str) -> Result<()> {
    ensure!(
        is_canonical_git_oid(value),
        EXIT_SCHEMA,
        "{field} must be a full resolved Git object id"
    );
    Ok(())
}

pub const OBJECT_FORMAT: &str = "engr-object";
pub const EVENT_FORMAT: &str = "engr-event";
pub const CANDIDATE_FORMAT: &str = "engr-candidate";

/// A reference to one section, pinned to what it said and the
/// commit it said it at. `sha256` makes "my basis changed" computable locally;
/// `commit` makes the old wording recoverable with `git show`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Ref {
    pub object: String,
    pub section: u64,
    pub sha256: String,
    pub commit: String,
}

impl Ref {
    fn validate(&self) -> Result<()> {
        validate_object_id(&self.object)?;
        validate_git_oid("reference commit", &self.commit)
    }
}

/// The part of a section a human actually reads and assents to.
///
/// Every semantic field a Section carries lives here, and only here, because
/// this is what the section hash covers. A field held outside it would be
/// authoritative meaning that `verify` cannot see and a ref cannot pin — which
/// is exactly how `role` or a relation could be changed after the fact without
/// anything reporting it.
///
/// The new fields are all skipped when empty, so a Section that carries none of
/// them serializes and hashes byte for byte as it did before they existed.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Content {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    pub text: String,
    /// Ordered, unlike `refs` and `relations`: these are excerpts a reader goes
    /// through in sequence, so moving one is a change to the assertion.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Supplement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub based_on: Option<String>,
    #[serde(default)]
    pub refs: Vec<Ref>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<Relation>,
}

impl Content {
    /// SHA-256 over canonical JSON. `serde_json::Map` is a `BTreeMap`, so going
    /// through `Value` sorts keys and the digest is independent of field order.
    pub fn sha256(&self) -> Result<String> {
        canonical_sha256_with_basis(self)
    }

    /// Put the order-insensitive collections in one order before anything is
    /// hashed.
    ///
    /// `refs` and `relations` are sets: the same three references written in
    /// another order are the same assertion. Sorting them at the gate — before
    /// the payload is fingerprinted and before a human is shown it — is what
    /// makes that true in practice, and it costs nothing on the read path, so
    /// every hash already stored stays valid.
    ///
    /// Supplementary bodies are **not** touched here. A body is literal, and
    /// #14 says only that it is non-empty UTF-8 — so every byte of it is
    /// significant, `"x"` and `"x\n"` are different sections with different
    /// hashes, and it is not this function's place to decide otherwise. That
    /// makes the trailing whitespace a *presentation* obligation instead: see
    /// `render_supplement_bodies`, which says how a body ends when the way it
    /// ends would otherwise be invisible.
    pub fn canonicalize_order(&mut self) {
        self.refs.sort();
        self.relations.sort();
    }

    fn validate(&self) -> Result<()> {
        if let Some(based_on) = &self.based_on {
            validate_git_oid("based_on", based_on)?;
        }
        for entry in &self.content {
            entry.validate()?;
        }
        for reference in &self.refs {
            reference.validate()?;
        }
        for relation in &self.relations {
            relation.validate()?;
        }
        // A set cannot hold the same member twice. Enforced on the way in *and*
        // wherever a stored payload is read, because a duplicate that only the
        // write path refuses is a duplicate one hand-edit away from existing.
        check_unique(&self.refs, "reference")?;
        check_unique(&self.relations, "relation")?;
        Ok(())
    }

    /// Every replacement this content names.
    pub fn replacements(&self) -> Result<Vec<String>> {
        let mut found = Vec::new();
        for relation in &self.relations {
            if let Some(target) = relation.replacement()? {
                found.push(target);
            }
        }
        Ok(found)
    }
}

/// Exact duplicates only. Two refs to the same section pinned at different
/// wording are different statements, and saying so is not this function's job.
fn check_unique<T: PartialEq>(items: &[T], what: &str) -> Result<()> {
    for (index, item) in items.iter().enumerate() {
        ensure!(
            !items[..index].contains(item),
            EXIT_SCHEMA,
            "the same {what} is listed twice"
        );
    }
    Ok(())
}

fn canonical_sha256<T: Serialize>(value: &T) -> Result<String> {
    crate::confirmation::fingerprint(value)
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Section {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Supplement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub based_on: Option<String>,
    #[serde(default)]
    pub refs: Vec<Ref>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<Relation>,
    /// Hash of the confirmed content — role, text, supplementary content,
    /// `based_on`, `refs` and `relations` together, not text alone. Repointing a
    /// ref, or retyping a relation, would otherwise pass `verify`.
    pub sha256: String,
    pub confirmed_at: String,
}

impl Section {
    pub fn content(&self) -> Content {
        Content {
            role: self.role,
            text: self.text.clone(),
            content: self.content.clone(),
            based_on: self.based_on.clone(),
            refs: self.refs.clone(),
            relations: self.relations.clone(),
        }
    }

    /// Recompute the hash from what is stored, so `verify` needs nothing else.
    pub fn recomputed_sha256(&self) -> Result<String> {
        self.content().sha256()
    }

    fn validate(&self) -> Result<()> {
        ensure!(self.id > 0, EXIT_SCHEMA, "section ids start at 1");
        self.content().validate()
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Object {
    /// Retained when a migrated v0 object carried redundant resource schema
    /// markers. New objects rely only on the workspace authority.
    #[serde(rename = "format", skip_serializing_if = "Option::is_none")]
    pub legacy_format: Option<String>,
    #[serde(rename = "version", skip_serializing_if = "Option::is_none")]
    pub legacy_version: Option<u32>,
    pub id: String,
    pub title: String,
    /// Optional, and absent is a first-class answer rather than a missing one.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub object_type: Option<ObjectType>,
    /// The one lifecycle field. `status` is read as an alias so a workspace
    /// migrated from v0 keeps loading, and is never written back under that
    /// name: two spellings of one truth is how they start disagreeing.
    #[serde(alias = "status")]
    pub state: State,
    /// Increments on every confirmed action. Candidates pin it, so one prepared
    /// against an older state cannot be confirmed after the object moved.
    pub rev: u64,
    /// Monotonic and never reset. Section ids are never reused, so this cannot
    /// be derived as `max(existing) + 1`: that would hand out the id of a section
    /// that was deleted, and every outside reference to it would silently point
    /// at different content.
    pub next_section_id: u64,
    pub sections: Vec<Section>,
}

impl Object {
    pub fn new(id: String, title: String) -> Result<Self> {
        let object = Self {
            legacy_format: None,
            legacy_version: None,
            id,
            title,
            object_type: None,
            state: State::Open,
            rev: 0,
            next_section_id: 1,
            sections: Vec::new(),
        };
        object.validate()?;
        Ok(object)
    }

    /// Whether this Object belongs in the default attention set. Derived from
    /// `(type, state)` on every read; never stored.
    pub fn needs_attention(&self) -> bool {
        needs_attention(self.object_type, self.state)
    }

    /// Every replacement any of this Object's sections names.
    pub fn replacements(&self) -> Result<Vec<String>> {
        let mut found = Vec::new();
        for section in &self.sections {
            found.extend(section.content().replacements()?);
        }
        Ok(found)
    }

    pub fn section(&self, id: u64) -> Result<&Section> {
        self.sections
            .iter()
            .find(|section| section.id == id)
            .ok_or_else(|| Error::new(EXIT_NOT_FOUND, format!("section §{id} does not exist")))
    }

    /// Confirmed content is not revised while nobody is looking at it.
    ///
    /// This is the old "reopen first" rule stated in the terms Phase 3 gives it.
    /// For an untyped Object attention is exactly `open`, so nothing about the
    /// old behaviour changed; for a typed one it is the derived class, so an
    /// `accepted` design has to be moved back to `draft` or `proposed` before it
    /// can be reworded. Renewed engineering work returns to the attention set
    /// rather than happening out of sight of everyone who reads the default
    /// listing.
    ///
    /// The rule is about *renewed* engineering work: wording that was confirmed
    /// once being changed again while nobody is looking. Reclassify, then
    /// revise — two confirmations, and the object is visible in the default
    /// listing for the second.
    ///
    /// `object.superseded` is deliberately not one of the callers, and the
    /// exemption is the rule rather than a hole in it. Superseding is not
    /// resumed work on the Object; it is the act of retiring it, and the case it
    /// exists for is an `accepted` design or decision that a newer one replaced
    /// — an Object that by definition needs no attention. Demanding a
    /// reclassification first would confirm an intermediate state the Object was
    /// never in, and would split into two confirmations the one operation the
    /// protocol requires to be atomic.
    fn require_attention(&self, what: &str) -> Result<()> {
        // The way through is named, and named in the caller's own vocabulary: an
        // untyped object is reopened, a typed one is classified. A refusal that
        // makes someone go and look up which states are in the attention set is
        // a refusal they will work around rather than read.
        let way_through = match self.object_type {
            None => "reopen it first".to_owned(),
            Some(_) => format!(
                "classify it into {} first",
                crate::semantics::attention_states(self.object_type)
            ),
        };
        ensure!(
            self.needs_attention(),
            EXIT_INVARIANT,
            "{what} requires an object that needs attention, and {} does not; {way_through}",
            self.state.as_str()
        );
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        validate_object_id(&self.id)?;
        if let Some(format) = &self.legacy_format {
            ensure!(
                format == OBJECT_FORMAT,
                EXIT_SCHEMA,
                "not an engr object: format is {format:?}"
            );
        }
        if let Some(version) = self.legacy_version {
            ensure!(
                version == LEGACY_OBJECT_VERSION_V0,
                EXIT_SCHEMA,
                "unsupported legacy object version {version}"
            );
        }
        validate_state(EXIT_SCHEMA, self.object_type, self.state)?;
        check_supersession(self, EXIT_SCHEMA)?;
        ensure!(
            self.next_section_id > 0,
            EXIT_SCHEMA,
            "{}: next_section_id must start at 1",
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

/// The supersession invariant, checked from both directions.
///
/// `state = superseded` and exactly one `superseded_by` relation are one fact
/// written in two places, so either without the other is a record that
/// contradicts itself: a superseded Object with nothing to forward a reader to,
/// or a replacement edge that the state does not honour. Checking it after every
/// projection is what makes the supersession operation atomic in practice — a
/// revision that would drop the relation, or a deletion that would remove the
/// Section holding it, fails here rather than quietly breaking the pair.
fn check_supersession(object: &Object, code: i32) -> Result<()> {
    let replacements = object
        .sections
        .iter()
        .flat_map(|section| &section.relations)
        .filter(|relation| relation.relation == RelationType::SupersededBy)
        .count();
    match (object.state == State::Superseded, replacements) {
        (true, 1) | (false, 0) => Ok(()),
        (true, found) => Err(Error::new(
            code,
            format!(
                "{}: a superseded object needs exactly one superseded_by relation, and has {found}",
                object.id
            ),
        )),
        (false, _) => Err(Error::new(
            code,
            format!(
                "{}: a superseded_by relation says the object was replaced, so its state must be superseded, not {}",
                object.id,
                object.state.as_str()
            ),
        )),
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    ObjectCreated,
    ObjectRenamed,
    SectionAdded,
    SectionRevised {
        section: u64,
    },
    SectionMerged {
        absorbs: Vec<u64>,
    },
    SectionDeleted {
        section: u64,
    },
    ObjectClosed,
    ObjectReopened,
    /// Declare what this Object is and what state it is in, both explicitly.
    ///
    /// One action rather than two, because a type change without a destination
    /// state has no answer: the vocabularies do not overlap, and any mapping
    /// engr invented would be engr deciding that an `accepted` design becomes an
    /// `accepted` decision — which is a judgement, not a conversion. Both values
    /// are always restated, so the human reads the whole destination rather than
    /// a delta against something they have to remember.
    ObjectClassified {
        #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
        object_type: Option<ObjectType>,
        state: State,
    },
    /// Replace this Object with another, in one confirmation.
    ///
    /// The state, the replacement relation and the human-readable reason are one
    /// semantic act. Splitting them into separately confirmable steps would be
    /// easier to implement and would mean a record can sit in the state where it
    /// says it was replaced and cannot say by what.
    ObjectSuperseded,
}

impl Action {
    pub fn label(&self) -> &'static str {
        match self {
            Action::ObjectCreated => "object.created",
            Action::ObjectRenamed => "object.renamed",
            Action::SectionAdded => "section.added",
            Action::SectionRevised { .. } => "section.revised",
            Action::SectionMerged { .. } => "section.merged",
            Action::SectionDeleted { .. } => "section.deleted",
            Action::ObjectClosed => "object.closed",
            Action::ObjectReopened => "object.reopened",
            Action::ObjectClassified { .. } => "object.classified",
            Action::ObjectSuperseded => "object.superseded",
        }
    }

    /// Actions that carry wording a human must read.
    pub fn carries_content(&self) -> bool {
        matches!(
            self,
            Action::ObjectCreated
                | Action::ObjectRenamed
                | Action::SectionAdded
                | Action::SectionRevised { .. }
                | Action::SectionMerged { .. }
                | Action::ObjectSuperseded
        )
    }

    /// The actions that refuse to run on an Object nobody is looking at.
    ///
    /// Exactly the ones a [`Payload::becomes`] destination is for, and the list
    /// is shared with the guard rather than written twice. That sharing is the
    /// whole rule: a destination is admissible on an action *because* the guard
    /// would otherwise refuse it. An action that sets the Object's own state
    /// cannot also be handed one, and `object_superseded` is exempt from the
    /// guard entirely, so neither takes a destination.
    pub fn requires_attention(&self) -> bool {
        matches!(
            self,
            Action::ObjectRenamed
                | Action::SectionAdded
                | Action::SectionRevised { .. }
                | Action::SectionMerged { .. }
                | Action::SectionDeleted { .. }
        )
    }

    /// Actions that add wording as a new Section rather than replacing existing
    /// wording or a label.
    pub fn adds_section(&self) -> bool {
        matches!(
            self,
            Action::SectionAdded | Action::SectionMerged { .. } | Action::ObjectSuperseded
        )
    }

    /// Actions whose content is the object's title rather than a section's
    /// wording. The gate holds both to the same shape, so a body cannot be
    /// pasted into a label through the door that was added later.
    pub fn carries_title(&self) -> bool {
        matches!(self, Action::ObjectCreated | Action::ObjectRenamed)
    }
}

/// Exactly what a human is asked to assent to. The action is inside the hash,
/// so "delete §3" cannot become "delete §5" after it was displayed.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Payload {
    #[serde(flatten)]
    pub action: Action,
    pub object: String,
    /// Where this action leaves the Object, applied atomically with it.
    ///
    /// It exists because a no-attention Object may be revised in **one**
    /// confirmed operation when that operation returns it to attention. Without
    /// it the only path was to confirm a reclassification the Object was never
    /// really in, and then confirm the revision — two authoritative statements
    /// for one piece of work.
    ///
    /// Narrow on purpose. `becomes` is admissible only on an Object that does
    /// **not** currently need attention, only on the actions the attention guard
    /// would otherwise refuse, and only towards a destination that does need
    /// attention. An Object already in the listing has no use for it: nothing is
    /// being unblocked, so a destination there would be an unrelated change
    /// riding along inside someone else's confirmation. That is what
    /// `object_classified` is for.
    ///
    /// Absent by default and skipped when empty, so a payload that does not
    /// carry one serializes and hashes exactly as it did before it existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub becomes: Option<Destination>,
    #[serde(flatten)]
    pub content: Content,
}

/// Where a [`Payload::becomes`] lands the Object.
///
/// Field-for-field what [`Action::ObjectClassified`] carries, and deliberately
/// so: a destination means the same thing whether it is the whole operation or
/// the half of one that makes the other half legal.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Destination {
    /// Absent means untyped, explicitly — the same first-class answer it is
    /// everywhere else.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub object_type: Option<ObjectType>,
    pub state: State,
}

impl Payload {
    pub fn sha256(&self) -> Result<String> {
        canonical_sha256_with_basis(self)
    }

    pub fn validate(&self) -> Result<()> {
        validate_object_id(&self.object)?;
        self.content.validate()?;
        if self.action.carries_content() {
            ensure!(
                !self.content.text.trim().is_empty(),
                EXIT_USAGE,
                "{} requires text",
                self.action.label()
            );
        } else {
            ensure!(
                self.content.text.is_empty()
                    && self.content.based_on.is_none()
                    && self.content.refs.is_empty()
                    && self.content.role.is_none()
                    && self.content.content.is_empty()
                    && self.content.relations.is_empty(),
                EXIT_INVARIANT,
                "{} does not carry content",
                self.action.label()
            );
        }
        if let Action::ObjectClassified { object_type, state } = &self.action {
            validate_state(EXIT_USAGE, *object_type, *state)?;
        }
        // `superseded_by` may only arrive through the one action that also sets
        // the state, and that action may carry nothing else. Any other way in
        // would let the relation and the state be confirmed apart, which is the
        // whole thing supersession is defined to prevent.
        if matches!(self.action, Action::ObjectSuperseded) {
            ensure!(
                self.content.role == Some(Role::Supersession),
                EXIT_INVARIANT,
                "object.superseded carries the reason it was replaced, so its section is role=supersession"
            );
            ensure!(
                self.content.relations.len() == 1
                    && self.content.relations[0].relation == RelationType::SupersededBy,
                EXIT_INVARIANT,
                "object.superseded carries exactly one relation, the superseded_by naming the replacement"
            );
        } else {
            ensure!(
                !self
                    .content
                    .relations
                    .iter()
                    .any(|relation| relation.relation == RelationType::SupersededBy),
                EXIT_INVARIANT,
                "a superseded_by relation only enters through object.superseded, which confirms the state, the replacement and the reason together"
            );
        }
        if let Action::SectionMerged { absorbs } = &self.action {
            ensure!(
                absorbs.len() >= 2,
                EXIT_INVARIANT,
                "a merge must absorb at least two sections"
            );
            let mut unique = absorbs.clone();
            unique.sort_unstable();
            unique.dedup();
            ensure!(
                unique.len() == absorbs.len(),
                EXIT_INVARIANT,
                "a merge cannot absorb the same section twice"
            );
        }
        Ok(())
    }
}

/// `based_on: null` was part of the v0 hash canonical form before no-basis was
/// represented by an absent persisted field. Keeping it in the digest form
/// lets compatible legacy sections and confirmed events validate unchanged;
/// serialization and hashing are separate representations for this one field.
fn canonical_sha256_with_basis<T: Serialize>(value: &T) -> Result<String> {
    let mut value = serde_json::to_value(value)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("canonical form: {error}")))?;
    if let serde_json::Value::Object(object) = &mut value {
        object.entry("based_on").or_insert(serde_json::Value::Null);
    }
    canonical_sha256(&value)
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Confirmation {
    pub challenge: String,
    pub payload_sha256: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Event {
    pub format: String,
    pub version: u32,
    pub event_id: String,
    pub rev: u64,
    pub time: String,
    #[serde(flatten)]
    pub payload: Payload,
    pub confirmation: Confirmation,
}

/// Apply only the event suffix newer than the persisted projection.
///
/// Retained history may have been purged at its beginning, so events at or
/// below the projection revision are evidence rather than replay input. Once a
/// newer event exists, though, it must begin at the next revision and continue
/// without a gap: otherwise a later confirmation could be inserted before an
/// unreachable future event.
pub fn replay_recoverable_tail(mut object: Object, events: &[Event]) -> Result<(Object, bool)> {
    let future: Vec<_> = events
        .iter()
        .filter(|event| event.rev > object.rev)
        .collect();
    if future.is_empty() {
        return Ok((object, false));
    }

    let mut expected = object.rev.checked_add(1).ok_or_else(|| {
        Error::new(
            EXIT_INVARIANT,
            format!("{} cannot advance past its highest revision", object.id),
        )
    })?;
    for event in future {
        ensure!(
            event.rev == expected,
            EXIT_INVARIANT,
            "{}: recoverable event tail begins or continues at rev {}, not rev {}",
            object.id,
            event.rev,
            expected
        );
        project(&mut object, event)?;
        expected = object.rev.checked_add(1).ok_or_else(|| {
            Error::new(
                EXIT_INVARIANT,
                format!("{} cannot advance past its highest revision", object.id),
            )
        })?;
    }
    Ok((object, true))
}

/// Apply a confirmed event to an object. Deterministic by construction: no
/// clocks, no git, no interpretation of prose. Everything it needs is in the
/// event.
pub fn project(object: &mut Object, event: &Event) -> Result<()> {
    let content = &event.payload.content;
    // Applied before the action, so the attention guard below sees the state
    // this confirmation *arrives at* rather than the one it left.
    //
    // This is what makes the rule reachable: a no-attention Object may be
    // revised in one confirmed operation **only if** that same operation
    // atomically moves it to a state that needs attention. The guard is
    // unchanged — it still refuses to let confirmed wording change while nobody
    // is looking — but the object is no longer out of sight by the time the
    // section mutation applies, so no artificial intermediate state has to be
    // confirmed first.
    //
    // All three conditions are stated here rather than left to fall out of the
    // guard below. A destination that still needs no attention would be caught
    // either way, but "you may not put it *there*" and "you may not do this at
    // all" are different refusals, and a reducer that says the second when it
    // means the first sends someone to fix the wrong half.
    if let Some(becomes) = &event.payload.becomes {
        ensure!(
            event.payload.action.requires_attention(),
            EXIT_INVARIANT,
            "{} sets the object's own state, so it cannot also carry one",
            event.payload.action.label()
        );
        // The narrow reading, and the one that keeps a confirmation honest: a
        // destination is admissible because it is what makes this action legal,
        // not as a second operation smuggled into the same signature. An Object
        // already in the listing is not blocked, so there is nothing to unblock
        // — and `object_classified` already says what it would be trying to say.
        ensure!(
            !object.needs_attention(),
            EXIT_INVARIANT,
            "{} already needs attention, so a destination here would be a second, unrelated change inside one confirmation; classify it on its own",
            object.id
        );
        validate_state(EXIT_INVARIANT, becomes.object_type, becomes.state)?;
        ensure!(
            needs_attention(becomes.object_type, becomes.state),
            EXIT_INVARIANT,
            "a destination of {} needs no attention, so it cannot be what lets {} run; use one of {}",
            becomes.state.as_str(),
            event.payload.action.label(),
            crate::semantics::attention_states(becomes.object_type)
        );
        object.object_type = becomes.object_type;
        object.state = becomes.state;
    }
    match &event.payload.action {
        Action::ObjectCreated => {
            ensure!(
                object.rev == 0 && object.sections.is_empty(),
                EXIT_INVARIANT,
                "object.created must be the first action"
            );
            object.title.clone_from(&content.text);
        }
        // Open, like everything else that changes the object. A title is part of
        // what settled, so allowing it through on a closed object would make
        // `closed` mean "the sections have settled" rather than "this has" — and
        // it is the second reading that makes closed mean the whole object has
        // settled.
        Action::ObjectRenamed => {
            object.require_attention("object.renamed")?;
            object.title.clone_from(&content.text);
        }
        Action::SectionAdded => {
            object.require_attention("section.added")?;
            let id = take_id(object)?;
            object.sections.push(section_from(id, event)?);
        }
        Action::SectionRevised { section } => {
            object.require_attention("section.revised")?;
            object.section(*section)?;
            let replacement = section_from(*section, event)?;
            let slot = object
                .sections
                .iter_mut()
                .find(|item| item.id == *section)
                .expect("section presence checked above");
            *slot = replacement;
        }
        Action::SectionMerged { absorbs } => {
            object.require_attention("section.merged")?;
            for id in absorbs {
                object.section(*id)?;
            }
            let id = take_id(object)?;
            object
                .sections
                .retain(|section| !absorbs.contains(&section.id));
            object.sections.push(section_from(id, event)?);
        }
        Action::SectionDeleted { section } => {
            object.require_attention("section.deleted")?;
            object.section(*section)?;
            object.sections.retain(|item| item.id != *section);
        }
        // Kept, and kept narrow. These two are the Phase 0 spelling of the
        // untyped vocabulary, they are what every confirmed event in an existing
        // workspace says, and history is not rewritten to suit a newer action.
        // On a typed object they have no meaning at all, because `open` and
        // `closed` are not among its states.
        // Validity before attention, in this order deliberately: on a typed
        // object these two are categorically the wrong action, and being told
        // to classify it into the attention set first would send someone off to
        // do something that still would not let them close a design.
        Action::ObjectClosed => {
            validate_state(EXIT_INVARIANT, object.object_type, State::Closed)?;
            object.require_attention("object.closed")?;
            object.state = State::Closed;
        }
        Action::ObjectReopened => {
            validate_state(EXIT_INVARIANT, object.object_type, State::Open)?;
            ensure!(
                object.state == State::Closed,
                EXIT_INVARIANT,
                "object.reopened requires a closed object"
            );
            object.state = State::Open;
        }
        Action::ObjectClassified { object_type, state } => {
            // No transition graph, deliberately: v0 validates that the
            // destination is legal for the destination type and that the
            // semantic invariants still hold afterwards. Inventing a permitted
            // sequence would be inventing a process nobody agreed to.
            validate_state(EXIT_INVARIANT, *object_type, *state)?;
            object.object_type = *object_type;
            object.state = *state;
        }
        Action::ObjectSuperseded => {
            // `superseded` is only in the design and decision vocabularies, so
            // an untyped object or a risk cannot hold the state — and therefore,
            // by the coupled invariant, cannot hold the relation either.
            //
            // No attention check, deliberately: see `require_attention`. The
            // object this exists for is an `accepted` one, and v0 defines no
            // transition graph — a destination valid for the type, with the
            // semantic invariants holding afterwards, is the whole test.
            // Superseding an already-superseded object is refused by the coupled
            // invariant below, which counts two replacement relations, not by an
            // invented lifecycle.
            validate_state(EXIT_INVARIANT, object.object_type, State::Superseded)?;
            let id = take_id(object)?;
            object.sections.push(section_from(id, event)?);
            object.state = State::Superseded;
        }
    }
    object.sections.sort_by_key(|section| section.id);
    object.rev = event.rev;
    check_supersession(object, EXIT_INVARIANT)?;
    Ok(())
}

fn take_id(object: &mut Object) -> Result<u64> {
    let id = object.next_section_id;
    object.next_section_id = object.next_section_id.checked_add(1).ok_or_else(|| {
        Error::new(
            EXIT_INVARIANT,
            format!("{} has no remaining section ids", object.id),
        )
    })?;
    Ok(id)
}

fn section_from(id: u64, event: &Event) -> Result<Section> {
    let content = event.payload.content.clone();
    Ok(Section {
        id,
        sha256: content.sha256()?,
        role: content.role,
        text: content.text,
        content: content.content,
        based_on: content.based_on,
        refs: content.refs,
        relations: content.relations,
        confirmed_at: event.time.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section_added(id: &str, rev: u64, text: &str) -> Event {
        let payload = Payload {
            action: Action::SectionAdded,
            object: id.to_owned(),
            becomes: None,
            content: Content {
                text: text.to_owned(),
                ..Content::default()
            },
        };
        Event {
            format: EVENT_FORMAT.to_owned(),
            version: crate::EVENT_ENVELOPE_VERSION_V0,
            event_id: new_id(),
            rev,
            time: "2026-08-17T00:00:00Z".to_owned(),
            confirmation: Confirmation {
                challenge: "TEST00".to_owned(),
                payload_sha256: payload.sha256().expect("payload hash"),
            },
            payload,
        }
    }

    #[test]
    fn recoverable_tail_allows_purged_history_but_rejects_future_gaps() {
        let id = new_id();
        let mut completed = Object::new(id.clone(), "completed".to_owned()).expect("object");
        completed.rev = 2;
        assert!(
            replay_recoverable_tail(completed, &[section_added(&id, 2, "old evidence")]).is_ok(),
            "retained evidence at or below the projection may have a missing prefix"
        );

        let mut crashed = Object::new(id.clone(), "crashed".to_owned()).expect("object");
        crashed.rev = 1;
        let (recovered, applied) = replay_recoverable_tail(
            crashed,
            &[
                section_added(&id, 2, "first recovery event"),
                section_added(&id, 3, "second recovery event"),
            ],
        )
        .expect("contiguous future tail");
        assert!(applied);
        assert_eq!(recovered.rev, 3);
        assert_eq!(recovered.sections.len(), 2);

        let mut projection = Object::new(id.clone(), "gap".to_owned()).expect("object");
        projection.rev = 1;
        assert!(
            replay_recoverable_tail(projection.clone(), &[section_added(&id, 3, "gap")]).is_err(),
            "the first future event must be exactly the next revision"
        );
        assert!(
            replay_recoverable_tail(
                projection,
                &[section_added(&id, 2, "first"), section_added(&id, 4, "gap"),],
            )
            .is_err(),
            "the future tail must remain contiguous"
        );
    }
}
