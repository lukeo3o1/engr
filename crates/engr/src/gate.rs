//! The Human Alignment Gate — the only way anything enters the record.
//!
//! `prepare` puts a candidate up and mints a challenge; `confirm` admits it only
//! against the exact response. There is no unconfirmed write path.

use crate::backlog;
use crate::model::{
    canonical_object_id, project, Action, Confirmation, Content, Event, Object, Payload,
    CANDIDATE_FORMAT, EVENT_FORMAT,
};
use crate::semantics::{self, Relation, RelationType, Role, Supplement, Target};
use crate::{
    ensure, git, ops, store, tool_error, Error, Result, CANDIDATE_ENVELOPE_VERSION,
    CANDIDATE_ENVELOPE_VERSION_V0, EVENT_ENVELOPE_VERSION_V0, EXIT_INVARIANT, EXIT_NOT_FOUND,
    EXIT_SCHEMA, EXIT_USAGE,
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
    #[serde(flatten)]
    pub context: PreparedContext,
}

/// Everything prepared beside the mutation itself: what the human will be
/// shown, and what a successful confirmation will reconcile in Backlog.
///
/// None of it belongs in `payload_sha256` — that value travels into the
/// confirmed Event and identifies the mutation, and Backlog must not become
/// part of the authoritative record. All of it belongs in `integrity_sha256`,
/// because all of it changes what happens at confirm.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct PreparedContext {
    /// What the human is shown when the candidate carries a change to existing
    /// wording, so they read the change rather than the whole section again.
    #[serde(default)]
    pub previous_text: Option<String>,
    /// Previous semantic content for revisions. These remain separate from
    /// `previous_text`; the action tells the renderer whether absence means
    /// "no basis" or "not a section revision".
    #[serde(default)]
    pub previous_based_on: Option<String>,
    #[serde(default)]
    pub previous_refs: Vec<crate::model::Ref>,
    /// The rest of the previous semantic content, all skipped when empty so a
    /// candidate prepared before these fields existed still hashes to the same
    /// integrity value and remains admissible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_role: Option<Role>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub previous_content: Vec<Supplement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub previous_relations: Vec<Relation>,
    /// Distinguishes a new revision with no old basis/refs from a legacy
    /// candidate that never captured those values at all.
    #[serde(default)]
    pub previous_semantics_recorded: bool,
    /// The size exception this proposal was admitted under.
    ///
    /// Here rather than in the payload, because it is a fact about admission
    /// rather than about the mutation: it decides what the human is shown and
    /// what confirm will re-check, and it must never become a lasting property
    /// of the Section. `integrity_sha256` covers it, so a candidate cannot be
    /// edited on disk into an exception nobody granted.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub oversize: bool,
    /// The unresolved points this candidate says it came from, each pinned to
    /// the resolution basis it was prepared against.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backlog: Vec<backlog::Source>,
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

/// What a confirmation produced: the Event that entered the record, the object
/// it produced, and what that did to the Backlog sources the candidate named.
#[derive(Debug)]
pub struct Admitted {
    pub event: Event,
    pub object: Object,
    pub backlog: Vec<backlog::Outcome>,
}

/// A Backlog source a caller declares a candidate was derived from. The
/// resolution basis is pinned by `prepare`, not supplied, so the pin is always
/// what the source actually said at that moment.
#[derive(Clone, Debug)]
pub struct SourceRequest {
    /// Any unique Backlog id prefix, or an `engr:backlog:<id>` reference.
    pub item: String,
    pub section: u64,
    pub produced: Vec<backlog::Produced>,
    pub resolves: bool,
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("formatting a timestamp cannot fail")
}

/// Every challenge code a candidate file currently claims.
///
/// Read from the filenames, without loading anything. Minting a fresh code has
/// to avoid every code on disk, including one this build refuses to admit —
/// otherwise a candidate left by an older build would either be silently
/// overwritten, or would make preparing anything at all fail with a message
/// about some unrelated code.
pub fn pending_codes(root: &Path) -> Result<Vec<String>> {
    let dir = store::candidates_dir(root);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut codes = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|error| tool_error(dir.display(), error))? {
        let entry = entry.map_err(|error| tool_error(dir.display(), error))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(code) = name.strip_suffix(".json") {
            if crate::confirmation::valid_challenge(code) {
                codes.push(code.to_owned());
            }
        }
    }
    codes.sort();
    Ok(codes)
}

