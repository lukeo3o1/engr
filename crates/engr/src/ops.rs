//! Maintenance: crash reconciliation and verify.

use crate::model::{project, replay_recoverable_tail, Action, Event, Object, Section};
use crate::{ensure, git, store, Error, Result, EXIT_INVARIANT, EXIT_NOT_FOUND, EXIT_SCHEMA};
use std::path::Path;

/// Replay the recoverable tail without choosing whether it may be persisted.
///
/// Reads need the same effective Object a reconciled workspace would hold, and
/// getting there must not require a write.
fn replay(root: &Path, id: &str, require_integrity: bool) -> Result<(Object, bool)> {
    let events = store::load_events(root, id)?;
    let mut created_from_history = false;
    let object = match store::load_object(root, id) {
        Ok(object) => {
            if crate::integrity::check_stored_object_integrity(&object).is_err() {
                if require_integrity {
                    crate::integrity::check_stored_object_integrity(&object)?;
                }
                // A read surface can still display and diagnose the stored
                // bytes, but no Event tail is projected over authority whose
                // predecessor seal failed.
                return Ok((object, false));
            }
            // And the same for a projection that seals perfectly and is not what
            // its own history produced. A recoverable tail applied on top of one
            // builds admitted authority on wording nobody admitted, and
            // reconciliation would then *save* the result — resealing the
            // unauthorized semantics into a newer revision and destroying the
            // exact bytes `repair` exists to compare against, before anybody
            // reached a diagnostic.
            //
            // The prefix check does not misread a legitimate crash tail as
            // divergence: it compares only Events up to the projection's own
            // revision, which is precisely the predecessor the tail would be
            // applied to.
            if let Some(fault) = history_fault_with(&events, &object) {
                if require_integrity {
                    return Err(Error::new(EXIT_INVARIANT, fault.message(id)));
                }
                return Ok((object, false));
            }
            object
        }
        Err(error) if error.code == EXIT_NOT_FOUND => {
            let created = events.iter().find(|event| event.rev == 1).ok_or(error)?;
            ensure!(
                matches!(
                    &created.action,
                    Action::ObjectCreated { .. } | Action::ObjectMigrated { .. }
                ),
                EXIT_INVARIANT,
                "{id}: event rev 1 cannot reconstruct a missing object"
            );
            created_from_history = true;
            let mut empty = Object::new(id.to_owned(), String::new())?;
            empty.rev = 0;
            empty.reseal()?;
            empty
        }
        Err(error) => return Err(error),
    };
    let predecessor = object.clone();
    let (projected, applied) = replay_recoverable_tail(object, &events).map_err(|error| {
        Error::new(
            EXIT_SCHEMA,
            format!("{id}: event tail cannot reconcile: {}", error.message),
        )
    })?;
    if !applied {
        return Ok((projected, applied));
    }
    let sealed = if created_from_history {
        crate::integrity::seal_migrated(projected)?.object
    } else {
        crate::integrity::mutate(&predecessor, |next| {
            *next = replay_recoverable_tail(next.clone(), &events)?.0;
            Ok(())
        })?
        .object
    };
    Ok((sealed, true))
}

/// Read the effective authority after applying any recoverable crash tail in
/// memory. Unlike [`reconcile`], this never writes a projection.
pub fn effective(root: &Path, id: &str) -> Result<Object> {
    Ok(replay(root, id, false)?.0)
}

/// Identities present in either required materialized projections or admitted
/// history. Validation remains on the read path; discovery must not erase an
/// EventStore-established Object merely because its projection is missing.
pub fn object_ids(root: &Path) -> Result<Vec<String>> {
    let mut ids = store::object_ids(root)?;
    ids.extend(store::event_ids(root)?);
    ids.sort();
    ids.dedup();
    Ok(ids)
}

/// Read one current target section through the same effective authority used by
/// every read surface. Reference admission must never pin a stale projection
/// while an admitted recovery tail already carries newer wording.
pub fn effective_section(root: &Path, id: &str, section: u64) -> Result<Section> {
    effective(root, id)?.section(section).cloned()
}

