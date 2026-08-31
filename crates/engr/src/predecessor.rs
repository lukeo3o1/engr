//! The one predecessor generation this build reads.
//!
//! Everything here decodes the workspace the **officially released** `latest`
//! build wrote — commit
//! `e7d9f99733407a8c31cec33af18a92480f4f4c6f`, bootstrapping
//! `{"format":"engr-workspace","version":1}`. Nothing here decodes any of the
//! unreleased shapes that also said version 1, 2 or 3 during development: no
//! workspace holding one exists that its owner did not build, and defining a
//! route for them would freeze a serializer that was never shipped into the
//! permanent contract.
//!
//! It is a **separate schema**, not a compatibility mode of the current model.
//! That separation is the safety argument. A shared model with defaulted members
//! reads a predecessor file's missing member as a value the predecessor never
//! wrote, and reads a later generation's member out of a predecessor file as if
//! that generation had existed — which is how a classification nobody made once
//! became derivable from a version 1 history. Here the members are enumerated,
//! the reducer is the predecessor's own, and the seal is computed the way the
//! released build computed it.
//!
//! Two callers need it, and only two: the migration, and the historical
//! resolution of a Ref whose pinned commit predates that migration.

use crate::semantics::{Admission, Admitted, BasedOn, State};
use crate::{ensure, Error, Result, EXIT_INVARIANT, EXIT_NOT_FOUND, EXIT_SCHEMA};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::Path;

/// What the released build wrote into `.engr/format.json`.
pub const WORKSPACE_FORMAT: &str = "engr-workspace";
pub const WORKSPACE_VERSION: u32 = crate::PREDECESSOR_WORKSPACE_VERSION;
pub const OBJECT_FORMAT: &str = "engr-object";
pub const EVENT_FORMAT: &str = "engr-event";
pub const EVENT_VERSION: u32 = 1;

/// The predecessor bootstrap file.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Format {
    pub format: String,
    pub version: u32,
}

/// The predecessor's whole-content reference.
///
/// It pinned the target Section's own seal and the commit it was read at, and
/// nothing about *which* facts the source depended on — so migration converts it
/// into the selective form over exactly the fields the predecessor had, and
/// never into one that claims to have attested `admission` or `header`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Ref {
    pub object: String,
    pub section: u64,
    pub sha256: String,
    pub commit: String,
}

/// The predecessor Section.
#[derive(Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Section {
    pub id: u64,
    pub text: String,
    #[serde(default)]
    pub based_on: Option<String>,
    pub refs: Vec<Ref>,
    pub sha256: String,
    pub confirmed_at: String,
}

/// Exactly the value the released build hashed for `Section.sha256`.
///
/// `based_on` is always present, `null` when there was no basis: the released
/// build injected it that way, and reproducing the *number* means reproducing
/// the *input*, not tidying it. Written out here rather than derived from the
/// current `Content`, which has since gained `header` and turned `based_on`
/// into an object — a shared type would have silently changed a historical
/// seal's meaning.
#[derive(Serialize)]
struct SealedContent<'a> {
    based_on: Option<&'a str>,
    refs: &'a [Ref],
    text: &'a str,
}

/// The released build's canonical hash: `serde_json` over a `Value`, whose map
/// is a `BTreeMap` and therefore key-sorted.
///
/// Deliberately not [`crate::proof::canonical_bytes`]. That one is RFC 8785,
/// which the redesigned contracts use; this one is what actually produced the
/// numbers sitting in a predecessor workspace, and the two differ on escaping
/// and number formatting. A historical seal is checked under the rules that
/// made it.
fn released_sha256<T: Serialize>(value: &T) -> Result<String> {
    let value = serde_json::to_value(value)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("predecessor canonical form: {error}")))?;
    let canonical = serde_json::to_string(&value)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("predecessor canonical form: {error}")))?;
    Ok(format!("{:x}", Sha256::digest(canonical.as_bytes())))
}

impl Section {
    /// The seal the released build would have taken over this content.
    pub fn recomputed_sha256(&self) -> Result<String> {
        released_sha256(&SealedContent {
            based_on: self.based_on.as_deref(),
            refs: &self.refs,
            text: &self.text,
        })
    }

