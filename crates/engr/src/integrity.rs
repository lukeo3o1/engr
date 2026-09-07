//! Persisted resource integrity: what a seal covers, and what breaks it.
//!
//! The property this exists for is narrow and worth stating exactly:
//!
//! > A stable persisted Object/Section value changed outside the supported
//! > transition, while retaining its previous stored seal, is detected.
//!
//! It is not a signature. Anyone able to rewrite the content can rewrite the
//! seal beside it. What it catches is the schema-valid hand-edit — the one that
//! leaves a file every reader still parses, and that no other check in this
//! crate would notice.
//!
//! **Integrity is not authority.** Resealing is mechanical maintenance *after*
//! some authority path has already admitted a mutation. Recomputing a seal never
//! turns Agent-admitted state into Human-authoritative state, and a migration
//! that reseals a resource has not confirmed anything about it.
//!
//! What a seal is taken over is the **persisted value itself**, minus its own
//! digest — so there is no second projection here that has to be kept in step
//! with the schema. That is the change #66 makes and the reason this module is
//! now thin: [`crate::model::Section::digest_under`] and
//! [`crate::model::Object::digest_under`] are the calculation, and everything
//! here is about when to run it and what a mismatch means.
//!
//! Because the input is the persisted value, the canonical omission rule is part
//! of the seal: an Object with no Sections omits `sections`, and a Section with
//! no references omits `refs`. Set order is part of it too, which is why
//! `store::check_current_object_shape` refuses a stored set that is not in
//! canonical order rather than quietly canonicalizing it on the way past — two
//! spellings of one value would otherwise seal two ways.

use crate::model::{Object, Section};
use crate::{ensure, Error, Result, EXIT_INVARIANT, EXIT_SCHEMA};
use std::collections::BTreeMap;

/// Recompute a Section's seal and require it to be the one stored.
///
/// Recomputed **under the contract the stored value names**, never under
/// whichever emitter is newest: a historical seal checked against a newer
/// calculation disagrees by construction, and a valid proof would be reported as
/// a changed Section.
pub fn check_section_seal(section: &Section) -> Result<()> {
    let attested =
        crate::digest::SECTION.recheck(&section.digest, |version| section.digest_under(version))?;
    ensure!(
        attested.agrees(),
        EXIT_INVARIANT,
        "section {} was sealed as {} and its current contents seal as {}",
        section.id,
        attested.attested,
        attested.expected
    );
    Ok(())
}

/// Recompute an Object's aggregate seal and require it to be the one stored.
///
/// This alone is **not** the integrity check a trust-sensitive path needs. The
/// aggregate covers each Section's stored seal as a value — so it catches an
/// edited Section and an edited seal, but it cannot tell you whether a Section's
/// stored seal still follows from that Section's contents. Both edited together
/// is a coherent aggregate over incoherent parts, which is what
/// [`check_object_integrity`] is for.
pub fn check_object_seal(object: &Object) -> Result<()> {
    let attested =
        crate::digest::OBJECT.recheck(&object.digest, |version| object.digest_under(version))?;
    ensure!(
        attested.agrees(),
        EXIT_INVARIANT,
        "object {} was sealed as {} and its current contents seal as {}",
        object.id,
        attested.attested,
        attested.expected
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
pub fn check_object_integrity(object: &Object) -> Result<()> {
    for section in &object.sections {
        check_section_seal(section)?;
    }
    check_object_seal(object)
}

/// The name the read paths use. Kept distinct from [`check_object_integrity`]
/// only so a call site reads as a statement about *stored* state.
pub fn check_stored_object_integrity(object: &Object) -> Result<()> {
    check_object_integrity(object)
}

/// An Object with fresh seals, and the aggregate value that goes with it.
///
/// Returned together because they are one result. A caller holding the Object
/// without the aggregate has the half that cannot be verified, and the write
/// that follows is atomic over both.
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
        section.digest = String::new();
        section.digest = section.recomputed_digest()?;
    }
    object.reseal()?;
    let seal = object.digest.clone();
    Ok(Resealed { object, seal })
}