/// Every pending candidate, fully loaded and checked.
///
/// The strict half of the pair: one file this build refuses fails the whole
/// call. That is right for a caller that wants candidates, and wrong for a
/// listing — which is why the listing walks [`pending_codes`] and loads each
/// one on its own, so a file it cannot read becomes a line rather than the
/// absence of every other line.
pub fn pending(root: &Path) -> Result<Vec<Candidate>> {
    let mut found = Vec::new();
    for code in pending_codes(root)? {
        found.push(find(root, &code)?);
    }
    found.sort_by(|left, right| left.created_at.cmp(&right.created_at));
    Ok(found)
}

/// Which object a candidate file is about, without holding it to this build's
/// rules. Superseding has to work against a candidate this build would refuse,
/// because leaving that one live is what would give one object two codes.
fn candidate_object(root: &Path, code: &str) -> Option<String> {
    let path = store::candidate_path(root, code).ok()?;
    let value: serde_json::Value = store::read_json(&path).ok()?;
    value.get("object")?.as_str().map(str::to_owned)
}

pub fn find(root: &Path, challenge: &str) -> Result<Candidate> {
    let path = store::candidate_path(root, challenge)?;
    ensure!(
        path.exists(),
        EXIT_NOT_FOUND,
        "no candidate awaiting {challenge}"
    );
    let candidate: Candidate = store::read_json(&path)?;
    // The code a candidate names is the code it will be admitted by, so a file
    // that names a different one is not a candidate to render — it is a
    // redirect. Rendering it would show one mutation and ask for the answer to
    // another, which is the exact thing the gate exists to prevent, and both
    // files would pass every check of their own.
    ensure!(
        candidate.challenge == challenge,
        EXIT_SCHEMA,
        "candidate {challenge} names challenge {}; it would show one change and admit another",
        candidate.challenge
    );
    validate_candidate(&candidate)?;
    Ok(candidate)
}

fn validate_candidate(candidate: &Candidate) -> Result<()> {
    ensure!(
        candidate.format == CANDIDATE_FORMAT,
        EXIT_SCHEMA,
        "candidate {} has an unsupported format",
        candidate.challenge
    );
    ensure!(
        candidate.version != CANDIDATE_ENVELOPE_VERSION_V0,
        EXIT_SCHEMA,
        "candidate {} predates candidate integrity, so what it would present cannot be checked against what was prepared; prepare it again",
        candidate.challenge
    );
    ensure!(
        candidate.version == CANDIDATE_ENVELOPE_VERSION,
        EXIT_SCHEMA,
        "candidate {} has an unsupported envelope version {}",
        candidate.challenge,
        candidate.version
    );
    ensure!(
        crate::confirmation::valid_challenge(&candidate.challenge),
        EXIT_SCHEMA,
        "candidate {} has an invalid challenge code",
        candidate.challenge
    );
    // Both fingerprints wherever a candidate is loaded, not only at confirm.
    // Re-rendering a candidate is a use of its prepared context: `engr candidate
    // <code>` is the screen a human reads hours later, and it has to be the
    // screen `prepare` produced.
    candidate.verify_payload_with(Payload::sha256)?;
    candidate.verify_integrity(&candidate.context)?;
    candidate.payload.validate()?;
    for source in &candidate.context.backlog {
        source.validate().map_err(|error| {
            Error::new(
                EXIT_SCHEMA,
                format!("candidate {}: {}", candidate.challenge, error.message),
            )
        })?;
    }
    validate_title_context(&candidate.payload).map_err(|error| {
        Error::new(
            EXIT_SCHEMA,
            format!("candidate {}: {}", candidate.challenge, error.message),
        )
    })
}

/// The actionable state of an outstanding candidate.
///
/// A candidate can survive the narrow crash window after its Event/projection
/// are durable but before its file is deleted. That is retryable, not stale:
/// the same exact confirmation must finish cleanup without applying again.
#[derive(Debug)]
pub enum CandidateState {
    Pending,
    AlreadyApplied(Box<Event>),
    Stale { current_rev: u64 },
}

