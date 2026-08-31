//! The object/section model, the confirmed payload, and projection.
//!
//! An object is an aggregate of sections. A section's `text` is always its
//! current wording, because wording only ever changes through a confirmed
//! action. Nothing is derived at read time except staleness, which lives in
//! [`crate::git`].

use crate::semantics::{
    needs_attention, validate_state, Admission, Admitted, BasedOn, ObjectType, Relation,
    RelationType, Role, State, Supplement,
};
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

/// Git resolves selectors such as `HEAD` for input, but admitted records pin
/// the immutable object id it produced. Accept SHA-1 and SHA-256 repositories.
pub fn is_canonical_git_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// A Section dependency.
///
/// One shape, because there is one generation. The predecessor's whole-content
/// reference is decoded only by [`crate::predecessor`], and only for the
/// migration that converts it — it is never a value a current resource can
/// hold, so it has no variant here to be mistaken for one.
pub type Ref = crate::dependency::SelectiveRef;

/// The part of a section that is put through admission — the wording a human is
/// shown at the gate, and the wording Rule Review is run against.
///
/// Every selectable semantic field a Section carries lives here, and only here.
/// The Section seal additionally covers identity and provenance, while a Ref can
/// select only these semantics plus `admission`. A semantic field held outside
/// this value would be authoritative meaning a Ref cannot pin.
///
/// Everything optional is skipped when empty, per the canonical omission rule:
/// an absent optional, an empty array and an empty object are all simply not
/// written, and none of them is ever encoded as `null`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Content {
    /// A short label for the Section, for navigation and for the reader's eye.
    ///
    /// Semantic rather than presentational, and therefore selectable by a Ref:
    /// a header is what a reader uses to decide whether the wording beneath it
    /// is the wording they meant to depend on, so a header that moves under a
    /// dependency is drift like any other.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    pub text: String,
    /// Ordered, unlike `refs` and `relations`: these are excerpts a reader goes
    /// through in sequence, so moving one is a change to the assertion.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Supplement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub based_on: Option<BasedOn>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<Ref>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<Relation>,
}

impl Content {
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
    pub fn canonicalize_order(&mut self) -> Result<()> {
        crate::proof::canonical_set(&mut self.refs, "reference")?;
        crate::proof::canonical_set(&mut self.relations, "relation")?;
        Ok(())
    }

    pub(crate) fn require_canonical_order(&self) -> Result<()> {
        let mut canonical = self.clone();
        canonical.canonicalize_order()?;
        ensure!(
            canonical.refs == self.refs && canonical.relations == self.relations,
            EXIT_SCHEMA,
            "refs and relations must use canonical set order"
        );
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        // Empty is not a header. An optional member whose absence and whose
        // empty value mean the same thing is one member with two spellings,
        // and #66's omission rule says absence is written by omitting it.
        if let Some(header) = &self.header {
            ensure!(
                !header.is_empty(),
                EXIT_SCHEMA,
                "a section with no header omits the member rather than carrying an empty one"
            );
        }
        if let Some(based_on) = &self.based_on {
            based_on.validate()?;
        }
        for entry in &self.content {
            entry.validate()?;
        }
        // A stored Ref validates at construction: `SelectiveRef::stored` is the
        // only way one enters the model, and it refuses anything this loop
        // could have re-checked. Nothing to do here.
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

/// The complete resulting semantic state of one Section, without its identity
/// or its seal.
///
/// This is what an Event carries. #66 requires a Section payload to include
/// `admitted`, so replay reads a Section's provenance out of the value that was
/// admitted rather than inferring it from the Event's own metadata. The two are
/// different facts — the Event's metadata says how the Event entered history,
/// this says what the Section now means and by whose authority — and migration
/// is the case where they legitimately differ.
///
/// No `deny_unknown_fields`, here or on any of the other flattened envelopes:
/// serde cannot carry it through `#[serde(flatten)]`, and a struct that declares
/// both refuses its own members. Strictness is recovered where it can actually
/// be exact — `store::check_event_record` requires the stored bytes to be the
/// canonical serialization of what they parsed to, which refuses an extra
/// member as surely and can say which one.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct SectionValue {
    pub admitted: Admitted,
    #[serde(flatten)]
    pub content: Content,
}

impl SectionValue {
    pub fn new(admitted: Admitted, content: Content) -> Self {
        Self { admitted, content }
    }

    pub fn validate(&self) -> Result<()> {
        self.admitted.validate()?;
        self.content.validate()
    }
}

/// Schema-exact, and that is a trust boundary rather than strictness for its own
/// sake.
///
/// A current resource carries exactly the fields its version defines, and an
/// unknown one fails closed. Without that, `admitted` being absent from the
/// persisted shape would mean a file could *carry* it and be read as though it
/// did not: serde would ignore the member and this build would reconstruct
/// something — and a reconstruction is only exact for the exact representation
/// this generation defines, never for a file already answering the question
/// differently.
///
/// It is the read-side counterpart of the write boundary in
/// [`crate::store::save_object`]. Writes must not drop authority state they
/// cannot represent; reads must not silently reinterpret authority state they
/// were not expecting.
///
/// `deny_unknown_fields` cannot travel through `#[serde(flatten)]`, so the
/// members are written out here rather than embedding [`SectionValue`]. The two
/// are kept in step by [`Section::value`] and [`Section::from_value`], which are
/// the only ways either is built from the other.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Section {
    pub id: u64,
    /// Which door these exact semantics came through, and when.
    pub admitted: Admitted,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Supplement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub based_on: Option<BasedOn>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<Ref>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<Relation>,
    /// Integrity seal over the complete canonical Section except itself.
    ///
    /// Identity and provenance are inside it, not only the wording. A seal over
    /// semantics alone would let a Section be repointed at another id, or have
    /// its admission rewritten from agent to human, and still verify.
    pub digest: String,
}

impl Section {
    pub fn content(&self) -> Content {
        Content {
            header: self.header.clone(),
            role: self.role,
            text: self.text.clone(),
            content: self.content.clone(),
            based_on: self.based_on.clone(),
            refs: self.refs.clone(),
            relations: self.relations.clone(),
        }
    }

    /// The complete semantic value, as an Event carries it.
    pub fn value(&self) -> SectionValue {
        SectionValue {
            admitted: self.admitted.clone(),
            content: self.content(),
        }
    }

    /// Build a Section from an admitted value, sealed.
    ///
    /// The seal is computed here rather than supplied, because a constructor
    /// that took one would be a way to store a Section whose digest describes
    /// something else.
    pub fn from_value(id: u64, value: SectionValue) -> Result<Self> {
        let mut section = Self {
            id,
            admitted: value.admitted,
            header: value.content.header,
            role: value.content.role,
            text: value.content.text,
            content: value.content.content,
            based_on: value.content.based_on,
            refs: value.content.refs,
            relations: value.content.relations,
            digest: String::new(),
        };
        section.digest = section.recomputed_digest()?;
        Ok(section)
    }

    /// Recompute the seal from what is stored, so `verify` needs nothing else.
    pub fn recomputed_digest(&self) -> Result<String> {
        crate::digest::SECTION
            .emit(self.digest_under(crate::digest::SECTION.current)?)
            .map(|value| value.to_string())
    }

    /// The seal under one named contract version.
    ///
    /// Versioned rather than current-only for the reason every other contract
    /// here is: a stored seal must be checked under the contract it names, not
    /// under whichever emitter happens to be newest.
    pub fn digest_under(&self, version: u32) -> Result<String> {
        match version {
            1 => {
                let mut value = serde_json::to_value(self).map_err(|error| {
                    Error::new(EXIT_SCHEMA, format!("canonical section: {error}"))
                })?;
                if let serde_json::Value::Object(members) = &mut value {
                    members.remove("digest");
                }
                Ok(crate::proof::sha256_of(&crate::proof::canonical_bytes(
                    &value, "section",
                )?))
            }
            other => Err(Error::new(
                EXIT_SCHEMA,
                format!("SectionDigestContract: no contract for version {other}"),
            )),
        }
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(self.id > 0, EXIT_SCHEMA, "section ids start at 1");
        self.admitted
            .validate()
            .map_err(|error| Error::new(error.code, format!("§{}: {error}", self.id)))?;
        crate::digest::SECTION.verify(&self.digest)?;
        // Two pieces of the semantic surface are Human-authoritative in
        // themselves, so an Agent Section may not carry them at all.
        //
        // A relation is a claim about the Object's own standing — `superseded_by`
        // is half of the supersession invariant, and `implemented_by` asserts
        // that a repository artifact realizes admitted knowledge. Neither is
        // wording somebody can read and check; both are statements the record
        // then acts on. `role=supersession` is the other half of the same act:
        // it is the human-readable reason an Object was retired, and retiring an
        // Object is not something an agent does.
        //
        // Checked on the stored Section rather than only on the way in, because
        // a rule the write path alone enforces is one hand-edit away from being
        // untrue of the record.
        if self.admitted.by == Admission::Agent {
            ensure!(
                self.relations.is_empty(),
                EXIT_SCHEMA,
                "§{}: relations are human-authoritative, so an agent-admitted section does not carry them",
                self.id
            );
            ensure!(
                self.role != Some(Role::Supersession),
                EXIT_SCHEMA,
                "§{}: role=supersession is the reason an object was retired, which is a human admission",
                self.id
            );
        }
        self.content().validate()
    }
}

