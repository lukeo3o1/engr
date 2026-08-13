//! The Human Alignment Gate — the only way anything enters the record.
//!
//! `prepare` puts a candidate up and mints a challenge; `confirm` admits it only
//! against the exact response. There is no unconfirmed write path.

use crate::model::{
    project, Action, Confirmation, Content, Event, Object, Payload, CANDIDATE_FORMAT, EVENT_FORMAT,
};
use crate::{
    ensure, git, ops, store, tool_error, Error, Result, EXIT_INVARIANT, EXIT_NOT_FOUND,
    EXIT_SCHEMA, EXIT_STALE, EXIT_USAGE, FORMAT_VERSION,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Excludes the characters people misread aloud or mistype: 0/O, 1/I.
const ALPHABET: &[u8] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";
const CHALLENGE_LEN: usize = 6;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Candidate {
    pub format: String,
    pub version: u32,
    pub challenge: String,
    pub created_at: String,
    /// The object revision this was prepared against.
    pub expected_rev: u64,
    #[serde(flatten)]
    pub payload: Payload,
    pub payload_sha256: String,
    /// What the human is shown when the candidate carries a change to existing
    /// wording, so they read the change rather than the whole section again.
    pub previous_text: Option<String>,
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("formatting a timestamp cannot fail")
}

fn mint_challenge(taken: &[String]) -> String {
    let mut rng = rand::thread_rng();
    loop {
        let code: String = (0..CHALLENGE_LEN)
            .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
            .collect();
        if !taken.contains(&code) {
            return code;
        }
    }
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
        Ok(object) => object.rev == candidate.expected_rev,
        Err(_) => candidate.expected_rev == 0,
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

    let previous_text = match (&payload.action, &object) {
        (Action::ObjectRenamed, Some(object)) => Some(object.title.clone()),
        (Action::SectionRevised { section }, Some(object)) => {
            Some(object.section(*section)?.text.clone())
        }
        (Action::SectionDeleted { section }, Some(object)) => {
            Some(object.section(*section)?.text.clone())
        }
        (Action::SectionMerged { absorbs }, Some(object)) => {
            let mut parts = Vec::new();
            for id in absorbs {
                parts.push(format!("§{id}: {}", object.section(*id)?.text.trim_end()));
            }
            Some(parts.join("\n"))
        }
        _ => None,
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
        .map(|candidate| candidate.challenge)
        .collect();
    let challenge = mint_challenge(&taken);
    let candidate = Candidate {
        format: CANDIDATE_FORMAT.to_owned(),
        version: FORMAT_VERSION,
        challenge: challenge.clone(),
        created_at: now(),
        expected_rev,
        payload_sha256: payload.sha256()?,
        payload,
        previous_text,
    };

    // One live candidate per object: a second proposal supersedes the first, so
    // a human is never holding two codes for the same thing.
    let mut superseded = Vec::new();
    for existing in pending(root)? {
        if existing.payload.object == candidate.payload.object {
            fs::remove_file(store::candidate_path(root, &existing.challenge))
                .map_err(|error| tool_error("discarding a superseded candidate", error))?;
            superseded.push(existing.challenge);
        }
    }
    store::write_json(&store::candidate_path(root, &challenge), &candidate)?;

    let notes = notes_for(root, &candidate);

    Ok(Prepared {
        candidate,
        superseded,
        notes,
    })
}

fn validate_refs(root: &Path, payload: &Payload) -> Result<()> {
    for reference in &payload.content.refs {
        // Stated as a fact about what was typed, then what the flag is for. The
        // previous wording described the invariant being checked ("points at
        // another object") to someone who had just done the opposite, and sent
        // them looking for a mistyped id that was not there.
        ensure!(
            reference.object != payload.object,
            EXIT_INVARIANT,
            "§{} belongs to this object; --ref is only for other objects",
            reference.section
        );
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
    }
    Ok(())
}

pub fn discard(root: &Path, challenge: &str) -> Result<()> {
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
    let mut words = response.split(' ');
    let head = words.next().unwrap_or_default();
    let code = words.next().unwrap_or_default();
    let exact = head == "CONFIRM"
        && words.next().is_none()
        && code.len() == CHALLENGE_LEN
        && code.bytes().all(|byte| ALPHABET.contains(&byte));

    if !exact {
        // Words beyond the code are commentary — "yes, but reword the second
        // line" is not assent, and letting it through would make the agent the
        // judge of whether a qualified yes counted. That discards the candidate.
        // A whitespace or casing slip is a typo, not a qualification, so it is
        // merely rejected and the candidate survives to be confirmed properly.
        if let Some(rest) = response.strip_prefix("CONFIRM ") {
            let mut words = rest.split_whitespace();
            if let Some(first) = words.next() {
                let qualified = words.next().is_some();
                if qualified && store::candidate_path(root, first).exists() {
                    discard(root, first)?;
                    return Err(Error::new(
                        EXIT_USAGE,
                        format!(
                            "`CONFIRM {first}` carried commentary, so it is a qualified yes rather than assent; candidate {first} was discarded"
                        ),
                    ));
                }
            }
        }
        return Err(Error::new(
            EXIT_USAGE,
            "the response must be exactly `CONFIRM <code>`, with nothing else on the line",
        ));
    }

    let candidate = find(root, code)?;
    ensure!(
        candidate.payload_sha256 == candidate.payload.sha256()?,
        EXIT_SCHEMA,
        "candidate {code} does not match its own hash"
    );

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
    if object.rev > candidate.expected_rev {
        let events = store::load_events(root, &candidate.payload.object)?;
        if let Some(applied) = events.iter().find(|event| {
            event.rev == candidate.expected_rev + 1
                && event.confirmation.payload_sha256 == candidate.payload_sha256
        }) {
            let applied = applied.clone();
            discard(root, code)?;
            return Ok((applied, object));
        }
    }

    ensure!(
        object.rev == candidate.expected_rev,
        EXIT_STALE,
        "the object moved to rev {} after this candidate was prepared at rev {}; prepare it again",
        object.rev,
        candidate.expected_rev
    );

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
    refs: Vec<crate::model::Ref>,
) -> Content {
    let based_on = match based_on {
        Some(revision) => git::resolve(root, &revision),
        None if text.is_some() => git::head(root),
        None => None,
    };
    Content {
        text: text.unwrap_or_default(),
        based_on,
        refs,
    }
}
