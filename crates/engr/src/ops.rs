//! Maintenance: crash reconciliation, purge, and verify.

use crate::model::{project, Action, Object};
use crate::{ensure, git, store, Result, EXIT_INVARIANT, EXIT_NOT_FOUND};
use std::path::Path;

/// Close the window between appending an event and saving the projection.
///
/// `confirm` writes the event first, then the object. A crash in between leaves
/// an event whose rev is ahead of the object; replaying the tail here makes the
/// two agree instead of leaving the workspace wedged.
pub fn reconcile(root: &Path, id: &str) -> Result<Object> {
    let events = store::load_events(root, id)?;
    let mut object = match store::load_object(root, id) {
        Ok(object) => object,
        Err(error) if error.code == EXIT_NOT_FOUND => {
            let created = events.iter().find(|event| event.rev == 1).ok_or(error)?;
            ensure!(
                created.payload.object == id
                    && matches!(&created.payload.action, Action::ObjectCreated),
                EXIT_INVARIANT,
                "{id}: event rev 1 cannot reconstruct a missing object"
            );
            Object::new(id.to_owned(), String::new())
        }
        Err(error) => return Err(error),
    };
    let mut applied = false;
    // Walk forward one rev at a time, which also refuses to skip a gap.
    while let Some(event) = events.iter().find(|event| event.rev == object.rev + 1) {
        project(&mut object, event)?;
        applied = true;
    }
    if applied {
        store::save_object(root, &object)?;
    }
    Ok(object)
}

#[derive(Debug)]
pub struct Purged {
    pub events: usize,
    pub commit: Option<String>,
}

/// Discard the event buffer for one object.
///
/// Not gated: it changes nothing a human confirmed, so it is garbage collection
/// rather than a semantic act. The guard is mechanical instead — refuse unless
/// every event being dropped is already reflected in the sections, because
/// silently dropping an unprojected event would lose confirmed content.
pub fn purge(root: &Path, id: &str) -> Result<Purged> {
    let mut object = reconcile(root, id)?;
    let events = store::load_events(root, id)?;
    let unprojected = events.iter().filter(|event| event.rev > object.rev).count();
    ensure!(
        unprojected == 0,
        EXIT_INVARIANT,
        "{unprojected} events are not reflected in the sections yet; refusing to purge"
    );
    let recovery_events = crate::gate::pending(root)?
        .iter()
        .filter(|candidate| candidate.payload.object == object.id)
        .filter(|candidate| {
            events.iter().any(|event| {
                event.rev == candidate.expected_rev + 1
                    && event.confirmation.payload_sha256 == candidate.payload_sha256
            })
        })
        .count();
    ensure!(
        recovery_events == 0,
        EXIT_INVARIANT,
        "{recovery_events} events are still needed to finish crash recovery; refusing to purge"
    );
    let count = events.len();
    object.last_projection_commit = git::head(root);
    store::save_object(root, &object)?;
    store::discard_events(root, id)?;
    Ok(Purged {
        events: count,
        commit: object.last_projection_commit.clone(),
    })
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
/// None of this catches an edit that recomputes the hash too — once the events
/// are purged the hash sits beside the content it covers, so committed git
/// history is the real tamper anchor. That is why an uncommitted object file is
/// reported here.
pub fn verify(root: &Path, id: &str) -> Result<Report> {
    let object = store::load_object(root, id)?;
    let events = store::load_events(root, id)?;
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
        unprojected: events.iter().filter(|event| event.rev > object.rev).count(),
        uncommitted: git::uncommitted(root, &store::object_path(root, id)),
    })
}