/// Schema-exact for the same reason [`Section`] is: a resource that carries a
/// field this version never defined is not a file of this version, whatever else
/// about it reads correctly.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Object {
    pub id: String,
    pub title: String,
    /// Optional, and absent is a first-class answer rather than a missing one.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub object_type: Option<ObjectType>,
    /// The one lifecycle field.
    pub state: State,
    /// Increments on every admitted action. Candidates and reviews pin it, so
    /// one prepared against an older state cannot be admitted after the Object
    /// moved.
    pub rev: u64,
    /// Monotonic and never reset. Section ids are never reused, so this cannot
    /// be derived as `max(existing) + 1`: that would hand out the id of a section
    /// that was deleted, and every outside reference to it would silently point
    /// at different content.
    pub next_section_id: u64,
    /// Omitted when empty, per the canonical omission rule. An Object with no
    /// Sections and an Object whose `sections` is `[]` would otherwise be one
    /// record with two spellings, and both would seal differently.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<Section>,
    /// Aggregate integrity over the complete canonical Object except itself.
    pub digest: String,
}

impl Object {
    pub fn new(id: String, title: String) -> Result<Self> {
        let mut object = Self {
            id,
            title,
            object_type: None,
            state: State::Open,
            rev: 0,
            next_section_id: 1,
            sections: Vec::new(),
            digest: String::new(),
        };
        object.reseal()?;
        object.validate()?;
        Ok(object)
    }

    /// Recompute the aggregate seal to match what the Object now holds.
    ///
    /// Every write path goes through this rather than assigning `digest`, so a
    /// stored Object whose seal describes a different value cannot be produced
    /// by forgetting a step.
    pub fn reseal(&mut self) -> Result<()> {
        self.digest = String::new();
        self.digest = self.recomputed_digest()?;
        Ok(())
    }

    pub fn recomputed_digest(&self) -> Result<String> {
        crate::digest::OBJECT
            .emit(self.digest_under(crate::digest::OBJECT.current)?)
            .map(|value| value.to_string())
    }

    pub fn digest_under(&self, version: u32) -> Result<String> {
        match version {
            1 => {
                let mut value = serde_json::to_value(self).map_err(|error| {
                    Error::new(EXIT_SCHEMA, format!("canonical object: {error}"))
                })?;
                if let serde_json::Value::Object(members) = &mut value {
                    members.remove("digest");
                }
                Ok(crate::proof::sha256_of(&crate::proof::canonical_bytes(
                    &value, "object",
                )?))
            }
            other => Err(Error::new(
                EXIT_SCHEMA,
                format!("ObjectDigestContract: no contract for version {other}"),
            )),
        }
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

    /// Admitted content is not revised while nobody is looking at it.
    ///
    /// This is the "reopen first" rule stated in the terms typed lifecycles give
    /// it. For an untyped Object attention is exactly `open`, so nothing about
    /// the untyped behaviour changed; for a typed one it is the derived class, so an
    /// `accepted` design has to be moved back to `draft` or `proposed` before it
    /// can be reworded. Renewed engineering work returns to the attention set
    /// rather than happening out of sight of everyone who reads the default
    /// listing.
    ///
    /// The rule is about *renewed* engineering work: wording that was admitted
    /// once being changed again while nobody is looking.
    ///
    /// It does **not** mean two confirmations are required. The guard reads the
    /// state the confirmation *arrives at*, so a guarded action may carry a
    /// [`Payload::becomes`] to a destination that needs attention and land both
    /// halves in one confirmed operation. That is the canonical path, and this
    /// function is what makes it legal rather than something it routes around.
    /// Classifying separately first remains available and says something
    /// different — two authoritative statements, because there were two. It is
    /// a choice, not the required route.
    ///
    /// So the refusal below fires on a guarded action carrying no destination,
    /// and on `object.closed`, which is not an action a destination is
    /// admissible on: for that one, classifying first really is the only way
    /// through.
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
        crate::digest::OBJECT.verify(&self.digest)?;
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

/// A Section id is positive wherever it is written down.
///
/// Stated once and used by every action that names one, because "ids start at
/// 1" is a property of the field rather than of any one operation. A payload is
/// validated when an Event is *loaded*, so a value outside the field's schema
/// that only the reducer catches is a stored record that passed validation.
fn check_section_id(section: u64) -> Result<()> {
    ensure!(
        section > 0,
        EXIT_SCHEMA,
        "section ids start at 1, so §{section} is not one"
    );
    Ok(())
}

/// Which sections a merge consolidates, and which one comes out the other side.
///
/// One shape. An explicit Section survives, keeping its id and taking the merged
/// wording; the sources are consumed and their ids are never handed out again.
/// Nothing is allocated, so a reference to the survivor keeps meaning what it
/// meant.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Merge {
    pub destination: u64,
    pub sources: Vec<u64>,
}

impl Merge {
    /// Every Section this merge reads, survivor included.
    pub fn participants(&self) -> Vec<u64> {
        let mut all = vec![self.destination];
        all.extend(self.sources.iter().copied());
        all
    }

    /// The Sections this merge removes.
    pub fn consumed(&self) -> &[u64] {
        &self.sources
    }