    /// Prove the stored seal against the stored content.
    pub fn check_seal(&self) -> Result<()> {
        ensure!(
            self.recomputed_sha256()? == self.sha256,
            EXIT_INVARIANT,
            "predecessor §{} does not match its own seal",
            self.id
        );
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            self.id > 0,
            EXIT_SCHEMA,
            "predecessor section ids start at 1"
        );
        ensure!(
            self.id <= crate::proof::MAX_SAFE_INTEGER,
            EXIT_SCHEMA,
            "predecessor §{} is outside the shared safe-integer range",
            self.id
        );
        check_lower_sha256(&self.sha256, "predecessor section seal")?;
        ensure!(
            time::OffsetDateTime::parse(
                &self.confirmed_at,
                &time::format_description::well_known::Rfc3339
            )
            .is_ok(),
            EXIT_SCHEMA,
            "predecessor §{}: confirmed_at is not RFC3339",
            self.id
        );
        if let Some(based_on) = &self.based_on {
            ensure!(
                crate::model::is_canonical_git_oid(based_on),
                EXIT_SCHEMA,
                "predecessor §{}: based_on must be a full resolved Git object id",
                self.id
            );
        }
        for reference in &self.refs {
            reference.validate()?;
        }
        Ok(())
    }
}

impl Ref {
    fn validate(&self) -> Result<()> {
        crate::model::validate_object_id(&self.object)?;
        ensure!(
            self.section > 0,
            EXIT_SCHEMA,
            "a predecessor reference cannot name section 0"
        );
        ensure!(
            self.section <= crate::proof::MAX_SAFE_INTEGER,
            EXIT_SCHEMA,
            "predecessor reference section {} is outside the shared safe-integer range",
            self.section
        );
        check_lower_sha256(&self.sha256, "predecessor reference seal")?;
        ensure!(
            crate::model::is_canonical_git_oid(&self.commit),
            EXIT_SCHEMA,
            "a predecessor reference pins a full resolved Git object id"
        );
        Ok(())
    }
}

fn check_lower_sha256(value: &str, what: &str) -> Result<()> {
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        EXIT_SCHEMA,
        "{what} must be 64 lowercase hexadecimal characters"
    );
    Ok(())
}

/// The predecessor Object.
///
/// `format` and `version` are accepted and discarded: the released build
/// converted a still older workspace in place by renaming the lifecycle key and
/// leaving the per-resource markers where they were, so a workspace it wrote can
/// legitimately carry them. They mean nothing to the redesigned Object, which
/// takes its generation from the workspace alone.
#[derive(Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Object {
    #[serde(rename = "format", default)]
    pub legacy_format: Option<String>,
    #[serde(rename = "version", default)]
    pub legacy_version: Option<u32>,
    pub id: String,
    pub title: String,
    pub state: State,
    pub rev: u64,
    pub next_section_id: u64,
    pub sections: Vec<Section>,
}

impl Object {
    fn empty(id: &str) -> Self {
        Self {
            legacy_format: None,
            legacy_version: None,
            id: id.to_owned(),
            title: String::new(),
            state: State::Open,
            rev: 0,
            next_section_id: 1,
            sections: Vec::new(),
        }
    }

    pub fn section(&self, id: u64) -> Result<&Section> {
        self.sections
            .iter()
            .find(|section| section.id == id)
            .ok_or_else(|| Error::new(EXIT_NOT_FOUND, format!("predecessor §{id} does not exist")))
    }

    pub fn validate(&self) -> Result<()> {
        crate::model::validate_object_id(&self.id)?;
        if let Some(format) = &self.legacy_format {
            ensure!(
                format == OBJECT_FORMAT,
                EXIT_SCHEMA,
                "not an engr object: format is {format:?}"
            );
        }
        if let Some(version) = self.legacy_version {
            ensure!(
                version == 1,
                EXIT_SCHEMA,
                "unsupported legacy object version {version}"
            );
        }
        // The released generation had no Object `type`, so its whole state
        // vocabulary is the untyped one. A predecessor file saying `draft` is
        // not an old typed Object, it is a file that generation could not have
        // written.
        ensure!(
            matches!(self.state, State::Open | State::Closed),
            EXIT_SCHEMA,
            "{}: {} is not a state the released generation had",
            self.id,
            self.state.as_str()
        );
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

    /// Every stored Section seal, proven against the stored content.
    pub fn check_seals(&self) -> Result<()> {
        for section in &self.sections {
            section.check_seal()?;
        }
        Ok(())
    }
}

/// What the released build recorded as a confirmation.
#[derive(Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Confirmation {
    pub challenge: String,
    pub payload_sha256: String,
}

