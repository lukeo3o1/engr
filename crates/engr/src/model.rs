//! The object/section model, the confirmed payload, and projection.
//!
//! An object is an aggregate of sections. A section's `text` is always its
//! current wording, because wording only ever changes through a confirmed
//! action. Nothing is derived at read time except staleness, which lives in
//! [`crate::git`].

use crate::FORMAT_VERSION;
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

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Open,
    Closed,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Open => "open",
            Status::Closed => "closed",
        }
    }
}

/// A reference to one section, pinned to what it said and the
/// commit it said it at. `sha256` makes "my basis changed" computable locally;
/// `commit` makes the old wording recoverable with `git show`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
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
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Content {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub based_on: Option<String>,
    #[serde(default)]
    pub refs: Vec<Ref>,
}

impl Content {
    /// SHA-256 over canonical JSON. `serde_json::Map` is a `BTreeMap`, so going
    /// through `Value` sorts keys and the digest is independent of field order.
    pub fn sha256(&self) -> Result<String> {
        canonical_sha256_with_basis(self)
    }

    fn validate(&self) -> Result<()> {
        if let Some(based_on) = &self.based_on {
            validate_git_oid("based_on", based_on)?;
        }
        for reference in &self.refs {
            reference.validate()?;
        }
        Ok(())
    }
}

fn canonical_sha256<T: Serialize>(value: &T) -> Result<String> {
    crate::confirmation::fingerprint(value)
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Section {
    pub id: u64,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub based_on: Option<String>,
    #[serde(default)]
    pub refs: Vec<Ref>,
    /// Hash of the confirmed content — text, `based_on` and `refs` together, not
    /// text alone. Repointing a ref would otherwise pass `verify`.
    pub sha256: String,
    pub confirmed_at: String,
}

impl Section {
    pub fn content(&self) -> Content {
        Content {
            text: self.text.clone(),
            based_on: self.based_on.clone(),
            refs: self.refs.clone(),
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
    #[serde(alias = "status")]
    pub state: Status,
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
            state: Status::Open,
            rev: 0,
            next_section_id: 1,
            sections: Vec::new(),
        };
        object.validate()?;
        Ok(object)
    }

    pub fn section(&self, id: u64) -> Result<&Section> {
        self.sections
            .iter()
            .find(|section| section.id == id)
            .ok_or_else(|| Error::new(EXIT_NOT_FOUND, format!("section §{id} does not exist")))
    }

    fn require_open(&self, what: &str) -> Result<()> {
        ensure!(
            self.state == Status::Open,
            EXIT_INVARIANT,
            "{what} requires an open object; reopen it first"
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
                version == FORMAT_VERSION,
                EXIT_SCHEMA,
                "unsupported legacy object version {version}"
            );
        }
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

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    ObjectCreated,
    ObjectRenamed,
    SectionAdded,
    SectionRevised { section: u64 },
    SectionMerged { absorbs: Vec<u64> },
    SectionDeleted { section: u64 },
    ObjectClosed,
    ObjectReopened,
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
    #[serde(flatten)]
    pub content: Content,
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
                    && self.content.refs.is_empty(),
                EXIT_INVARIANT,
                "{} does not carry content",
                self.action.label()
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
            object.require_open("object.renamed")?;
            object.title.clone_from(&content.text);
        }
        Action::SectionAdded => {
            object.require_open("section.added")?;
            let id = take_id(object)?;
            object.sections.push(section_from(id, event)?);
        }
        Action::SectionRevised { section } => {
            object.require_open("section.revised")?;
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
            object.require_open("section.merged")?;
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
            object.require_open("section.deleted")?;
            object.section(*section)?;
            object.sections.retain(|item| item.id != *section);
        }
        Action::ObjectClosed => {
            object.require_open("object.closed")?;
            object.state = Status::Closed;
        }
        Action::ObjectReopened => {
            ensure!(
                object.state == Status::Closed,
                EXIT_INVARIANT,
                "object.reopened requires a closed object"
            );
            object.state = Status::Open;
        }
    }
    object.sections.sort_by_key(|section| section.id);
    object.rev = event.rev;
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
        text: content.text,
        based_on: content.based_on,
        refs: content.refs,
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
            content: Content {
                text: text.to_owned(),
                based_on: None,
                refs: Vec::new(),
            },
        };
        Event {
            format: EVENT_FORMAT.to_owned(),
            version: FORMAT_VERSION,
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