    fn validate(&self) -> Result<()> {
        // The field's own lower bound, checked where the field is validated.
        // Section ids start at 1, so 0 is not a section that happens to be
        // missing — it is a value outside the schema, and letting the reducer
        // discover it means a persisted Event was accepted as well formed on the
        // strength of what some Object happened to contain.
        check_section_id(self.destination)?;
        for source in &self.sources {
            check_section_id(*source)?;
        }
        ensure!(
            !self.sources.is_empty(),
            EXIT_INVARIANT,
            "a merge needs at least one section to consume"
        );
        ensure!(
            !self.sources.contains(&self.destination),
            EXIT_INVARIANT,
            "§{} survives the merge, so it cannot also be consumed by it",
            self.destination
        );
        // `sources[]` is a protocol-defined set, and it takes the one shared
        // algorithm: JCS each element, then order by those bytes. The two
        // disagree as soon as the ids differ in digit count: `[2, 10]` is
        // ascending, and canonical is `[10, 2]`, because "10" sorts before "2".
        // A reader may still render numerically.
        let mut canonical = self.sources.clone();
        crate::proof::canonical_set(&mut canonical, "merge source")?;
        ensure!(
            canonical == self.sources,
            EXIT_INVARIANT,
            "the sections a merge consumes are listed once each, in canonical set order"
        );
        Ok(())
    }
}

/// What an Event says was done, and the data that says it.
///
/// The variant names are the protocol's Event types minus their `.v1` suffix;
/// [`Action::event_type`] spells the persisted one. Splitting the version off
/// the tag is deliberate: the version belongs to the *type*, and a Rust variant
/// called `SectionUpdatedV1` would put a schema generation into every match arm
/// in the crate.
///
/// `data` is always present in the envelope and may be `{}` — that is one of the
/// two explicit exceptions to the canonical omission rule, and it is why
/// `object.repaired.v1` carries an empty object rather than no member.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "type", content = "data")]
pub enum Action {
    /// The Object is brought into existence, neutrally: untyped and open.
    #[serde(rename = "object.created.v1")]
    ObjectCreated { title: String },
    #[serde(rename = "object.renamed.v1")]
    ObjectRenamed {
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        becomes: Option<Destination>,
    },
    /// Declare what this Object is and what state it is in, both explicitly.
    ///
    /// One action rather than two, because a type change without a destination
    /// state has no answer: the vocabularies do not overlap, and any mapping
    /// engr invented would be engr deciding that an `accepted` design becomes an
    /// `accepted` decision — which is a judgement, not a conversion. Both values
    /// are always restated, so the human reads the whole destination rather than
    /// a delta against something they have to remember.
    #[serde(rename = "object.classified.v1")]
    ObjectClassified {
        #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
        object_type: Option<ObjectType>,
        state: State,
    },
    /// Move the Object within the state vocabulary its current type defines.
    ///
    /// The general form of what `object.closed` and `object.reopened` used to
    /// say for an untyped Object. Those two named a destination in their own
    /// spelling and had no meaning at all on a typed Object; this one names the
    /// destination outright and works wherever the destination is legal for the
    /// type the Object already has. Changing the *type* is still
    /// `object.classified.v1`, because that is a different judgement.
    #[serde(rename = "object.state_changed.v1")]
    ObjectStateChanged { state: State },
    /// Replace this Object with another, in one confirmation.
    ///
    /// The state, the replacement relation and the human-readable reason are one
    /// semantic act. Splitting them into separately confirmable steps would be
    /// easier to implement and would mean a record can sit in the state where it
    /// says it was replaced and cannot say by what.
    #[serde(rename = "object.superseded.v1")]
    ObjectSuperseded { value: SectionValue },
    /// Restore a current projection that failed integrity to what admitted
    /// history derives.
    ///
    /// A distinct type rather than an ordinary mutation with a flag, because it
    /// means something an ordinary edit never means: the stored authority had
    /// stopped being trustworthy, and was put back from previously provable
    /// history. That stays visible in immutable history rather than reading as a
    /// normal revision.
    ///
    /// It carries no parameters because it cannot: repair restores *exactly* the
    /// replay-derived projection and nothing else. Anything a person wants to
    /// change afterwards goes through the normal admission path, so the record
    /// reads `object.repaired.v1` then `section.updated.v1` rather than letting
    /// one repair quietly legitimize semantics nobody admitted.
    #[serde(rename = "object.repaired.v1")]
    ObjectRepaired {},
    /// The bootstrap Event of a migrated Object, and the only one that carries a
    /// whole Object snapshot.
    ///
    /// Emitted only by the supported released-workspace migration, exactly once
    /// per migrated Object, and permanently revision 1 of its stream. It exists
    /// because the predecessor's own history is discarded rather than translated
    /// — so without it a migrated Object would have current authority and no
    /// history at all, and nothing could ever answer "what does replay derive".
    #[serde(rename = "object.migrated.v1")]
    ObjectMigrated { snapshot: Box<Snapshot> },
    #[serde(rename = "section.created.v1")]
    SectionCreated {
        value: SectionValue,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        becomes: Option<Destination>,
    },
    #[serde(rename = "section.updated.v1")]
    SectionUpdated {
        section: u64,
        value: SectionValue,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        becomes: Option<Destination>,
    },
    #[serde(rename = "section.deleted.v1")]
    SectionDeleted {
        section: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        becomes: Option<Destination>,
    },
    /// Destination-survives: the destination keeps its id and takes `value`,
    /// the sources are consumed, and no id is reused.
    #[serde(rename = "section.merged.v1")]
    SectionMerged {
        #[serde(flatten)]
        merge: Merge,
        value: SectionValue,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        becomes: Option<Destination>,
    },
}

/// The complete current Object state a migration bootstrap Event carries.
///
/// Object identity is excluded because the stream already binds it — every
/// Event's digest names the owning Object, so repeating it here would be a
/// second answer to a question that already has one. Revision is excluded
/// because the Event's own `rev` fixes it at 1. Seals are excluded because they
/// are integrity over a persisted representation, and this is a payload rather
/// than that representation.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub title: String,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub object_type: Option<ObjectType>,
    pub state: State,
    pub next_section_id: u64,
    /// Required, and may be `[]`.
    ///
    /// The one place a Section list is written out empty rather than omitted: a
    /// migration snapshot is a complete statement of what the Object became, so
    /// "it has no Sections" is an answer it must give rather than leave out.
    pub sections: Vec<SnapshotSection>,
}

/// One Section inside a migration snapshot: identity and value, no seal.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct SnapshotSection {
    pub id: u64,
    #[serde(flatten)]
    pub value: SectionValue,
}

impl Action {
    /// The persisted Event type.
    pub fn event_type(&self) -> &'static str {
        match self {
            Action::ObjectCreated { .. } => "object.created.v1",
            Action::ObjectRenamed { .. } => "object.renamed.v1",
            Action::ObjectClassified { .. } => "object.classified.v1",
            Action::ObjectStateChanged { .. } => "object.state_changed.v1",
            Action::ObjectSuperseded { .. } => "object.superseded.v1",
            Action::ObjectRepaired {} => "object.repaired.v1",
            Action::ObjectMigrated { .. } => "object.migrated.v1",
            Action::SectionCreated { .. } => "section.created.v1",
            Action::SectionUpdated { .. } => "section.updated.v1",
            Action::SectionDeleted { .. } => "section.deleted.v1",
            Action::SectionMerged { .. } => "section.merged.v1",
        }
    }

    /// Rebuild an action from a Challenge subject's `action` and `value`.
    ///
    /// The inverse of [`Action::command`], and deliberately the only way back:
    /// a Challenge names what is being asked for, and turning that into what
    /// enters history is a mapping with one place to live. A command outside the
    /// vocabulary is refused by name rather than by a deserializer talking about
    /// variants.
    pub fn from_command(command: &str, value: serde_json::Value) -> Result<Self> {
        let event_type = match command {
            "create" => "object.created.v1",
            "rename" => "object.renamed.v1",
            "classify" => "object.classified.v1",
            "change_state" => "object.state_changed.v1",
            "supersede" => "object.superseded.v1",
            "repair" => "object.repaired.v1",
            "section.create" => "section.created.v1",
            "section.update" => "section.updated.v1",
            "section.delete" => "section.deleted.v1",
            "section.merge" => "section.merged.v1",
            other => {
                return Err(Error::new(
                    EXIT_SCHEMA,
                    format!(
                        "{other:?} is not an Object command; the vocabulary is {}",
                        crate::confirmation::OBJECT_COMMANDS.join(", ")
                    ),
                ))
            }
        };
        serde_json::from_value(serde_json::json!({ "type": event_type, "data": value }))
            .map_err(|error| Error::new(EXIT_SCHEMA, format!("{command}: {error}")))
    }

    /// The command vocabulary a Challenge subject names.
    ///
    /// Distinct from [`Action::event_type`] on purpose: one names what is being
    /// asked for, the other names what entered history. They correspond one to
    /// one today, and nothing should make a reader assume they must.
    pub fn command(&self) -> &'static str {
        match self {
            Action::ObjectCreated { .. } => "create",
            Action::ObjectRenamed { .. } => "rename",
            Action::ObjectClassified { .. } => "classify",
            Action::ObjectStateChanged { .. } => "change_state",
            Action::ObjectSuperseded { .. } => "supersede",
            Action::ObjectRepaired {} => "repair",
            Action::ObjectMigrated { .. } => "migrate",
            Action::SectionCreated { .. } => "section.create",
            Action::SectionUpdated { .. } => "section.update",
            Action::SectionDeleted { .. } => "section.delete",
            Action::SectionMerged { .. } => "section.merge",
        }
    }

    /// The label used in human-facing output.
    pub fn label(&self) -> &'static str {
        self.event_type()
    }

    /// The Section value this action admits, where it admits one.
    pub fn value(&self) -> Option<&SectionValue> {
        match self {
            Action::ObjectSuperseded { value }
            | Action::SectionCreated { value, .. }
            | Action::SectionUpdated { value, .. }
            | Action::SectionMerged { value, .. } => Some(value),
            _ => None,
        }
    }

    pub fn value_mut(&mut self) -> Option<&mut SectionValue> {
        match self {
            Action::ObjectSuperseded { value }
            | Action::SectionCreated { value, .. }
            | Action::SectionUpdated { value, .. }
            | Action::SectionMerged { value, .. } => Some(value),
            _ => None,
        }
    }

    /// Actions that carry wording a human must read.
    pub fn carries_content(&self) -> bool {
        self.value().is_some() || self.carries_title()
    }

    /// Where this action leaves the Object, when it carries a destination.
    pub fn becomes(&self) -> Option<&Destination> {
        match self {
            Action::ObjectRenamed { becomes, .. }
            | Action::SectionCreated { becomes, .. }
            | Action::SectionUpdated { becomes, .. }
            | Action::SectionDeleted { becomes, .. }
            | Action::SectionMerged { becomes, .. } => becomes.as_ref(),
            _ => None,
        }
    }

    pub fn set_becomes(&mut self, destination: Option<Destination>) -> Result<()> {
        match self {
            Action::ObjectRenamed { becomes, .. }
            | Action::SectionCreated { becomes, .. }
            | Action::SectionUpdated { becomes, .. }
            | Action::SectionDeleted { becomes, .. }
            | Action::SectionMerged { becomes, .. } => {
                *becomes = destination;
                Ok(())
            }
            other => {
                ensure!(
                    destination.is_none(),
                    EXIT_INVARIANT,
                    "{} sets the object's own state, so it cannot also carry one",
                    other.event_type()
                );
                Ok(())
            }
        }
    }

    /// The actions that refuse to run on an Object nobody is looking at.
    ///
    /// Exactly the ones a destination is for, and the list is shared with the
    /// guard rather than written twice. That sharing is the whole rule: a
    /// destination is admissible on an action *because* the guard would
    /// otherwise refuse it. An action that sets the Object's own state cannot
    /// also be handed one, and `object.superseded.v1` is exempt from the guard
    /// entirely, so neither takes a destination.
    pub fn requires_attention(&self) -> bool {
        matches!(
            self,
            Action::ObjectRenamed { .. }
                | Action::SectionCreated { .. }
                | Action::SectionUpdated { .. }
                | Action::SectionDeleted { .. }
                | Action::SectionMerged { .. }
        )
    }

    /// Actions that add wording as a new Section rather than replacing existing
    /// wording or a label.
    pub fn adds_section(&self) -> bool {
        matches!(
            self,
            Action::SectionCreated { .. } | Action::ObjectSuperseded { .. }
        )
    }

    /// Actions whose content is the object's title rather than a section's
    /// wording. The gate holds both to the same shape, so a body cannot be
    /// pasted into a label through the door that was added later.
    pub fn carries_title(&self) -> bool {
        matches!(
            self,
            Action::ObjectCreated { .. } | Action::ObjectRenamed { .. }
        )
    }

    pub fn title(&self) -> Option<&str> {
        match self {
            Action::ObjectCreated { title } | Action::ObjectRenamed { title, .. } => Some(title),
            _ => None,
        }
    }
}