/// The predecessor action vocabulary. Eight, and no more.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    ObjectCreated,
    ObjectRenamed,
    SectionAdded,
    SectionRevised {
        section: u64,
    },
    /// The predecessor merge consumed every listed Section and allocated a fresh
    /// id for the result. It is projected exactly as it was written — replaying
    /// it under the current destination-survives rule would reconstruct an
    /// Object that never existed.
    SectionMerged {
        absorbs: Vec<u64>,
    },
    SectionDeleted {
        section: u64,
    },
    ObjectClosed,
    ObjectReopened,
}

impl Action {
    fn label(&self) -> &'static str {
        match self {
            Action::ObjectCreated => "object_created",
            Action::ObjectRenamed => "object_renamed",
            Action::SectionAdded => "section_added",
            Action::SectionRevised { .. } => "section_revised",
            Action::SectionMerged { .. } => "section_merged",
            Action::SectionDeleted { .. } => "section_deleted",
            Action::ObjectClosed => "object_closed",
            Action::ObjectReopened => "object_reopened",
        }
    }

    fn carries_content(&self) -> bool {
        matches!(
            self,
            Action::ObjectCreated
                | Action::ObjectRenamed
                | Action::SectionAdded
                | Action::SectionRevised { .. }
                | Action::SectionMerged { .. }
        )
    }
}

/// The predecessor payload, in the shape its `payload_sha256` was taken over.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
struct Payload {
    #[serde(flatten)]
    action: serde_json::Value,
    object: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    based_on: Option<String>,
    refs: Vec<Ref>,
    text: String,
}

/// The predecessor Event line.
///
/// No `deny_unknown_fields`: the action is flattened, and serde forbids the two
/// together. [`decode_events`] enumerates the exact member set of every action
/// before anything decodes, which is the stricter check and the one that can
/// name what it refused.
#[derive(Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Event {
    pub format: String,
    pub version: u32,
    pub event_id: String,
    pub rev: u64,
    pub time: String,
    #[serde(flatten)]
    pub action: Action,
    pub object: String,
    pub text: String,
    #[serde(default)]
    pub based_on: Option<String>,
    pub refs: Vec<Ref>,
    pub confirmation: Confirmation,
}

impl Event {
    /// The mutation hash the released build recorded.
    ///
    /// Recomputed from the decoded line rather than from its raw bytes, exactly
    /// as the released build computed it from its own in-memory payload: the
    /// action tag and its parameters flattened beside `object`, `text`, `refs`
    /// and an always-present `based_on`.
    fn payload_sha256(&self) -> Result<String> {
        let action = serde_json::to_value(&self.action).map_err(|error| {
            Error::new(EXIT_SCHEMA, format!("predecessor event action: {error}"))
        })?;
        let payload = Payload {
            action,
            object: self.object.clone(),
            based_on: self.based_on.clone(),
            refs: self.refs.clone(),
            text: self.text.clone(),
        };
        let mut value = serde_json::to_value(&payload).map_err(|error| {
            Error::new(EXIT_SCHEMA, format!("predecessor event payload: {error}"))
        })?;
        if let serde_json::Value::Object(members) = &mut value {
            members.entry("based_on").or_insert(serde_json::Value::Null);
        }
        released_sha256(&value)
    }