/// Produce the first seals of a migrated Object, after the caller has verified
/// the predecessor under its historical contract and proved the conversion.
pub(crate) fn seal_migrated(object: Object) -> Result<Resealed> {
    seal_in_place(object)
}

/// The mutation sequence, as one call so the order cannot be got wrong at a call
/// site.
///
/// ```text
/// verify predecessor integrity
/// apply the authorized mutation
/// recompute affected Section seals
/// recompute the Object seal
/// -> caller writes atomically or not at all
/// ```
///
/// Predecessor verification comes first and `apply` does not run without it: an
/// integrity-invalid resource rejects ordinary semantic mutation rather than
/// being quietly normalized on the way past. Letting unrelated work reseal
/// invalid state is how an out-of-band edit gets laundered into apparently valid
/// authority.
///
/// **Authority is the caller's.** This verifies that the predecessor is what it
/// says it is; whether the mutation is allowed at all is the Human Gate's
/// question or Rule Review's, and resealing afterwards decides nothing about
/// admission.
///
/// The caller is expected to hold its concurrency boundary across this call and
/// the write that follows. The shape that goes wrong is: verify, release, then
/// reseal a file that is no longer the one verified.
pub fn mutate<F>(object: &Object, apply: F) -> Result<Resealed>
where
    F: FnOnce(&mut Object) -> Result<()>,
{
    check_object_integrity(object)?;
    let mut next = object.clone();
    apply(&mut next)?;
    seal_in_place(next)
}

/// Reseal a resource whose *representation* changed, changing nothing it means.
///
/// A representation migration may recompute seals without creating a new
/// semantic admission. What it may not do is change `admitted`, or manufacture
/// provenance — so none of that is checked by reading the code, it is checked by
/// comparing the two values. A future edit that reaches a semantic field through
/// this path fails here rather than shipping a reseal that quietly admitted
/// something.
pub fn reseal(object: &Object) -> Result<Resealed> {
    let resealed = mutate(object, |_| Ok(()))?;
    check_mechanical_reseal(object, &resealed.object)?;
    Ok(resealed)
}

/// Require two Objects to differ in nothing but their seals.
///
/// Public because the migration needs to make exactly this claim: a mechanical
/// reseal is permitted only where semantic equivalence is provable, and this is
/// the proof. Stating it as a check over the two values, rather than as a
/// property of whichever function produced them, is what keeps it true when a
/// later path also reseals.
pub fn check_mechanical_reseal(before: &Object, after: &Object) -> Result<()> {
    ensure!(
        before.sections.len() == after.sections.len(),
        EXIT_INVARIANT,
        "a mechanical reseal cannot add or remove a section"
    );
    // Paired by `Section.id`, never by array position. Incidental stored order
    // is not integrity meaning, so a positional walk would call a pure
    // reordering a semantic change and refuse the exact representation-only
    // rewrite this permission exists to authorize.
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
            before.admitted == after.admitted,
            EXIT_INVARIANT,
            "a mechanical reseal cannot change how or when section {} was admitted",
            before.id
        );
        ensure!(
            before.content() == after.content(),
            EXIT_INVARIANT,
            "a mechanical reseal cannot change what section {} says",
            before.id
        );
    }
    ensure!(
        (&before.id, &before.title, before.object_type, before.state)
            == (&after.id, &after.title, after.object_type, after.state)
            && (before.rev, before.next_section_id) == (after.rev, after.next_section_id),
        EXIT_INVARIANT,
        "a mechanical reseal cannot change the object's own state"
    );
    Ok(())
}

/// A Section, resealed from what it now holds.
///
/// The one place outside the model that produces a Section seal, so a caller
/// that has just built a Section value has somewhere to go that is not
/// assigning `digest` by hand.
pub fn sealed_section(section: &Section) -> Result<Section> {
    let mut section = section.clone();
    section.digest = String::new();
    section.digest = section.recomputed_digest()?;
    section
        .validate()
        .map_err(|error| Error::new(EXIT_SCHEMA, error.message))?;
    Ok(section)
}
