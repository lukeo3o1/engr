//! Persisted resource integrity: what a seal covers, and what breaks it.
//!
//! The property this exists for is narrow and worth stating exactly:
//!
//! > A stable persisted Object/Section value changed outside the supported
//! > transition, while retaining its previous stored seal, is detected.
//!
//! It is not a signature. Anyone able to rewrite the content can rewrite the
//! seal beside it, and #31 says so plainly. What it catches is the schema-valid
//! hand-edit — the one that leaves a file every reader still parses, and that no
//! other check in this crate would notice.
//!
//! **Integrity is not authority.** Resealing is mechanical maintenance *after*
//! some authority path has already admitted a mutation. Recomputing a seal never
//! turns Agent-admitted state into Human-authoritative state, and a migration
//! that reseals a resource has not confirmed anything about it.
//!
//! # Nothing here is durable yet
//!
//! These are the **Phase-3** projections. The seal a Section carries on disk
//! today covers its content only — not `id`, not `admission`, not `admitted_at`
//! — and no Object carries an aggregate seal at all. So nothing in this crate
//! calls any of it yet, and no path writes what it produces: that is #13's
//! in-progress write boundary.
//!
//! It is also why [`check_section_seal`] takes the expected digest as an
//! argument rather than reading `Section.sha256` itself. A verifier that read
//! the stored field would be one call site away from checking a v3 projection
//! against a v2 seal and reporting every existing Section as corrupt. The one
//! function that does read stored seals is [`check_object_integrity`], because
//! that is the question it exists to ask, and it says so where it is defined.

use crate::model::{Object, Ref, Section};
use crate::proof::{canonical_bytes, canonical_set, sha256_of};
use crate::semantics::{Admission, ObjectType, Relation, Role, State, Supplement};
use crate::{ensure, Error, Result, EXIT_INVARIANT, EXIT_SCHEMA};
use serde::Serialize;
use std::collections::BTreeMap;

/// The canonical persisted Section representation, minus its own seal.
///
/// **Every member is present.** An absent `role` or `based_on` is an explicit
/// `null`; an empty `content`, `refs` or `relations` is an explicit `[]`.
/// Nothing here is omitted for being empty, and nothing may become so: the
/// canonical v3 shape is one representation, and a member that sometimes
/// disappears is two.
///
/// Ruled on #35 (`5392551560`) after being raised there as open. It needed a
/// ruling because v2 has no single convention to inherit — `refs` is always
/// written while `relations`, `content`, `role` and `based_on` are omitted when
/// empty — so "keep doing what v2 does" was never an available answer.
///
/// The bytes are pinned in `tests/integrity.rs`, because a member quietly
/// gaining `skip_serializing_if` is a tidy-looking change that would give every
/// Section with an empty collection a different seal from the one another
/// implementation computes.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct SealedSection {
    pub id: u64,
    /// Persisted here, unlike the v2 shape, which does not carry it at all.
    /// #35 requires the seal to protect it: `admission` is the Section's trust
    /// state, and trust state outside the seal is trust state a hand-edit can
    /// promote.
    pub admission: Admission,
    pub role: Option<Role>,
    pub text: String,
    /// Ordered. Supplements are read in sequence, so moving one is a change.
    pub content: Vec<Supplement>,
    pub based_on: Option<String>,
    /// A set, canonicalized by [`canonical_set`].
    pub refs: Vec<Ref>,
    /// A set, canonicalized by [`canonical_set`].
    pub relations: Vec<Relation>,
    pub admitted_at: String,
}

impl SealedSection {
    /// The Section's integrity seal: bare lowercase hex, no contract prefix.
    ///
    /// Unversioned on purpose. A versioned scalar exists where a *calculation*
    /// may be superseded while old values must still verify — Ref, candidate and
    /// review digests. A resource seal is recomputed from the resource every
    /// time the schema changes it, so there is never an old calculation left
    /// answering for old bytes.
    pub fn seal(&self) -> Result<String> {
        Ok(sha256_of(&canonical_bytes(self, "section")?))
    }

