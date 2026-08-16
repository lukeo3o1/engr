//! The Human Alignment Gate — the only way anything enters the record.
//!
//! `prepare` puts a candidate up and mints a challenge; `confirm` admits it only
//! against the exact response. There is no unconfirmed write path.

use crate::model::{
    project, Action, Confirmation, Content, Event, Object, Payload, CANDIDATE_FORMAT, EVENT_FORMAT,
};
use crate::{
    ensure, git, ops, store, tool_error, Error, Result, EXIT_INVARIANT, EXIT_NOT_FOUND,
    EXIT_SCHEMA, EXIT_USAGE, FORMAT_VERSION,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Candidate {
    pub format: String,
    pub version: u32,
    #[serde(flatten)]
    pub gate: crate::confirmation::Candidate<Payload, ObjectBinding>,
    /// What the human is shown when the candidate carries a change to existing
    /// wording, so they read the change rather than the whole section again.
    pub previous_text: Option<String>,
    /// Previous semantic content for revisions. These remain separate from
    /// `previous_text` so candidates minted by older builds still load; the
    /// action tells the renderer whether absence means "no basis" or "not a
    /// section revision".
    #[serde(default)]
    pub previous_based_on: Option<String>,
    #[serde(default)]
    pub previous_refs: Vec<crate::model::Ref>,
    /// Distinguishes a new revision with no old basis/refs from a legacy
    /// candidate that never captured those values at all.
    #[serde(default)]
    pub previous_semantics_recorded: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ObjectBinding {
    /// The object revision this was prepared against.
    pub expected_rev: u64,
}

impl std::ops::Deref for Candidate {
    type Target = crate::confirmation::Candidate<Payload, ObjectBinding>;

    fn deref(&self) -> &Self::Target {
        &self.gate
    }
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("formatting a timestamp cannot fail")
}

pub fn pending(root: &Path) -> Result<Vec<Candidate>> {
    let dir = store::candidates_dir(root);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|error| tool_error(dir.display(), error))? {
        let entry = entry.map_err(|error| tool_error(dir.display(), error))?;
        let path = entry.path();
        if path.extension().and_then(|item| item.to_str()) != Some("json") {
            continue;
        }
        let candidate: Candidate = store::read_json(&path)?;
        ensure!(
            candidate.format == CANDIDATE_FORMAT,
            EXIT_SCHEMA,
            "{}: not an engr candidate",
            path.display()
        );
        found.push(candidate);
    }
    found.sort_by(|left, right| left.created_at.cmp(&right.created_at));
    Ok(found)
}

pub fn find(root: &Path, challenge: &str) -> Result<Candidate> {
    let path = store::candidate_path(root, challenge);
    ensure!(
        path.exists(),
        EXIT_NOT_FOUND,
        "no candidate awaiting {challenge}"
    );
    store::read_json(&path)
}

/// Whether a pending candidate can still be confirmed, or was overtaken.
pub fn is_live(root: &Path, candidate: &Candidate) -> bool {
    match store::load_object(root, &candidate.payload.object) {
        Ok(object) => object.rev == candidate.binding.expected_rev,
        Err(_) => candidate.binding.expected_rev == 0,
    }
}

/// Something worth weighing before typing the code, but not grounds to refuse.
/// Rendered with the candidate, because the moment to reconsider is while the
/// human is still holding the code.
#[derive(Debug)]
pub enum Note {
    DuplicateTitle { object: String },
}

#[derive(Debug)]
pub struct Prepared {
    pub candidate: Candidate,
    pub superseded: Vec<String>,
    pub notes: Vec<Note>,
}

/// A title is a label, not a body.
///
/// It is the line `ls` prints, so a paragraph pasted in here degrades the
/// listing for every other object as well as its own. `--rename` makes the
/// mistake recoverable rather than fatal, which is a reason to keep this check
/// and not a reason to drop it — the listing is wrecked either way until
/// someone notices. 120 characters is wide for a label and nowhere near a
/// paragraph.
const TITLE_MAX: usize = 120;

/// `flag` names what the caller actually typed, so the refusal reads as a fact
/// about their command rather than about the one that happens to be older.
fn check_title(flag: &str, text: &str) -> Result<()> {
    ensure!(
        !text.contains('\n'),
        EXIT_USAGE,
        "{flag} --text is the object's title, so it cannot span lines. \
         Keep it to a short label and put the detail in a section with --add."
    );
    let length = text.chars().count();
    ensure!(
        length <= TITLE_MAX,
        EXIT_USAGE,
        "{flag} --text is the object's title, not its body \
         ({length} characters, limit {TITLE_MAX}). Keep it to a short label and \
         put the detail in a section with --add."
    );
    Ok(())
}

/// Recomputed rather than stored, so `engr candidate <code>` shows the same
/// notes as `prepare` did — that screen is where a human reads before typing,
/// and a note absent from it is a note that missed its moment.
pub fn notes_for(root: &Path, candidate: &Candidate) -> Vec<Note> {
    let mut notes = Vec::new();
    if candidate.payload.action.carries_title() {
        let clash = object_with_title(
            root,
            &candidate.payload.content.text,
            &candidate.payload.object,
        );
        if let Some(object) = clash {
            notes.push(Note::DuplicateTitle { object });
        }
    }
    notes
}

/// Titles are not unique and are not meant to be — but two objects sharing one
/// cannot be told apart in `ls`, so say so rather than deciding for the human.
///
/// `excluding` is the object being written. A rename that only changes casing or
/// spacing would otherwise report a clash with itself, and a note that fires on
/// a non-problem is how people learn to skip the notes.
fn object_with_title(root: &Path, title: &str, excluding: &str) -> Option<String> {
    let needle = title.trim().to_lowercase();
    store::object_ids(root)
        .ok()?
        .iter()
        .filter(|id| id.as_str() != excluding)
        .filter_map(|id| store::load_object(root, id).ok())
        .find(|object| object.title.trim().to_lowercase() == needle)
        .map(|object| object.id)
}

/// Validate a proposed action against the current object and put it up for
/// confirmation. References are checked here, at the gate — not in `verify`.
/// Deferring that check is what let one mistyped id in the previous design
/// poison a global health check permanently.
pub fn prepare(root: &Path, mut payload: Payload) -> Result<Prepared> {
    store::require_current(root)?;
    // A title is the line a listing prints, so whitespace around it is never
    // meaningful and always visible — it pushes one row out of column. The
    // duplicate check already ignores it; storing what that check ignores is how
    // a listing ends up misaligned underneath a note saying the titles match.
    // Normalised here, before the candidate is minted, so the human confirms the
    // exact string that will be stored.
    if payload.action.carries_title() {
        payload.content.text = payload.content.text.trim().to_owned();
    }
    payload.validate()?;
    let object = match ops::reconcile(root, &payload.object) {
        Ok(object) => Some(object),
        Err(error) if error.code == EXIT_NOT_FOUND => None,
        Err(error) => return Err(error),
    };

    match (&payload.action, &object) {
        (Action::ObjectCreated, Some(_)) => {
            return Err(Error::new(
                EXIT_INVARIANT,
                "that object already exists".to_owned(),
            ))
        }
        // Here, not in `Payload::validate`: that runs when events are *loaded*,
        // so a limit enforced there would make a workspace holding an
        // over-long title unable to replay its own history.
        (Action::ObjectCreated, None) => check_title("--new", &payload.content.text)?,
        (Action::ObjectRenamed, Some(_)) => check_title("--rename", &payload.content.text)?,
        (_, None) => {
            return Err(Error::new(
                EXIT_NOT_FOUND,
                format!("no object {}", payload.object),
            ))
        }
        (_, Some(_)) => {}
    }

    let (previous_text, previous_based_on, previous_refs) = match (&payload.action, &object) {
        (Action::ObjectRenamed, Some(object)) => (Some(object.title.clone()), None, Vec::new()),
        (Action::SectionRevised { section }, Some(object)) => {
            let section = object.section(*section)?;
            (
                Some(section.text.clone()),
                section.based_on.clone(),
                section.refs.clone(),
            )
        }
        (Action::SectionDeleted { section }, Some(object)) => (
            Some(object.section(*section)?.text.clone()),
            None,
            Vec::new(),
        ),
        (Action::SectionMerged { absorbs }, Some(object)) => {
            let mut parts = Vec::new();
            for id in absorbs {
                parts.push(format!("§{id}: {}", object.section(*id)?.text.trim_end()));
            }
            (Some(parts.join("\n")), None, Vec::new())
        }
        _ => (None, None, Vec::new()),
    };

    // Preflight the reducer so a candidate that cannot possibly apply never
    // reaches a human.
    if let Some(object) = &object {
        let mut trial = object.clone();
        let probe = Event {
            format: EVENT_FORMAT.to_owned(),
            version: FORMAT_VERSION,
            event_id: String::new(),
            rev: object.rev + 1,
            time: now(),
            payload: payload.clone(),
            confirmation: Confirmation {
                challenge: String::new(),
                payload_sha256: String::new(),
            },
        };
        project(&mut trial, &probe)?;
    }

    validate_refs(root, &payload)?;
    if let Some(commit) = &payload.content.based_on {
        ensure!(
            git::exists(root, commit),
            EXIT_INVARIANT,
            "based_on {commit} is not a commit in this repository"
        );
    }

    let expected_rev = object.as_ref().map(|object| object.rev).unwrap_or(0);
    let taken: Vec<String> = pending(root)?
        .into_iter()
        .map(|candidate| candidate.challenge.clone())
        .collect();
    let previous_semantics_recorded = matches!(payload.action, Action::SectionRevised { .. });
    let gate = crate::confirmation::Candidate::prepare_with(
        payload,
        ObjectBinding { expected_rev },
        &taken,
        now(),
        Payload::sha256,
    )?;
    let candidate = Candidate {
        format: CANDIDATE_FORMAT.to_owned(),
        version: FORMAT_VERSION,
        gate,
        previous_text,
        previous_based_on,
        previous_refs,
        previous_semantics_recorded,
    };

    // One live candidate per object: a second proposal supersedes the first, so
    // a human is never holding two codes for the same thing.
    let mut superseded = Vec::new();
    for existing in pending(root)? {
        if existing.payload.object == candidate.payload.object {
            fs::remove_file(store::candidate_path(root, &existing.challenge))
                .map_err(|error| tool_error("discarding a superseded candidate", error))?;
            superseded.push(existing.challenge.clone());
        }
    }
    store::write_json(
        &store::candidate_path(root, &candidate.challenge),
        &candidate,
    )?;

    let notes = notes_for(root, &candidate);

    Ok(Prepared {
        candidate,
        superseded,
        notes,
    })
}

fn validate_refs(root: &Path, payload: &Payload) -> Result<()> {
    for reference in &payload.content.refs {
        if let Action::SectionRevised { section } = payload.action {
            ensure!(
                reference.object != payload.object || reference.section != section,
                EXIT_INVARIANT,
                "section §{section} cannot directly reference itself"
            );
        }
        let target = store::load_object(root, &reference.object).map_err(|error| {
            if error.code == EXIT_NOT_FOUND {
                Error::new(
                    EXIT_NOT_FOUND,
                    format!(
                        "reference target object {} does not exist",
                        reference.object
                    ),
                )
            } else {
                error
            }
        })?;
        let section = target.section(reference.section).map_err(|_| {
            Error::new(
                EXIT_NOT_FOUND,
                format!(
                    "reference target {} §{} does not exist",
                    reference.object, reference.section
                ),
            )
        })?;
        ensure!(
            section.sha256 == reference.sha256,
            EXIT_INVARIANT,
            "reference to {} §{} pins {} but that section is now {}",
            reference.object,
            reference.section,
            &reference.sha256[..8.min(reference.sha256.len())],
            &section.sha256[..8.min(section.sha256.len())]
        );
        ensure!(
            section.recomputed_sha256()? == reference.sha256,
            EXIT_INVARIANT,
            "reference target {} §{} current wording does not match its recorded hash",
            reference.object,
            reference.section
        );
        let committed = git::object_at(root, &reference.commit, &reference.object)?
            .and_then(|object| object.section(reference.section).ok().cloned())
            .ok_or_else(|| Error::new(
                EXIT_INVARIANT,
                format!(
                    "reference target {} §{} is not present at commit {}; commit the target wording first",
                    reference.object, reference.section, reference.commit
                ),
            ))?;
        ensure!(
            committed.recomputed_sha256()? == reference.sha256,
            EXIT_INVARIANT,
            "reference target {} §{} at commit {} does not contain the pinned wording; commit the target wording first",
            reference.object,
            reference.section,
            reference.commit
        );
    }
    Ok(())
}

pub fn discard(root: &Path, challenge: &str) -> Result<()> {
    store::require_current(root)?;
    let path = store::candidate_path(root, challenge);
    ensure!(
        path.exists(),
        EXIT_NOT_FOUND,
        "no candidate awaiting {challenge}"
    );
    fs::remove_file(&path).map_err(|error| tool_error(path.display(), error))?;
    Ok(())
}

/// Admit a candidate against the exact response.
///
/// A response that begins with `CONFIRM ` but is not exactly the phrase is not a
/// near miss to be helpful about — it is hedged assent, and it discards the
/// candidate. Accepting a bare code instead would put the agent in the position
/// of deciding whether "yes, but reword the second sentence" counted as a yes.
pub fn confirm(root: &Path, response: &str) -> Result<(Event, Object)> {
    store::require_current(root)?;
    let code = crate::confirmation::authorize(
        response,
        |code| store::candidate_path(root, code).exists(),
        |code| discard(root, code),
    )?;

    let candidate = find(root, code)?;
    ensure!(
        !matches!(candidate.payload.action, Action::SectionRevised { .. })
            || candidate.previous_semantics_recorded,
        EXIT_SCHEMA,
        "candidate {code} predates complete semantic revision rendering; prepare it again"
    );
    candidate.verify_payload_with(Payload::sha256)?;

    let mut object = match ops::reconcile(root, &candidate.payload.object) {
        Ok(object) => object,
        Err(error) if error.code == EXIT_NOT_FOUND => {
            Object::new(candidate.payload.object.clone(), String::new())
        }
        Err(error) => return Err(error),
    };
    // Idempotent recovery. `confirm` appends the event, saves the projection,
    // then clears the candidate; a crash in the middle leaves the change applied
    // and the candidate still pending. Re-confirming the same code must report
    // what already happened rather than apply it a second time.
    let already_applied = if object.rev > candidate.binding.expected_rev {
        let events = store::load_events(root, &candidate.payload.object)?;
        events.into_iter().find(|event| {
            event.rev == candidate.binding.expected_rev + 1
                && event.confirmation.payload_sha256 == candidate.payload_sha256
        })
    } else {
        None
    };

    match crate::confirmation::admission(
        &candidate.binding.expected_rev,
        &object.rev,
        already_applied.is_some(),
        "the object revision",
    )? {
        crate::confirmation::Admission::AlreadyApplied => {
            let applied = already_applied.expect("the shared gate classified this as applied");
            discard(root, code)?;
            return Ok((applied, object));
        }
        crate::confirmation::Admission::Apply => {}
    }

    // Re-check references at the moment of admission, not only at prepare: a
    // target may have been revised while the human was reading.
    validate_refs(root, &candidate.payload)?;

    let event = Event {
        format: EVENT_FORMAT.to_owned(),
        version: FORMAT_VERSION,
        event_id: crate::model::new_id(),
        rev: object.rev + 1,
        time: now(),
        payload: candidate.payload.clone(),
        confirmation: Confirmation {
            challenge: candidate.challenge.clone(),
            payload_sha256: candidate.payload_sha256.clone(),
        },
    };

    project(&mut object, &event)?;
    store::append_event(root, &event)?;
    store::save_object(root, &object)?;
    discard(root, code)?;
    Ok((event, object))
}

/// Build the content half of a payload, defaulting `based_on` to HEAD so a
/// section records the code state it was written against without being asked.
pub fn content(
    root: &Path,
    text: Option<String>,
    based_on: Option<String>,
    no_based_on: bool,
    refs: Vec<crate::model::Ref>,
) -> Result<Content> {
    let based_on = match based_on {
        Some(revision) => Some(git::resolve(root, &revision).ok_or_else(|| {
            Error::new(
                EXIT_INVARIANT,
                format!("based_on {revision} is not a commit in this repository"),
            )
        })?),
        None if no_based_on => None,
        None if text.is_some() => match git::source_dirty(root) {
            Some(false) => Some(git::head(root).ok_or_else(|| {
                Error::new(
                    EXIT_INVARIANT,
                    "there is no repository HEAD to use as a basis; explicitly choose no repository basis with --no-based-on",
                )
            })?),
            Some(true) => {
                return Err(Error::new(
                    EXIT_INVARIANT,
                    "source files have uncommitted changes; choose a committed basis with --based-on or explicitly choose no repository basis with --no-based-on",
                ));
            }
            None => {
                return Err(Error::new(
                    EXIT_INVARIANT,
                    "could not determine whether source files are clean; choose a committed basis with --based-on or explicitly choose no repository basis with --no-based-on",
                ));
            }
        },
        None => None,
    };
    Ok(Content {
        text: text.unwrap_or_default(),
        based_on,
        refs,
    })
}