/// Exactly what a human is asked to assent to. The action is inside the digest,
/// so "delete §3" cannot become "delete §5" after it was displayed.
///
/// The object identity travels beside the action rather than inside `data`,
/// because the durable Event does not repeat it — the stream and the Event
/// digest bind it — while a Challenge subject must name it outright.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Payload {
    pub object: String,
    #[serde(flatten)]
    pub action: Action,
}

/// Where a destination lands the Object.
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
    pub fn new(object: impl Into<String>, action: Action) -> Self {
        Self {
            object: object.into(),
            action,
        }
    }

    /// The Section value this payload admits, where it admits one.
    pub fn value(&self) -> Option<&SectionValue> {
        self.action.value()
    }

    pub fn content(&self) -> Content {
        self.action
            .value()
            .map(|value| value.content.clone())
            .unwrap_or_default()
    }

    pub fn becomes(&self) -> Option<&Destination> {
        self.action.becomes()
    }

    pub fn validate(&self) -> Result<()> {
        validate_object_id(&self.object)?;
        if let Some(value) = self.action.value() {
            value.validate()?;
            // Text may be empty only when there is non-empty literal content to
            // carry the meaning instead. Blank is not a section.
            ensure!(
                !value.content.text.trim().is_empty() || !value.content.content.is_empty(),
                EXIT_USAGE,
                "{} requires text, or literal content to stand in its place",
                self.action.event_type()
            );
        }
        if let Some(title) = self.action.title() {
            ensure!(
                !title.trim().is_empty(),
                EXIT_USAGE,
                "{} requires a title",
                self.action.event_type()
            );
        }
        match &self.action {
            Action::ObjectClassified { object_type, state } => {
                validate_state(EXIT_USAGE, *object_type, *state)?;
            }
            Action::SectionMerged { merge, .. } => merge.validate()?,
            // The same rule, for the other two actions that name a Section.
            // A payload naming §0 was once accepted here and only refused later,
            // by a lookup that failed for a different reason.
            Action::SectionUpdated { section, .. } | Action::SectionDeleted { section, .. } => {
                check_section_id(*section)?
            }
            Action::ObjectMigrated { snapshot } => snapshot.validate()?,
            _ => {}
        }
        // `superseded_by` may only arrive through the one action that also sets
        // the state, and that action may carry nothing else. Any other way in
        // would let the relation and the state be confirmed apart, which is the
        // whole thing supersession is defined to prevent.
        if let Action::ObjectSuperseded { value } = &self.action {
            ensure!(
                value.content.role == Some(Role::Supersession),
                EXIT_INVARIANT,
                "object.superseded.v1 carries the reason it was replaced, so its section is role=supersession"
            );
            ensure!(
                value.content.relations.len() == 1
                    && value.content.relations[0].relation == RelationType::SupersededBy,
                EXIT_INVARIANT,
                "object.superseded.v1 carries exactly one relation, the superseded_by naming the replacement"
            );
        } else if let Some(value) = self.action.value() {
            ensure!(
                !value
                    .content
                    .relations
                    .iter()
                    .any(|relation| relation.relation == RelationType::SupersededBy),
                EXIT_INVARIANT,
                "a superseded_by relation only enters through object.superseded.v1, which confirms the state, the replacement and the reason together"
            );
        }
        Ok(())
    }
}

impl Snapshot {
    fn validate(&self) -> Result<()> {
        validate_state(EXIT_SCHEMA, self.object_type, self.state)?;
        ensure!(
            self.next_section_id > 0,
            EXIT_SCHEMA,
            "next_section_id must start at 1"
        );
        let mut ids = BTreeSet::new();
        for section in &self.sections {
            check_section_id(section.id)?;
            section.value.validate()?;
            ensure!(
                ids.insert(section.id),
                EXIT_SCHEMA,
                "migration snapshot lists §{} more than once",
                section.id
            );
            ensure!(
                section.id < self.next_section_id,
                EXIT_SCHEMA,
                "migration snapshot next_section_id {} would reuse live §{}",
                self.next_section_id,
                section.id
            );
        }
        Ok(())
    }
}

/// What a Human typed.
///
/// The challenge is a random token a human hands back, and the *only* thing the
/// record keeps of it. #66 is explicit that the Challenge digest is local and is
/// not durable Event provenance: the file it seals is gone by the time anybody
/// reads this, so storing its hash would be storing a proof of something
/// unverifiable. What the record needs to say is which question was answered.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct HumanConfirmation {
    pub challenge: String,
}

/// What Rule Review concluded, as durable provenance.
///
/// Minimal on purpose: an outcome and an identity. The EventStore does not keep
/// a ReviewSeries, per-Rule counters, or the Agent's natural-language
/// reasoning — those belong to the moment, not to the record.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct ReviewProvenance {
    pub outcome: ReviewOutcome,
    pub digest: String,
}

/// How a reviewed mutation ended up admitted.
///
/// There is no `failed` here, and that is the point: a failed review does not
/// produce an Event. `overridden` records that a human looked at a failed or
/// exhausted review and admitted the mutation anyway — which is a thing only a
/// human can do, and a thing the record must not lose.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ReviewOutcome {
    Passed,
    Overridden,
}

/// How this Event was allowed into durable history.
///
/// Deliberately not the same fact as [`crate::semantics::Admitted`] on a
/// Section. That one says what the Section's current wording is worth; this one
/// says why the Event was let in. Normally they agree. Migration is where they
/// do not: the bootstrap Event records the migration's own Human confirmation
/// and its time, while every Section inside it keeps the admission provenance it
/// already had.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct EventAdmission {
    pub by: Admission,
    pub at: String,
    /// Present exactly when `by = human`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmation: Option<HumanConfirmation>,
    /// Absent when no Rule Review applied — never a null placeholder, because
    /// "no rule governed this" and "a rule governed it and said nothing" are
    /// different facts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<ReviewProvenance>,
}

/// The Event envelope's metadata member.
///
/// One member today, and a struct rather than a flattened `admitted` so the
/// envelope keeps the shape #66 fixes: `metadata.admitted`, not `admitted`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub admitted: EventAdmission,
}

impl EventAdmission {
    pub fn human(at: impl Into<String>, challenge: impl Into<String>) -> Self {
        Self {
            by: Admission::Human,
            at: at.into(),
            confirmation: Some(HumanConfirmation {
                challenge: challenge.into(),
            }),
            review: None,
        }
    }

