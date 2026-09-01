//! Selective semantic dependency: what a Ref actually depends on.
//!
//! The split this module exists to hold is #35's core invariant:
//!
//! > Stable persisted resource data is integrity-protected; semantic dependency
//! > identity is modeled separately by Ref.
//!
//! [`crate::integrity`] answers *was this stored state changed outside the
//! supported transition*. This module answers a different question — *did the
//! facts my source actually relies on change* — and the two can disagree in
//! both directions on purpose. Promoting a Section from Agent to Human
//! admission moves its seal and its Object's seal, and a Ref that selected only
//! `text` has not drifted; one that selected `admission` has.
//!
//! Persisted Refs carry `{target, fields, commit, digest}`. The predecessor's
//! `{object, section, sha256, commit}` shape is decoded only by
//! [`crate::predecessor`], so migration can recover its full semantic dependency
//! without pretending it selected `admission` or `header`.

use crate::model::{Object, Section};
use crate::proof::{canonical_bytes, canonical_set, sha256_of};
use crate::{ensure, Error, Result, EXIT_INVARIANT, EXIT_NOT_FOUND, EXIT_SCHEMA, EXIT_USAGE};
use serde::Serialize;
use serde_json::{Map, Value};
use std::path::Path;

/// The closed vocabulary of selectable semantic facts (#35 §3).
///
/// Closed, and that is the point. `Section.id`, `Section.admitted.at` and
/// `Section.digest` are deliberately outside it: identity, timing and integrity
/// are not semantic facts a source can depend on, and a Ref that could select
/// `digest` would be pinning the answer rather than the assertion.
///
/// Asking for a name outside this list is an error rather than a `null`. A
/// silent `null` would let `fields: ["admited_at"]` produce a perfectly valid
/// digest over a typo, and the Ref would then never drift because the thing it
/// selected never existed.
#[derive(Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SemanticField {
    Admission,
    BasedOn,
    Content,
    Header,
    Refs,
    Relations,
    Role,
    Text,
}

impl SemanticField {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admission => "admission",
            Self::BasedOn => "based_on",
            Self::Content => "content",
            Self::Header => "header",
            Self::Refs => "refs",
            Self::Relations => "relations",
            Self::Role => "role",
            Self::Text => "text",
        }
    }

    /// Read by name, so an unsupported selector is refused with the legal set
    /// spelled out rather than by a deserializer talking about variants.
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "admission" => Ok(Self::Admission),
            "based_on" => Ok(Self::BasedOn),
            "content" => Ok(Self::Content),
            "header" => Ok(Self::Header),
            "refs" => Ok(Self::Refs),
            "relations" => Ok(Self::Relations),
            "role" => Ok(Self::Role),
            "text" => Ok(Self::Text),
            other => Err(Error::new(
                EXIT_USAGE,
                format!(
                    "{other:?} is not a selectable semantic field; the vocabulary is {}",
                    ALL.iter()
                        .map(|field| field.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )),
        }
    }
}

/// The whole vocabulary, in one place so nothing enumerates it twice.
pub const ALL: &[SemanticField] = &[
    SemanticField::Admission,
    SemanticField::BasedOn,
    SemanticField::Content,
    SemanticField::Header,
    SemanticField::Refs,
    SemanticField::Relations,
    SemanticField::Role,
    SemanticField::Text,
];