/// Classify a candidate from the same effective projection and durable Event
/// evidence used by confirmation. Read surfaces and admission must agree about
/// whether a code is still actionable.
pub fn candidate_state(root: &Path, candidate: &Candidate) -> Result<CandidateState> {
    match ops::effective(root, &candidate.payload.object) {
        Ok(object) => {
            if object.rev > candidate.binding.expected_rev {
                let applied_rev =
                    candidate
                        .binding
                        .expected_rev
                        .checked_add(1)
                        .ok_or_else(|| {
                            Error::new(EXIT_INVARIANT, "candidate revision cannot advance")
                        })?;
                if let Some(event) = store::load_events(root, &candidate.payload.object)?
                    .into_iter()
                    .find(|event| {
                        event.rev == applied_rev
                            && event.confirmation.payload_sha256 == candidate.payload_sha256
                    })
                {
                    return Ok(CandidateState::AlreadyApplied(Box::new(event)));
                }
            }
            if object.rev == candidate.binding.expected_rev {
                Ok(CandidateState::Pending)
            } else {
                Ok(CandidateState::Stale {
                    current_rev: object.rev,
                })
            }
        }
        Err(error) if error.code == EXIT_NOT_FOUND => {
            if candidate.binding.expected_rev == 0
                && store::load_events(root, &candidate.payload.object)?.is_empty()
            {
                Ok(CandidateState::Pending)
            } else {
                Ok(CandidateState::Stale { current_rev: 0 })
            }
        }
        Err(error) => Err(error),
    }
}