/// Refuse authority whose stored seal no longer matches its own wording.
///
/// Existence is not soundness. [`effective`] answers whether an Object is there
/// and readable, which is a different question from whether what it says is
/// still what was admitted — a Section edited outside an admission path loads
/// perfectly and reads as authority.
///
/// Recomputed, never compared seal-to-seal: the stored value is a claim about
/// what was admitted, and only recomputing establishes what the target says
/// now. `section` narrows the *object-level* half to one Section; integrity is
/// always judged over the whole aggregate.
pub fn sound(root: &Path, object: &Object, section: Option<u64>) -> Result<()> {
    // Aggregate integrity is never narrowed to one Section: the parent Object is
    // the authority that says this Section belongs here, and the aggregate seal
    // covers every nested Section's own seal.
    crate::integrity::check_object_integrity(object)?;
    if section.is_none() {
        object_level_authority(root, object)?;
    }
    Ok(())
}

/// Reconcile the current aggregate with its admitted history.
///
/// The Object seal covers title, type, state, revision, id counter and nested
/// Sections. Event replay answers the separate crash-recovery question: rebuilt
/// from rev 0, the log yields what admission produced and exposes a projection
/// that missed or invented an Event.
///
/// Fails closed when the log cannot rebuild the Object at all — a gap or a
/// missing beginning means there is nothing to check the projection against,
/// which is a reason to refuse the claim rather than to wave it through.
fn object_level_authority(root: &Path, object: &Object) -> Result<()> {
    let events = store::load_events(root, &object.id)?;
    let admitted = rebuilt(&object.id, &events)?;
    if let Some(what) = disagreement(&admitted, object) {
        return Err(Error::new(
            EXIT_INVARIANT,
            format!(
                "{}: its {what} is not what was admitted; it was changed outside an admission path",
                object.id
            ),
        ));
    }
    Ok(())
}

/// The projection admitted history produces, rebuilt from rev 0.
fn rebuilt(id: &str, events: &[Event]) -> Result<Object> {
    let empty = Object::new(id.to_owned(), String::new())?;
    let (admitted, _) = replay_recoverable_tail(empty, events).map_err(|error| {
        Error::new(
            EXIT_INVARIANT,
            format!(
                "{id}: its history cannot be rebuilt, so there is nothing to check it against: {}",
                error.message
            ),
        )
    })?;
    Ok(admitted)
}

/// The first fact a projection and its own admitted history disagree about.
///
/// The whole aggregate, not a sample of it. Naming five scalars would leave the
/// rest unexamined, and the gap that hides in is not hypothetical: a seal loop
/// walks the Sections the *projection* holds, so one deleted outside an
/// admission path is never visited, every remaining seal passes, and the
/// counters do not move — gaps below `next_section_id` are what legitimate
/// admitted deletion looks like. Comparing what the history rebuilt is the only
/// form of this check that covers what was not looked at.
fn disagreement(admitted: &Object, object: &Object) -> Option<&'static str> {
    [
        ("title", admitted.title == object.title),
        ("type", admitted.object_type == object.object_type),
        ("state", admitted.state == object.state),
        ("revision", admitted.rev == object.rev),
        (
            "section id counter",
            admitted.next_section_id == object.next_section_id,
        ),
        ("sections", admitted.sections == object.sections),
    ]
    .into_iter()
    .find_map(|(what, agrees)| (!agrees).then_some(what))
}

/// Whether this projection is the value its own admitted history produced, as
/// far as its own revision.
///
/// **The prefix, not the whole stream.** An Event that is durable but not yet
/// projected is a recoverable crash tail, which is a legitimate state and not a
/// divergence — [`reconcile`] exists to apply it. What this refuses is the other
/// direction: a projection saying something no admitted Event ever said.
///
/// Integrity does not answer this and cannot. A Section edited out of band and
/// then resealed — by a repair-shaped script, by another implementation, by
/// hand — satisfies every seal, because the seals are recomputed from whatever
/// the bytes now say. Only the record of admissions establishes that the bytes
/// were ever admitted.
pub fn history_consistent(root: &Path, object: &Object) -> Result<()> {
    history_consistent_with(&store::load_events(root, &object.id)?, object)
}