/// One selected fact's canonical **effective** value (#35 §5).
///
/// Effective, not stored: the value a conforming reader derives, never the
/// incidental spelling a particular file happened to use. A legacy Section with
/// no `admission` member projects `human`, and so does one that says `human`
/// outright — they are the same fact, so they must be the same bytes, or every
/// migration would look like drift.
///
/// An absent optional projects JSON `null` rather than being omitted. There is
/// nowhere to omit it *to*: `values` must carry exactly the selected keys, so
/// "absent" has to be a value.
///
/// **One field, and only that field.** Selecting `[text]` must not look at
/// `refs` at all.
///
/// This projects per field rather than materializing all eight, and the reason
/// is correctness rather than economy. Materializing the whole vocabulary walks
/// every member of the Section — including `refs`, whose own contents are
/// validated on the way through — so a Section carrying something this build
/// would refuse elsewhere fails a dependency that never selected it. That makes
/// a Ref depend on a field it did not declare, which is the one thing
/// field-relative selection exists to prevent.
///
/// The canonicalization rule is still shared: `refs` and `relations` go through
/// the same [`canonical_set`] that [`crate::proof::SectionSemantic::of`] uses,
/// and `the_two_projections_agree_field_by_field` holds the two to the same
/// value for every member of the vocabulary. Sharing the *rule* per field is
/// what keeps one definition without making every selection pay for all of it.
pub fn semantic_value(section: &Section, field: SemanticField) -> Result<Value> {
    let value = match field {
        // `admission` selects the door and only the door. `admitted.at` is
        // outside the vocabulary on purpose: a dependency on when something was
        // admitted is a dependency on a clock, and a moved timestamp is not a
        // moved assertion.
        SemanticField::Admission => serde_json::to_value(section.admitted.by),
        SemanticField::BasedOn => serde_json::to_value(&section.based_on),
        SemanticField::Content => serde_json::to_value(&section.content),
        SemanticField::Header => serde_json::to_value(&section.header),
        SemanticField::Refs => {
            let mut refs = section.refs.clone();
            canonical_set(&mut refs, "reference")?;
            serde_json::to_value(refs)
        }
        SemanticField::Relations => {
            let mut relations = section.relations.clone();
            canonical_set(&mut relations, "relation")?;
            serde_json::to_value(relations)
        }
        SemanticField::Role => serde_json::to_value(section.role),
        SemanticField::Text => serde_json::to_value(&section.text),
    };
    value.map_err(|error| {
        Error::new(
            EXIT_SCHEMA,
            format!("semantic value for {}: {error}", field.as_str()),
        )
    })
}

/// The complete canonical Section semantic projection (#35 §3).
///
/// The whole vocabulary at once, for a caller that wants every fact.
///
/// #35 says there is one canonical projection, shared by dependency selection,
/// admission and Rule Review semantics, and drift. It is one **rule**, applied
/// per field: this and [`crate::proof::SectionSemantic::of`] canonicalize the
/// same members the same way, and `the_two_projections_agree_field_by_field`
/// holds them to identical values for every field in the vocabulary — so they
/// cannot drift the way they once did, when the proof projection copied `refs`
/// verbatim while this one canonicalized it.
///
/// Nothing on the selective-Ref path calls this. Selection is field-relative,
/// and projecting all seven fields to answer a question about one is how an
/// unselected field comes to break a dependency that never declared it.
///
/// A view, and only a view. #35 lists "a second persisted semantic/content
/// digest such as `semantic_sha256`" among its non-goals, so this deliberately
/// has no `digest()`: the only thing that hashes a semantic selection is a Ref,
/// over the fields that Ref declares.
pub fn semantic_projection(section: &Section) -> Result<Map<String, Value>> {
    let mut projected = Map::new();
    for field in ALL {
        projected.insert(field.as_str().to_owned(), semantic_value(section, *field)?);
    }
    Ok(projected)
}

/// A Ref's declared dependency, as this generation persists it (#66 §6).
///
/// `fields[]` is required and non-empty, and there is **no implicit
/// full-reference default**. The authoring agent names the facts the source
/// actually relies on, and that selection becomes part of the admitted source
/// Section's own semantics — which is why it cannot be inferred later from
/// whatever the target happens to contain.
///
/// # Authoring and reading are not the same boundary
///
/// [`admit`] may take a caller's selection in any order and *produce* the
/// canonical one. A value being read as an already-stored Ref may not: a
/// current resource has exactly one schema-canonical representation, so a
/// stored `fields[]` that is merely equivalent to the canonical order is
/// schema-invalid, not something to normalize on the way past.
///
/// Members are private and [`SelectiveRef::stored`] is the only way to build
/// one from outside, for the same reason [`RefSnapshot`]'s are. Without it, a
/// Ref stored as `[text, admission]` verified happily against the digest of
/// `[admission, text]`, because the read path canonicalized before hashing and
/// so never noticed it had been handed a second spelling of one Ref.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct SelectiveRef {
    target: String,
    fields: Vec<SemanticField>,
    commit: String,
    digest: String,
}

