//! The object/section model, the confirmed payload, and projection.
//!
//! An object is an aggregate of sections. A section's `text` is always its
//! current wording, because wording only ever changes through a confirmed
//! action. Nothing is derived at read time except staleness, which lives in
//! [`crate::git`].

use crate::semantics::{
    needs_attention, validate_state, Admission, ObjectType, Relation, RelationType, Role, State,
    Supplement,
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

/// Git resolves selectors such as `HEAD` for input, but admitted records pin
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

/// The retained whole-content reference written by predecessor workspaces and
/// Event v1. Current v3 Sections use [`crate::dependency::SelectiveRef`]; this
/// exact legacy shape remains so historical provenance can still be decoded and
/// migrated without reinterpretation.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[serde(deny_unknown_fields)]
pub struct LegacyRef {
    pub object: String,
    pub section: u64,
    pub sha256: String,
    pub commit: String,
}

impl LegacyRef {
    fn validate(&self) -> Result<()> {
        validate_object_id(&self.object)?;
        validate_git_oid("reference commit", &self.commit)
    }
}

/// A Section dependency under either persisted compatibility generation.
///
/// Current workspace-v3 resources use [`Ref::Selective`]. [`Ref::Legacy`]
/// remains only so immutable Event-v1 and Git history can still be decoded
/// under the contract that wrote them. The shapes share no member names except
/// `commit`, so the untagged representation is unambiguous.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
#[serde(untagged)]
pub enum Ref {
    Selective(crate::dependency::SelectiveRef),
    Legacy(LegacyRef),
}

impl<'de> Deserialize<'de> for Ref {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| serde::de::Error::custom("a reference must be a JSON object"))?;
        if object.contains_key("target") {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct StoredSelective {
                target: String,
                fields: Vec<crate::dependency::SemanticField>,
                commit: String,
                digest: String,
            }
            let stored: StoredSelective =
                serde_json::from_value(value).map_err(serde::de::Error::custom)?;
            crate::dependency::SelectiveRef::stored(
                stored.target,
                stored.fields,
                stored.commit,
                stored.digest,
            )
            .map(Ref::Selective)
            .map_err(serde::de::Error::custom)
        } else {
            serde_json::from_value(value)
                .map(Ref::Legacy)
                .map_err(serde::de::Error::custom)
        }
    }
}

impl Ref {
    pub fn legacy(
        object: impl Into<String>,
        section: u64,
        sha256: impl Into<String>,
        commit: impl Into<String>,
    ) -> Self {
        Self::Legacy(LegacyRef {
            object: object.into(),
            section,
            sha256: sha256.into(),
            commit: commit.into(),
        })
    }

    pub fn selective(reference: crate::dependency::SelectiveRef) -> Self {
        Self::Selective(reference)
    }

    pub fn as_legacy(&self) -> Option<&LegacyRef> {
        match self {
            Self::Legacy(reference) => Some(reference),
            Self::Selective(_) => None,
        }
    }

    pub fn as_selective(&self) -> Option<&crate::dependency::SelectiveRef> {
        match self {
            Self::Selective(reference) => Some(reference),
            Self::Legacy(_) => None,
        }
    }

    pub fn commit(&self) -> &str {
        match self {
            Self::Selective(reference) => reference.commit(),
            Self::Legacy(reference) => &reference.commit,
        }
    }

    pub fn target_identity(&self) -> Result<(String, u64)> {
        match self {
            Self::Selective(reference) => crate::dependency::parse_target(reference.target()),
            Self::Legacy(reference) => Ok((reference.object.clone(), reference.section)),
        }
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::Selective(reference) => crate::dependency::SelectiveRef::stored(
                reference.target(),
                reference.fields().to_vec(),
                reference.commit(),
                reference.digest(),
            )
            .map(|_| ()),
            Self::Legacy(reference) => reference.validate(),
        }
    }
}

/// The part of a section that is put through admission — the wording a human is
/// shown at the gate, and the wording Rule Review is run against.
///
/// Every selectable semantic field a Section carries lives here, and only here.
/// The v3 Section seal additionally covers identity and provenance, while a Ref
/// can select only these semantics. A semantic field held outside this value
/// would be authoritative meaning a Ref cannot pin.
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