/// The same question, for a caller that is already holding the history.
///
/// The durable append boundary has the stream in hand and must not read it a
/// third time to ask this; everything after the projection's own revision is
/// filtered out here, so a caller may pass a tail that already includes the
/// record it is about to write.
pub(crate) fn history_consistent_with(events: &[Event], object: &Object) -> Result<()> {
    match history_fault_with(events, object) {
        Some(fault) => Err(Error::new(EXIT_INVARIANT, fault.message(&object.id))),
        None => Ok(()),
    }
}

/// Why a projection and its own admitted history do not agree.
///
/// **Two faults, not one.** Both refuse an admission and both fail
/// verification, but they are damage to different things and a reader sent to
/// the wrong one wastes the only move they have. Divergence is a projection
/// that was changed outside an admission path: history replays perfectly and
/// produces something else, so `repair` restores it. Unreplayable history is
/// the opposite — the EventStore itself cannot be replayed, so there is nothing
/// to restore *from*, and `repair` is not the answer.
///
/// Collapsing them into one boolean is how a read surface came to report
/// EventStore corruption as "the projection was edited".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryFault {
    /// Admitted history cannot be replayed at all, so there is nothing to check
    /// the projection against.
    Unreplayable(String),
    /// History replays, and the fact it names is not what the projection holds.
    Divergent(&'static str),
}

impl HistoryFault {
    /// The machine-readable state, for structured output.
    pub fn key(&self) -> &'static str {
        match self {
            Self::Unreplayable(_) => "unreplayable",
            Self::Divergent(_) => "divergent",
        }
    }

    /// What to tell somebody holding this Object, including what to do next.
    pub fn message(&self, id: &str) -> String {
        match self {
            Self::Unreplayable(why) => why.clone(),
            Self::Divergent(what) => format!(
                "{id}: its {what} is not what its admitted history produced, so it was changed outside an admission path; restore it with `engr repair` before building on it"
            ),
        }
    }
}

/// The fault between this projection and its own admitted history, if there is
/// one.
pub fn history_fault(root: &Path, object: &Object) -> Result<Option<HistoryFault>> {
    Ok(history_fault_with(
        &store::load_events(root, &object.id)?,
        object,
    ))
}

/// The same, for a caller already holding the history.
pub(crate) fn history_fault_with(events: &[Event], object: &Object) -> Option<HistoryFault> {
    let prefix: Vec<Event> = events
        .iter()
        .filter(|event| event.rev <= object.rev)
        .cloned()
        .collect();
    match rebuilt(&object.id, &prefix) {
        Err(error) => Some(HistoryFault::Unreplayable(error.message)),
        Ok(admitted) => disagreement(&admitted, object).map(HistoryFault::Divergent),
    }
}

/// The predecessor an ordinary admission is allowed to build on.
///
/// Reconciliation alone is not enough, and that gap is the whole reason this
/// exists. It establishes that the stored bytes seal correctly and that any
/// durable Event tail has been applied — neither of which says the bytes were
/// admitted. So an out-of-band semantic edit that was resealed could become the
/// predecessor of an unrelated legitimate mutation, and that mutation would then
/// append normally and carry the unauthorized wording forward into a saved
/// projection the complete EventStore never produced. One Event later it reads
/// as ordinary admitted authority.
///
/// Reconstruction of a divergent projection stays with `repair`, which is the
/// one path that exists to admit it — visibly, through the Human Gate, as
/// exactly what history derives.
pub(crate) fn admission_predecessor(root: &Path, id: &str) -> Result<Object> {
    let object = reconcile_locked(root, id)?;
    history_consistent(root, &object)?;
    Ok(object)
}

/// Close the window between appending an event and saving the projection.
///
/// The projection is written back, so the next reader does not have to redo it.
pub fn reconcile(root: &Path, id: &str) -> Result<Object> {
    store::with_lock(root, || reconcile_locked(root, id))
}