impl SelectiveRef {
    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn fields(&self) -> &[SemanticField] {
        &self.fields
    }

    pub fn commit(&self) -> &str {
        &self.commit
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// Read a Ref that is already stored, requiring the canonical spelling.
    ///
    /// Every member is checked as written rather than repaired: the target
    /// parses as a canonical Section identity, `fields` already **equals** its
    /// canonical order, the commit is a full native object id, and the digest
    /// is a well-formed versioned scalar for a contract this build knows.
    ///
    /// Equality, not normalization. Sorting the fields here would reproduce the
    /// defect this exists to close — a second encoding accepted silently and
    /// verified against the first one's digest.
    pub fn stored(
        target: impl Into<String>,
        fields: Vec<SemanticField>,
        commit: impl Into<String>,
        digest: impl Into<String>,
    ) -> Result<Self> {
        let target = target.into();
        parse_target(&target)?;
        let commit = commit.into();
        ensure!(
            crate::model::is_canonical_git_oid(&commit),
            EXIT_SCHEMA,
            "a stored reference pins a full resolved Git object id, not {commit:?}"
        );
        ensure!(
            fields == canonical_fields(&fields)?,
            EXIT_SCHEMA,
            "a stored reference carries its selected fields in canonical order"
        );
        let digest = digest.into();
        crate::digest::REF.verify(&digest)?;
        Ok(Self {
            target,
            fields,
            commit,
            digest,
        })
    }
}

/// Reading a stored Ref goes through [`SelectiveRef::stored`], never around it.
///
/// A derived `Deserialize` would build the struct field by field and skip every
/// rule that makes a stored Ref canonical — which is how `[text, admission]`
/// once verified happily against the digest of `[admission, text]`.
impl<'de> serde::Deserialize<'de> for SelectiveRef {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Stored {
            target: String,
            fields: Vec<SemanticField>,
            commit: String,
            digest: String,
        }
        let stored = Stored::deserialize(deserializer)?;
        SelectiveRef::stored(stored.target, stored.fields, stored.commit, stored.digest)
            .map_err(serde::de::Error::custom)
    }
}

/// Validate a declared selection: non-empty, duplicate-free, canonically
/// ordered (#35 §4, §7).
///
/// Ordering is the protocol set rule, so it runs through the same
/// [`canonical_set`] every other set uses rather than `derive(Ord)` — even
/// though these happen to be an enum whose declaration order is alphabetical,
/// because that coincidence is exactly the kind a second implementation does
/// not share.
pub fn canonical_fields(fields: &[SemanticField]) -> Result<Vec<SemanticField>> {
    ensure!(
        !fields.is_empty(),
        EXIT_USAGE,
        "a reference declares the facts it depends on; there is no implicit full reference"
    );
    let mut fields = fields.to_vec();
    canonical_set(&mut fields, "selected field")?;
    Ok(fields)
}

/// Exactly what RefDigestContract 1 hashes (#35 §6).
///
/// Four members and no others, in the shape the contract writes out. #35 is
/// explicit that this must not be replaced by "tuple concatenation, host-struct
/// serialization, field declaration order or an equivalent-looking alternative
/// object shape" — the example's key order is explanatory, and JCS decides the
/// real one.
///
/// **The members are private and there is one constructor.** They were public
/// once, and that made every contract rule in this module advisory: a caller
/// could assemble an empty `fields`, a `values` map that did not match it, a
/// target that names nothing and a five-character commit, and still get a
/// perfectly well-formed `1:<sha256>` back. A digest over an illegal snapshot
/// is worse than no digest, because it verifies — against itself, forever, and
/// against nothing any other implementation would compute.
///
/// So the validation lives at construction rather than before each hash. That
/// is not the same as running a check inside `digest_under`: a check can be
/// skipped by a second entry point somebody adds later, while a value that
/// cannot be built wrong has no second entry point to add.
///
/// `values` is **not persisted inside the Ref**. It is reconstructed from the
/// target at the exact `commit` whenever the digest is computed or checked, so
/// a stored Ref cannot carry a snapshot that disagrees with the history it
/// names.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct RefSnapshot {
    target: String,
    fields: Vec<SemanticField>,
    values: Map<String, Value>,
    commit: String,
}