    fn validate(&self, id: &str) -> Result<()> {
        ensure!(
            self.format == EVENT_FORMAT && self.version == EVENT_VERSION,
            EXIT_SCHEMA,
            "a predecessor workspace carries only Event generation 1"
        );
        ensure!(
            self.object == id,
            EXIT_SCHEMA,
            "event belongs to object {:?}, not {:?}",
            self.object,
            id
        );
        ensure!(
            self.rev >= 1,
            EXIT_SCHEMA,
            "predecessor event revisions start at 1"
        );
        ensure!(
            time::OffsetDateTime::parse(&self.time, &time::format_description::well_known::Rfc3339)
                .is_ok(),
            EXIT_SCHEMA,
            "predecessor event time is not RFC3339"
        );
        ensure!(
            crate::confirmation::valid_challenge(&self.confirmation.challenge),
            EXIT_SCHEMA,
            "a predecessor confirmation carries an invalid challenge"
        );
        check_lower_sha256(
            &self.confirmation.payload_sha256,
            "predecessor payload seal",
        )?;
        ensure!(
            self.confirmation.payload_sha256 == self.payload_sha256()?,
            EXIT_SCHEMA,
            "predecessor confirmation does not match the event payload"
        );
        if self.action.carries_content() {
            ensure!(
                !self.text.trim().is_empty(),
                EXIT_SCHEMA,
                "{} requires text",
                self.action.label()
            );
        } else {
            ensure!(
                self.text.is_empty() && self.based_on.is_none() && self.refs.is_empty(),
                EXIT_SCHEMA,
                "{} does not carry content",
                self.action.label()
            );
        }
        if let Some(based_on) = &self.based_on {
            ensure!(
                crate::model::is_canonical_git_oid(based_on),
                EXIT_SCHEMA,
                "predecessor based_on must be a full resolved Git object id"
            );
        }
        for reference in &self.refs {
            reference.validate()?;
        }
        match &self.action {
            Action::SectionRevised { section } | Action::SectionDeleted { section } => {
                ensure!(
                    *section > 0,
                    EXIT_SCHEMA,
                    "predecessor section ids start at 1"
                );
            }
            Action::SectionMerged { absorbs } => {
                ensure!(
                    absorbs.len() >= 2,
                    EXIT_SCHEMA,
                    "a predecessor merge absorbs at least two sections"
                );
                let mut unique = absorbs.clone();
                unique.sort_unstable();
                unique.dedup();
                ensure!(
                    unique.len() == absorbs.len(),
                    EXIT_SCHEMA,
                    "a predecessor merge cannot absorb the same section twice"
                );
                for absorbed in absorbs {
                    ensure!(
                        *absorbed > 0,
                        EXIT_SCHEMA,
                        "predecessor section ids start at 1"
                    );
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn section(&self, id: u64) -> Result<Section> {
        let section = Section {
            id,
            text: self.text.clone(),
            based_on: self.based_on.clone(),
            refs: self.refs.clone(),
            sha256: String::new(),
            confirmed_at: self.time.clone(),
        };
        Ok(Section {
            sha256: section.recomputed_sha256()?,
            ..section
        })
    }
}

/// The predecessor's own reducer.
///
/// Its rules, not this build's. The merge allocates a fresh id and consumes
/// every participant; `open` is the whole attention vocabulary; a Section's
/// `confirmed_at` is the Event's own time. Reading a predecessor history under
/// the current reducer would reconstruct an Object the released build could
/// never have produced, which is exactly the failure a separate schema exists to
/// prevent.
fn project(object: &mut Object, event: &Event) -> Result<()> {
    match &event.action {
        Action::ObjectCreated => {
            ensure!(
                object.rev == 0 && object.sections.is_empty(),
                EXIT_INVARIANT,
                "object_created must be the first action"
            );
            object.title.clone_from(&event.text);
        }
        Action::ObjectRenamed => {
            require_open(object, "object_renamed")?;
            object.title.clone_from(&event.text);
        }
        Action::SectionAdded => {
            require_open(object, "section_added")?;
            let id = take_id(object)?;
            object.sections.push(event.section(id)?);
        }
        Action::SectionRevised { section } => {
            require_open(object, "section_revised")?;
            object.section(*section)?;
            let replacement = event.section(*section)?;
            let slot = object
                .sections
                .iter_mut()
                .find(|held| held.id == *section)
                .expect("section presence checked above");
            *slot = replacement;
        }
        Action::SectionMerged { absorbs } => {
            require_open(object, "section_merged")?;
            for absorbed in absorbs {
                object.section(*absorbed)?;
            }
            let id = take_id(object)?;
            object.sections.retain(|held| !absorbs.contains(&held.id));
            object.sections.push(event.section(id)?);
        }
        Action::SectionDeleted { section } => {
            require_open(object, "section_deleted")?;
            object.section(*section)?;
            object.sections.retain(|held| held.id != *section);
        }
        Action::ObjectClosed => {
            require_open(object, "object_closed")?;
            object.state = State::Closed;
        }
        Action::ObjectReopened => {
            ensure!(
                object.state == State::Closed,
                EXIT_INVARIANT,
                "object_reopened requires a closed object"
            );
            object.state = State::Open;
        }
    }
    object.sections.sort_by_key(|section| section.id);
    object.rev = event.rev;
    Ok(())
}

fn require_open(object: &Object, what: &str) -> Result<()> {
    ensure!(
        object.state == State::Open,
        EXIT_INVARIANT,
        "{what} requires an open object; reopen it first"
    );
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

/// Decode one predecessor Object file, held to the released member set.
///
/// The members are checked before anything decodes, because a decoder with
/// defaults reads a member the generation never had as though the generation
/// had defaulted it.
pub fn decode_object(path: &Path, id: &str, text: &str) -> Result<Object> {
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))?;
    crate::proof::stored_within_safe_integers(&value, &path.display().to_string())?;
    check_object_members(path, &value)?;
    let object: Object = serde_json::from_value(value)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))?;
    object.validate()?;
    ensure!(
        object.id == id,
        EXIT_SCHEMA,
        "{}: object id {:?} does not match its filename",
        path.display(),
        object.id
    );
    Ok(object)
}

const OBJECT_REQUIRED: &[&str] = &["id", "title", "state", "rev", "next_section_id", "sections"];
const OBJECT_OPTIONAL: &[&str] = &["format", "version"];
const SECTION_REQUIRED: &[&str] = &["id", "text", "refs", "sha256", "confirmed_at"];
const SECTION_OPTIONAL: &[&str] = &["based_on"];

fn check_object_members(path: &Path, value: &serde_json::Value) -> Result<()> {
    let object = value.as_object().ok_or_else(|| {
        Error::new(
            EXIT_SCHEMA,
            format!("{}: object must be a JSON object", path.display()),
        )
    })?;
    check_members(
        path,
        "released Object",
        object,
        OBJECT_REQUIRED,
        OBJECT_OPTIONAL,
    )?;
    // A workspace that declares its generation while its Objects still carry the
    // per-resource markers *and* spell the lifecycle `status` is a conversion the
    // predecessor build began and did not finish. The reader needs to be told
    // that rather than "carries `status`, which its generation never had".
    let markers = object.get("format").and_then(serde_json::Value::as_str) == Some(OBJECT_FORMAT)
        && object.get("version").and_then(serde_json::Value::as_u64) == Some(1);
    ensure!(
        !(markers && object.contains_key("status")),
        EXIT_SCHEMA,
        "{}: this Object still carries the older per-resource envelope, so its in-place conversion never finished",
        path.display()
    );
    let sections = object
        .get("sections")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            Error::new(
                EXIT_SCHEMA,
                format!("{}: sections must be an array", path.display()),
            )
        })?;
    for section in sections {
        let section = section.as_object().ok_or_else(|| {
            Error::new(
                EXIT_SCHEMA,
                format!("{}: a section must be a JSON object", path.display()),
            )
        })?;
        check_members(
            path,
            "released Section",
            section,
            SECTION_REQUIRED,
            SECTION_OPTIONAL,
        )?;
    }
    Ok(())
}

