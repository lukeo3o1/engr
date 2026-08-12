//! Maintenance: crash reconciliation, purge, and verify.

use crate::model::{project, Object};
use crate::{ensure, git, store, Result, EXIT_INVARIANT};
use std::path::Path;

/// Close the window between appending an event and saving the projection.
///
/// `confirm` writes the event first, then the object. A crash in between leaves
/// an event whose rev is ahead of the object; replaying the tail here makes the
/// two agree instead of leaving the workspace wedged.
pub fn reconcile(root: &Path, id: &str) -> Result<Object> {
    let mut object = store::load_object(root, id)?;
    let events = store::load_events(root, id)?;
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
    let count = events.len();
    object.last_projection_commit = git::head(root);
    store::save_object(root, &object)?;
    store::discard_events(root, id)?;
    Ok(Purged {
        events: count,
        commit: object.last_projection_commit.clone(),
    })
}

#[derive(Debug)]
pub struct Report {
    pub object: String,
    pub title: String,
    pub sections: usize,
    pub tampered: Vec<u64>,
    pub unprojected: usize,
    pub uncommitted: Option<bool>,
}

impl Report {
    pub fn passed(&self) -> bool {
        self.tampered.is_empty() && self.unprojected == 0
    }
}

/// Recompute every section's hash from what is stored.
///
/// This catches a section edited without recomputing its hash. It cannot catch
/// an edit that recomputes the hash too — once the events are purged, the hash
/// sits beside the content it covers, so committed git history is the real
/// tamper anchor. That is why an uncommitted object file is reported here.
pub fn verify(root: &Path, id: &str) -> Result<Report> {
    let object = store::load_object(root, id)?;
    let events = store::load_events(root, id)?;
    let mut tampered = Vec::new();
    for section in &object.sections {
        if section.recomputed_sha256()? != section.sha256 {
            tampered.push(section.id);
        }
    }
    Ok(Report {
        object: object.id.clone(),
        title: object.title.clone(),
        sections: object.sections.len(),
        tampered,
        unprojected: events.iter().filter(|event| event.rev > object.rev).count(),
        uncommitted: git::uncommitted(root, &store::object_path(root, id)),
    })
}
