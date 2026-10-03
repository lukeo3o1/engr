//! ChangeSet: several mutations of one Object or one backlog topic, reviewed
//! once and admitted together.
//!
//! A Rule Review costs a reader who never saw the draft, and an agent recording
//! eight decisions about one piece of work paid for eight of them — about half
//! of a dogfood session, and the reason the other sessions recorded none. The
//! growth rule names the signal for more than one action per admission ("one
//! piece of work needs the same object prepared … three times over, and the
//! human says so"), and both halves of it fired.
//!
//! A ChangeSet is the mutation boundary #41 describes, in its narrowest useful
//! form: an ordered list of mutations to **one Object** — its title and its
//! Sections, the first step creating it if it does not exist yet — kept as
//! local, non-authoritative working state until it is applied. Applying it
//! admits, under one review, every step that review passed. Each step is
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
//! **Or to one backlog topic**, for the same reason once a backlog Rule
//! governs every backlog write: a topic written point by point paid a review
//! per point. Its steps run the bodies the lone backlog mutations run, and it
//! is written whole, in the one rename that replaces the topic's file — so the
//! same one-thing-one-publication argument holds without a recovery model of
//! its own beyond `committing`.
//!
//! **Local, and not in git.** Under `.engr/local/changesets/`, which the
//! workspace never commits: a ChangeSet is intent that has not been admitted,
//! and it survives a restart on this machine so an interrupted agent can pick
//! the draft up again rather than rebuilding it from a conversation that is
//! gone. It is not a record and nothing refers to it.

use crate::backlog::{self, Item, TopicOutcome, TopicPlan};
use crate::gate::{self, ChangeSetOutcome, ChangeSetPlan, ReviewAttestation};
use crate::model::{Action, Event, Object, Payload};
use crate::{
    ensure, store, Error, Result, EXIT_INVARIANT, EXIT_NOT_FOUND, EXIT_SCHEMA, EXIT_USAGE,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A ChangeSet as it is kept on this machine.
///
/// It changes exactly one thing: an Object, or a backlog topic. Exactly one of
/// `object` and `topic` is set, and only that one's steps.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct ChangeSet {
    pub id: String,
    /// The one Object every step changes. When the Object does not exist yet,
    /// the first step creates it under this id, issued when the ChangeSet was
    /// started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
    /// The one backlog topic every step changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<Topic>,
    pub created_at: String,
    /// The Object's steps, in the order they will be admitted. Order is
    /// semantic: a revise after an add is not the same act as the add alone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<Payload>,
    /// The topic's steps, in order, for the same reason.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backlog_steps: Vec<backlog::Step>,
    /// What an apply was about to publish, written down before it published it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub committing: Option<Committing>,
}

/// The backlog topic a ChangeSet changes.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Topic {
    pub id: String,
    /// Set when the ChangeSet creates the topic: the title it will have. The id
    /// is issued when the ChangeSet is started, as an Object's is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creates: Option<String>,
}

/// What a ChangeSet changes, read off the one field that names it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target<'a> {
    Object(&'a str),
    Topic(&'a Topic),
}

impl ChangeSet {
    pub fn target(&self) -> Target<'_> {
        match (&self.object, &self.topic) {
            (Some(object), None) => Target::Object(object),
            (None, Some(topic)) => Target::Topic(topic),
            // `load_exact` refuses both shapes, so a ChangeSet in hand has one.
            _ => unreachable!("a loaded ChangeSet names exactly one Object or topic"),
        }
    }

    /// The Object or topic id, for a listing.
    pub fn subject(&self) -> &str {
        match self.target() {
            Target::Object(object) => object,
            Target::Topic(topic) => &topic.id,
        }
    }

    /// How many steps it holds, whichever kind they are.
    pub fn len(&self) -> usize {
        self.steps.len() + self.backlog_steps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn object_id(&self) -> Result<&str> {
        match self.target() {
            Target::Object(object) => Ok(object),
            Target::Topic(topic) => Err(Error::new(
                EXIT_USAGE,
                format!(
                    "ChangeSet {} changes backlog {}; its steps are added with `engr changeset backlog`",
                    self.id, topic.id
                ),
            )),
        }
    }

    fn topic(&self) -> Result<&Topic> {
        match self.target() {
            Target::Topic(topic) => Ok(topic),
            Target::Object(object) => Err(Error::new(
                EXIT_USAGE,
                format!(
                    "ChangeSet {} changes Object {object}; its steps are added with `engr changeset add`",
                    self.id
                ),
            )),
        }
    }
}

/// The records an apply was about to publish, and which steps they are.
///
/// The one thing a restart must be able to tell apart is steps that were
/// admitted and steps that were not, and never admit one twice. The Event ids
/// are minted fresh by each attempt, so finding them in the Object's history is
/// proof this attempt landed, and finding none is proof it did not. Nothing else
/// about a crash needs deciding here: the publication is one rename, so there is
/// no third answer. The steps are named because a review may have passed only
/// some of them, and the ones it failed must survive the crash still waiting.
///
/// A topic is one file replaced in one rename, so what an apply writes down
/// for it is the topic it is about to leave — as a token, or `null` for none —
/// and the same two answers follow: the topic is that, or it is what it was.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Committing {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<String>,
    /// Counted from 0, in the order they were admitted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<usize>,
    /// For a topic: what it will be once the apply has landed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<TopicResult>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct TopicResult {
    pub result: Option<String>,
}