    /// The nested form the Object aggregate hashes: these fields plus the seal
    /// **as the Section stores it**.
    ///
    /// Stored, not recomputed, and the difference is the whole of one line in
    /// #31's consequence table: *change only `Section.sha256` -> Section
    /// verification fails -> `Object.sha256` fails*. An aggregate that
    /// recomputed the nested seal would ignore a hand-edited one entirely, and
    /// that second consequence would silently not hold — the Object would still
    /// verify while carrying a Section whose seal is a lie.
    ///
    /// Built by inserting into the projection's own map rather than by
    /// declaring a second struct, so "every field appears exactly once" is
    /// enforced by the insert instead of asserted by a comment. #31 warns about
    /// this directly: the conceptual formula reads `id + all fields + sha256`,
    /// and an implementation that took it literally would hash `id` twice.
    fn nested(&self, stored_seal: &str) -> Result<serde_json::Value> {
        let mut value = serde_json::to_value(self)
            .map_err(|error| Error::new(EXIT_SCHEMA, format!("canonical section: {error}")))?;
        let seal = stored_seal.to_owned();
        let members = value.as_object_mut().ok_or_else(|| {
            Error::new(
                EXIT_SCHEMA,
                "a section projects to a JSON object".to_owned(),
            )
        })?;
        ensure!(
            members
                .insert("sha256".to_owned(), serde_json::Value::String(seal))
                .is_none(),
            EXIT_INVARIANT,
            "the section projection already carries a seal, and it may appear only once"
        );
        Ok(value)
    }
}

/// Project a stored Section into the representation its seal is taken of.
///
/// The sets are canonicalized here, not trusted as stored: a file whose `refs`
/// were reordered by hand is the same assertion and must produce the same seal,
/// and a file whose `refs` hold the same reference twice is not a set at all.
pub fn sealed_section(section: &Section) -> Result<SealedSection> {
    let mut refs = section.refs.clone();
    let mut relations = section.relations.clone();
    canonical_set(&mut refs, "reference")?;
    canonical_set(&mut relations, "relation")?;
    Ok(SealedSection {
        id: section.id,
        admission: section.admission,
        role: section.role,
        text: section.text.clone(),
        content: section.content.clone(),
        based_on: section.based_on.clone(),
        refs,
        relations,
        admitted_at: section.admitted_at.clone(),
    })
}

/// The canonical persisted Object representation, minus its own seal.
///
/// `format` and `version` are absent by construction. #31 removes the legacy
/// per-resource markers from current Objects, and leaving them representable
/// here would make the seal depend on where an Object came from rather than on
/// what it now says.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct SealedObject {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub object_type: Option<ObjectType>,
    pub state: State,
    pub rev: u64,
    /// Stable persisted Object state, and therefore protected. Its earlier
    /// omission from illustrative field lists was not an exclusion — an
    /// unprotected `next_section_id` could be wound back by hand, and the next
    /// Section would be created with the id of one that already existed.
    pub next_section_id: u64,
    /// Ordered by `Section.id`. Incidental stored array order is not integrity
    /// meaning, so a file whose sections were shuffled seals unchanged; a file
    /// where two sections claim one id is refused.
    pub sections: Vec<serde_json::Value>,
}

impl SealedObject {
    /// The aggregate seal: bare lowercase hex, like the Section seal.
    pub fn seal(&self) -> Result<String> {
        Ok(sha256_of(&canonical_bytes(self, "object")?))
    }
}

/// Project a stored Object into the representation its aggregate seal covers.
pub fn sealed_object(object: &Object) -> Result<SealedObject> {
    let mut sections: Vec<(u64, serde_json::Value)> = Vec::with_capacity(object.sections.len());
    for section in &object.sections {
        sections.push((
            section.id,
            sealed_section(section)?.nested(&section.sha256)?,
        ));
    }
    sections.sort_by_key(|(id, _)| *id);
    for pair in sections.windows(2) {
        ensure!(
            pair[0].0 != pair[1].0,
            EXIT_INVARIANT,
            "section {} appears twice in the same object",
            pair[0].0
        );
    }
    Ok(SealedObject {
        id: object.id.clone(),
        title: object.title.clone(),
        object_type: object.object_type,
        state: object.state,
        rev: object.rev,
        next_section_id: object.next_section_id,
        sections: sections.into_iter().map(|(_, value)| value).collect(),
    })
}