impl RefSnapshot {
    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn fields(&self) -> &[SemanticField] {
        &self.fields
    }

    pub fn values(&self) -> &Map<String, Value> {
        &self.values
    }

    pub fn commit(&self) -> &str {
        &self.commit
    }

    /// The versioned scalar this snapshot hashes to: `1:<64 lowercase hex>`.
    pub fn digest(&self) -> Result<crate::digest::Versioned> {
        crate::digest::REF.emit(self.digest_under(crate::digest::REF.current)?)
    }

    /// Recompute under one named contract version.
    ///
    /// Versioned rather than current-only because a historical `refs[].digest`
    /// must be verified under the contract it attests, never under today's
    /// emitter merely because it is newer.
    pub fn digest_under(&self, version: u32) -> Result<String> {
        match version {
            1 => Ok(sha256_of(&canonical_bytes(self, "reference snapshot")?)),
            other => Err(Error::new(
                EXIT_SCHEMA,
                format!("RefDigestContract: no contract for version {other}"),
            )),
        }
    }
}

/// Build the hash input for one selection over one historical Section.
///
/// The only way to obtain a [`RefSnapshot`], and therefore the one place every
/// Contract-1 rule is enforced:
///
/// - the target is a canonical Section identity, parsed rather than trusted;
/// - the commit is a full native Git object id, not an abbreviation;
/// - `fields` is non-empty, duplicate-free and in protocol set order;
/// - `values` keys are *derived from* `fields`, never supplied beside them.
///
/// That last one is why "no missing selected key, no unselected extra key" is
/// not a rule anyone can forget to apply: there is no argument through which a
/// mismatched `values` could arrive.
pub fn ref_snapshot(
    target: impl Into<String>,
    fields: &[SemanticField],
    section: &Section,
    commit: impl Into<String>,
) -> Result<RefSnapshot> {
    let target = target.into();
    parse_target(&target)?;
    let commit = commit.into();
    ensure!(
        crate::model::is_canonical_git_oid(&commit),
        EXIT_SCHEMA,
        "a reference snapshot pins a full resolved Git object id, not {commit:?}"
    );
    let fields = canonical_fields(fields)?;
    let mut values = Map::new();
    for field in &fields {
        values.insert(field.as_str().to_owned(), semantic_value(section, *field)?);
    }
    Ok(RefSnapshot {
        target,
        fields,
        values,
        commit,
    })
}
/// What a Ref's dependency looks like when it is read (#35 §9).
///
/// Kept as distinct outcomes rather than a boolean, because they call for
/// different responses and collapsing them is how a tampered target comes to be
/// reported as ordinary drift. A newer repository HEAD is not any of them: the
/// stored commit remains part of snapshot identity, and drift is about selected
/// facts.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Dependency {
    /// The historical digest verifies and the selected facts are unchanged.
    Unchanged,
    /// The historical digest verifies and at least one selected fact moved.
    Drifted { fields: Vec<SemanticField> },
    /// The current parent Object or target Section fails its own integrity.
    TargetIntegrityFailure,
    /// The current target Section is gone, after a transition the Object's own
    /// integrity still vouches for.
    ///
    /// Deliberately narrower than a name like `TargetUnavailable`: absence must
    /// not collapse with unreadable, schema-invalid or integrity-invalid
    /// authority. NOT_FOUND stays distinct from malformed, and the distinction
    /// survives as a machine state rather than escaping through a generic
    /// error.
    ///
    /// It says nothing about the Ref's proof, which may still verify against
    /// its recorded commit. What it says is that there is no current value left
    /// to compare that proof against.
    TargetMissing,
    /// The recorded commit is unavailable, or the target is absent there.
    ProvenanceUnavailable,
    /// A selected field cannot be interpreted under the applicable contract.
    SchemaMismatch,
    /// Recomputing the historical snapshot does not reproduce the stored digest.
    DigestInvalid,
}