/// What an apply did.
#[derive(Debug)]
pub enum Applied {
    /// The steps the review passed, admitted as these records and producing
    /// this Object. `remaining` steps failed and are still in the ChangeSet.
    Admitted {
        events: Vec<Event>,
        object: Object,
        remaining: usize,
    },
    /// The review failed every step. Nothing was written.
    NoneAdmitted { remaining: usize },
    /// Governed and not yet reviewed: the subject, and nothing written.
    NeedsReview(Box<ChangeSetPlan>),
    /// An earlier apply published these records and was interrupted before it
    /// could say so. Nothing was admitted twice, and the steps it admitted are
    /// gone from the ChangeSet now; `remaining` are the ones it did not.
    AlreadyAdmitted {
        events: Vec<String>,
        remaining: usize,
    },
}

pub fn dir(root: &Path) -> PathBuf {
    store::local_dir(root).join("changesets")
}

fn path(root: &Path, id: &str) -> PathBuf {
    dir(root).join(format!("{id}.json"))
}

fn empty(object: Option<String>, topic: Option<Topic>) -> ChangeSet {
    ChangeSet {
        id: crate::model::new_id(),
        object,
        topic,
        created_at: now(),
        steps: Vec::new(),
        backlog_steps: Vec::new(),
        committing: None,
    }
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
        let changeset = empty(Some(object.to_owned()), None);
        save(root, &changeset)?;
        Ok(changeset)
    })
}

/// Start a ChangeSet that creates an Object, with the creation as its first
/// step.
///
/// The id is issued now and kept with the ChangeSet, so every attempt at its
/// review is of the same Object: a creation alone names no target because its
/// id is minted afresh by each attempt, and here it is not.
pub fn create_object(root: &Path, title: &str) -> Result<(ChangeSet, ChangeSetPlan)> {
    store::require_current(root)?;
    store::with_lock(root, || {
        let object = crate::model::new_id();
        let mut changeset = empty(Some(object.clone()), None);
        let creation = Payload::new(
            object.clone(),
            Action::ObjectCreated {
                title: title.to_owned(),
            },
        );
        let plan = gate::plan_changeset_locked(root, &object, vec![creation])?;
        changeset.steps = plan.steps.iter().map(|step| step.payload.clone()).collect();
        store::create_dir_durably(&dir(root))?;
        save(root, &changeset)?;
        Ok((changeset, plan))
    })
}

/// Start a ChangeSet for one backlog topic: an existing one, or — with
/// `creates` — a new one under that title.
pub fn create_topic(root: &Path, topic: Option<&str>, creates: Option<&str>) -> Result<ChangeSet> {
    store::require_current(root)?;
    store::with_lock(root, || {
        let topic = match (topic, creates) {
            (Some(id), None) => {
                backlog::load(root, id)?;
                Topic {
                    id: id.to_owned(),
                    creates: None,
                }
            }
            (None, Some(title)) => Topic {
                id: backlog::mint_id(),
                creates: Some(title.trim().to_owned()),
            },
            _ => {
                return Err(Error::new(
                    EXIT_USAGE,
                    "a ChangeSet changes one existing topic or creates one, not both",
                ))
            }
        };
        // Asked now rather than at the first step: a ChangeSet over a topic no
        // Rule governs has nothing to share, and that is cheapest to hear before
        // any step is drafted for it.
        ensure!(
            !crate::rules::resolved(root, crate::rules::Domain::Backlog)?.is_empty(),
            EXIT_INVARIANT,
            "no backlog Rule applies here, so there is no review for a ChangeSet to share; make each change with its own `engr backlog` command"
        );
        store::create_dir_durably(&dir(root))?;
        let changeset = empty(None, Some(topic));
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
        let object = changeset.object_id()?.to_owned();
        ensure!(
            step.object == object,
            EXIT_USAGE,
            "this ChangeSet changes {object}; a step for another Object belongs in a ChangeSet of its own"
        );
        let mut steps = changeset.steps.clone();
        steps.push(step);
        let plan = gate::plan_changeset_locked(root, &object, steps)?;
        changeset.steps = plan.steps.iter().map(|step| step.payload.clone()).collect();
        save(root, &changeset)?;
        Ok((changeset, plan))
    })
}

