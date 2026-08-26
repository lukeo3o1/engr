//! Maintenance: crash reconciliation and verify.

use crate::model::{replay_recoverable_tail, Action, Object, Section};
use crate::{ensure, git, store, Error, Result, EXIT_INVARIANT, EXIT_NOT_FOUND, EXIT_SCHEMA};
use std::path::Path;

/// Replay the recoverable tail without choosing whether it may be persisted.
/// Reads need the same effective Object as a repaired current workspace, while
/// a legacy workspace must remain byte-for-byte read-only until migration.
fn replay(root: &Path, id: &str, require_integrity: bool) -> Result<(Object, bool)> {
    let events = store::load_events(root, id)?;
    let current = store::validate_format(root)? == store::WorkspaceFormat::Current;
    let mut created_from_history = false;
    let object = match store::load_object(root, id) {
        Ok(object) => {
            if current && crate::integrity::check_stored_object_integrity(&object).is_err() {
                if require_integrity {
                    crate::integrity::check_stored_object_integrity(&object)?;
                }
                // A read surface can still display and diagnose the stored
                // bytes, but no Event tail is projected over authority whose
                // predecessor seal failed.
                return Ok((object, false));
            }
            object
        }
        Err(error) if error.code == EXIT_NOT_FOUND => {
            let created = events.iter().find(|event| event.rev == 1).ok_or(error)?;
            ensure!(
                created.payload.object == id
                    && matches!(&created.payload.action, Action::ObjectCreated),
                EXIT_INVARIANT,
                "{id}: event rev 1 cannot reconstruct a missing object"
            );
            created_from_history = true;
            Object::new(id.to_owned(), String::new())?
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
    if !current || !applied {
        return Ok((projected, applied));
    }
    let sealed = if created_from_history {
        crate::integrity::seal_migrated(projected)?.object
    } else {
        let seal = predecessor.sha256.as_deref().ok_or_else(|| {
            Error::new(
                EXIT_SCHEMA,
                format!("object {id} has no aggregate integrity seal"),
            )
        })?;
        crate::integrity::mutate(&predecessor, seal, |next| {
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
/// now. `section` narrows it to one; without it the whole Object is judged,
/// because an Object-level claim is a claim about all of it.
pub fn sound(root: &Path, object: &Object, section: Option<u64>) -> Result<()> {
    if let Some(seal) = object.sha256.as_deref() {
        // Aggregate integrity is never narrowed to one Section: the parent
        // Object is the authority that says this Section belongs here.
        crate::integrity::check_object_integrity(object, seal)?;
        return Ok(());
    }
    let sections = match section {
        Some(id) => vec![object.section(id)?],
        None => object.sections.iter().collect(),
    };
    for section in sections {
        ensure!(
            section.recomputed_sha256()? == section.sha256,
            EXIT_INVARIANT,
            "{} §{} does not match its admitted integrity seal; it was changed outside an admission path",
            object.id,
            section.id
        );
    }
    if section.is_none() {
        object_level_authority(root, object)?;
    }
    Ok(())
}

/// Reconcile the current aggregate with its admitted history.
///
/// The v3 Object seal covers title, type, state, revision, id counter and nested
/// Sections. Event replay answers the separate crash-recovery question: rebuilt
/// from rev 0, the log yields what admission produced and exposes a projection
/// that missed or invented an Event.
///
/// Fails closed when the log cannot rebuild the Object at all — a gap or a
/// missing beginning means there is nothing to check the projection against,
/// which is a reason to refuse the claim rather than to wave it through.
fn object_level_authority(root: &Path, object: &Object) -> Result<()> {
    let events = store::load_events(root, &object.id)?;
    let (admitted, _) = crate::model::replay_recoverable_tail(
        Object::new(object.id.clone(), String::new())?,
        &events,
    )
    .map_err(|error| {
        Error::new(
            EXIT_INVARIANT,
            format!(
                "{}: its history cannot be rebuilt, so there is nothing to check it against: {}",
                object.id, error.message
            ),
        )
    })?;
    // The whole aggregate, not a sample of it. Naming five scalars would leave
    // the rest unexamined, and the gap that hides in is not hypothetical: the
    // seal loop above walks the Sections the *projection* holds, so one deleted
    // outside an admission path is never visited, every remaining seal passes, and the
    // counters do not move — gaps below `next_section_id` are what legitimate
    // admitted deletion looks like. Comparing what the history rebuilt is the
    // only form of this check that covers what was not looked at.
    for (what, agrees) in [
        ("title", admitted.title == object.title),
        ("type", admitted.object_type == object.object_type),
        ("state", admitted.state == object.state),
        ("revision", admitted.rev == object.rev),
        (
            "section id counter",
            admitted.next_section_id == object.next_section_id,
        ),
        ("sections", admitted.sections == object.sections),
    ] {
        ensure!(
            agrees,
            EXIT_INVARIANT,
            "{}: its {what} is not what was admitted; it was changed outside an admission path",
            object.id
        );
    }
    Ok(())
}

/// Close the window between appending an event and saving the projection.
///
/// Current workspaces persist crash repair. Legacy workspaces expose the same
/// effective Object in memory, because requiring a write here would make a
/// valid old workspace unreadable before its explicit migration.
pub fn reconcile(root: &Path, id: &str) -> Result<Object> {
    store::with_lock(root, || reconcile_locked(root, id))
}

pub(crate) fn reconcile_locked(root: &Path, id: &str) -> Result<Object> {
    let (object, applied) = replay(root, id, true)?;
    if applied && store::validate_format(root)? == store::WorkspaceFormat::Current {
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
    let object_tampered = object
        .sha256
        .as_deref()
        .is_some_and(|seal| crate::integrity::check_object_integrity(object, seal).is_err());
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
        let section_failed = if object.sha256.is_some() {
            crate::integrity::check_section_seal(section, &section.sha256).is_err()
        } else {
            section.recomputed_sha256()? != section.sha256
        };
        if section_failed {
            tampered.push(section.id);
        }
        for reference in &section.refs {
            let (target_id, target_section_id) = reference.target_identity()?;
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
            let Ok(target_section) = target.section(target_section_id) else {
                standing_on_missing.push(StandsOnMissing {
                    section: section.id,
                    target: target_id.clone(),
                    target_section: target_section_id,
                });
                continue;
            };
            if let crate::model::Ref::Selective(selective) = reference {
                let current_failed = target.sha256.as_deref().map_or(true, |seal| {
                    crate::integrity::check_object_integrity(&target, seal).is_err()
                });
                let Some(seal) = target.sha256.as_deref() else {
                    standing_on_unreadable.push(StandsOnUnreadable {
                        section: section.id,
                        target: target_id.clone(),
                        target_section: target_section_id,
                        reason: "target has no aggregate integrity seal".to_owned(),
                    });
                    continue;
                };
                match crate::dependency::evaluate(root, &target, seal, selective)? {
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
                continue;
            }
            if target_section.recomputed_sha256()? != target_section.sha256 {
                standing_on_tampered.push(StandsOnTampered {
                    section: section.id,
                    target: target_id,
                    target_section: target_section_id,
                    side: None,
                });
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
        uncommitted: git::uncommitted(root, &store::object_path(root, id)),
    })
}