/// Compare the facts a Ref selected, then and now.
///
/// Order matters and follows #35 §9. The stored digest is verified against the
/// historical snapshot **first**: if the recorded past does not reproduce the
/// value the Ref names, then nothing computed from it means anything, and
/// reporting "drift" would tell a reader their dependency moved when what
/// actually happened is that the record of it is unusable.
///
/// The caller establishes target integrity and provenance before calling; those
/// outcomes are not decidable from two Sections.
pub fn compare(stored: &RefSnapshot, attested: &str, current: &Section) -> Result<Dependency> {
    let checked = crate::digest::REF.recheck(attested, |version| stored.digest_under(version))?;
    if !checked.agrees() {
        return Ok(Dependency::DigestInvalid);
    }
    let mut moved = Vec::new();
    for field in &stored.fields {
        let now = semantic_value(current, *field)?;
        let then = stored.values.get(field.as_str()).ok_or_else(|| {
            Error::new(
                EXIT_INVARIANT,
                format!(
                    "the snapshot selected {} and carries no value for it",
                    field.as_str()
                ),
            )
        })?;
        if &now != then {
            moved.push(*field);
        }
    }
    Ok(if moved.is_empty() {
        Dependency::Unchanged
    } else {
        Dependency::Drifted { fields: moved }
    })
}

/// The stale-at-birth rule of #35 §8, which is field-relative on purpose.
///
/// A new Ref is admissible only when the facts it selects are the same now as
/// at the commit it pins. Two Refs to one target, admitted in the same moment,
/// may legitimately disagree about this when they select different fields —
/// that is the dependency each one actually declared, not an inconsistency.
///
/// Because birth and read use the same projection, a freshly admitted Ref is
/// non-drifting by construction, and any later drift means a selected fact
/// really moved.
pub fn check_not_stale_at_birth(
    historical: &Section,
    current: &Section,
    fields: &[SemanticField],
) -> Result<()> {
    for field in canonical_fields(fields)? {
        let then = semantic_value(historical, field)?;
        let now = semantic_value(current, field)?;
        ensure!(
            then == now,
            EXIT_INVARIANT,
            "a new reference cannot be stale at birth: {} already differs from the commit it pins",
            field.as_str()
        );
    }
    Ok(())
}

/// Split a canonical Section target back into the identity it names.
///
/// Strict rather than lenient, in two ways that both matter.
///
/// **The Section id obeys the shared numeric domain.** #35 §7 says a Section ID
/// embedded in canonical Ref target text is still bound by the workspace's
/// Section-ID domain — string embedding is not an escape from it. It has to be
/// enforced here explicitly, because nothing else would catch it: the shared
/// safe-integer walk that guards every other protocol integer runs over JSON
/// *numbers*, and this one lives inside a string, where that walk cannot see
/// it.
///
/// **The spelling is the canonical one.** `:01` and `:1` name the same Section
/// and produce different bytes, so accepting both would mean one Section
/// identity with two digests — exactly the ambiguity a canonical form exists to
/// remove. Parsed through the shared compact engr-reference codec, so the
/// emitter and reader cannot drift into separate raw-UUID and compact
/// identities.
///
/// A target this cannot read is refused rather than guessed at, because the
/// alternative is deciding which Section a stored Ref meant and then reporting
/// drift against whatever that guess landed on.
pub fn parse_target(target: &str) -> Result<(String, u64)> {
    let reference = crate::reference::canonical_embedded(
        target,
        &[crate::reference::ResourceKind::Object],
        "Ref target",
    )?;
    let section = reference.section().ok_or_else(|| {
        Error::new(
            EXIT_SCHEMA,
            format!("{target:?} is not a canonical section target"),
        )
    })?;
    ensure!(
        section <= crate::proof::MAX_SAFE_INTEGER,
        EXIT_SCHEMA,
        "section id {section} is outside the shared safe-integer domain, and \
         writing it inside a target string does not put it back in"
    );
    let id = crate::reference::decode_uuid(reference.id())?.to_string();
    Ok((id, section))
}