/// The last provable admitted projection, rebuilt from history alone.
///
/// Deliberately not [`reconcile`]. That one starts from the stored Object and
/// requires its seal to verify — which is precisely what has failed by the time
/// anyone needs this. Repair has to reach a state the stored bytes contribute
/// nothing to, or it would be restoring authority partly from the material that
/// lost its authority.
///
/// Exactness is the whole contract: repair may restore only
/// what admitted history derives, so if history cannot derive it this fails
/// closed rather than guessing. A log that does not begin with a creation
/// cannot reconstruct an Object, and that is a different failure from a damaged
/// current projection — it is not repairable through this path.
pub fn provable(root: &Path, id: &str) -> Result<Object> {
    let events = store::load_events(root, id)?;
    ensure!(
        events.first().is_some_and(|event| event.rev == 1
            && matches!(
                event.action,
                Action::ObjectCreated { .. } | Action::ObjectMigrated { .. }
            )),
        EXIT_INVARIANT,
        "{id}: admitted history does not begin with a creation, so no provable projection can be rebuilt from it"
    );
    let mut object = Object::new(id.to_owned(), String::new())?;
    object.rev = 0;
    object.reseal()?;
    for event in &events {
        project(&mut object, event).map_err(|error| {
            Error::new(
                EXIT_INVARIANT,
                format!(
                    "{id}: admitted history cannot be replayed: {}",
                    error.message
                ),
            )
        })?;
    }
    Ok(object)
}

pub(crate) fn reconcile_locked(root: &Path, id: &str) -> Result<Object> {
    let (object, applied) = replay(root, id, true)?;
    if applied {
        store::save_object(root, &object)?;
    }
    Ok(object)
}

/// A section here is sound, but a section it explicitly leans on is not.
#[derive(Debug)]
pub struct StandsOnTampered {
    pub section: u64,
    pub target: String,
    pub target_section: u64,
    /// `current` or `historical` for selective Ref integrity failures.
    pub side: Option<&'static str>,
}

/// A section here is sound, but what it leans on is not there at all.
///
/// Absence, kept apart from the two failures below: a reference whose target
/// was never created or has been removed is a different fact from one whose
/// target will not load, and reporting them the same way hides which is which.
#[derive(Debug)]
pub struct StandsOnMissing {
    pub section: u64,
    pub target: String,
    pub target_section: u64,
}

/// A section here is sound, and what it leans on cannot be read at all.
#[derive(Debug)]
pub struct StandsOnUnreadable {
    pub section: u64,
    pub target: String,
    pub target_section: u64,
    /// Why it would not load, kept so the report can say rather than imply.
    pub reason: String,
}

/// A section here declares a replacement, and the replacement is not there to
/// be read.
///
/// Kept apart from the three `standing_on_*` lists because it is a different
/// fact. Those are about `refs[]`: wording that leans on other wording, where
/// drift is an ordinary state and a person decides what it means. This is an
/// authoritative relation — `superseded_by` names an existing different Object,
/// and v1 has no Object delete, so a target that cannot be established means the
/// invariant is already broken rather than that something moved.
#[derive(Debug)]
pub struct BrokenReplacement {
    pub section: u64,
    pub target: String,
    pub reason: String,
}

/// Every `superseded_by` in one Section whose target cannot be established.
///
/// One implementation for both trust surfaces. `verify` reports these as
/// findings and `show` marks the Section, and two walks of the same relation
/// would be two answers to one question.
pub(crate) fn broken_replacements_in(
    root: &Path,
    section: &crate::model::Section,
) -> Vec<BrokenReplacement> {
    let mut broken = Vec::new();
    for relation in &section.relations {
        let crate::semantics::RelationType::SupersededBy = relation.relation else {
            continue;
        };
        let crate::semantics::Target::Engr { reference } = &relation.target else {
            continue;
        };
        let decoded = crate::reference::EngrRef::parse_embedded(reference)
            .and_then(|parsed| crate::reference::decode_uuid(parsed.id()));
        let Ok(uuid) = decoded else {
            broken.push(BrokenReplacement {
                section: section.id,
                target: reference.clone(),
                reason: "the replacement is not a resource this build can resolve".to_owned(),
            });
            continue;
        };
        let target = uuid.to_string();
        match effective(root, &target) {
            Ok(replacement) => {
                if let Err(error) = sound(root, &replacement, None) {
                    broken.push(BrokenReplacement {
                        section: section.id,
                        target,
                        reason: error.message,
                    });
                }
            }
            Err(error) if error.code == EXIT_NOT_FOUND => broken.push(BrokenReplacement {
                section: section.id,
                target,
                reason: "the replacement no longer exists".to_owned(),
            }),
            Err(error) => broken.push(BrokenReplacement {
                section: section.id,
                target,
                reason: error.message,
            }),
        }
    }
    broken
}