    /// The structural rules of the envelope.
    ///
    /// Structural, not authoritative: what an Agent-admitted mutation may *do*
    /// is decided by [`check_admitting_authority`] against current state, and it
    /// has to be, because deletion depends on what the target currently is.
    /// These are the rules a record must satisfy to be well formed at all — a
    /// shape that says `agent` while carrying a human's confirmation is not a
    /// record whose authority is wrong, it is a record that contradicts itself.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            time::OffsetDateTime::parse(&self.at, &time::format_description::well_known::Rfc3339)
                .is_ok(),
            EXIT_SCHEMA,
            "event admitted.at is not RFC3339"
        );
        match self.by {
            Admission::Human => {
                ensure!(
                    self.confirmation.is_some(),
                    EXIT_SCHEMA,
                    "a human admission records the confirmation it was admitted by"
                );
            }
            Admission::Agent => {
                ensure!(
                    self.confirmation.is_none(),
                    EXIT_SCHEMA,
                    "an agent admission passes through no human gate, so it carries no confirmation"
                );
                ensure!(
                    !self
                        .review
                        .as_ref()
                        .is_some_and(|review| review.outcome == ReviewOutcome::Overridden),
                    EXIT_SCHEMA,
                    "overriding a failed review is a human act, so an agent admission cannot record one"
                );
            }
        }
        if let Some(confirmation) = &self.confirmation {
            ensure!(
                crate::confirmation::valid_challenge(&confirmation.challenge),
                EXIT_SCHEMA,
                "a human admission carries an invalid challenge"
            );
        }
        if let Some(review) = &self.review {
            crate::digest::REVIEW.verify(&review.digest)?;
        }
        Ok(())
    }
}

/// One durable Event.
///
/// The owning Object is **not** a member. Its identity is bound by the stream
/// the Event lives in and, decisively, by [`Event::digest_under`], which hashes
/// the Object id beside the Event — so an Event moved into another Object's
/// stream stops verifying rather than quietly becoming that Object's history.
/// A member repeating it would be a second answer that could disagree with the
/// first.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Event {
    pub id: String,
    #[serde(flatten)]
    pub action: Action,
    pub rev: u64,
    pub metadata: Metadata,
    pub digest: String,
}

impl Event {
    /// Build a sealed Event for `object`.
    ///
    /// The seal is computed here rather than supplied, for the reason
    /// [`Section::from_value`]'s is: a constructor that took one would be a way
    /// to append an Event whose digest describes something else.
    pub fn sealed(
        object: &str,
        id: String,
        action: Action,
        rev: u64,
        admitted: EventAdmission,
    ) -> Result<Self> {
        let mut event = Self {
            id,
            action,
            rev,
            metadata: Metadata { admitted },
            digest: String::new(),
        };
        event.digest = event.recomputed_digest(object)?;
        Ok(event)
    }

    /// The Human-Gate proof, where there is one.
    pub fn human_confirmation(&self) -> Option<&HumanConfirmation> {
        match self.metadata.admitted.by {
            Admission::Human => self.metadata.admitted.confirmation.as_ref(),
            Admission::Agent => None,
        }
    }

    /// Which door this record came through.
    pub fn admitting_path(&self) -> Admission {
        self.metadata.admitted.by
    }

    pub fn recomputed_digest(&self, object: &str) -> Result<String> {
        crate::digest::EVENT
            .emit(self.digest_under(object, crate::digest::EVENT.current)?)
            .map(|value| value.to_string())
    }

    /// EventDigestContract 1: SHA-256 over the JCS of `{object, event}`, where
    /// `event` is this Event without its own digest.
    pub fn digest_under(&self, object: &str, version: u32) -> Result<String> {
        match version {
            1 => {
                let mut event = serde_json::to_value(self).map_err(|error| {
                    Error::new(EXIT_SCHEMA, format!("canonical event: {error}"))
                })?;
                if let serde_json::Value::Object(members) = &mut event {
                    members.remove("digest");
                }
                let bound = serde_json::json!({ "object": object, "event": event });
                Ok(crate::proof::sha256_of(&crate::proof::canonical_bytes(
                    &bound, "event",
                )?))
            }
            other => Err(Error::new(
                EXIT_SCHEMA,
                format!("EventDigestContract: no contract for version {other}"),
            )),
        }
    }

    /// The payload as the Object domain reads it.
    pub fn payload(&self, object: &str) -> Payload {
        Payload::new(object, self.action.clone())
    }

    pub fn validate(&self, object: &str) -> Result<()> {
        validate_object_id(object)?;
        canonical_object_id(&self.id).map_err(|_| {
            Error::new(
                EXIT_SCHEMA,
                format!("event id {:?} must be a canonical UUIDv7", self.id),
            )
        })?;
        ensure!(
            self.rev > 0,
            EXIT_SCHEMA,
            "event revisions are counted from 1"
        );
        // The seal binds the owning Object, so this is where an Event lifted
        // into another stream stops verifying rather than quietly becoming that
        // Object's history. Rechecked under the contract the value names, never
        // under whichever emitter is newest.
        let attested = crate::digest::EVENT
            .recheck(&self.digest, |version| self.digest_under(object, version))?;
        ensure!(
            attested.agrees(),
            EXIT_SCHEMA,
            "event {} does not match its own seal for object {object}",
            self.id
        );
        self.metadata.admitted.validate()?;
        self.payload(object).validate()
    }
}

/// Apply only the event suffix newer than the persisted projection.
///
/// Events at or below the projection revision are audit evidence rather than
/// replay input. The store requires complete, append-only history separately;
/// this reducer only decides whether its suffix can advance the projection.
/// Once a newer event exists, it must begin at the next revision and continue
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