/// The target as it stood at the commit a Ref pins.
///
/// Three outcomes, not two. "The commit does not have it" and "the commit has
/// something this build cannot interpret" call for different answers, and #35
/// §9 keeps them apart: provenance unavailable is a gap in history, schema
/// mismatch is a contract this reader cannot apply.
enum Historical {
    Found(Box<Section>),
    Unavailable,
    SchemaMismatch,
    /// Present and schema-valid, and its own stored seal does not follow from
    /// its contents.
    IntegrityFailure,
}

/// Require an object id to name a commit itself, not something that peels to
/// one.
///
/// `is_canonical_git_oid` says the id is well formed; it cannot say what kind
/// of object it is. An annotated tag has its own 40- or 64-character object id,
/// and every path that reads history peels it silently: `git rev-parse
/// <oid>^{commit}` resolves it, and `git show <tag-oid>:<path>` returns the
/// blob from the commit behind it.
///
/// So without this, a Ref could store the tag's id as its `commit` while its
/// `values` came from a different id entirely. The digest would be perfectly
/// stable and would attest the wrong kind of identity — and #35 §6 is specific
/// that `commit` is the exact full native Git object id *used for the
/// historical snapshot*, with commit OIDs persisted in native full form.
///
/// An id this repository does not have is **not** refused here. That is a
/// question about provenance, and the callers already answer it — admission
/// fails and read-time reports it — so answering it twice, differently, is how
/// the two would drift apart.
fn check_commit_identity(root: &Path, commit: &str) -> Result<()> {
    ensure!(
        crate::model::is_canonical_git_oid(commit),
        EXIT_SCHEMA,
        "a reference pins a full resolved Git object id, not {commit:?}"
    );
    match crate::git::object_type(root, commit).as_deref() {
        Some("commit") | None => Ok(()),
        Some(other) => Err(Error::new(
            EXIT_SCHEMA,
            format!(
                "{commit} names a {other} object; a reference pins the commit itself, \
                 not something that resolves to one"
            ),
        )),
    }
}

/// Resolve the target as it stood at one commit, and **verify it** before
/// handing it back as authority.
///
/// The verification is the point, and #35 §8 puts it before projection for a
/// reason: without it, a commit holding a schema-valid hand-edited Section with
/// a stale seal is a perfectly good source of `values` for a brand-new Ref
/// digest. If the current wording happens to match that edit, admission
/// succeeds — and the out-of-band change has been laundered into a fresh,
/// valid, permanently verifiable proof. #28's safety ordering says the same
/// thing in different words.
///
/// `git::object_at` does not close this. It validates the historical workspace
/// format and then the structure — ids, states, relations — which is a
/// different question from whether the bytes are the ones that were admitted.
///
/// The seal checked here is the one the historical contract attested: a
/// predecessor snapshot verifies under the predecessor's whole-content seal, a
/// current one under the Section and Object digests. This deliberately does not
/// pretend either projection applies to the other's bytes.
///
/// A predecessor Section is returned in the migrated spelling, because that is
/// the only spelling this build's projection understands — and it is the same
/// conversion the migration itself performs, so a Ref pinned before a migration
/// and one recomputed after it agree.
fn historical_section(root: &Path, commit: &str, id: &str, section: u64) -> Historical {
    if !crate::git::exists(root, commit) {
        return Historical::Unavailable;
    }
    match crate::git::object_at(root, commit, id) {
        Err(_) => Historical::SchemaMismatch,
        Ok(None) => Historical::Unavailable,
        Ok(Some(crate::git::HistoricalObject::Current(object))) => {
            if crate::integrity::check_stored_object_integrity(&object).is_err() {
                return Historical::IntegrityFailure;
            }
            match object.sections.into_iter().find(|held| held.id == section) {
                None => Historical::Unavailable,
                Some(section) => Historical::Found(Box::new(section)),
            }
        }
        Ok(Some(crate::git::HistoricalObject::Predecessor(object))) => {
            let Ok(held) = object.section(section) else {
                return Historical::Unavailable;
            };
            // The predecessor seal, proven against the predecessor content —
            // the only moment both are in hand. Without it, a commit holding a
            // schema-valid hand-edited Section with a stale seal is a perfectly
            // good source of `values` for a brand-new Ref digest.
            if held.check_seal().is_err() {
                return Historical::IntegrityFailure;
            }
            match crate::migration::migrated_historical_section(root, commit, id, section) {
                Ok(section) => Historical::Found(Box::new(section)),
                Err(_) => Historical::SchemaMismatch,
            }
        }
    }
}