#[derive(Debug)]
pub struct Report {
    pub object: String,
    pub title: String,
    pub sections: usize,
    /// The current Object aggregate or one of its Section seals failed.
    pub object_tampered: bool,
    pub tampered: Vec<u64>,
    pub standing_on_tampered: Vec<StandsOnTampered>,
    pub standing_on_missing: Vec<StandsOnMissing>,
    pub standing_on_unreadable: Vec<StandsOnUnreadable>,
    pub broken_replacements: Vec<BrokenReplacement>,
    pub unprojected: usize,
    pub projection_missing: bool,
    /// How the stored projection and its own admitted history fail to agree.
    ///
    /// Its own finding rather than a flavour of tampering, because the seals
    /// pass: this is the state a resealed out-of-band edit leaves, and reporting
    /// it as an integrity failure would say the bytes are damaged when what is
    /// wrong is that nothing ever admitted them. The two faults inside it are
    /// kept apart for the same reason — see [`HistoryFault`].
    pub history: Option<HistoryFault>,
    pub uncommitted: Option<bool>,
}

impl Report {
    pub fn passed(&self) -> bool {
        !self.object_tampered
            && self.tampered.is_empty()
            && self.standing_on_tampered.is_empty()
            && self.standing_on_missing.is_empty()
            && self.standing_on_unreadable.is_empty()
            && self.broken_replacements.is_empty()
            && !self.projection_missing
            && self.history.is_none()
            && self.unprojected == 0
    }
}