/// Apply an admitted Event to an Object. Deterministic by construction: no
/// clocks, no git, no interpretation of prose. Everything it needs is in the
/// event — including, since #66, every Section's own admission provenance, so
/// replay no longer has to infer one fact from the other.
pub fn project(object: &mut Object, event: &Event) -> Result<()> {
    let payload = event.payload(&object.id);
    let admitted_by = event.admitting_path();
    // Before anything is applied, the destination included: what the admitting
    // path may do at all is a different question from what the action does, and
    // asking it after the destination had been applied would be asking it too
    // late.
    check_admitting_authority(object, &payload, admitted_by)?;
    // Applied before the action, so the attention guard below sees the state
    // this confirmation *arrives at* rather than the one it left.
    //
    // This is what makes the rule reachable: a no-attention Object may be
    // revised in one confirmed operation **only if** that same operation
    // atomically moves it to a state that needs attention. The guard is
    // unchanged — it still refuses to let admitted wording change while nobody
    // is looking — but the object is no longer out of sight by the time the
    // section mutation applies, so no artificial intermediate state has to be
    // confirmed first.
    //
    // All three conditions are stated here rather than left to fall out of the
    // guard below. A destination that still needs no attention would be caught
    // either way, but "you may not put it *there*" and "you may not do this at
    // all" are different refusals, and a reducer that says the second when it
    // means the first sends someone to fix the wrong half.
    if let Some(becomes) = event.action.becomes() {
        // The narrow reading, and the one that keeps a confirmation honest: a
        // destination is admissible because it is what makes this action legal,
        // not as a second operation smuggled into the same signature. An Object
        // already in the listing is not blocked, so there is nothing to unblock
        // — and `object.classified.v1` already says what it would be trying to
        // say.
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
            event.action.event_type(),
            crate::semantics::attention_states(becomes.object_type)
        );
        object.object_type = becomes.object_type;
        object.state = becomes.state;
    }
    match &event.action {
        Action::ObjectCreated { title } => {
            ensure!(
                object.rev == 0 && object.sections.is_empty(),
                EXIT_INVARIANT,
                "object.created.v1 must be the first action"
            );
            check_neutral_creation(object)?;
            object.title.clone_from(title);
        }
        // The bootstrap of a migrated Object, and the only Event that replaces
        // the whole projection rather than changing part of it. It is the first
        // action of its stream for the same reason creation is: what it carries
        // is the complete state, so applying it to anything already built would
        // be discarding history rather than establishing it.
        Action::ObjectMigrated { snapshot } => {
            ensure!(
                object.rev == 0 && object.sections.is_empty(),
                EXIT_INVARIANT,
                "object.migrated.v1 is the bootstrap of a migrated stream, so it must be its first action"
            );
            check_neutral_creation(object)?;
            object.title.clone_from(&snapshot.title);
            object.object_type = snapshot.object_type;
            object.state = snapshot.state;
            object.next_section_id = snapshot.next_section_id;
            let mut sections = Vec::with_capacity(snapshot.sections.len());
            for section in &snapshot.sections {
                sections.push(Section::from_value(section.id, section.value.clone())?);
            }
            object.sections = sections;
            for section in &object.sections {
                section.validate()?;
            }
        }
        // Open, like everything else that changes the object. A title is part of
        // what settled, so allowing it through on a closed object would make
        // `closed` mean "the sections have settled" rather than "this has" — and
        // it is the second reading that makes closed mean the whole object has
        // settled.
        Action::ObjectRenamed { title, .. } => {
            object.require_attention("object.renamed.v1")?;
            object.title.clone_from(title);
        }
        Action::SectionCreated { value, .. } => {
            object.require_attention("section.created.v1")?;
            let id = take_id(object)?;
            object
                .sections
                .push(admitted_section(id, value, admitted_by, None)?);
        }
        Action::SectionUpdated { section, value, .. } => {
            object.require_attention("section.updated.v1")?;
            let previous = object.section(*section)?.admitted.by;
            let replacement = admitted_section(*section, value, admitted_by, Some(previous))?;
            let slot = object
                .sections
                .iter_mut()
                .find(|item| item.id == *section)
                .expect("section presence checked above");
            *slot = replacement;
        }
        Action::SectionMerged { merge, value, .. } => {
            object.require_attention("section.merged.v1")?;
            // Every participant is resolved before anything moves, survivor
            // included. A merge is one judgement about several Sections, so
            // consuming two of three and then discovering the third is not there
            // would leave the Object in a state nobody confirmed.
            for id in merge.participants() {
                object.section(id)?;
            }
            let admission = merged_admission(object, &merge.participants(), admitted_by)?;
            let replacement =
                admitted_section(merge.destination, value, admitted_by, Some(admission))?;
            object.sections.retain(|section| {
                section.id == merge.destination || !merge.sources.contains(&section.id)
            });
            let slot = object
                .sections
                .iter_mut()
                .find(|section| section.id == merge.destination)
                .expect("destination presence checked above");
            *slot = replacement;
        }
        Action::SectionDeleted { section, .. } => {
            object.require_attention("section.deleted.v1")?;
            object.section(*section)?;
            object.sections.retain(|item| item.id != *section);
        }
        // The general state move. Validity before attention, in this order
        // deliberately: a destination outside the type's own vocabulary is
        // categorically wrong, and being told to classify it into the attention
        // set first would send someone off to do something that still would not
        // let them make that move.
        Action::ObjectStateChanged { state } => {
            validate_state(EXIT_INVARIANT, object.object_type, *state)?;
            // Returning an Object *to* attention is how work resumes, and it has
            // to be possible from outside the attention set — that is the whole
            // point of it. Moving it *within* or *out of* attention is a change
            // to admitted standing, so it takes an Object somebody is looking at.
            if !needs_attention(object.object_type, *state) {
                object.require_attention("object.state_changed.v1")?;
            }
            ensure!(
                object.state != *state,
                EXIT_INVARIANT,
                "{} is already {}",
                object.id,
                state.as_str()
            );
            object.state = *state;
        }
        Action::ObjectClassified { object_type, state } => {
            // No transition graph, deliberately: validate that the destination
            // is legal for the destination type and that the semantic invariants
            // still hold afterwards. Inventing a permitted sequence would be
            // inventing a process nobody agreed to.
            validate_state(EXIT_INVARIANT, *object_type, *state)?;
            object.object_type = *object_type;
            object.state = *state;
        }
        Action::ObjectSuperseded { value } => {
            // `superseded` is only in the design and decision vocabularies, so
            // an untyped object or a risk cannot hold the state — and therefore,
            // by the coupled invariant, cannot hold the relation either.
            //
            // No attention check, deliberately: see `require_attention`. The
            // object this exists for is an `accepted` one, and there is no
            // transition graph — a destination valid for the type, with the
            // semantic invariants holding afterwards, is the whole test.
            // Superseding an already-superseded object is refused by the coupled
            // invariant below, which counts two replacement relations, not by an
            // invented lifecycle.
            validate_state(EXIT_INVARIANT, object.object_type, State::Superseded)?;
            let id = take_id(object)?;
            object
                .sections
                .push(admitted_section(id, value, admitted_by, None)?);
            object.state = State::Superseded;
        }
        // Nothing, and that is the whole point of it.
        //
        // Replay arrives here already holding the last provable admitted
        // projection, because these same events built it. So a repair says
        // something about the *stored file*, not about the history: the bytes on
        // disk had failed integrity and were put back to what this history
        // derives. Being a semantic no-op is what makes it safe to record —
        // `object.repaired.v1` cannot smuggle a change, because applying it does
        // nothing but advance the revision.
        //
        // No attention check either. Integrity failure is not a semantic change
        // and does not wait for the Object to be in the listing; a closed record
        // whose bytes were edited is exactly the one that needs this.
        Action::ObjectRepaired {} => {}
    }
    object.sections.sort_by_key(|section| section.id);
    object.rev = event.rev;
    check_supersession(object, EXIT_INVARIANT)?;
    object.reseal()?;
    Ok(())
}

/// Build the Section an Event admits, holding its carried `admitted` to what the
/// admitting path is allowed to produce.
///
/// The value carries the provenance now, so the reducer's job changed from
/// *deriving* it to *checking* it. That is the safer half of the trade: a
/// derived value can only be as good as the derivation, while a carried one is
/// what the human was shown and what the digest covers, and it still cannot
/// claim an authority the door it came through does not grant.
///
/// `previous` is the surviving Section's existing admission where there is one,
/// which is what makes the one-way ordering checkable: Human may take over Agent
/// wording, and nothing demotes Human to Agent.
fn admitted_section(
    id: u64,
    value: &SectionValue,
    admitted_by: Admission,
    previous: Option<Admission>,
) -> Result<Section> {
    ensure!(
        value.admitted.by == admitted_by,
        EXIT_INVARIANT,
        "§{id}: the section says it was admitted by {}, but the event came through the {} door",
        value.admitted.by.as_str(),
        admitted_by.as_str()
    );
    if let Some(previous) = previous {
        ensure!(
            admitted_by == Admission::Human || previous == Admission::Agent,
            EXIT_INVARIANT,
            "§{id} was admitted through the human gate, so its wording is changed there too"
        );
    }
    let section = Section::from_value(id, value.clone())?;
    // Checked here, so a state transition is closed under the model's own
    // invariants rather than producing something a later save is expected to
    // catch. The Agent restrictions are the case that matters: `role` and
    // `relations[]` come from the payload, and whether they are admissible
    // depends on the admission this Section is being given.
    section.validate()?;
    Ok(section)
}

/// What the admitting path is allowed to do at all.
///
/// The other half of mixed authority, and the half that is about the *Object*
/// rather than about a Section's own admission. `type` and `state` are
/// Human-authoritative after the neutral initialization, and a field does not
/// become Agent-writable because it is reached through a different action — so
/// the four lifecycle actions are Human-only, and a `becomes` destination is
/// refused on the Agent path even though it is admissible on the Human one.
///
/// Deletion is the case that cannot be answered anywhere else. Whether removing
/// §3 is legal depends on what §3 currently is, which no envelope carries and no
/// schema can express: an Agent may retire knowledge an agent admitted, and may
/// not delete wording that went through the Human Gate. That is why this lives
/// in the current-state model rather than waiting for the Event layer.
///
/// Titles are deliberately absent from the refusals. `Object.title` is
/// non-authoritative navigation metadata, so Agent create and rename are allowed
/// here; whether a *particular* one passes is Rule Review's question, not this
/// one's.
///
/// Public and taking the admitting path as an argument, so the whole matrix is
/// exercisable now — the Agent path has no envelope to arrive through until the
/// provenance slice, and a rule that cannot be run until then is a rule nobody
/// has checked.
pub fn check_admitting_authority(
    object: &Object,
    payload: &Payload,
    admitted_by: Admission,
) -> Result<()> {
    if admitted_by == Admission::Human {
        return Ok(());
    }
    ensure!(
        payload.becomes().is_none(),
        EXIT_INVARIANT,
        "an agent-admitted {} cannot carry a destination: type and state are human-authoritative, and reaching them through another action does not change that",
        payload.action.event_type()
    );
    ensure!(
        !matches!(
            payload.action,
            Action::ObjectStateChanged { .. }
                | Action::ObjectClassified { .. }
                | Action::ObjectSuperseded { .. }
        ),
        EXIT_INVARIANT,
        "{} sets the object's own lifecycle, which is a human admission",
        payload.action.event_type()
    );
    // Repair is not lifecycle, so it gets its own refusal and its own reason:
    // it re-establishes authority that stopped verifying, and that is behind the
    // Human Gate whatever admission class the Sections being restored carry.
    //
    // Here rather than only at the gate, because `project` is also how stored
    // history is read. A record tagged `agent` carrying this action is refused
    // on the way in *and* every time it is replayed, so writing one into a log
    // by hand cannot establish an Agent repair after the fact.
    ensure!(
        !matches!(payload.action, Action::ObjectRepaired {}),
        EXIT_INVARIANT,
        "object.repaired.v1 restores authority that failed integrity, which is a human admission"
    );
    // Migration is Human-confirmed as a whole, and its bootstrap Event carries
    // whatever Section provenance the predecessor had. An agent-tagged one would
    // be an Object arriving with history nobody confirmed.
    ensure!(
        !matches!(payload.action, Action::ObjectMigrated { .. }),
        EXIT_INVARIANT,
        "object.migrated.v1 is emitted by a human-confirmed migration, so it is never an agent admission"
    );
    if let Action::SectionDeleted { section, .. } = &payload.action {
        let target = object.section(*section)?;
        ensure!(
            target.admitted.by == Admission::Agent,
            EXIT_INVARIANT,
            "§{section} was admitted through the human gate, so it is removed there too"
        );
    }
    Ok(())
}