/// Whether a pending candidate can still be acted on. An already-applied
/// candidate remains live only for its idempotent cleanup retry.
pub fn is_live(root: &Path, candidate: &Candidate) -> bool {
    matches!(
        candidate_state(root, candidate),
        Ok(CandidateState::Pending | CandidateState::AlreadyApplied(_))
    )
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

/// A direct domain caller has no CLI adapter to normalize its values. Resolve
/// every semantic identifier before the payload is hashed, so the candidate the
/// human reads is byte-for-byte the one that becomes the Event.
fn canonicalize_payload(root: &Path, payload: &mut Payload) -> Result<()> {
    payload.object = canonical_object_id(&payload.object)?;
    if let Some(based_on) = payload.content.based_on.clone() {
        payload.content.based_on = Some(resolve_commit(root, "based_on", &based_on)?);
    }
    for reference in &mut payload.content.refs {
        reference.object = canonical_object_id(&reference.object)?;
        reference.commit = resolve_commit(root, "reference commit", &reference.commit)?;
    }
    for relation in &mut payload.content.relations {
        if let Target::File { commit, .. } | Target::Symbol { commit, .. } = &mut relation.target {
            *commit = resolve_commit(root, "relation commit", commit)?;
        }
    }
    // After resolution, not before: two spellings of the same commit are the
    // same relation, and sorting has to see the resolved values or the order
    // would depend on what the caller happened to type.
    payload.content.canonicalize_order();
    payload.validate()
}

/// Titles name the Object; unlike section wording they are not assertions made
/// against repository context or another section. Keeping that rule at
/// admission prevents the renderer from hiding semantic fields a human never
/// had a chance to read.
fn validate_title_context(payload: &Payload) -> Result<()> {
    if payload.action.carries_title() {
        ensure!(
            payload.content.based_on.is_none()
                && payload.content.refs.is_empty()
                && payload.content.role.is_none()
                && payload.content.content.is_empty()
                && payload.content.relations.is_empty(),
            EXIT_INVARIANT,
            "{} titles carry no repository basis, references, role, supplementary content or relations",
            payload.action.label()
        );
    }
    Ok(())
}

/// Confirmed payloads must already be canonical; retries must never use this to
/// silently change what an earlier human confirmed.
fn validate_persisted_payload(root: &Path, payload: &Payload) -> Result<()> {
    payload.validate()?;
    validate_title_context(payload)?;
    if let Some(based_on) = &payload.content.based_on {
        ensure!(
            git::resolve(root, based_on).as_deref() == Some(based_on.as_str()),
            EXIT_INVARIANT,
            "based_on {based_on} is not a commit in this repository"
        );
    }
    for reference in &payload.content.refs {
        ensure!(
            git::resolve(root, &reference.commit).as_deref() == Some(reference.commit.as_str()),
            EXIT_INVARIANT,
            "reference commit {} is not a commit in this repository",
            reference.commit
        );
    }
    Ok(())
}

fn resolve_commit(root: &Path, field: &str, revision: &str) -> Result<String> {
    git::resolve(root, revision).ok_or_else(|| {
        Error::new(
            EXIT_INVARIANT,
            format!("{field} {revision} is not a commit in this repository"),
        )
    })
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
        .filter_map(|id| ops::effective(root, id).ok())
        .find(|object| object.title.trim().to_lowercase() == needle)
        .map(|object| object.id)
}

/// Validate a proposed action against the current object and put it up for
/// confirmation. References are checked here, at the gate — not in `verify`.
/// Deferring that check is what let one mistyped id in the previous design
/// poison a global health check permanently.
pub fn prepare(root: &Path, payload: Payload) -> Result<Prepared> {
    prepare_sized(root, payload, Vec::new(), false)
}

/// Prepare a proposal that broke a normal size threshold, after a first attempt
/// was already refused.
///
/// The two entry points are the mechanism. A normal limit is writing friction
/// for an agent, not a ceiling on what a human may admit — but the friction only
/// works if the agent has to stop, read what the refusal suggested, and decide
/// again. One call that took a flag would let the flag be set by default;
/// needing a different call after a refusal is what makes the retry explicit.
/// The hard ceiling refuses here too.
pub fn prepare_oversize(root: &Path, payload: Payload) -> Result<Prepared> {
    prepare_sized(root, payload, Vec::new(), true)
}

/// Prepare a record mutation that the caller declares came from unresolved
/// staging.
///
/// The sources are the candidate's own statement about where the work came
/// from and what confirming it settles. engr never derives that from the fact
/// that an Object changed: an inferred link would eventually consume an
/// unresolved point nobody meant to resolve.
pub fn prepare_from_backlog(
    root: &Path,
    payload: Payload,
    sources: Vec<SourceRequest>,
) -> Result<Prepared> {
    prepare_sized(root, payload, sources, false)
}

fn prepare_sized(
    root: &Path,
    payload: Payload,
    sources: Vec<SourceRequest>,
    oversize: bool,
) -> Result<Prepared> {
    // Refuse legacy workspaces before opening the writer lock: even creating a
    // lock file would violate their explicit read-only migration boundary.
    store::require_current(root)?;
    store::with_lock(root, move || {
        prepare_locked(root, payload, sources, oversize)
    })
}

/// Resolve each declared source and pin what it currently says.
///
/// Refused up front, like every other precondition at this gate: a candidate
/// naming an unresolved point that does not exist cannot reconcile later, and
/// the moment to say so is before a human is holding a code.
/// Whether a declared outcome names authority that will exist once this
/// candidate is admitted.
///
/// Checked against the projected object rather than the stored one, because the
/// usual outcome of working on an unresolved point is the very Object or Section
/// this candidate creates — refusing that would make the field useless for the
/// case it was designed for. The projection is exact: the candidate pins
/// `expected_rev`, so the state confirmation applies to is the state this saw.
fn produced_outcome_exists(
    root: &Path,
    projected: &Object,
    outcome: &backlog::Produced,
) -> Result<()> {
    let (target, section) = outcome.target()?;
    let authority = if target == projected.id {
        projected.clone()
    } else {
        ops::effective(root, &target).map_err(|error| {
            if error.code == EXIT_NOT_FOUND {
                Error::new(
                    EXIT_NOT_FOUND,
                    format!("produced outcome names object {target}, which does not exist"),
                )
            } else {
                error
            }
        })?
    };
    if let Some(section) = section {
        authority.section(section).map_err(|_| {
            Error::new(
                EXIT_NOT_FOUND,
                format!("produced outcome names {target} §{section}, which does not exist"),
            )
        })?;
    }
    Ok(())
}

fn pin_sources(
    root: &Path,
    requests: Vec<SourceRequest>,
    projected: &Object,
) -> Result<Vec<backlog::Source>> {
    let mut sources: Vec<backlog::Source> = Vec::new();
    for request in requests {
        let item = backlog::resolve_id(root, &request.item)?;
        let loaded = backlog::load(root, &item)?;
        let section = loaded.section(request.section)?;
        ensure!(
            !sources
                .iter()
                .any(|other| other.item == item && other.section == request.section),
            EXIT_USAGE,
            "backlog source {} §{} is declared twice",
            item,
            request.section
        );
        let mut produced: Vec<backlog::Produced> = Vec::new();
        for outcome in request.produced {
            outcome.validate()?;
            produced_outcome_exists(root, projected, &outcome)?;
            ensure!(
                !produced.contains(&outcome),
                EXIT_USAGE,
                "backlog source {} §{} declares the same outcome twice",
                item,
                request.section
            );
            produced.push(outcome);
        }
        let source = backlog::Source {
            item,
            section: request.section,
            basis_sha256: section.resolution_basis()?,
            produced,
            resolves: request.resolves,
        };
        source.validate()?;
        sources.push(source);
    }
    Ok(sources)
}

fn prepare_locked(
    root: &Path,
    mut payload: Payload,
    sources: Vec<SourceRequest>,
    oversize: bool,
) -> Result<Prepared> {
    store::require_current(root)?;
    validate_title_context(&payload)?;
    canonicalize_payload(root, &mut payload)?;
    // A title is the line a listing prints, so whitespace around it is never
    // meaningful and always visible — it pushes one row out of column. The
    // duplicate check already ignores it; storing what that check ignores is how
    // a listing ends up misaligned underneath a note saying the titles match.
    // Normalised here, before the candidate is minted, so the human confirms the
    // exact string that will be stored.
    if payload.action.carries_title() {
        std::mem::take(&mut payload.content.text)
            .trim()
            .clone_into(&mut payload.content.text);
    }
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

    // Every threshold this proposal breaks, refused here unless the caller came
    // back through the oversize door. Not in `Payload::validate`, for the same
    // reason the title limit is not: that runs when events are loaded, and a
    // workspace holding a Section admitted under an exception has to keep being
    // able to replay its own history.
    if payload.action.carries_content() && !payload.action.carries_title() {
        semantics::check_size(&payload.content.text, &payload.content.content, oversize)?;
    }

    let mut previous = PreviousContent::default();
    let previous_text = match (&payload.action, &object) {
        (Action::ObjectRenamed, Some(object)) => Some(object.title.clone()),
        (Action::SectionRevised { section }, Some(object)) => {
            let section = object.section(*section)?;
            previous = PreviousContent {
                based_on: section.based_on.clone(),
                refs: section.refs.clone(),
                role: section.role,
                content: section.content.clone(),
                relations: section.relations.clone(),
            };
            // A revision that changes nothing is not a change to confirm. It
            // matters here specifically because `refs` and `relations` are sets:
            // the same proposal written in another order canonicalizes to the
            // same content, and admitting it would spend a confirmation and a
            // revision on a reordering the model says is not a difference.
            ensure!(
                section.content() != payload.content,
                EXIT_INVARIANT,
                "§{} already says exactly this, so there is nothing to confirm",
                section.id
            );
            Some(section.text.clone())
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
    // reaches a human. The result is kept rather than thrown away: it is the
    // authority this candidate will produce, and declared Backlog outcomes are
    // checked against that rather than against a state the confirmation is
    // about to replace.
    let projected = {
        let mut trial = match &object {
            Some(object) => object.clone(),
            None => Object::new(payload.object.clone(), String::new())?,
        };
        let probe = Event {
            format: EVENT_FORMAT.to_owned(),
            version: EVENT_ENVELOPE_VERSION_V0,
            event_id: String::new(),
            rev: trial.rev + 1,
            time: now(),
            payload: payload.clone(),
            confirmation: Confirmation {
                challenge: String::new(),
                payload_sha256: String::new(),
            },
        };
        project(&mut trial, &probe)?;
        trial
    };

    validate_refs(root, &payload)?;
    validate_relations(root, &payload, &projected)?;

    let expected_rev = object.as_ref().map(|object| object.rev).unwrap_or(0);
    let taken = pending_codes(root)?;
    let context = PreparedContext {
        previous_text,
        previous_based_on: previous.based_on,
        previous_refs: previous.refs,
        previous_role: previous.role,
        previous_content: previous.content,
        previous_relations: previous.relations,
        previous_semantics_recorded: matches!(payload.action, Action::SectionRevised { .. }),
        oversize,
        backlog: pin_sources(root, sources, &projected)?,
    };
    let gate = crate::confirmation::Candidate::prepare_with(
        payload,
        ObjectBinding { expected_rev },
        &context,
        &taken,
        now(),
        Payload::sha256,
    )?;
    let candidate = Candidate {
        format: CANDIDATE_FORMAT.to_owned(),
        version: CANDIDATE_ENVELOPE_VERSION,
        gate,
        context,
    };

    // One live candidate per object: a second proposal supersedes the first, so
    // a human is never holding two codes for the same thing. Read leniently, so
    // a candidate this build would refuse still gets superseded rather than
    // being left beside its replacement.
    let mut superseded = Vec::new();
    for code in pending_codes(root)? {
        if code != candidate.challenge
            && candidate_object(root, &code).as_deref() == Some(candidate.payload.object.as_str())
        {
            fs::remove_file(store::candidate_path(root, &code)?)
                .map_err(|error| tool_error("discarding a superseded candidate", error))?;
            superseded.push(code);
        }
    }
    store::write_json(
        &store::candidate_path(root, &candidate.challenge)?,
        &candidate,
    )?;

    let notes = notes_for(root, &candidate);

    Ok(Prepared {
        candidate,
        superseded,
        notes,
    })
}

/// The semantic content a revision is replacing, gathered in one place so the
/// candidate can show the change rather than the whole section again.
#[derive(Default)]
struct PreviousContent {
    based_on: Option<String>,
    refs: Vec<crate::model::Ref>,
    role: Option<Role>,
    content: Vec<Supplement>,
    relations: Vec<Relation>,
}

/// Everything a relation claims about the world outside this payload.
///
/// Checked at the gate rather than in the reducer, which stays a pure function
/// of the event: these questions need the workspace and the repository, and the
/// answers change over time. Checked again at admission, because a replacement
/// object can be superseded, and a graph that was acyclic while the human read
/// the candidate may not be by the time they type.
///
/// `projected` is the object this candidate will produce, so a supersession can
/// see the section it is itself adding when the cycle is walked.
fn validate_relations(root: &Path, payload: &Payload, projected: &Object) -> Result<()> {
    for relation in &payload.content.relations {
        match (relation.relation, &relation.target) {
            (RelationType::SupersededBy, _) => {
                let target = relation
                    .replacement()?
                    .expect("a superseded_by relation names a replacement");
                ensure!(
                    target != payload.object,
                    EXIT_INVARIANT,
                    "an object cannot supersede itself"
                );
                ops::effective(root, &target).map_err(|error| {
                    if error.code == EXIT_NOT_FOUND {
                        Error::new(
                            EXIT_NOT_FOUND,
                            format!("superseded_by names object {target}, which does not exist"),
                        )
                    } else {
                        error
                    }
                })?;
                check_acyclic(root, projected)?;
            }
            (RelationType::ImplementedBy, Target::File { path, commit })
            | (RelationType::ImplementedBy, Target::Symbol { path, commit, .. }) => {
                ensure!(
                    git::resolve(root, commit).as_deref() == Some(commit.as_str()),
                    EXIT_INVARIANT,
                    "implemented_by pins commit {commit}, which is not a commit in this repository"
                );
                // The path, not the symbol. A symbol target says which file the
                // implementation is in and what it is called there; v0 does not
                // parse the language, and pretending to would be a check that
                // fails on anything engr cannot compile.
                ensure!(
                    git::path_at(root, commit, path),
                    EXIT_INVARIANT,
                    "implemented_by names {path}, which does not exist at commit {}",
                    &commit[..8.min(commit.len())]
                );
            }
            (RelationType::ImplementedBy, Target::Engr { .. }) => {
                return Err(Error::new(
                    EXIT_INVARIANT,
                    "implemented_by names a repository artifact, not an engr resource".to_owned(),
                ))
            }
        }
    }
    Ok(())
}

/// Walk the replacement chain forward from `object` and refuse to close it.
///
/// A cycle in `superseded_by` is a set of objects each of which claims the next
/// one replaced it, so a reader following the chain to find current knowledge
/// never arrives anywhere. The walk is bounded by the objects in the workspace,
/// and an object that cannot be loaded ends that branch rather than the check —
/// a chain into a missing object is a dangling pointer, not a cycle.
fn check_acyclic(root: &Path, object: &Object) -> Result<()> {
    let mut seen = vec![object.id.clone()];
    let mut frontier = object.replacements()?;
    while let Some(next) = frontier.pop() {
        ensure!(
            next != object.id,
            EXIT_INVARIANT,
            "that replacement closes a supersession cycle back onto {}",
            object.id
        );
        if seen.contains(&next) {
            continue;
        }
        seen.push(next.clone());
        if let Ok(target) = ops::effective(root, &next) {
            frontier.extend(target.replacements()?);
        }
    }
    Ok(())
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
        let section = ops::effective_section(root, &reference.object, reference.section).map_err(
            |error| {
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
            },
        )?;
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
    store::with_lock(root, || discard_locked(root, challenge))
}

fn discard_locked(root: &Path, challenge: &str) -> Result<()> {
    store::require_current(root)?;
    let path = store::candidate_path(root, challenge)?;
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
pub fn confirm(root: &Path, response: &str) -> Result<Admitted> {
    store::require_current(root)?;
    store::with_lock(root, || confirm_locked(root, response))
}

/// Reconcile the candidate's Backlog sources, inside the same lock that made
/// the Object mutation durable and before the candidate is disposed of.
///
/// Both halves of that matter. The lock is what stops a Backlog edit landing
/// between the basis check and the write, so compare-and-consume cannot delete
/// wording it never compared against. Running before disposal is what makes the
/// crash retry finish the job: the already-applied path comes back through here
/// with the same declarations, and appending an outcome already listed does
/// nothing.
fn reconcile_backlog(root: &Path, candidate: &Candidate) -> Result<Vec<backlog::Outcome>> {
    if candidate.context.backlog.is_empty() {
        return Ok(Vec::new());
    }
    backlog::reconcile(root, &candidate.context.backlog).map_err(|error| {
        Error::new(
            error.code,
            format!(
                "the object change was confirmed and saved; reconciling backlog failed: {}. \
                 Repeat the same confirmation to finish it",
                error.message
            ),
        )
    })
}

fn confirm_locked(root: &Path, response: &str) -> Result<Admitted> {
    store::require_current(root)?;
    let code = crate::confirmation::authorize(
        response,
        |code| {
            store::candidate_path(root, code)
                .map(|path| path.exists())
                .unwrap_or(false)
        },
        |code| discard_locked(root, code),
    )?;

    let candidate = find(root, code)?;
    ensure!(
        !matches!(candidate.payload.action, Action::SectionRevised { .. })
            || candidate.context.previous_semantics_recorded,
        EXIT_SCHEMA,
        "candidate {code} predates complete semantic revision rendering; prepare it again"
    );
    validate_persisted_payload(root, &candidate.payload)?;

    let mut object = match candidate_state(root, &candidate)? {
        CandidateState::AlreadyApplied(applied) => {
            let object = ops::reconcile(root, &candidate.payload.object)?;
            // The retry's whole job is to finish what a crash interrupted, and
            // Backlog reconciliation is part of that job.
            let backlog = reconcile_backlog(root, &candidate)?;
            discard_locked(root, code)?;
            return Ok(Admitted {
                event: *applied,
                object,
                backlog,
            });
        }
        CandidateState::Stale { current_rev } => {
            crate::confirmation::admission(
                &candidate.binding.expected_rev,
                &current_rev,
                false,
                "the object revision",
            )?;
            unreachable!("a stale candidate cannot be admitted")
        }
        CandidateState::Pending => match ops::reconcile(root, &candidate.payload.object) {
            Ok(object) => object,
            Err(error) if error.code == EXIT_NOT_FOUND => {
                Object::new(candidate.payload.object.clone(), String::new())?
            }
            Err(error) => return Err(error),
        },
    };
    crate::confirmation::admission(
        &candidate.binding.expected_rev,
        &object.rev,
        false,
        "the object revision",
    )?;

    // Re-check references at the moment of admission, not only at prepare: a
    // target may have been revised while the human was reading.
    validate_refs(root, &candidate.payload)?;

    let event = Event {
        format: EVENT_FORMAT.to_owned(),
        version: EVENT_ENVELOPE_VERSION_V0,
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
    // After the projection, because the acyclic walk has to see the section this
    // event is adding: the relation being admitted is part of the graph it must
    // not close. The size re-check uses the exception the candidate carries, so
    // a candidate edited on disk to drop it cannot get in — though candidate
    // integrity has already refused that file by this point.
    validate_relations(root, &candidate.payload, &object)?;
    if candidate.payload.action.carries_content() && !candidate.payload.action.carries_title() {
        semantics::check_size(
            &candidate.payload.content.text,
            &candidate.payload.content.content,
            candidate.context.oversize,
        )?;
    }
    store::append_event(root, &event)?;
    store::save_object(root, &object)?;
    // Backlog last, and never able to undo any of the above. What a human
    // confirmed is in the record; a source that moved since prepare is a
    // reconciliation outcome to report, not a failed admission.
    let backlog = reconcile_backlog(root, &candidate)?;
    discard_locked(root, code)?;
    Ok(Admitted {
        event,
        object,
        backlog,
    })
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
        ..Content::default()
    })
}
