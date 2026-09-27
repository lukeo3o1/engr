//! ChangeSet: several mutations of one Object, reviewed once and admitted
//! together.
//!
//! A Rule Review costs a reader who never saw the draft, and an agent recording
//! eight decisions about one piece of work paid for eight of them — about half
//! of a dogfood session, and the reason the other sessions recorded none. The
//! growth rule names the signal for more than one action per admission ("one
//! piece of work needs the same object prepared … three times over, and the
//! human says so"), and both halves of it fired.
//!
//! A ChangeSet is the mutation boundary #41 describes, in its narrowest useful
//! form: an ordered list of Section mutations to **one existing Object**, kept
//! as local, non-authoritative working state until it is applied. Applying it is
//! one Agent admission of the whole sequence under one review. Each step is
//! checked exactly as a lone mutation is, and each becomes its own Event, so
//! nothing a ChangeSet admits could not have been admitted one step at a time —
//! it is the review that is shared, not the rules.
//!
//! **One Object is what makes it atomic without a new recovery model.** The
//! records go out as one publication of the Object's stream, which readers see
//! whole or not at all, and history ahead of the projection is the crash the
//! store already reconciles. Across several Objects there is no single
//! publication to lean on, and #41's reader invariant would need a mechanism of
//! its own; that is left until something needs it.
//!
//! **Local, and not in git.** Under `.engr/local/changesets/`, which the
//! workspace never commits: a ChangeSet is intent that has not been admitted,
//! and it survives a restart on this machine so an interrupted agent can pick
//! the draft up again rather than rebuilding it from a conversation that is
//! gone. It is not a record and nothing refers to it.

use crate::gate::{self, ChangeSetOutcome, ChangeSetPlan, ReviewAttestation};
use crate::model::{Event, Object, Payload};
use crate::{
    ensure, store, Error, Result, EXIT_INVARIANT, EXIT_NOT_FOUND, EXIT_SCHEMA, EXIT_USAGE,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A ChangeSet as it is kept on this machine.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct ChangeSet {
    pub id: String,
    /// The one Object every step changes.
    pub object: String,
    pub created_at: String,
    /// In the order they will be admitted. Order is semantic: a revise after an
    /// add is not the same act as the add alone.
    pub steps: Vec<Payload>,
    /// The records an apply was about to publish, written down before it
    /// published them.
    ///
    /// The one thing a restart must be able to tell apart is a ChangeSet that
    /// was admitted and one that was not, and never admit it twice. The Event
    /// ids are minted fresh by each attempt, so finding them in the Object's
    /// history is proof this attempt landed, and finding none is proof it did
    /// not. Nothing else about a crash needs deciding here: the publication is
    /// one rename, so there is no third answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub committing: Option<Vec<String>>,
}

/// What an apply did.
#[derive(Debug)]
pub enum Applied {
    /// Every step admitted, as these records, producing this Object.
    Admitted { events: Vec<Event>, object: Object },
    /// Governed and not yet reviewed: the subject, and nothing written.
    NeedsReview(Box<ChangeSetPlan>),
    /// An earlier apply published these records and was interrupted before it
    /// could say so. Nothing was admitted twice; the ChangeSet is gone now.
    AlreadyAdmitted { events: Vec<String> },
}

pub fn dir(root: &Path) -> PathBuf {
    store::local_dir(root).join("changesets")
}

fn path(root: &Path, id: &str) -> PathBuf {
    dir(root).join(format!("{id}.json"))
}

/// Start a ChangeSet for one existing Object.
pub fn create(root: &Path, object: &str) -> Result<ChangeSet> {
    store::require_current(root)?;
    store::with_lock(root, || {
        // The Object has to be one an admission could build on today. Finding
        // out at apply time, after the steps were drafted against it, is the
        // expensive time to find out.
        crate::ops::admission_predecessor(root, object)?;
        store::create_dir_durably(&dir(root))?;
        let changeset = ChangeSet {
            id: crate::model::new_id(),
            object: object.to_owned(),
            created_at: now(),
            steps: Vec::new(),
            committing: None,
        };
        save(root, &changeset)?;
        Ok(changeset)
    })
}

/// Every ChangeSet on this machine, oldest first.
pub fn list(root: &Path) -> Result<Vec<ChangeSet>> {
    store::require_current(root)?;
    let mut found = Vec::new();
    for id in ids(root)? {
        found.push(load_exact(root, &id)?);
    }
    found.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(found)
}

/// Find one ChangeSet by a unique prefix of its id, like an Object.
pub fn resolve(root: &Path, prefix: &str) -> Result<String> {
    ensure!(
        !prefix.is_empty(),
        EXIT_USAGE,
        "name a ChangeSet by its id or a unique prefix of it"
    );
    let matches: Vec<String> = ids(root)?
        .into_iter()
        .filter(|id| id.starts_with(prefix))
        .collect();
    match matches.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err(Error::new(
            EXIT_NOT_FOUND,
            format!("no ChangeSet on this machine starts with {prefix:?}; `engr changeset ls` lists them"),
        )),
        _ => Err(Error::new(
            EXIT_USAGE,
            format!("{prefix:?} names {} ChangeSets; give more of the id", matches.len()),
        )),
    }
}