/// Creation begins from the protocol's neutral initialization, whichever path
/// admits it.
///
/// This is an invariant of the operation rather than a rule about authority, and
/// the distinction matters. `object.created` carries no lifecycle members at
/// all: its confirmed projection is a title, no type, `open`, no Sections. There
/// is nothing in the action for a human to have authorized a `design/accepted`
/// with, so preserving one would admit a lifecycle no confirmation represented —
/// on either path.
///
/// Revision zero with no Sections is *not* the same condition. A lifecycle can
/// already have been set, and creation preserves what it does not overwrite, so
/// the reducer has to say what an Object may be brought into existence *as*.
///
/// The Agent path then adds its own, separate rule — that an Agent may not
/// select lifecycle values at all — in [`check_admitting_authority`].
fn check_neutral_creation(object: &Object) -> Result<()> {
    ensure!(
        object.object_type.is_none() && object.state == State::Open,
        EXIT_INVARIANT,
        "object.created carries no lifecycle, so it cannot arrive at one; this object is already {}",
        crate::view::classification(object)
    );
    Ok(())
}

/// What the surviving Section of a merge is admitted as.
///
/// The two paths are not symmetric, and the asymmetry is the rule rather than
/// an accident of implementation. The merged wording goes through the Human
/// Gate as one statement, whatever the parts were admitted as before — so a
/// Human merge may consume Agent Sections and what comes out is Human. An Agent
/// merge passes through no such door: it may consolidate only knowledge that was
/// already Agent-admitted, because absorbing a Human Section into an Agent one
/// would take gated wording and leave it standing as ungated.
///
/// That is the same one-way ordering [`Admission`] states, applied where it
/// would otherwise be possible to launder: not by demoting a Section, but by
/// consuming it into one that was never Human.
pub fn merged_admission(
    object: &Object,
    participants: &[u64],
    admitted_by: Admission,
) -> Result<Admission> {
    if admitted_by == Admission::Human {
        return Ok(Admission::Human);
    }
    for id in participants.iter().copied() {
        let section = object.section(id)?;
        ensure!(
            section.admitted.by == Admission::Agent,
            EXIT_INVARIANT,
            "§{id} was admitted through the human gate, so an agent merge cannot consume it; the merge itself has to be confirmed"
        );
    }
    Ok(Admission::Agent)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantics::{Relation, Target};

    const AT: &str = "2026-08-31T00:00:00Z";

    fn wording(text: &str) -> Content {
        Content {
            text: text.to_owned(),
            ..Content::default()
        }
    }

    fn value(text: &str, by: Admission) -> SectionValue {
        SectionValue::new(Admitted::new(by, AT), wording(text))
    }

    fn admission(by: Admission) -> EventAdmission {
        match by {
            Admission::Human => EventAdmission::human(AT, "TEST22"),
            Admission::Agent => EventAdmission {
                by: Admission::Agent,
                at: AT.to_owned(),
                confirmation: None,
                review: Some(ReviewProvenance {
                    outcome: ReviewOutcome::Passed,
                    digest: format!("1:{}", "a".repeat(64)),
                }),
            },
        }
    }

    fn event(object: &str, rev: u64, action: Action, by: Admission) -> Event {
        Event::sealed(object, new_id(), action, rev, admission(by)).expect("sealed event")
    }

    fn section(id: u64, by: Admission) -> Section {
        Section::from_value(id, value(&format!("section {id}"), by)).expect("section")
    }

    fn holding(sections: Vec<Section>) -> Object {
        let mut object = Object::new(new_id(), "holder".to_owned()).expect("object");
        object.next_section_id = sections.iter().map(|s| s.id).max().unwrap_or(0) + 1;
        object.sections = sections;
        object.rev = 1;
        object.reseal().expect("seal");
        object
    }

    /// Human wins in both directions. New wording for an Agent Section, put
    /// through the gate, becomes Human; an Agent may not reword Human wording.
    #[test]
    fn admission_never_goes_backwards() {
        let mut object = holding(vec![section(1, Admission::Agent)]);
        let promoted = event(
            &object.id,
            2,
            Action::SectionUpdated {
                section: 1,
                value: value("promoted", Admission::Human),
                becomes: None,
            },
            Admission::Human,
        );
        project(&mut object, &promoted).expect("human may take over agent wording");
        assert_eq!(object.section(1).expect("§1").admitted.by, Admission::Human);

        let demoted = event(
            &object.id,
            3,
            Action::SectionUpdated {
                section: 1,
                value: value("demoted", Admission::Agent),
                becomes: None,
            },
            Admission::Agent,
        );
        let error = project(&mut object.clone(), &demoted)
            .expect_err("nothing demotes human wording to agent");
        assert_eq!(error.code, EXIT_INVARIANT);
    }

    /// The value carries its own admission, and it has to be the one the door
    /// grants: a Section claiming `human` inside an Agent Event is a record that
    /// contradicts itself.
    #[test]
    fn a_carried_admission_cannot_outrank_the_door_it_came_through() {
        let mut object = Object::new(new_id(), "holder".to_owned()).expect("object");
        object.rev = 0;
        object.reseal().expect("seal");
        let lying = event(
            &object.id,
            1,
            Action::SectionCreated {
                value: value("wording", Admission::Human),
                becomes: None,
            },
            Admission::Agent,
        );
        let error =
            project(&mut object, &lying).expect_err("an agent event admits agent sections only");
        assert_eq!(error.code, EXIT_INVARIANT);
    }

    /// An Agent merge may consolidate only Agent knowledge; a Human merge may
    /// consume anything, and what comes out is Human.
    #[test]
    fn an_agent_merge_may_consolidate_only_agent_knowledge() {
        let mixed = holding(vec![
            section(1, Admission::Human),
            section(2, Admission::Agent),
        ]);
        let merge = Merge {
            destination: 2,
            sources: vec![1],
        };
        let agent = event(
            &mixed.id,
            2,
            Action::SectionMerged {
                merge: merge.clone(),
                value: value("merged", Admission::Agent),
                becomes: None,
            },
            Admission::Agent,
        );
        let error = project(&mut mixed.clone(), &agent)
            .expect_err("an agent merge cannot absorb human wording");
        assert_eq!(error.code, EXIT_INVARIANT);

        let mut human = mixed.clone();
        let confirmed = event(
            &human.id,
            2,
            Action::SectionMerged {
                merge,
                value: value("merged", Admission::Human),
                becomes: None,
            },
            Admission::Human,
        );
        project(&mut human, &confirmed).expect("a human merge may consume either");
        assert_eq!(human.sections.len(), 1);
        assert_eq!(human.section(2).expect("§2").admitted.by, Admission::Human);
    }

    /// `type` and `state` are Human-authoritative, and reaching them through
    /// another action does not change that.
    #[test]
    fn an_agent_admission_cannot_reach_the_objects_lifecycle() {
        let object = holding(vec![section(1, Admission::Agent)]);
        for action in [
            Action::ObjectStateChanged {
                state: State::Closed,
            },
            Action::ObjectClassified {
                object_type: Some(ObjectType::Design),
                state: State::Draft,
            },
            Action::ObjectRepaired {},
        ] {
            let attempt = event(&object.id, 2, action, Admission::Agent);
            let error = project(&mut object.clone(), &attempt)
                .expect_err("the lifecycle is a human admission");
            assert_eq!(error.code, EXIT_INVARIANT);
        }
    }

    /// A destination is a lifecycle move, so it is refused on the Agent path
    /// even where the action itself is admissible there.
    #[test]
    fn an_agent_admission_cannot_carry_a_destination() {
        let mut object = holding(vec![section(1, Admission::Agent)]);
        object.state = State::Closed;
        object.reseal().expect("seal");
        let attempt = event(
            &object.id,
            2,
            Action::SectionCreated {
                value: value("more", Admission::Agent),
                becomes: Some(Destination {
                    object_type: None,
                    state: State::Open,
                }),
            },
            Admission::Agent,
        );
        let error =
            project(&mut object, &attempt).expect_err("a destination is human-authoritative");
        assert_eq!(error.code, EXIT_INVARIANT);
    }

    /// Creation begins from the neutral initialization whichever path admits it.
    #[test]
    fn creation_arrives_at_the_neutral_initialization_whoever_admits_it() {
        let mut object = Object::new(new_id(), String::new()).expect("object");
        object.object_type = Some(ObjectType::Design);
        object.state = State::Accepted;
        object.rev = 0;
        object.reseal().expect("seal");
        let created = event(
            &object.id,
            1,
            Action::ObjectCreated {
                title: "a title".to_owned(),
            },
            Admission::Human,
        );
        let error = project(&mut object, &created)
            .expect_err("creation carries no lifecycle, so it cannot arrive at one");
        assert_eq!(error.code, EXIT_INVARIANT);
    }

    /// A projected Section cannot be one the model would refuse if it were read
    /// back: an Agent Section carrying a relation is refused at projection.
    #[test]
    fn a_projected_section_cannot_be_one_the_model_would_refuse() {
        let mut object = holding(vec![section(1, Admission::Agent)]);
        let mut content = wording("agent wording");
        content.relations = vec![Relation {
            relation: RelationType::ImplementedBy,
            target: Target::File {
                commit: "a".repeat(40),
                path: "src/lib.rs".to_owned(),
            },
        }];
        let attempt = event(
            &object.id,
            2,
            Action::SectionCreated {
                value: SectionValue::new(Admitted::new(Admission::Agent, AT), content),
                becomes: None,
            },
            Admission::Agent,
        );
        let error = project(&mut object, &attempt).expect_err("relations are human-authoritative");
        assert_eq!(error.code, EXIT_SCHEMA);
    }

    /// Every action that names a Section holds it to the positive id bound, at
    /// payload validation rather than at a lookup that fails for another reason.
    #[test]
    fn every_action_naming_a_section_holds_it_to_the_positive_id_bound() {
        let id = new_id();
        for action in [
            Action::SectionUpdated {
                section: 0,
                value: value("x", Admission::Human),
                becomes: None,
            },
            Action::SectionDeleted {
                section: 0,
                becomes: None,
            },
            Action::SectionMerged {
                merge: Merge {
                    destination: 0,
                    sources: vec![1],
                },
                value: value("x", Admission::Human),
                becomes: None,
            },
        ] {
            let error = Payload::new(id.clone(), action)
                .validate()
                .expect_err("section ids start at 1");
            assert!(matches!(error.code, EXIT_SCHEMA | EXIT_INVARIANT));
        }
    }

    /// The reducer skips evidence at or below the projection and refuses a gap.
    #[test]
    fn the_reducer_skips_old_evidence_and_rejects_future_gaps() {
        let object = holding(vec![section(1, Admission::Human)]);
        let old = event(
            &object.id,
            1,
            Action::SectionCreated {
                value: value("already applied", Admission::Human),
                becomes: None,
            },
            Admission::Human,
        );
        let (unchanged, applied) =
            replay_recoverable_tail(object.clone(), std::slice::from_ref(&old))
                .expect("no tail to apply");
        assert!(!applied);
        assert_eq!(unchanged.rev, 1);

        let gap = event(
            &object.id,
            3,
            Action::SectionCreated {
                value: value("from the future", Admission::Human),
                becomes: None,
            },
            Admission::Human,
        );
        let error = replay_recoverable_tail(object, &[old, gap])
            .expect_err("a tail begins at the next revision");
        assert_eq!(error.code, EXIT_INVARIANT);
    }

    /// A migration bootstrap establishes an Object and may appear only first.
    #[test]
    fn a_migration_bootstrap_is_the_first_action_or_none() {
        let id = new_id();
        let snapshot = Snapshot {
            title: "migrated".to_owned(),
            object_type: None,
            state: State::Open,
            next_section_id: 2,
            sections: vec![SnapshotSection {
                id: 1,
                value: value("carried over", Admission::Human),
            }],
        };
        let mut object = Object::new(id.clone(), String::new()).expect("object");
        object.rev = 0;
        object.reseal().expect("seal");
        let bootstrap = event(
            &id,
            1,
            Action::ObjectMigrated {
                snapshot: Box::new(snapshot.clone()),
            },
            Admission::Human,
        );
        project(&mut object, &bootstrap).expect("a bootstrap establishes the object");
        assert_eq!(object.rev, 1);
        assert_eq!(object.title, "migrated");
        assert_eq!(object.next_section_id, 2);
        assert_eq!(object.sections.len(), 1);
        // The Section keeps the provenance the snapshot carried, not the Event's.
        assert_eq!(object.section(1).expect("§1").admitted.at, AT);

        let second = event(
            &id,
            2,
            Action::ObjectMigrated {
                snapshot: Box::new(snapshot),
            },
            Admission::Human,
        );
        let error = project(&mut object, &second)
            .expect_err("a second bootstrap would discard everything before it");
        assert_eq!(error.code, EXIT_INVARIANT);
    }

    /// The seal binds the owning Object, so an Event lifted into another
    /// stream stops verifying rather than becoming that Object's history.
    #[test]
    fn an_event_seals_against_exactly_one_object() {
        let mine = new_id();
        let theirs = new_id();
        let admitted = event(
            &mine,
            1,
            Action::ObjectCreated {
                title: "mine".to_owned(),
            },
            Admission::Human,
        );
        admitted.validate(&mine).expect("valid in its own stream");
        let error = admitted
            .validate(&theirs)
            .expect_err("moving a record between streams breaks its seal");
        assert_eq!(error.code, EXIT_SCHEMA);
    }

    /// `data` is present and may be `{}` — one of the two explicit exceptions to
    /// the canonical omission rule.
    #[test]
    fn a_repair_carries_an_empty_data_member_rather_than_none() {
        let admitted = event(&new_id(), 2, Action::ObjectRepaired {}, Admission::Human);
        let value = serde_json::to_value(&admitted).expect("json");
        assert_eq!(value["type"], "object.repaired.v1");
        assert_eq!(value["data"], serde_json::json!({}));
    }
}