/// Recompute the Object aggregate and every Section seal, then validate every
/// dependency this Object explicitly stands on.
///
/// The second half is not redundant. A selective Ref digest tracks semantic
/// drift, while current and historical resource seals establish that the values
/// being compared are intact. Only the Section directly referenced is checked:
/// the target's own `verify` covers what *it* stands on.
///
/// None of this catches an edit that recomputes every seal too. Append-only Events
/// preserve admitted evidence, while committed git history remains an
/// additional tamper anchor. That is why an uncommitted object file is reported.
pub fn verify(root: &Path, id: &str) -> Result<Report> {
    let events = store::load_events(root, id)?;
    let persisted = match store::load_object(root, id) {
        Ok(object) => Some(object),
        Err(error) if error.code == EXIT_NOT_FOUND => None,
        Err(error) => return Err(error),
    };
    // Replay remains a read-only recovery check, but verification must report
    // the bytes actually persisted as the projection. Otherwise a valid Event
    // tail could make an unrepaired Object look synchronized.
    let recovered = effective(root, id)?;
    let object = persisted.as_ref().unwrap_or(&recovered);
    let projection_rev = persisted.as_ref().map_or(0, |object| object.rev);
    // Absent is not "nothing to check" in a current workspace. The decode
    // boundary refuses a current Object without an aggregate seal, so this is
    // defence in depth for a value arriving another way — but answering `false`
    // is precisely how an unsealed Object with no Sections reported as healthy,
    // because every other field of the report was empty too.
    let object_tampered = crate::integrity::check_object_integrity(object).is_err();
    // The question no seal answers. Every integrity check recomputes from the
    // bytes on disk, so wording edited outside an admission path and then
    // resealed verifies perfectly; what exposes it is that admitted history
    // never produced it. Reported over the bytes actually persisted, for the
    // same reason the seals are: an unrepaired projection is the finding.
    let history = match &persisted {
        Some(persisted) => history_fault_with(&events, persisted),
        None => None,
    };
    let mut tampered = Vec::new();
    let mut standing_on_tampered = Vec::new();
    let mut standing_on_missing = Vec::new();
    let mut standing_on_unreadable = Vec::new();
    let mut broken_replacements = Vec::new();
    for section in &object.sections {
        // An authoritative forward link, checked here because nothing else
        // rechecks it after admission. The gate proves the target exists when
        // the relation is admitted; from then on the source Object's own seals
        // pass whatever happens to the replacement, and `refs[]` — the only
        // thing this walk used to follow — does not include it. So a reader
        // following the chain to find current knowledge could arrive nowhere
        // while `verify` said PASS.
        broken_replacements.extend(broken_replacements_in(root, section));
        if crate::integrity::check_section_seal(section).is_err() {
            tampered.push(section.id);
        }
        for reference in &section.refs {
            let (target_id, target_section_id) =
                crate::dependency::parse_target(reference.target())?;
            // Neither `continue` that used to be here was safe. Skipping an
            // unreadable target let a source object PASS while standing on
            // authority nobody could read, and skipping a missing one reported
            // health for a dependency that is not there. This is the
            // authoritative trust path: absence and malformed authority are
            // both findings, and they are different findings.
            //
            // Read through [`effective`], not the persisted projection. The
            // source is judged on the bytes actually stored — that is the point
            // of `persisted` above, and an unrepaired projection there is a
            // finding of its own. A *target* is a different question: it is
            // being asked "can this authority be read, and does it still say
            // what was pinned", which is the question `show` and reference
            // admission already ask through the same path. Loading it directly
            // answered a third question nobody asked, and answered it two ways
            // that both contradict `show` — a target whose durable tail will not
            // reconcile has a projection that loads fine, so it passed; and a
            // target whose projection is gone but whose events rebuild it is
            // present authority, so calling it missing was wrong in the other
            // direction.
            let target = match effective(root, &target_id) {
                Ok(target) => target,
                Err(error) if error.code == EXIT_NOT_FOUND => {
                    standing_on_missing.push(StandsOnMissing {
                        section: section.id,
                        target: target_id.clone(),
                        target_section: target_section_id,
                    });
                    continue;
                }
                Err(error) => {
                    standing_on_unreadable.push(StandsOnUnreadable {
                        section: section.id,
                        target: target_id.clone(),
                        target_section: target_section_id,
                        reason: error.message,
                    });
                    continue;
                }
            };
            let Ok(_) = target.section(target_section_id) else {
                standing_on_missing.push(StandsOnMissing {
                    section: section.id,
                    target: target_id.clone(),
                    target_section: target_section_id,
                });
                continue;
            };
            let current_failed = crate::integrity::check_object_integrity(&target).is_err();
            match crate::dependency::evaluate(root, &target, reference)? {
                crate::dependency::Dependency::TargetIntegrityFailure => {
                    standing_on_tampered.push(StandsOnTampered {
                        section: section.id,
                        target: target_id.clone(),
                        target_section: target_section_id,
                        side: Some(if current_failed {
                            "current"
                        } else {
                            "historical"
                        }),
                    });
                }
                crate::dependency::Dependency::TargetMissing => {
                    standing_on_missing.push(StandsOnMissing {
                        section: section.id,
                        target: target_id.clone(),
                        target_section: target_section_id,
                    });
                }
                state @ (crate::dependency::Dependency::ProvenanceUnavailable
                | crate::dependency::Dependency::SchemaMismatch
                | crate::dependency::Dependency::DigestInvalid) => {
                    standing_on_unreadable.push(StandsOnUnreadable {
                        section: section.id,
                        target: target_id.clone(),
                        target_section: target_section_id,
                        reason: format!("selective reference cannot be verified: {state:?}"),
                    });
                }
                crate::dependency::Dependency::Unchanged
                | crate::dependency::Dependency::Drifted { .. } => {}
            }
        }
    }
    Ok(Report {
        object: object.id.clone(),
        title: object.title.clone(),
        sections: object.sections.len(),
        object_tampered,
        tampered,
        standing_on_tampered,
        standing_on_missing,
        standing_on_unreadable,
        broken_replacements,
        unprojected: events
            .iter()
            .filter(|event| event.rev > projection_rev)
            .count(),
        projection_missing: persisted.is_none(),
        history,
        uncommitted: git::uncommitted(root, &store::object_path(root, id)),
    })
}