pub fn load(root: &Path, id: &str) -> Result<ChangeSet> {
    store::require_current(root)?;
    load_exact(root, &resolve(root, id)?)
}

/// Append a step, and keep it only if the whole sequence still plans.
///
/// Checked now rather than at apply, because a step that cannot be admitted is
/// cheapest to hear about while its author still remembers writing it. What is
/// kept is the canonical payload the plan produced — bases and references
/// resolved — so the thing a reviewer is later shown is the thing that was
/// checked.
pub fn add(root: &Path, id: &str, step: Payload) -> Result<(ChangeSet, ChangeSetPlan)> {
    store::require_current(root)?;
    let id = resolve(root, id)?;
    store::with_lock(root, || {
        let mut changeset = load_exact(root, &id)?;
        refuse_while_committing(&changeset)?;
        ensure!(
            step.object == changeset.object,
            EXIT_USAGE,
            "this ChangeSet changes {}; a step for another Object belongs in a ChangeSet of its own",
            changeset.object
        );
        let mut steps = changeset.steps.clone();
        steps.push(step);
        let plan = gate::plan_changeset_locked(root, &changeset.object, steps)?;
        changeset.steps = plan.steps.iter().map(|step| step.payload.clone()).collect();
        save(root, &changeset)?;
        Ok((changeset, plan))
    })
}

/// Remove one step by its position, counted from 1.
pub fn remove(root: &Path, id: &str, step: usize) -> Result<ChangeSet> {
    store::require_current(root)?;
    let id = resolve(root, id)?;
    store::with_lock(root, || {
        let mut changeset = load_exact(root, &id)?;
        refuse_while_committing(&changeset)?;
        ensure!(
            step >= 1 && step <= changeset.steps.len(),
            EXIT_USAGE,
            "this ChangeSet has {} step(s); there is no step {step}",
            changeset.steps.len()
        );
        changeset.steps.remove(step - 1);
        save(root, &changeset)?;
        Ok(changeset)
    })
}

/// What the ChangeSet would admit, and the review it needs. Writes nothing.
pub fn plan(root: &Path, id: &str) -> Result<(ChangeSet, ChangeSetPlan)> {
    store::require_current(root)?;
    let id = resolve(root, id)?;
    store::with_lock(root, || {
        let changeset = load_exact(root, &id)?;
        refuse_while_committing(&changeset)?;
        let plan = gate::plan_changeset_locked(root, &changeset.object, changeset.steps.clone())?;
        Ok((changeset, plan))
    })
}