/// Schema-exact, and that is a trust boundary rather than strictness for its own
/// sake.
///
/// A current resource carries exactly the fields its version defines, and an
/// unknown one fails closed. Without that, [`Section::admission`] being absent
/// from the persisted shape would mean a file could *carry* it and be read as
/// though it did not: serde would ignore the field, this build would reconstruct
/// `human` — and the reconstruction is only exact for the exact representation
/// this version defines, never for a file already answering the question
/// differently.
///
/// It is the read-side counterpart of the write boundary in
/// [`crate::store::save_object`]. Writes must not drop authority state they
/// cannot represent; reads must not silently reinterpret authority state they
/// were not expecting.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Section {
    pub id: u64,
    /// Which door these exact semantics came through.
    ///
    /// Legacy workspaces omit this member and are decoded under their own
    /// generation as Human. Current workspace-v3 Sections persist it.
    #[serde(default = "human_admission")]
    pub admission: Admission,
    #[serde(default)]
    pub role: Option<Role>,
    pub text: String,
    #[serde(default)]
    pub content: Vec<Supplement>,
    #[serde(default)]
    pub based_on: Option<String>,
    #[serde(default)]
    pub refs: Vec<Ref>,
    #[serde(default)]
    pub relations: Vec<Relation>,
    /// Integrity seal under the owning workspace generation. Predecessor
    /// workspaces seal semantic content; v3 seals every other Section member,
    /// including identity, admission and timestamp.
    pub sha256: String,
    /// When these semantics were admitted, by whichever path admitted them.
    ///
    /// Legacy workspaces spell this `confirmed_at`; the migration retains the
    /// instant and current workspace-v3 resources spell the authority-neutral
    /// name.
    #[serde(alias = "confirmed_at")]
    pub admitted_at: String,
}