/// Recompute a Section's seal and require it to be the one named.
///
/// The expected value is an argument rather than `section.sha256` — see the
/// module note. Fails closed: a caller gets an error, never a boolean it can
/// forget to read.
pub fn check_section_seal(section: &Section, expected: &str) -> Result<()> {
    let found = sealed_section(section)?.seal()?;
    ensure!(
        found == expected,
        EXIT_INVARIANT,
        "section {} was sealed as {expected} and its current contents seal as {found}",
        section.id
    );
    Ok(())
}

/// Recompute an Object's aggregate seal and require it to be the one named.
///
/// This alone is **not** the integrity check a trust-sensitive path needs. It
/// covers the aggregate, including each Section's stored seal as a value — so
/// it catches an edited Section and an edited seal, but it cannot tell you
/// whether a Section's stored seal still follows from that Section's contents.
/// Both edited together is a coherent aggregate over incoherent parts, which is
/// what [`check_object_integrity`] is for.
pub fn check_object_seal(object: &Object, expected: &str) -> Result<()> {
    let found = sealed_object(object)?.seal()?;
    ensure!(
        found == expected,
        EXIT_INVARIANT,
        "object {} was sealed as {expected} and its current contents seal as {found}",
        object.id
    );
    Ok(())
}

/// Everything a trust-sensitive operation must establish before believing an
/// Object: every Section seals to what it says it seals to, and the aggregate
/// seals to what it was stored as.
///
/// One entry point rather than two the caller must remember to pair, because a
/// caller that checked only the aggregate would accept an Object whose Section
/// contents and Section seal were rewritten together.
///
/// The Section seals are read from the Sections. That is correct **only** for a
/// workspace whose Sections carry the Phase-3 seal, which no workspace does
/// yet — see the module note. Nothing calls this; it is the verifier the
/// migration and the trust-sensitive paths will share, written once so there is
/// no second one to disagree with it.
pub fn check_object_integrity(object: &Object, expected: &str) -> Result<()> {
    for section in &object.sections {
        check_section_seal(section, &section.sha256)?;
    }
    check_object_seal(object, expected)
}

/// An Object with fresh seals, and the aggregate value that goes with it.
///
/// Returned together because they are one result. A caller holding the Object
/// without the aggregate has the half that cannot be verified, and the write
/// #35 §12 asks for is atomic over both.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Resealed {
    pub object: Object,
    pub seal: String,
}

/// Recompute every Section seal, then the aggregate over them.
///
/// In that order, because the aggregate carries the stored Section seals: an
/// implementation that computed the aggregate first would seal the Object over
/// the seals it was about to replace.
fn seal_in_place(mut object: Object) -> Result<Resealed> {
    for section in &mut object.sections {
        section.sha256 = sealed_section(section)?.seal()?;
    }
    let seal = sealed_object(&object)?.seal()?;
    Ok(Resealed { object, seal })
}

/// The mutation sequence of #35 §12, as one call so the order cannot be got
/// wrong at a call site.
///
/// ```text
/// verify predecessor integrity
/// apply the authorized mutation
/// recompute affected Section seals
/// recompute Object.sha256
/// -> caller writes atomically or not at all
/// ```
///
/// Predecessor verification comes first and `apply` does not run without it,
/// which is #35 §10: an integrity-invalid resource rejects ordinary semantic
/// mutation rather than being quietly normalized on the way past. Letting
/// unrelated work reseal invalid state is how an out-of-band edit gets laundered
/// into apparently valid authority.
///
/// **Authority is the caller's.** This verifies that the predecessor is what it
/// says it is; whether the mutation is allowed at all is the Human Gate's
/// question or Rule Review's, and resealing afterwards decides nothing about
/// admission.
///
/// The caller is expected to hold its concurrency boundary across this call and
/// the write that follows. #31 is explicit about the shape that goes wrong:
/// verify, release, then reseal a file that is no longer the one verified.
pub fn mutate<F>(object: &Object, expected: &str, apply: F) -> Result<Resealed>
where
    F: FnOnce(&mut Object) -> Result<()>,
{
    check_object_integrity(object, expected)?;
    let mut next = object.clone();
    apply(&mut next)?;
    seal_in_place(next)
}

