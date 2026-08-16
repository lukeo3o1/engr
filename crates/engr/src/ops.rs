//! Maintenance: crash reconciliation and verify.

use crate::model::{replay_recoverable_tail, Action, Object, Section};
use crate::{ensure, git, store, Error, Result, EXIT_INVARIANT, EXIT_NOT_FOUND, EXIT_SCHEMA};
use std::path::Path;

/// Replay the recoverable tail without choosing whether it may be persisted.
/// Reads need the same effective Object as a repaired current workspace, while
/// a legacy workspace must remain byte-for-byte read-only until migration.
fn replay(root: &Path, id: &str) -> Result<(Object, bool)> {
    let events = store::load_events(root, id)?;
    let object = match store::load_object(root, id) {
        Ok(object) => object,
        Err(error) if error.code == EXIT_NOT_FOUND => {
            let created = events.iter().find(|event| event.rev == 1).ok_or(error)?;
            ensure!(
                created.payload.object == id
                    && matches!(&created.payload.action, Action::ObjectCreated),
                EXIT_INVARIANT,
                "{id}: event rev 1 cannot reconstruct a missing object"
            );
            Object::new(id.to_owned(), String::new())?
        }
        Err(error) => return Err(error),
    };
    replay_recoverable_tail(object, &events).map_err(|error| {
        Error::new(
            EXIT_SCHEMA,
            format!("{id}: event tail cannot reconcile: {}", error.message),
        )
    })
}

/// Read the effective authority after applying any recoverable crash tail in
/// memory. Unlike [`reconcile`], this never writes a projection.
pub fn effective(root: &Path, id: &str) -> Result<Object> {
    Ok(replay(root, id)?.0)
}

/// Read one current target section through the same effective authority used by
/// every read surface. Reference admission must never pin a stale projection
/// while a confirmed recovery tail already carries newer wording.
pub fn effective_section(root: &Path, id: &str, section: u64) -> Result<Section> {
    effective(root, id)?.section(section).cloned()
}

/// Close the window between appending an event and saving the projection.
///
/// Current workspaces persist crash repair. Legacy workspaces expose the same
/// effective Object in memory, because requiring a write here would make a
/// valid old workspace unreadable before its explicit migration.
pub fn reconcile(root: &Path, id: &str) -> Result<Object> {
    let (object, applied) = replay(root, id)?;
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
}

#[derive(Debug)]
pub struct Report {
    pub object: String,
    pub title: String,
    pub sections: usize,
    pub tampered: Vec<u64>,
    pub standing_on_tampered: Vec<StandsOnTampered>,
    pub unprojected: usize,
    pub uncommitted: Option<bool>,
}

impl Report {
    pub fn passed(&self) -> bool {
        self.tampered.is_empty() && self.standing_on_tampered.is_empty() && self.unprojected == 0
    }
}

/// Recompute every section's hash from what is stored, and the hash of every
/// section this object explicitly stands on.
///
/// The second half is not redundant. A ref pins the target's hash, so drift is
/// detected by comparing hashes — but an edit that rewrites the target's text
/// and leaves its stored hash alone moves neither side of that comparison. The
/// ref looks unmoved, and this object would report PASS while the wording under
/// it says something nobody agreed to. Only the section directly referenced is
/// checked: the target's own `verify` covers what *it* stands on.
///
/// None of this catches an edit that recomputes the hash too. Append-only events
/// preserve confirmed evidence, while committed git history remains an
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
    let mut tampered = Vec::new();
    let mut standing_on_tampered = Vec::new();
    for section in &object.sections {
        if section.recomputed_sha256()? != section.sha256 {
            tampered.push(section.id);
        }
        for reference in &section.refs {
            let Ok(target) = store::load_object(root, &reference.object) else {
                continue;
            };
            let Ok(target_section) = target.section(reference.section) else {
                continue;
            };
            if target_section.recomputed_sha256()? != target_section.sha256 {
                standing_on_tampered.push(StandsOnTampered {
                    section: section.id,
                    target: reference.object.clone(),
                    target_section: reference.section,
                });
            }
        }
    }
    Ok(Report {
        object: object.id.clone(),
        title: object.title.clone(),
        sections: object.sections.len(),
        tampered,
        standing_on_tampered,
        unprojected: events
            .iter()
            .filter(|event| event.rev > projection_rev)
            .count(),
        uncommitted: git::uncommitted(root, &store::object_path(root, id)),
    })
}