/// Find a Section in the Object that holds it.
fn section_of(object: &Object, section: u64) -> Result<&Section> {
    object
        .sections
        .iter()
        .find(|held| held.id == section)
        .ok_or_else(|| {
            Error::new(
                EXIT_NOT_FOUND,
                format!("object {} has no section {section}", object.id),
            )
        })
}

/// Create a Ref, in the order #35 §8 freezes.
///
/// ```text
/// 1. load current parent Object and target Section
/// 2. validate applicable current Object and Section integrity
/// 3. validate fields[]
/// 4. resolve exact commit and historical target
/// 5. validate historical integrity under the historical contract where required
/// 6. project current and historical selected effective values
/// 7. require current selected values == historical selected values
/// 8. compute Ref.digest
/// 9. persist target + fields + commit + digest
/// ```
///
/// Integrity comes **first**, before anything about the selection is trusted.
/// #35 says why in one sentence: a target whose own integrity fails can never
/// be legitimized by creating a Ref to a still-hashable subset. Selecting only
/// the fields that happen to look intact would otherwise launder a tampered
/// Section into a dependency somebody relies on.
///
/// Step 9 is the caller's: the returned value is ready to persist in the source
/// Section after that Section's own admission path has accepted the mutation.
///
/// The Object carries its own seal, so there is nothing for a caller to capture
/// early and then accidentally validate against.
pub fn admit(
    root: &Path,
    current: &Object,
    section: u64,
    fields: &[SemanticField],
    commit: &str,
) -> Result<SelectiveRef> {
    let target = crate::proof::section_target(&current.id, section)?;
    // 1 + 2. The whole aggregate, not just the Section being referenced: a
    // Section is only as trustworthy as the Object that says it belongs there.
    //
    // Integrity **before** the existence lookup, for the reason `evaluate`
    // gives: the aggregate seal covers `sections[]`, so a Section deleted out
    // of band breaks it. Looking it up first answered NOT_FOUND and returned
    // before the aggregate was ever checked — reporting tampered authority as a
    // merely missing target, and #13 keeps invalid distinct from absent
    // precisely so that cannot happen.
    crate::integrity::check_object_integrity(current)?;
    let now = section_of(current, section)?;
    // 3.
    let fields = canonical_fields(fields)?;
    // 4 + 5.
    check_commit_identity(root, commit)?;
    let then = match historical_section(root, commit, &current.id, section) {
        Historical::Found(section) => *section,
        Historical::Unavailable => {
            return Err(Error::new(
                EXIT_NOT_FOUND,
                format!("section {section} is not in {} at {commit}", current.id),
            ))
        }
        Historical::SchemaMismatch => {
            return Err(Error::new(
                EXIT_SCHEMA,
                format!(
                    "{} at {commit} cannot be read under any contract this build applies",
                    current.id
                ),
            ))
        }
        Historical::IntegrityFailure => {
            return Err(Error::new(
                EXIT_INVARIANT,
                format!(
                    "section {section} of {} at {commit} does not match its own seal; a reference \
                     cannot be built over history that was changed outside the gate",
                    current.id
                ),
            ))
        }
    };
    // 6 + 7.
    check_not_stale_at_birth(&then, now, &fields)?;
    // 8.
    let snapshot = ref_snapshot(&target, &fields, &then, commit)?;
    Ok(SelectiveRef {
        target,
        fields,
        commit: commit.to_owned(),
        digest: snapshot.digest()?.to_string(),
    })
}