/// Reseal a resource whose *representation* changed, changing nothing it means.
///
/// The narrow permission in #35 §11: a representation migration may recompute
/// seals without creating a new semantic admission. What it may not do is
/// change `admission`, change `admitted_at`, or manufacture provenance — so
/// none of those is checked by reading the code, they are checked by comparing
/// the projections either side. A future edit that reaches a semantic field
/// through this path fails here rather than shipping a reseal that quietly
/// admitted something.
///
/// Requires the old integrity to be valid, for the same reason [`mutate`] does.
///
/// This is the *within-v3* operation. Migrating v2 seals to v3 is P3-E's, and
/// it cannot use this: the predecessor there verifies under the contract it was
/// written with, not under this one.
pub fn reseal(object: &Object, expected: &str) -> Result<Resealed> {
    let resealed = mutate(object, expected, |_| Ok(()))?;
    check_mechanical_reseal(object, &resealed.object)?;
    Ok(resealed)
}

/// Require two Objects to differ in nothing but their seals.
///
/// Public because the migration needs to make exactly this claim: #35 §11
/// permits a mechanical reseal only where "semantic equivalence is
/// protocol-defined/provable", and this is the proof. Stating it as a check
/// over the two values, rather than as a property of whichever function
/// produced them, is what keeps it true when a later path also reseals.
pub fn check_mechanical_reseal(before: &Object, after: &Object) -> Result<()> {
    ensure!(
        before.sections.len() == after.sections.len(),
        EXIT_INVARIANT,
        "a mechanical reseal cannot add or remove a section"
    );
    // Paired by `Section.id`, never by array position. The protocol
    // canonicalizes `sections[]` by id and says incidental stored order is not
    // integrity meaning — so a positional walk would call a pure reordering a
    // semantic change, and refuse the exact representation-only rewrite this
    // permission exists to authorize.
    let mut after_by_id: BTreeMap<u64, &Section> = BTreeMap::new();
    for section in &after.sections {
        ensure!(
            after_by_id.insert(section.id, section).is_none(),
            EXIT_INVARIANT,
            "section {} appears twice in the resealed object",
            section.id
        );
    }
    for before in &before.sections {
        let after = after_by_id.get(&before.id).ok_or_else(|| {
            Error::new(
                EXIT_INVARIANT,
                format!(
                    "a mechanical reseal cannot change which sections exist; {} is gone",
                    before.id
                ),
            )
        })?;
        ensure!(
            before.admission == after.admission,
            EXIT_INVARIANT,
            "a mechanical reseal cannot change how section {} was admitted",
            before.id
        );
        ensure!(
            before.admitted_at == after.admitted_at,
            EXIT_INVARIANT,
            "a mechanical reseal cannot change when section {} was admitted",
            before.id
        );
        ensure!(
            sealed_section(before)? == sealed_section(after)?,
            EXIT_INVARIANT,
            "a mechanical reseal cannot change what section {} says",
            before.id
        );
    }
    let (before, after) = (sealed_object(before)?, sealed_object(after)?);
    ensure!(
        (&before.id, &before.title, before.object_type, before.state)
            == (&after.id, &after.title, after.object_type, after.state)
            && (before.rev, before.next_section_id) == (after.rev, after.next_section_id),
        EXIT_INVARIANT,
        "a mechanical reseal cannot change the object's own state"
    );
    Ok(())
}