#[cfg(test)]
mod admission_tests {
    use super::*;

    const AT: &str = "2026-08-31T00:00:00Z";

    fn human() -> EventAdmission {
        EventAdmission::human(AT, "ABC234")
    }

    fn review(outcome: ReviewOutcome) -> ReviewProvenance {
        ReviewProvenance {
            outcome,
            digest: format!("1:{}", "b".repeat(64)),
        }
    }

    /// A shape that says `agent` while carrying a human's confirmation is not a
    /// record whose authority is wrong — it is one that contradicts itself.
    #[test]
    fn an_admission_cannot_contradict_itself() {
        human().validate().expect("a human admission with its code");

        let mut missing = human();
        missing.confirmation = None;
        assert!(
            missing.validate().is_err(),
            "human admission needs its code"
        );

        let mut agent = EventAdmission {
            by: Admission::Agent,
            at: AT.to_owned(),
            confirmation: None,
            review: Some(review(ReviewOutcome::Passed)),
        };
        agent
            .validate()
            .expect("an agent admission with its review");
        agent.confirmation = Some(HumanConfirmation {
            challenge: "ABC234".to_owned(),
        });
        assert!(
            agent.validate().is_err(),
            "an agent admission passes through no human gate"
        );

        let overriding = EventAdmission {
            by: Admission::Agent,
            at: AT.to_owned(),
            confirmation: None,
            review: Some(review(ReviewOutcome::Overridden)),
        };
        assert!(
            overriding.validate().is_err(),
            "overriding a failed review is a human act"
        );
    }

    /// The review digest is read for its contract, so an invented scalar is
    /// refused rather than stored as provenance.
    #[test]
    fn review_provenance_is_checked_against_its_contract() {
        let mut admitted = human();
        admitted.review = Some(ReviewProvenance {
            outcome: ReviewOutcome::Passed,
            digest: "not-a-digest".to_owned(),
        });
        let error = admitted
            .validate()
            .expect_err("a review names its contract");
        assert_eq!(error.code, EXIT_SCHEMA);
    }

    /// A human confirmation records the spent code and nothing else — the
    /// Challenge's own digest is local and never becomes durable provenance.
    #[test]
    fn a_human_confirmation_records_only_the_spent_code() {
        let value = serde_json::to_value(human()).expect("json");
        assert_eq!(
            value["confirmation"],
            serde_json::json!({"challenge": "ABC234"})
        );
    }
}