fn check_members(
    path: &Path,
    what: &str,
    value: &serde_json::Map<String, serde_json::Value>,
    required: &[&str],
    optional: &[&str],
) -> Result<()> {
    for member in required {
        ensure!(
            value.contains_key(*member),
            EXIT_SCHEMA,
            "{}: a {what} carries {member}",
            path.display()
        );
    }
    for member in value.keys() {
        ensure!(
            required.contains(&member.as_str()) || optional.contains(&member.as_str()),
            EXIT_SCHEMA,
            "{}: {member:?} is not a member of a {what}",
            path.display()
        );
    }
    Ok(())
}

const EVENT_ENVELOPE: &[&str] = &[
    "format",
    "version",
    "event_id",
    "rev",
    "time",
    "action",
    "object",
    "text",
    "refs",
    "confirmation",
];
const EVENT_OPTIONAL: &[&str] = &["based_on"];

/// The parameters each predecessor action flattened beside its own name.
fn action_parameters(label: &str) -> Result<&'static [&'static str]> {
    match label {
        "object_created" | "object_renamed" | "section_added" | "object_closed"
        | "object_reopened" => Ok(&[]),
        "section_revised" | "section_deleted" => Ok(&["section"]),
        "section_merged" => Ok(&["absorbs"]),
        other => Err(Error::new(
            EXIT_SCHEMA,
            format!("{other:?} is not an action the released generation had"),
        )),
    }
}

