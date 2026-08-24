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
//! # Nothing here is durable yet
//!
//! A persisted Ref today carries `{object, section, sha256, commit}`. The
//! Phase-3 shape is `{target, fields, commit, digest}`, and it is not written
//! anywhere: [`SelectiveRef`] is a separate type rather than a change to
//! [`crate::model::Ref`], because editing that struct would change the bytes of
//! every Section already on disk. #13's in-progress write boundary, again.

use crate::model::Section;
use crate::proof::{canonical_bytes, canonical_set, sha256_of};
use crate::{ensure, Error, Result, EXIT_INVARIANT, EXIT_SCHEMA, EXIT_USAGE};
use serde::Serialize;
use serde_json::{Map, Value};

/// The closed vocabulary of selectable semantic facts (#35 §3).
///
/// Closed, and that is the point. `Section.id`, `Section.admitted_at` and
/// `Section.sha256` are deliberately outside it: identity, provenance and
/// integrity are not semantic facts a source can depend on, and a Ref that
/// could select `sha256` would be pinning the answer rather than the assertion.
///
/// Asking for a name outside this list is an error rather than a `null`. A
/// silent `null` would let `fields: ["admited_at"]` produce a perfectly valid
/// digest over a typo, and the Ref would then never drift because the thing it
/// selected never existed.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SemanticField {
    Admission,
    BasedOn,
    Content,
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
pub fn semantic_value(section: &Section, field: SemanticField) -> Result<Value> {
    let value = match field {
        SemanticField::Admission => serde_json::to_value(section.admission),
        SemanticField::BasedOn => serde_json::to_value(&section.based_on),
        SemanticField::Content => serde_json::to_value(&section.content),
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

/// A Ref's declared dependency, as Phase 3 persists it (#35 §4).
///
/// `fields[]` is required and non-empty, and there is **no implicit
/// full-reference default**. The authoring agent names the facts the source
/// actually relies on, and that selection becomes part of the admitted source
/// Section's own semantics — which is why it cannot be inferred later from
/// whatever the target happens to contain.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct SelectiveRef {
    pub target: String,
    pub fields: Vec<SemanticField>,
    pub commit: String,
    pub digest: String,
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
/// Four members and no others, in the shape the contract writes out. It is
/// spelled as a struct so the bytes come from one place, and #35 is explicit
/// that this must not be replaced by "tuple concatenation, host-struct
/// serialization, field declaration order or an equivalent-looking alternative
/// object shape" — the example's key order is explanatory, and JCS decides the
/// real one.
///
/// `values` is **not persisted inside the Ref**. It is reconstructed from the
/// target at the exact `commit` whenever the digest is computed or checked, so
/// a stored Ref cannot carry a snapshot that disagrees with the history it
/// names.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct RefSnapshot {
    pub target: String,
    pub fields: Vec<SemanticField>,
    pub values: Map<String, Value>,
    pub commit: String,
}

impl RefSnapshot {
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
/// The `values` keys are derived from `fields`, never supplied alongside them.
/// #35 requires "no missing selected key, no unselected extra key", and the way
/// to guarantee that is to make the two impossible to state separately.
pub fn ref_snapshot(
    target: impl Into<String>,
    fields: &[SemanticField],
    section: &Section,
    commit: impl Into<String>,
) -> Result<RefSnapshot> {
    let fields = canonical_fields(fields)?;
    let mut values = Map::new();
    for field in &fields {
        values.insert(field.as_str().to_owned(), semantic_value(section, *field)?);
    }
    Ok(RefSnapshot {
        target: target.into(),
        fields,
        values,
        commit: commit.into(),
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