/// Admit every step under one review, or none of them.
///
/// Without an attestation this is [`plan`] with the same refusal a lone Agent
/// mutation gives. With one, the digest is recomputed under the writer lock
/// from the Object as it now stands, and the records are published together.
pub fn apply(root: &Path, id: &str, review: Option<ReviewAttestation>) -> Result<Applied> {
    store::require_current(root)?;
    let id = resolve(root, id)?;
    store::with_lock(root, || {
        let mut changeset = load_exact(root, &id)?;
        if let Some(events) = changeset.committing.clone() {
            if landed(root, &changeset.object, &events)? {
                store::remove_durably(&path(root, &id))?;
                return Ok(Applied::AlreadyAdmitted { events });
            }
            // Written down and never published: the attempt did not happen, and
            // the ChangeSet is exactly as it was before it.
            changeset.committing = None;
            save(root, &changeset)?;
        }
        let sealed = match gate::seal_changeset_locked(
            root,
            &changeset.object,
            changeset.steps.clone(),
            review,
        )? {
            ChangeSetOutcome::NeedsReview(plan) => return Ok(Applied::NeedsReview(plan)),
            ChangeSetOutcome::Sealed(sealed) => *sealed,
        };
        changeset.committing = Some(sealed.events.iter().map(|event| event.id.clone()).collect());
        save(root, &changeset)?;
        store::append_events_locked(root, &changeset.object, &sealed.events)?;
        store::save_object(root, &sealed.object)?;
        store::remove_durably(&path(root, &id))?;
        Ok(Applied::Admitted {
            events: sealed.events,
            object: sealed.object,
        })
    })
}

/// Throw a ChangeSet away. It was never a record, so nothing else changes.
pub fn discard(root: &Path, id: &str) -> Result<ChangeSet> {
    store::require_current(root)?;
    let id = resolve(root, id)?;
    store::with_lock(root, || {
        let changeset = load_exact(root, &id)?;
        // A ChangeSet caught mid-apply may already be admitted, and discarding
        // it would throw away the only thing that can say so.
        refuse_while_committing(&changeset)?;
        store::remove_durably(&path(root, &id))?;
        Ok(changeset)
    })
}

fn refuse_while_committing(changeset: &ChangeSet) -> Result<()> {
    ensure!(
        changeset.committing.is_none(),
        EXIT_INVARIANT,
        "an apply of this ChangeSet was interrupted; run `engr changeset apply {}` to find out \
         whether it was admitted before changing it",
        changeset.id
    );
    Ok(())
}

/// Whether an interrupted apply's records reached the Object's history.
///
/// All or none, because they were published in one rename. Some but not all
/// would mean the stream was written some other way, and that is refused
/// rather than resolved in either direction.
fn landed(root: &Path, object: &str, events: &[String]) -> Result<bool> {
    let history = store::load_events(root, object)?;
    let present = events
        .iter()
        .filter(|id| history.iter().any(|event| &event.id == *id))
        .count();
    ensure!(
        present == 0 || present == events.len(),
        EXIT_INVARIANT,
        "{present} of this ChangeSet's {} records are in the history of {object}; they were \
         published together, so the stream was changed some other way",
        events.len()
    );
    Ok(present == events.len())
}

fn ids(root: &Path) -> Result<Vec<String>> {
    let dir = dir(root);
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(crate::tool_error(dir.display(), error)),
    };
    let mut ids = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| crate::tool_error(dir.display(), error))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(id) = name.strip_suffix(".json") else {
            continue;
        };
        if crate::model::validate_object_id(id).is_ok() {
            ids.push(id.to_owned());
        }
    }
    ids.sort();
    Ok(ids)
}

fn load_exact(root: &Path, id: &str) -> Result<ChangeSet> {
    let changeset: ChangeSet = store::read_resource(root, &path(root, id))?;
    ensure!(
        changeset.id == id,
        EXIT_SCHEMA,
        "ChangeSet file {id} names {}; it would show one set of steps and admit another",
        changeset.id
    );
    for step in &changeset.steps {
        ensure!(
            step.object == changeset.object,
            EXIT_SCHEMA,
            "ChangeSet {id} holds a step for another Object"
        );
    }
    Ok(changeset)
}

fn save(root: &Path, changeset: &ChangeSet) -> Result<()> {
    store::write_json(&path(root, &changeset.id), changeset)
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("formatting a timestamp cannot fail")
}