/// Append a step to a topic's ChangeSet, and keep it only if the whole
/// sequence still plans — for the reason [`add`] gives.
pub fn add_backlog(root: &Path, id: &str, step: backlog::Step) -> Result<(ChangeSet, TopicPlan)> {
    store::require_current(root)?;
    let id = resolve(root, id)?;
    store::with_lock(root, || {
        let mut changeset = load_exact(root, &id)?;
        refuse_while_committing(&changeset)?;
        let topic = changeset.topic()?.clone();
        let mut steps = changeset.backlog_steps.clone();
        steps.push(step);
        let plan =
            backlog::plan_changeset_locked(root, &topic.id, topic.creates.as_deref(), &steps)?;
        changeset.backlog_steps = steps;
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
            step >= 1 && step <= changeset.len(),
            EXIT_USAGE,
            "this ChangeSet has {} step(s); there is no step {step}",
            changeset.len()
        );
        // A new Object is its first step. Taking the creation out would leave
        // steps acting on nothing, and starting again is the honest way back.
        if let (Target::Object(_), Some(first)) = (changeset.target(), changeset.steps.first()) {
            ensure!(
                !(step == 1 && matches!(first.action, Action::ObjectCreated { .. })),
                EXIT_USAGE,
                "step 1 creates the Object every other step changes; discard the ChangeSet instead"
            );
        }
        match changeset.target() {
            Target::Object(_) => {
                changeset.steps.remove(step - 1);
            }
            Target::Topic(_) => {
                changeset.backlog_steps.remove(step - 1);
            }
        }
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
        let object = changeset.object_id()?.to_owned();
        let plan = gate::plan_changeset_locked(root, &object, changeset.steps.clone())?;
        Ok((changeset, plan))
    })
}

/// What a topic's ChangeSet would leave, and the review it needs. Writes
/// nothing.
pub fn plan_topic(root: &Path, id: &str) -> Result<(ChangeSet, TopicPlan)> {
    store::require_current(root)?;
    let id = resolve(root, id)?;
    store::with_lock(root, || {
        let changeset = load_exact(root, &id)?;
        refuse_while_committing(&changeset)?;
        let topic = changeset.topic()?.clone();
        let plan = backlog::plan_changeset_locked(
            root,
            &topic.id,
            topic.creates.as_deref(),
            &changeset.backlog_steps,
        )?;
        Ok((changeset, plan))
    })
}

/// Admit the steps a review passed, and keep the ones it failed.
///
/// Without an attestation this is [`plan`] with the same refusal a lone Agent
/// mutation gives. With one, the digest is recomputed under the writer lock
/// from the Object as it now stands, and every step the review passed is
/// published together; `failed` names the rest, counted from 1.
pub fn apply(
    root: &Path,
    id: &str,
    review: Option<ReviewAttestation>,
    failed: &[usize],
) -> Result<Applied> {
    store::require_current(root)?;
    let id = resolve(root, id)?;
    store::with_lock(root, || {
        let mut changeset = load_exact(root, &id)?;
        let object = changeset.object_id()?.to_owned();
        if let Some(committing) = changeset.committing.clone() {
            if landed(root, &object, &committing.events)? {
                let remaining = finish(root, &mut changeset, &committing.steps)?;
                return Ok(Applied::AlreadyAdmitted {
                    events: committing.events,
                    remaining,
                });
            }
            // Written down and never published: the attempt did not happen, and
            // the ChangeSet is exactly as it was before it.
            changeset.committing = None;
            save(root, &changeset)?;
        }
        let sealed = match gate::seal_changeset_locked(
            root,
            &object,
            changeset.steps.clone(),
            review,
            failed,
        )? {
            ChangeSetOutcome::NeedsReview(plan) => return Ok(Applied::NeedsReview(plan)),
            ChangeSetOutcome::Sealed(sealed) => *sealed,
        };
        if sealed.events.is_empty() {
            return Ok(Applied::NoneAdmitted {
                remaining: changeset.steps.len(),
            });
        }
        changeset.committing = Some(Committing {
            events: sealed.events.iter().map(|event| event.id.clone()).collect(),
            steps: sealed.admitted.clone(),
            topic: None,
        });
        save(root, &changeset)?;
        store::append_events_locked(root, &object, &sealed.events)?;
        store::save_object(root, &sealed.object)?;
        let remaining = finish(root, &mut changeset, &sealed.admitted)?;
        Ok(Applied::Admitted {
            events: sealed.events,
            object: sealed.object,
            remaining,
        })
    })
}