/// Read one stored Ref and say what its dependency looks like now (#35 §9).
///
/// The order is the contract's, and each step can only be asked once the one
/// before it has an answer:
///
/// ```text
/// current target integrity  -> TargetIntegrityFailure
/// commit / target present   -> ProvenanceUnavailable
/// historical interpretable  -> SchemaMismatch
/// stored digest reproduces  -> DigestInvalid
/// selected facts compared   -> Unchanged | Drifted
/// ```
///
/// Integrity is asked first for the same reason it is in [`admit`]: values read
/// out of a target that fails its own integrity are not evidence of anything,
/// so calling them "unchanged" would be the most misleading answer available.
pub fn evaluate(root: &Path, current: &Object, reference: &SelectiveRef) -> Result<Dependency> {
    let (id, section) = parse_target(&reference.target)?;
    ensure!(
        id == current.id,
        EXIT_INVARIANT,
        "reference names object {id} and was evaluated against {}",
        current.id
    );
    // Integrity **before** absence, which is not the order the ruling's table
    // lists but is what it means. The table's integrity row says "present but
    // ... integrity fails", leaving absent-and-tampered unstated — and the
    // aggregate seal covers `sections[]`, so a Section deleted by hand breaks
    // it. Asking about absence first would report that hand-deletion as
    // `TargetMissing`, which reads as a legitimate removal and is the one
    // answer a tampered Object must not be able to produce.
    //
    // Taken in this order, each state means what the ruling says: an Object
    // that still verifies has genuinely had the Section removed through a
    // supported transition and resealed, and one that does not verify is
    // reported as what it is.
    if crate::integrity::check_object_integrity(current).is_err() {
        return Ok(Dependency::TargetIntegrityFailure);
    }
    // Ruled on #35 (`5395844059`): the current target being gone is its own
    // state. It is not integrity failure, not drift, and not a raw NOT_FOUND
    // escaping the classification — the Ref's historical proof may still be
    // perfectly valid, and what a reader needs to know is that there is nothing
    // current left to compare it against.
    let Ok(now) = section_of(current, section) else {
        return Ok(Dependency::TargetMissing);
    };
    // A stored `commit` that is not a commit id is malformed Ref data, not a
    // dependency outcome — the same treatment a malformed target already gets
    // above. Reporting it as one of §9's states would put a verdict on a
    // reference that does not say what it depends on.
    check_commit_identity(root, &reference.commit)?;
    let then = match historical_section(root, &reference.commit, &id, section) {
        Historical::Found(section) => *section,
        Historical::Unavailable => return Ok(Dependency::ProvenanceUnavailable),
        Historical::SchemaMismatch => return Ok(Dependency::SchemaMismatch),
        // Ruled on #35 (`5395192931`): one coarse machine state covers
        // integrity failure of either the current target material or the
        // historically pinned material. Reporting it as drift would be worse
        // than imprecise — it would tell a reader their dependency moved, when
        // what happened is that the record it was pinned to was rewritten.
        //
        // The same ruling says diagnostics SHOULD still say **which** side
        // failed, and this return value cannot: it is one variant with no
        // payload, deliberately. That obligation therefore falls on the read
        // surfaces, which do not exist yet — nothing calls `evaluate`. It is
        // recorded here rather than in a plan, so whoever writes those surfaces
        // finds it at the line that would otherwise lose the distinction.
        Historical::IntegrityFailure => return Ok(Dependency::TargetIntegrityFailure),
    };
    // #35 §9: a selected field that cannot be interpreted under the applicable
    // historical *or* current semantic contract is a schema mismatch. Letting
    // the error escape instead would hand the caller a failure where the
    // protocol defines an answer, and a caller matching on `Dependency` would
    // never see the state the contract told it to expect.
    //
    // Only the declared fields are consulted, on both sides, for the same
    // reason `semantic_value` projects one field at a time.
    for field in &reference.fields {
        if semantic_value(&then, *field).is_err() || semantic_value(now, *field).is_err() {
            return Ok(Dependency::SchemaMismatch);
        }
    }
    let snapshot = ref_snapshot(
        &reference.target,
        &reference.fields,
        &then,
        &reference.commit,
    )?;
    compare(&snapshot, &reference.digest, now)
}