fn human_admission() -> Admission {
    Admission::Human
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
        ensure!(
            time::OffsetDateTime::parse(
                &self.admitted_at,
                &time::format_description::well_known::Rfc3339
            )
            .is_ok(),
            EXIT_SCHEMA,
            "§{}: admitted_at is not RFC3339",
            self.id
        );
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
        if self.admission == Admission::Agent {
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
    /// Retained when a migrated v0 object carried redundant resource schema
    /// markers. New objects rely only on the workspace authority.
    #[serde(rename = "format", skip_serializing_if = "Option::is_none")]
    pub legacy_format: Option<String>,
    #[serde(rename = "version", skip_serializing_if = "Option::is_none")]
    pub legacy_version: Option<u32>,
    pub id: String,
    pub title: String,
    /// Optional, and absent is a first-class answer rather than a missing one.
    #[serde(rename = "type", default)]
    pub object_type: Option<ObjectType>,
    /// The one lifecycle field. `status` is read as an alias so a workspace
    /// migrated from v0 keeps loading, and is never written back under that
    /// name: two spellings of one truth is how they start disagreeing.
    #[serde(alias = "status")]
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
    pub sections: Vec<Section>,
    /// Aggregate integrity over the canonical Object representation.
    ///
    /// Optional in the in-memory compatibility model so historical v1/v2
    /// Objects can still be decoded. A current workspace-v3 Object is accepted
    /// by the store only when this is present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
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
            sha256: None,
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

    /// Admitted content is not revised while nobody is looking at it.
    ///
    /// This is the old "reopen first" rule stated in the terms Phase 3 gives it.
    /// For an untyped Object attention is exactly `open`, so nothing about the
    /// old behaviour changed; for a typed one it is the derived class, so an
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
        if let Some(seal) = &self.sha256 {
            ensure!(
                seal.len() == 64
                    && seal
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
                EXIT_SCHEMA,
                "{}: object sha256 must be 64 lowercase hexadecimal characters",
                self.id
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
/// Two shapes, because the answer to "which id survives" changed and the events
/// that gave the old answer are still on disk. Untagged, and unambiguously so:
/// the two shapes share no field, so a record decodes as exactly one of them.
/// Serializing is the same story in reverse — a retained event re-serializes to
/// the bytes it was written with, which is what keeps its `payload_sha256`
/// verifiable rather than needing an exemption.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(untagged)]
pub enum Merge {
    /// The current shape. An explicit Section survives, keeping its id and
    /// taking the merged wording; the sources are consumed and their ids are
    /// never handed out again. Nothing is allocated.
    Into { destination: u64, sources: Vec<u64> },
    /// Retained history only.
    ///
    /// The Phase 0 merge consumed every listed Section and allocated a fresh id
    /// for the result, so a reference to any of them pointed at a Section that
    /// no longer existed and there was nothing to forward the reader to. It is
    /// still projected exactly as it was written — history is evidence, and
    /// replaying it under today's rule would reconstruct an Object that never
    /// existed — but it is not a shape this build ever writes.
    Absorbing { absorbs: Vec<u64> },
}

impl Merge {
    /// Every Section this merge reads, survivor included.
    pub fn participants(&self) -> Vec<u64> {
        match self {
            Merge::Into {
                destination,
                sources,
            } => {
                let mut all = vec![*destination];
                all.extend(sources.iter().copied());
                all
            }
            Merge::Absorbing { absorbs } => absorbs.clone(),
        }
    }

    /// The Sections this merge removes.
    pub fn consumed(&self) -> &[u64] {
        match self {
            Merge::Into { sources, .. } => sources,
            Merge::Absorbing { absorbs } => absorbs,
        }
    }

    fn validate(&self) -> Result<()> {
        match self {
            Merge::Into {
                destination,
                sources,
            } => {
                // The field's own lower bound, checked where the field is
                // validated. Section ids start at 1, so 0 is not a section that
                // happens to be missing — it is a value outside the schema, and
                // letting the reducer discover it means a persisted Event was
                // accepted as well formed on the strength of what some Object
                // happened to contain.
                check_section_id(*destination)?;
                for source in sources {
                    check_section_id(*source)?;
                }
                ensure!(
                    !sources.is_empty(),
                    EXIT_INVARIANT,
                    "a merge needs at least one section to consume"
                );
                ensure!(
                    !sources.contains(destination),
                    EXIT_INVARIANT,
                    "§{destination} survives the merge, so it cannot also be consumed by it"
                );
                // One canonical spelling, and it is the shared one: JCS each
                // element, then order by those bytes. Not a field-local numeric
                // rule — that is a second canonicalization algorithm in a
                // protocol that has one, and the two disagree the moment the
                // ids differ in digit count. `[2, 10]` is ascending; canonical
                // is `[10, 2]`, because "10" sorts before "2". Two events
                // consuming the same sections must be one payload, so the check
                // is against the persisted order rather than a sorted copy.
                let mut canonical = sources.clone();
                crate::proof::canonical_set(&mut canonical, "merge source")?;
                ensure!(
                    canonical == *sources,
                    EXIT_INVARIANT,
                    "the sections a merge consumes are listed once each, in canonical set order"
                );
            }
            Merge::Absorbing { absorbs } => {
                for absorbed in absorbs {
                    check_section_id(*absorbed)?;
                }
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
    SectionRevised {
        section: u64,
    },
    SectionMerged {
        #[serde(flatten)]
        merge: Merge,
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
    ///
    /// A current merge is not one of them: it revises the destination in place.
    /// Only the retained Phase 0 shape allocated an id, and it is here because
    /// history still has to project correctly, not because anything writes it.
    pub fn adds_section(&self) -> bool {
        match self {
            Action::SectionAdded | Action::ObjectSuperseded => true,
            Action::SectionMerged { merge } => matches!(merge, Merge::Absorbing { .. }),
            _ => false,
        }
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
        match &self.action {
            Action::SectionMerged { merge } => merge.validate()?,
            // The same rule, for the other two actions that name a Section.
            // These predate the merge representation and had the same gap: a
            // payload naming §0 was accepted here and only refused later, by a
            // lookup that failed for a different reason.
            Action::SectionRevised { section } | Action::SectionDeleted { section } => {
                check_section_id(*section)?
            }
            _ => {}
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
#[serde(deny_unknown_fields)]
pub struct Confirmation {
    pub challenge: String,
    pub payload_sha256: String,
}

/// What a Human typed, under the mixed-authority generation.
///
/// The challenge is still a random token a human hands back — that mechanism is
/// unchanged, and a human never types a digest. What changed is the second
/// member: `payload_sha256` identified the bytes of one mutation, while
/// `candidate_digest` names the semantic transition that was authorized, so the
/// same assent stays recognizable across a representation change.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct HumanConfirmation {
    pub challenge: String,
    pub candidate_digest: String,
}

/// What Rule Review concluded, as durable provenance.
///
/// Minimal on purpose: an outcome and an identity. #25 is explicit that the
/// EventStore does not keep a ReviewSeries, per-Rule counters, or the Agent's
/// natural-language reasoning — those belong to the moment, not to the record.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct ReviewProvenance {
    pub outcome: ReviewOutcome,
    pub review_digest: String,
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

/// The one tagged admission structure of the mixed-authority Event generation.
///
/// One structure rather than a scattering of optional fields, because "which
/// door" is a single fact and a reader must not have to infer it from which
/// members happen to be present.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct TaggedAdmission {
    pub kind: Admission,
    /// Present exactly when `kind = human`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmation: Option<HumanConfirmation>,
    /// Absent when no Rule Review applied — never a null placeholder, because
    /// "no rule governed this" and "a rule governed it and said nothing" are
    /// different facts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_review: Option<ReviewProvenance>,
}

/// How an Event says what admitted it.
///
/// Two shapes, because the answer changed generation and the records that gave
/// the old answer are still on disk. Untagged and unambiguous: the two share no
/// member, so a record decodes as exactly one of them, and a retained record
/// re-serializes to the bytes it was written with — which is what keeps its
/// `payload_sha256` verifiable rather than needing an exemption.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(untagged)]
pub enum Provenance {
    /// The retained generation: a spent challenge and the hash of the mutation
    /// it was typed against.
    Confirmed { confirmation: Confirmation },
    /// The mixed-authority generation, where the Human Gate is one door of two.
    Tagged { admission: TaggedAdmission },
}

impl Provenance {
    /// The retained generation's shape, for a caller building one.
    pub fn confirmed(challenge: impl Into<String>, payload_sha256: impl Into<String>) -> Self {
        Provenance::Confirmed {
            confirmation: Confirmation {
                challenge: challenge.into(),
                payload_sha256: payload_sha256.into(),
            },
        }
    }

    /// Which door this record came through.
    ///
    /// The retained generation has only one answer available, and it is the
    /// right one rather than a fallback: while it was the current generation the
    /// Human Gate was the only way in, so every record written under it went
    /// through it.
    pub fn admitting_path(&self) -> Admission {
        match self {
            Provenance::Confirmed { .. } => Admission::Human,
            Provenance::Tagged { admission } => admission.kind,
        }
    }

    /// The structural rules of the tagged generation.
    ///
    /// Structural, not authoritative: what an Agent-admitted mutation may *do*
    /// is decided by [`check_admitting_authority`] against current state, and it
    /// has to be, because deletion depends on what the target currently is.
    /// These are the rules a record must satisfy to be well formed at all —
    /// a shape that says `agent` while carrying a human's confirmation is not a
    /// record whose authority is wrong, it is a record that contradicts itself.
    pub fn validate(&self) -> Result<()> {
        let Provenance::Tagged { admission } = self else {
            return Ok(());
        };
        match admission.kind {
            Admission::Human => {
                ensure!(
                    admission.confirmation.is_some(),
                    EXIT_SCHEMA,
                    "a human admission records the confirmation it was admitted by"
                );
            }
            Admission::Agent => {
                ensure!(
                    admission.confirmation.is_none(),
                    EXIT_SCHEMA,
                    "an agent admission passes through no human gate, so it carries no confirmation"
                );
                ensure!(
                    !admission
                        .rule_review
                        .as_ref()
                        .is_some_and(|review| review.outcome == ReviewOutcome::Overridden),
                    EXIT_SCHEMA,
                    "overriding a failed review is a human act, so an agent admission cannot record one"
                );
            }
        }
        if let Some(confirmation) = &admission.confirmation {
            ensure!(
                crate::confirmation::valid_challenge(&confirmation.challenge),
                EXIT_SCHEMA,
                "a human admission carries an invalid challenge"
            );
            crate::digest::CANDIDATE.verify(&confirmation.candidate_digest)?;
        }
        if let Some(review) = &admission.rule_review {
            crate::digest::REVIEW.verify(&review.review_digest)?;
        }
        Ok(())
    }
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
    #[serde(flatten)]
    pub provenance: Provenance,
}

impl Event {
    /// The retained generation's confirmation, where there is one.
    ///
    /// A named accessor rather than a field, because provenance is one fact
    /// with two shapes and a caller that reaches past that has to say which
    /// shape it is assuming.
    pub fn confirmation(&self) -> Option<&Confirmation> {
        match &self.provenance {
            Provenance::Confirmed { confirmation } => Some(confirmation),
            Provenance::Tagged { .. } => None,
        }
    }

    /// The Human-Gate proof carried by Event generation 2, where there is one.
    pub fn human_confirmation(&self) -> Option<&HumanConfirmation> {
        match &self.provenance {
            Provenance::Tagged { admission } if admission.kind == Admission::Human => {
                admission.confirmation.as_ref()
            }
            Provenance::Confirmed { .. } | Provenance::Tagged { .. } => None,
        }
    }
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

/// Apply an admitted Event to an Object. Deterministic by construction: no
/// clocks, no git, no interpretation of prose. Everything it needs is in the
/// event.
pub fn project(object: &mut Object, event: &Event) -> Result<()> {
    let content = &event.payload.content;
    let admitted_by = admitting_path(event);
    // Before anything is applied, `becomes` included: what the admitting path
    // may do at all is a different question from what the action does, and
    // asking it after the destination had been applied would be asking it too
    // late.
    check_admitting_authority(object, &event.payload, admitted_by)?;
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
            check_neutral_creation(object)?;
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
            object.sections.push(section_from(id, admitted_by, event)?);
        }
        Action::SectionRevised { section } => {
            object.require_attention("section.revised")?;
            let admission = revised_admission(object.section(*section)?, admitted_by)?;
            let replacement = section_from(*section, admission, event)?;
            let slot = object
                .sections
                .iter_mut()
                .find(|item| item.id == *section)
                .expect("section presence checked above");
            *slot = replacement;
        }
        Action::SectionMerged { merge } => {
            object.require_attention("section.merged")?;
            // Every participant is resolved before anything moves, survivor
            // included. A merge is one judgement about several Sections, so
            // consuming two of three and then discovering the third is not there
            // would leave the Object in a state nobody confirmed.
            for id in merge.participants() {
                object.section(id)?;
            }
            // Asked of every participant, whichever shape the merge is written
            // in. The rule is about what is being consolidated, not about how
            // the operation spells itself: a merge that consumed everything and
            // allocated a fresh id could otherwise absorb Human wording into
            // Agent knowledge, which is the same laundering by another route.
            let admission = merged_admission(object, &merge.participants(), admitted_by)?;
            match merge {
                Merge::Into {
                    destination,
                    sources,
                } => {
                    let replacement = section_from(*destination, admission, event)?;
                    object.sections.retain(|section| {
                        section.id == *destination || !sources.contains(&section.id)
                    });
                    let slot = object
                        .sections
                        .iter_mut()
                        .find(|section| section.id == *destination)
                        .expect("destination presence checked above");
                    *slot = replacement;
                }
                Merge::Absorbing { absorbs } => {
                    let id = take_id(object)?;
                    object
                        .sections
                        .retain(|section| !absorbs.contains(&section.id));
                    object.sections.push(section_from(id, admission, event)?);
                }
            }
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
            object.sections.push(section_from(id, admitted_by, event)?);
            object.state = State::Superseded;
        }
    }
    object.sections.sort_by_key(|section| section.id);
    object.rev = event.rev;
    check_supersession(object, EXIT_INVARIANT)?;
    Ok(())
}

/// Which door this event came through, and therefore what the Sections it
/// admits are worth.
///
/// One seam, deliberately, rather than an [`Admission`] threaded through every
/// arm of the reducer. It now reads the record's own provenance: the retained
/// generation answers `human` because while it was current the Human Gate was
/// the only way in, and the tagged generation says so outright.
fn admitting_path(event: &Event) -> Admission {
    event.provenance.admitting_path()
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
        payload.becomes.is_none(),
        EXIT_INVARIANT,
        "an agent-admitted {} cannot carry a destination: type and state are human-authoritative, and reaching them through another action does not change that",
        payload.action.label()
    );
    ensure!(
        !matches!(
            payload.action,
            Action::ObjectClosed
                | Action::ObjectReopened
                | Action::ObjectClassified { .. }
                | Action::ObjectSuperseded
        ),
        EXIT_INVARIANT,
        "{} sets the object's own lifecycle, which is a human admission",
        payload.action.label()
    );
    if let Action::SectionDeleted { section } = &payload.action {
        let target = object.section(*section)?;
        ensure!(
            target.admission == Admission::Agent,
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

/// What a revised Section is admitted as.
///
/// Human wins in both directions, and that is the whole rule. New wording for a
/// Section an agent wrote, put through the Human Gate, has been through the
/// door where a human is asked — so the Section becomes Human, and the
/// promotion is the point rather than a side effect. The other direction never
/// went through that door at all: an agent rewording a Human Section would
/// replace gated wording with ungated wording, while the Section went on
/// claiming Human authority or quietly stopped claiming it. Neither is a thing
/// engr may decide, so it is refused.
fn revised_admission(section: &Section, admitted_by: Admission) -> Result<Admission> {
    ensure!(
        admitted_by == Admission::Human || section.admission == Admission::Agent,
        EXIT_INVARIANT,
        "§{} was admitted through the human gate, so its wording is changed there too",
        section.id
    );
    Ok(admitted_by)
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
            section.admission == Admission::Agent,
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

fn section_from(id: u64, admission: Admission, event: &Event) -> Result<Section> {
    let content = event.payload.content.clone();
    let section = Section {
        id,
        admission,
        sha256: content.sha256()?,
        role: content.role,
        text: content.text,
        content: content.content,
        based_on: content.based_on,
        refs: content.refs,
        relations: content.relations,
        // The event's own time, not the clock. Replaying history has to
        // reconstruct the Section that was admitted, and a reducer that read a
        // clock would produce a different Object every time it ran.
        admitted_at: event.time.clone(),
    };
    // Checked here, so a state transition is closed under the model's own
    // invariants rather than producing something a later save is expected to
    // catch. The Agent restrictions are the case that matters: `role` and
    // `relations[]` come from the payload, and whether they are admissible
    // depends on the admission this Section is being given — which is a fact
    // the payload does not carry and only this point knows.
    section.validate()?;
    Ok(section)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantics::Target;

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
            provenance: Provenance::confirmed(
                "TEST00".to_owned(),
                payload.sha256().expect("payload hash"),
            ),
            payload,
        }
    }

    fn section(id: u64, admission: Admission) -> Section {
        let content = Content {
            text: format!("section {id}"),
            ..Content::default()
        };
        Section {
            id,
            admission,
            sha256: content.sha256().expect("hash"),
            role: None,
            text: content.text,
            content: Vec::new(),
            based_on: None,
            refs: Vec::new(),
            relations: Vec::new(),
            admitted_at: "2026-08-23T00:00:00Z".to_owned(),
        }
    }

    fn holding(sections: Vec<Section>) -> Object {
        let mut object = Object::new(new_id(), "mixed".to_owned()).expect("object");
        object.next_section_id = sections.iter().map(|section| section.id).max().unwrap_or(0) + 1;
        object.sections = sections;
        object
    }

    /// Reached only through the Agent path, which has no envelope to arrive
    /// through yet — so the rules are stated against the model directly rather
    /// than left unexercised until something can reach them.
    #[test]
    fn admission_never_goes_backwards() {
        let human = section(1, Admission::Human);
        let agent = section(2, Admission::Agent);

        assert_eq!(
            revised_admission(&human, Admission::Human).expect("human revises human"),
            Admission::Human
        );
        assert_eq!(
            revised_admission(&agent, Admission::Human).expect("human revises agent"),
            Admission::Human,
            "wording put through the human gate carries what that door confers"
        );
        assert_eq!(
            revised_admission(&agent, Admission::Agent).expect("agent revises agent"),
            Admission::Agent
        );
        assert!(
            revised_admission(&human, Admission::Agent).is_err(),
            "an agent rewording gated wording would leave it claiming a door it never went through"
        );
    }

    #[test]
    fn an_agent_merge_may_consolidate_only_agent_knowledge() {
        let object = holding(vec![
            section(1, Admission::Agent),
            section(2, Admission::Agent),
            section(3, Admission::Human),
        ]);

        assert_eq!(
            merged_admission(&object, &[1, 2], Admission::Agent).expect("all agent"),
            Admission::Agent
        );
        assert!(
            merged_admission(&object, &[1, 3], Admission::Agent).is_err(),
            "consuming a human section into an agent one launders away the gate"
        );
        assert!(
            merged_admission(&object, &[3, 1], Admission::Agent).is_err(),
            "and so does merging agent wording into a human destination"
        );
        assert_eq!(
            merged_admission(&object, &[3, 1, 2], Admission::Human).expect("human merge"),
            Admission::Human,
            "a human merge may consume either, because the result went through the gate"
        );
        assert_eq!(
            merged_admission(&object, &[1, 2], Admission::Human).expect("human merge"),
            Admission::Human,
            "and what comes out of a human merge is human, whatever went in"
        );

        // The rule is about what is being consolidated, not about how the merge
        // spells itself. The retained shape names no survivor and allocates a
        // fresh id, and it is the same laundering if it may absorb human wording.
        assert!(
            merged_admission(&object, &[1, 2, 3], Admission::Agent).is_err(),
            "the shape that consumes every participant is held to the same rule"
        );
    }

    /// The whole Agent action matrix, exercised now rather than when an
    /// envelope can finally carry an Agent admission. Every one of these is a
    /// consequence of `type` and `state` being Human-authoritative, or of a
    /// Human Section's wording not being an agent's to remove.
    #[test]
    fn an_agent_admission_cannot_reach_the_objects_lifecycle() {
        let object = holding(vec![
            section(1, Admission::Agent),
            section(2, Admission::Human),
        ]);
        let carrying = |action: Action| Payload {
            action,
            object: object.id.clone(),
            becomes: None,
            content: Content {
                text: "wording".to_owned(),
                ..Content::default()
            },
        };
        let bare = |action: Action| Payload {
            action,
            object: object.id.clone(),
            becomes: None,
            content: Content::default(),
        };

        for action in [
            Action::ObjectClosed,
            Action::ObjectReopened,
            Action::ObjectClassified {
                object_type: None,
                state: State::Closed,
            },
        ] {
            let payload = bare(action);
            let label = payload.action.label();
            assert!(
                check_admitting_authority(&object, &payload, Admission::Agent).is_err(),
                "{label} is a human admission"
            );
            assert!(
                check_admitting_authority(&object, &payload, Admission::Human).is_ok(),
                "{label} is unchanged for the human path"
            );
        }
        assert!(
            check_admitting_authority(
                &object,
                &carrying(Action::ObjectSuperseded),
                Admission::Agent
            )
            .is_err(),
            "retiring an object is a human admission"
        );

        // A destination is admissible on the human path and refused on the
        // agent one, on exactly the same action.
        let mut becoming = carrying(Action::SectionAdded);
        becoming.becomes = Some(Destination {
            object_type: None,
            state: State::Open,
        });
        assert!(check_admitting_authority(&object, &becoming, Admission::Human).is_ok());
        assert!(
            check_admitting_authority(&object, &becoming, Admission::Agent).is_err(),
            "a field does not become agent-writable by being reached through another action"
        );

        // Deletion depends on what the target currently is, which is why it
        // cannot be answered by any envelope.
        let agent_target = bare(Action::SectionDeleted { section: 1 });
        let human_target = bare(Action::SectionDeleted { section: 2 });
        assert!(check_admitting_authority(&object, &agent_target, Admission::Agent).is_ok());
        assert!(
            check_admitting_authority(&object, &human_target, Admission::Agent).is_err(),
            "an agent may retire agent knowledge, not wording that went through the gate"
        );
        assert!(check_admitting_authority(&object, &human_target, Admission::Human).is_ok());

        // And the positive case: a title is navigation metadata, so the agent
        // path reaches it. Whether a particular title passes is Rule Review's
        // question, not this one's.
        for action in [Action::ObjectCreated, Action::ObjectRenamed] {
            assert!(
                check_admitting_authority(&object, &carrying(action), Admission::Agent).is_ok(),
                "object title is non-authoritative navigation metadata"
            );
        }
    }

    /// Creation is neutral on **both** paths, because the operation carries no
    /// lifecycle for either to have authorized. Revision zero with no Sections
    /// is a different condition: creation preserves what it does not overwrite,
    /// so a pre-set lifecycle would survive an operation that never represented
    /// one.
    ///
    /// The Agent restriction is a separate rule and lives separately: an Agent
    /// may not select lifecycle values at all.
    #[test]
    fn creation_arrives_at_the_neutral_initialization_whoever_admits_it() {
        let id = new_id();
        let created = |object: &Object| {
            let payload = Payload {
                action: Action::ObjectCreated,
                object: object.id.clone(),
                becomes: None,
                content: Content {
                    text: "a title".to_owned(),
                    ..Content::default()
                },
            };
            Event {
                format: EVENT_FORMAT.to_owned(),
                version: crate::EVENT_ENVELOPE_VERSION_V0,
                event_id: new_id(),
                rev: 1,
                time: "2026-08-23T00:00:00Z".to_owned(),
                provenance: Provenance::confirmed(
                    "TEST00".to_owned(),
                    payload.sha256().expect("hash"),
                ),
                payload,
            }
        };

        let mut neutral = Object::new(id.clone(), String::new()).expect("object");
        let event = created(&neutral);
        project(&mut neutral, &event).expect("the neutral initialization is what creation is for");
        assert_eq!(neutral.title, "a title");

        let mut settled = Object::new(id, String::new()).expect("object");
        settled.object_type = Some(ObjectType::Design);
        settled.state = State::Accepted;
        let event = created(&settled);
        let error = project(&mut settled, &event)
            .expect_err("object.created carries no lifecycle, so it cannot arrive at one");
        assert_eq!(error.code, EXIT_INVARIANT);
    }

    /// A state transition is closed under the model's own invariants. Without
    /// this an Agent add could produce a Section carrying human-authoritative
    /// semantics and leave a later save to notice.
    #[test]
    fn a_projected_section_cannot_be_one_the_model_would_refuse() {
        let id = new_id();
        let mut object = Object::new(id.clone(), "closure".to_owned()).expect("object");
        object.rev = 1;
        let mut payload = Payload {
            action: Action::SectionAdded,
            object: id.clone(),
            becomes: None,
            content: Content {
                text: "wording".to_owned(),
                role: Some(Role::Supersession),
                ..Content::default()
            },
        };
        payload.content.role = Some(Role::Supersession);
        let event = Event {
            format: EVENT_FORMAT.to_owned(),
            version: crate::EVENT_ENVELOPE_VERSION_V0,
            event_id: new_id(),
            rev: 2,
            time: "2026-08-23T00:00:00Z".to_owned(),
            provenance: Provenance::confirmed("TEST00".to_owned(), payload.sha256().expect("hash")),
            payload,
        };

        assert!(
            section_from(1, Admission::Human, &event).is_ok(),
            "a human section may carry the supersession role"
        );
        assert!(
            section_from(1, Admission::Agent, &event).is_err(),
            "an agent section may not, and the reducer is where that is caught"
        );
    }

    /// A payload is validated when an Event is *loaded*, so a value outside a
    /// field's schema that only the reducer catches is a stored record that
    /// passed validation. Section ids start at 1, so §0 is not a section that
    /// happens to be missing — it is not a section id.
    #[test]
    fn every_action_naming_a_section_holds_it_to_the_positive_id_bound() {
        let object = new_id();
        let carrying = |action: Action| Payload {
            action,
            object: object.clone(),
            becomes: None,
            content: Content {
                text: "wording".to_owned(),
                ..Content::default()
            },
        };
        let bare = |action: Action| Payload {
            action,
            object: object.clone(),
            becomes: None,
            content: Content::default(),
        };

        for (what, payload) in [
            (
                "merge destination",
                carrying(Action::SectionMerged {
                    merge: Merge::Into {
                        destination: 0,
                        sources: vec![1],
                    },
                }),
            ),
            (
                "merge source",
                carrying(Action::SectionMerged {
                    merge: Merge::Into {
                        destination: 1,
                        sources: vec![0],
                    },
                }),
            ),
            (
                "retained merge",
                carrying(Action::SectionMerged {
                    merge: Merge::Absorbing {
                        absorbs: vec![0, 1],
                    },
                }),
            ),
            ("revision", carrying(Action::SectionRevised { section: 0 })),
            ("deletion", bare(Action::SectionDeleted { section: 0 })),
        ] {
            let error = payload
                .validate()
                .expect_err("§0 is outside the field's schema");
            assert_eq!(error.code, EXIT_SCHEMA, "{what}: {}", error.message);
        }
    }

    #[test]
    fn an_agent_section_carries_no_human_authoritative_semantics() {
        let mut relation = section(1, Admission::Agent);
        relation.relations = vec![Relation {
            relation: RelationType::ImplementedBy,
            target: Target::File {
                path: "src/lib.rs".to_owned(),
                commit: "0".repeat(40),
            },
        }];
        assert!(
            holding(vec![relation.clone()]).validate().is_err(),
            "a relation is a claim the record acts on, not wording anybody can check"
        );
        relation.admission = Admission::Human;
        assert!(holding(vec![relation]).validate().is_ok());

        let mut supersession = section(1, Admission::Agent);
        supersession.role = Some(Role::Supersession);
        assert!(
            holding(vec![supersession]).validate().is_err(),
            "retiring an object is a human admission, reason included"
        );
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

#[cfg(test)]
mod provenance_tests {
    use super::*;
    use crate::semantics::Admission;

    fn tagged(kind: Admission) -> TaggedAdmission {
        TaggedAdmission {
            kind,
            confirmation: None,
            rule_review: None,
        }
    }

    fn confirmation() -> HumanConfirmation {
        HumanConfirmation {
            challenge: "ABC234".to_owned(),
            candidate_digest: format!("1:{}", "a".repeat(64)),
        }
    }

    fn review(outcome: ReviewOutcome) -> ReviewProvenance {
        ReviewProvenance {
            outcome,
            review_digest: format!("1:{}", "b".repeat(64)),
        }
    }

    /// The retained generation has one door available, and it is the right
    /// answer rather than a fallback: while it was current the Human Gate was
    /// the only way in.
    #[test]
    fn the_admitting_path_is_read_from_the_record_itself() {
        assert_eq!(
            Provenance::confirmed("ABC234", "0".repeat(64)).admitting_path(),
            Admission::Human
        );
        for kind in [Admission::Agent, Admission::Human] {
            let mut admission = tagged(kind);
            admission.confirmation = (kind == Admission::Human).then(confirmation);
            assert_eq!(
                Provenance::Tagged { admission }.admitting_path(),
                kind,
                "the tagged generation says which door outright"
            );
        }
    }

    /// Structural rules, not authority ones: a record that says `agent` while
    /// carrying a human's confirmation is not a record whose authority is
    /// wrong, it is a record that contradicts itself.
    #[test]
    fn a_tagged_admission_cannot_contradict_itself() {
        let mut human = tagged(Admission::Human);
        assert!(
            Provenance::Tagged {
                admission: human.clone()
            }
            .validate()
            .is_err(),
            "a human admission records the confirmation it was admitted by"
        );
        human.confirmation = Some(confirmation());
        assert!(Provenance::Tagged { admission: human }.validate().is_ok());

        let mut agent = tagged(Admission::Agent);
        assert!(Provenance::Tagged {
            admission: agent.clone()
        }
        .validate()
        .is_ok());
        agent.confirmation = Some(confirmation());
        assert!(
            Provenance::Tagged {
                admission: agent.clone()
            }
            .validate()
            .is_err(),
            "an agent admission passes through no human gate"
        );

        let mut overriding = tagged(Admission::Agent);
        overriding.rule_review = Some(review(ReviewOutcome::Overridden));
        assert!(
            Provenance::Tagged {
                admission: overriding.clone()
            }
            .validate()
            .is_err(),
            "overriding a failed review is a human act"
        );
        overriding.rule_review = Some(review(ReviewOutcome::Passed));
        assert!(Provenance::Tagged {
            admission: overriding
        }
        .validate()
        .is_ok());
    }

    /// A digest scalar in the record is checked against its own contract family
    /// rather than taken as text.
    #[test]
    fn provenance_digests_are_checked_against_their_contracts() {
        let mut admission = tagged(Admission::Human);
        admission.confirmation = Some(HumanConfirmation {
            challenge: "ABC234".to_owned(),
            candidate_digest: "not-a-versioned-digest".to_owned(),
        });
        assert!(Provenance::Tagged { admission }.validate().is_err());

        let mut admission = tagged(Admission::Agent);
        admission.rule_review = Some(ReviewProvenance {
            outcome: ReviewOutcome::Passed,
            review_digest: "1:short".to_owned(),
        });
        assert!(Provenance::Tagged { admission }.validate().is_err());
    }
}