/// What applying a topic's ChangeSet did.
#[derive(Debug)]
pub enum TopicApplied {
    /// Every step admitted: the topic as they left it, or `None` if the last
    /// step consumed its last point.
    Admitted { result: Option<Item> },
    /// Governed and not yet reviewed: the subject, and nothing written.
    NeedsReview(Box<TopicPlan>),
    /// An earlier apply wrote the topic and was interrupted before it could
    /// say so. Nothing was written twice, and the ChangeSet is gone now.
    AlreadyAdmitted,
}

/// Admit every step of a topic's ChangeSet under one review, or none.
///
/// Without an attestation this is [`plan_topic`] with a refusal; with one, the
/// digest is recomputed under the writer lock from the topic as it now stands.
pub fn apply_topic(root: &Path, id: &str, prepared: &backlog::Prepared) -> Result<TopicApplied> {
    store::require_current(root)?;
    let id = resolve(root, id)?;
    store::with_lock(root, || {
        let mut changeset = load_exact(root, &id)?;
        let topic = changeset.topic()?.clone();
        if let Some(committing) = changeset.committing.clone() {
            let token = committing.topic.and_then(|topic| topic.result);
            if backlog::landed(root, &topic.id, token.as_deref())? {
                store::remove_durably(&path(root, &changeset.id))?;
                return Ok(TopicApplied::AlreadyAdmitted);
            }
            changeset.committing = None;
            save(root, &changeset)?;
        }
        let sealed = match backlog::seal_changeset_locked(
            root,
            &topic.id,
            topic.creates.as_deref(),
            &changeset.backlog_steps,
            prepared,
        )? {
            TopicOutcome::NeedsReview(plan) => {
                return Ok(TopicApplied::NeedsReview(Box::new(plan)))
            }
            TopicOutcome::Sealed(sealed) => sealed,
        };
        changeset.committing = Some(Committing {
            events: Vec::new(),
            steps: Vec::new(),
            topic: Some(TopicResult {
                result: backlog::result_token(sealed.result.as_ref())?,
            }),
        });
        save(root, &changeset)?;
        backlog::publish_changeset_locked(root, &sealed)?;
        store::remove_durably(&path(root, &changeset.id))?;
        Ok(TopicApplied::Admitted {
            result: sealed.result,
        })
    })
}

/// Take the admitted steps out, and the ChangeSet with them once it is empty.
fn finish(root: &Path, changeset: &mut ChangeSet, admitted: &[usize]) -> Result<usize> {
    changeset.steps = std::mem::take(&mut changeset.steps)
        .into_iter()
        .enumerate()
        .filter(|(index, _)| !admitted.contains(index))
        .map(|(_, step)| step)
        .collect();
    changeset.committing = None;
    if changeset.steps.is_empty() {
        store::remove_durably(&path(root, &changeset.id))?;
    } else {
        save(root, changeset)?;
    }
    Ok(changeset.steps.len())
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
    match (&changeset.object, &changeset.topic) {
        (Some(object), None) => {
            ensure!(
                changeset.backlog_steps.is_empty(),
                EXIT_SCHEMA,
                "ChangeSet {id} changes an Object and holds backlog steps"
            );
            for step in &changeset.steps {
                ensure!(
                    &step.object == object,
                    EXIT_SCHEMA,
                    "ChangeSet {id} holds a step for another Object"
                );
            }
        }
        (None, Some(_)) => ensure!(
            changeset.steps.is_empty(),
            EXIT_SCHEMA,
            "ChangeSet {id} changes a backlog topic and holds Object steps"
        ),
        _ => {
            return Err(Error::new(
                EXIT_SCHEMA,
                format!("ChangeSet {id} must name exactly one Object or one backlog topic"),
            ))
        }
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