/// Decode a predecessor Event stream, held to the released envelope.
pub fn decode_events(path: &Path, id: &str, text: &str) -> Result<Vec<Event>> {
    let mut events = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let where_ = format!("{}:{}", path.display(), index + 1);
        let value: serde_json::Value = serde_json::from_str(line)
            .map_err(|error| Error::new(EXIT_SCHEMA, format!("{where_}: {error}")))?;
        crate::proof::stored_within_safe_integers(&value, &where_)?;
        let members = value.as_object().ok_or_else(|| {
            Error::new(EXIT_SCHEMA, format!("{where_}: an event is a JSON object"))
        })?;
        let label = members
            .get("action")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                Error::new(EXIT_SCHEMA, format!("{where_}: an event names its action"))
            })?;
        let mut required: Vec<&str> = EVENT_ENVELOPE.to_vec();
        required.extend_from_slice(action_parameters(label)?);
        check_members(
            Path::new(&where_),
            "released Event",
            members,
            &required,
            EVENT_OPTIONAL,
        )?;
        let confirmation = members
            .get("confirmation")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| {
                Error::new(
                    EXIT_SCHEMA,
                    format!("{where_}: confirmation is a JSON object"),
                )
            })?;
        check_members(
            Path::new(&where_),
            "released confirmation",
            confirmation,
            &["challenge", "payload_sha256"],
            &[],
        )?;
        let event: Event = serde_json::from_value(value)
            .map_err(|error| Error::new(EXIT_SCHEMA, format!("{where_}: {error}")))?;
        event
            .validate(id)
            .map_err(|error| Error::new(error.code, format!("{where_}: {error}")))?;
        events.push(event);
    }
    // Append-only and gapless from 1, which is what makes "the effective state"
    // a well-defined question rather than a guess about which lines survived.
    for (index, event) in events.iter().enumerate() {
        let expected = index as u64 + 1;
        ensure!(
            event.rev == expected,
            EXIT_SCHEMA,
            "{}: predecessor history is a gapless sequence from rev 1, and rev {} is at position {expected}",
            path.display(),
            event.rev
        );
    }
    Ok(events)
}

/// The predecessor's **effective current state** for one Object.
///
/// Effective, not merely stored. The released build wrote the Object file after
/// appending the Event, so a crash between the two leaves durable history the
/// projection has not caught up with. #66 requires migration to take that tail
/// into account, so this replays it — under the predecessor's own reducer — and
/// then requires the stored projection, where there is one, to be exactly what
/// that history derives.
pub fn effective_state(
    object_path: &Path,
    events_path: &Path,
    id: &str,
    stored: Option<&str>,
    history: &str,
) -> Result<Object> {
    let stored = match stored {
        Some(text) => {
            let object = decode_object(object_path, id, text)?;
            object.check_seals()?;
            Some(object)
        }
        None => None,
    };
    let events = decode_events(events_path, id, history)?;
    ensure!(
        !events.is_empty(),
        EXIT_SCHEMA,
        "{id}: a predecessor Object has admitted history"
    );
    ensure!(
        events[0].action == Action::ObjectCreated,
        EXIT_SCHEMA,
        "{id}: predecessor history begins with object_created"
    );
    let mut reconciled = Object::empty(id);
    for event in &events {
        project(&mut reconciled, event).map_err(|error| {
            Error::new(
                EXIT_SCHEMA,
                format!("{id}: predecessor history cannot reconcile: {error}"),
            )
        })?;
    }
    reconciled.validate()?;
    reconciled.check_seals()?;
    if let Some(stored) = stored {
        // The projection may legitimately lag its history by a tail, and may
        // never lead it or disagree with it.
        ensure!(
            stored.rev <= reconciled.rev,
            EXIT_INVARIANT,
            "{id}: the stored predecessor projection is ahead of its own admitted history"
        );
        let mut replayed = Object::empty(id);
        for event in events.iter().filter(|event| event.rev <= stored.rev) {
            project(&mut replayed, event)?;
        }
        ensure!(
            stored.title == replayed.title
                && stored.state == replayed.state
                && stored.rev == replayed.rev
                && stored.next_section_id == replayed.next_section_id
                && stored.sections == replayed.sections,
            EXIT_INVARIANT,
            "{id}: predecessor Object projection is not exactly derivable from admitted history"
        );
    }
    Ok(reconciled)
}

/// A predecessor Section's admission provenance, in the redesigned vocabulary.
///
/// Every predecessor Section went through the Human Gate, because while that
/// generation was current there was no other door. The instant is the one the
/// predecessor recorded, not the migration's — #66 is explicit that a migrated
/// Section keeps the provenance it already had.
pub fn admitted(section: &Section) -> Admitted {
    Admitted::new(Admission::Human, section.confirmed_at.clone())
}

/// The basis a predecessor Section recorded, in the redesigned shape.
pub fn based_on(section: &Section) -> Option<BasedOn> {
    section.based_on.as_ref().map(BasedOn::new)
}
