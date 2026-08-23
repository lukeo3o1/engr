//! The Human Alignment Gate — the only way anything enters the record.
//!
//! `prepare` puts a candidate up and mints a challenge; `confirm` admits it only
//! against the exact response. There is no unconfirmed write path.

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
/// shown when they are asked to confirm it.
///
/// Backlog is deliberately not here any more. Admission and Backlog bookkeeping
/// are two independent operations, so confirming an Object reaches into staging
/// for nothing — which also means a candidate outstanding across that change
/// names an integrity value this build cannot reproduce.
///
/// It **fails closed at use**, and that is the chosen policy rather than a
/// consequence nobody looked at. Migration does not block on such a candidate
/// and does not rewrite or discard it: moving representation is not a licence to
/// decide the fate of material a human was in the middle of. Nor is a decoder
/// kept for the withdrawn shape, which would outlive the design that needed it
/// and move the failure somewhere quieter than the moment somebody acts on it.
///
/// None of it belongs in `payload_sha256` — that value travels into the
/// confirmed Event and identifies the mutation. All of it belongs in
/// `integrity_sha256`, because all of it changes what happens at confirm.
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
    /// The Object's title as it stood at prepare, for the screen to name the
    /// record by something a human recognises.
    ///
    /// A snapshot, not a lookup. Reading the current title at render time would
    /// put a piece of the confirmation identity outside the candidate and
    /// outside `integrity_sha256` — so a title rewritten in the projection
    /// afterwards would change what a pending candidate presents while the
    /// candidate file, its payload hash and its `expected_rev` all still
    /// checked out. #20 requires the opposite: the same candidate re-rendered
    /// later represents the exact prepared confirmation context.
    ///
    /// Absent for `object_created`, which has no prior title — its title is the
    /// wording on the screen already — and absent on candidates prepared before
    /// this field existed, which therefore hash exactly as they did and go on
    /// rendering without a title rather than being invalidated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_title: Option<String>,
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

/// What a confirmation produced: the Event that entered the record and the
/// Object it produced.
///
/// It no longer reports anything about Backlog. Admission and Backlog
/// bookkeeping are two independent operations: confirming an Object says
/// nothing about which unresolved point it came from, and an inferred link
/// would eventually consume a point nobody meant to resolve.
#[derive(Debug)]
pub struct Admitted {
    pub event: Event,
    pub object: Object,
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
    prepare_admitting(root, payload, Allowance::Normal)
}

/// Prepare a proposal that broke a normal size threshold, after a first attempt
/// was already refused.
pub fn prepare_oversize(root: &Path, payload: Payload) -> Result<Prepared> {
    prepare_admitting(root, payload, Allowance::Oversize)
}

/// How much this proposal is allowed to be.
///
/// Not to be confused with [`crate::semantics::Admission`], which says which
/// door a Section came through. This one is about size, and about one proposal
/// rather than about the record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Allowance {
    /// Normal thresholds apply, and breaking one is refused.
    Normal,
    /// The explicit retry of a refusal, admitting this one proposal past a
    /// normal threshold. Never past a hard ceiling.
    Oversize,
}

/// The one entry point, so the size axis composes with everything else.
///
/// Where a proposal came from is no longer part of preparing it. Object
/// admission and Backlog bookkeeping are two independent operations, so a
/// candidate carries no declared source and confirming one settles nothing in
/// staging by itself.
pub fn prepare_admitting(root: &Path, payload: Payload, allowance: Allowance) -> Result<Prepared> {
    // Refuse legacy workspaces before opening the writer lock: even creating a
    // lock file would violate their explicit read-only migration boundary.
    store::require_current(root)?;
    store::with_lock(root, move || prepare_locked(root, payload, allowance))
}

/// Which proposals engr has refused for size, so an explicit retry can prove it
/// is the retry of something that was actually refused.
///
/// A list rather than a slot, because a workspace holds work on many Objects at
/// once. One slot would mean any second proposal considered anywhere revoked the
/// first one's refusal, and the agent that had already done what the rule asks
/// would be sent back to do it again — enforcing the rule for whichever proposal
/// happened to be refused last, rather than for each.
///
/// Admission-time only, like the exception itself: nothing here is the record,
/// nothing here survives being consumed, and it is capped because it is scratch
/// memory rather than history. Past the cap the oldest entry is dropped and that
/// proposal simply earns its refusal again, which is the safe direction.
#[derive(Serialize, Deserialize, Default)]
struct Refusals {
    /// Payload hashes, most recently refused first.
    refused: Vec<String>,
}

const REFUSALS_REMEMBERED: usize = 32;

/// The two-stage size allowance enforced, rather than described.
///
/// #14 says the first `prepare` above a normal threshold MUST refuse and only an
/// explicit retry may ask for the exception. A flag cannot carry that on its
/// own, because a flag can be passed the first time — so the refusal writes down
/// which proposal it refused and the retry is admitted only if it is that same
/// proposal, byte for byte. The payload is already canonical here, so the same
/// wording written against a different basis is a different proposal and earns
/// its own refusal first.
fn check_allowance(root: &Path, payload: &Payload, allowance: Allowance) -> Result<()> {
    let oversize = allowance == Allowance::Oversize;
    let breaches = semantics::exceeded(&payload.content.text, &payload.content.content);
    let hard = breaches.iter().any(|item| item.hard);

    if oversize {
        // Both halves matter. There must be something to except — an exception
        // over nothing is an agent setting the flag by default — and engr must
        // already have said no to this exact proposal.
        ensure!(
            !breaches.is_empty(),
            EXIT_USAGE,
            "nothing in this Section exceeds a normal limit, so there is no exception to make; prepare it without --oversize"
        );
        if !hard {
            ensure!(
                refusals(root).refused.contains(&payload.sha256()?),
                EXIT_INVARIANT,
                "an oversize exception is the retry of a refusal, and engr has not refused this proposal; prepare it without --oversize first and read what that refusal suggests"
            );
        }
    }

    // The refusals themselves stay in one place, so the wording an agent reads
    // and the wording the unit tests pin are the same wording.
    let result = semantics::check_size(&payload.content.text, &payload.content.content, oversize);
    // Remember what was refused, so the retry has something to be a retry of. A
    // hard-ceiling refusal is deliberately not remembered: it has no retry, and
    // leaving a receipt behind would make `--oversize` look like the answer to a
    // refusal that always says no.
    if result.is_err() && !hard {
        remember_refusal(root, &payload.sha256()?)?;
    }
    result.map(|_| ())
}

/// Unreadable or absent is treated as "nothing refused" rather than as an error:
/// this file is one machine's scratch memory, and a corrupted one should cost a
/// second refusal, not make the workspace unusable.
fn refusals(root: &Path) -> Refusals {
    store::read_json::<Refusals>(&store::refusal_path(root)).unwrap_or_default()
}

fn remember_refusal(root: &Path, payload_sha256: &str) -> Result<()> {
    let mut held = refusals(root);
    held.refused.retain(|held| held != payload_sha256);
    held.refused.insert(0, payload_sha256.to_owned());
    held.refused.truncate(REFUSALS_REMEMBERED);
    store::write_json(&store::refusal_path(root), &held)
}

/// One refusal admits one retry, and only the retry of that proposal. Spent
/// once the candidate exists, so a proposal that failed some later check does
/// not silently lose its place — and spending one leaves every other
/// outstanding refusal alone.
fn spend_refusal(root: &Path, payload_sha256: &str) -> Result<()> {
    let mut held = refusals(root);
    let before = held.refused.len();
    held.refused.retain(|held| held != payload_sha256);
    if held.refused.len() == before {
        return Ok(());
    }
    let path = store::refusal_path(root);
    if held.refused.is_empty() {
        return fs::remove_file(&path).map_err(|error| tool_error(path.display(), error));
    }
    store::write_json(&path, &held)
}

/// Resolve each declared source and pin what it currently says.
///
/// Refused up front, like every other precondition at this gate: a candidate
fn prepare_locked(root: &Path, mut payload: Payload, allowance: Allowance) -> Result<Prepared> {
    store::require_current(root)?;
    // Here, not in `Payload::validate`: that runs when events are loaded, and
    // the model has to keep being able to project a merge that names its
    // survivor. What it may not do is *admit* one — that representation belongs
    // to the Event generation the coordinated Phase-3 transition targets, and
    // nothing writes that generation yet. Admitting one here would append a
    // record claiming version 1 while carrying a shape version 1 never defined,
    // and it would do it after a human had confirmed it.
    if let Action::SectionMerged {
        merge: crate::model::Merge::Into { .. },
    } = &payload.action
    {
        return Err(Error::new(
            EXIT_INVARIANT,
            "a merge that names the section surviving it belongs to the coordinated Phase-3 Event generation, which is implemented but not yet written"
                .to_owned(),
        ));
    }
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

    // Every threshold this proposal breaks, refused here unless it is the
    // explicit retry of a refusal. Not in `Payload::validate`, for the same
    // reason the title limit is not: that runs when events are loaded, and a
    // workspace holding a Section admitted under an exception has to keep being
    // able to replay its own history.
    if payload.action.carries_content() && !payload.action.carries_title() {
        check_allowance(root, &payload, allowance)?;
    } else {
        // Nothing here is measured against a Section threshold, so an exception
        // would be one the candidate claims and no refusal ever granted — a
        // screen saying engr already refused this when it never did.
        ensure!(
            allowance == Allowance::Normal,
            EXIT_USAGE,
            "{} carries no Section content, so there is no size exception to make",
            payload.action.label()
        );
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
            //
            // Both sides are canonicalized for the comparison, and only for it.
            // A Section stored before Phase 3 holds whatever order its gate
            // happened to write, so comparing a sorted proposal against an
            // unsorted stored value would find a difference that is not one and
            // spend an Event on sorting an array. The stored Section is left
            // exactly as it is: its hash covers the order it was written in, and
            // rewriting it to tidy the order would be the same non-change from
            // the other direction.
            let mut current = section.content();
            current.canonicalize_order();
            ensure!(
                current != payload.content,
                EXIT_INVARIANT,
                "§{} already says exactly this, so there is nothing to confirm",
                section.id
            );
            Some(section.text.clone())
        }
        (Action::SectionDeleted { section }, Some(object)) => {
            Some(object.section(*section)?.text.clone())
        }
        (Action::SectionMerged { merge }, Some(object)) => {
            // Every participant, survivor first, because the survivor's own
            // wording is being replaced too. Showing only what is consumed
            // would present a merge as if the destination were untouched.
            let mut parts = Vec::new();
            for id in merge.participants() {
                parts.push(format!("§{id}: {}", object.section(id)?.text.trim_end()));
            }
            Some(parts.join("\n"))
        }
        // The same rule the revision above applies to wording, applied to the
        // Object's own classification. Confirming a move to the state it is
        // already in appends a permanent Event that records no change, spends a
        // `rev`, and invalidates every other live candidate for this Object —
        // three lasting consequences for an operation that does nothing. A
        // human asked to confirm it is being asked to assent to nothing.
        (Action::ObjectClassified { object_type, state }, Some(object)) => {
            ensure!(
                (*object_type, *state) != (object.object_type, object.state),
                EXIT_INVARIANT,
                "{} is already {}, so there is nothing to confirm",
                object.id,
                crate::view::classification(object)
            );
            None
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
        oversize: allowance == Allowance::Oversize,
        object_title: object
            .as_ref()
            .map(|object| object.title.clone())
            .filter(|title| !title.is_empty()),
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
    if allowance == Allowance::Oversize {
        spend_refusal(root, &candidate.payload_sha256)?;
    }

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
        // A branch that stops because the authority would not load is a branch
        // nobody walked, and an unwalked branch can hide the cycle this
        // function exists to find. Swallowing the error collapsed "no further
        // replacements" and "this file is unreadable" into one answer, which is
        // the downgrade the shared resolution rule forbids on an authoritative
        // path. Genuine absence still terminates the branch — there is nothing
        // further to walk — and is governed where references are admitted.
        match ops::effective(root, &next) {
            Ok(target) => frontier.extend(target.replacements()?),
            Err(error) if error.code == EXIT_NOT_FOUND => continue,
            Err(error) => {
                return Err(Error::new(
                    error.code,
                    format!(
                        "the supersession chain passes through {next}, which will not load, so whether this replacement closes a cycle cannot be established: {}",
                        error.message
                    ),
                ))
            }
        }
    }
    Ok(())
}

/// Refuse a merge that would consume a Section something still points at.
///
/// v1 has no redirect, no tombstone and no automatic rewrite of an inbound Ref,
/// and adding one would be inventing a forwarding semantics nobody agreed to —
/// so the only honest answer is to refuse the merge and let whoever holds the
/// reference decide what it should say now. A consumed id is never reused, so
/// the alternative is a reference pinned to wording that no longer exists
/// anywhere, pointing at an id that will never exist again.
///
/// The merge's own participants are excluded, and precisely: the sources are
/// removed by this same operation, and the destination's content is replaced
/// wholesale by the wording being confirmed. Their current references do not
/// survive it, so counting them would refuse merges that leave nothing dangling.
/// What the destination's *new* wording may point at is checked below, with the
/// self-reference rule it is a case of.
fn check_consumed_sections_are_unreferenced(root: &Path, payload: &Payload) -> Result<()> {
    let Action::SectionMerged { merge } = &payload.action else {
        return Ok(());
    };
    let consumed = merge.consumed();
    let participants = merge.participants();
    for id in store::object_ids(root)? {
        let object = ops::effective(root, &id)?;
        for section in &object.sections {
            if object.id == payload.object && participants.contains(&section.id) {
                continue;
            }
            for reference in &section.refs {
                ensure!(
                    reference.object != payload.object
                        || !consumed.contains(&reference.section),
                    EXIT_INVARIANT,
                    "§{} of {} depends on §{}, which this merge would consume; a consumed id is never reused, so revise that reference first",
                    section.id,
                    object.id,
                    reference.section
                );
            }
        }
    }
    Ok(())
}

fn validate_refs(root: &Path, payload: &Payload) -> Result<()> {
    check_consumed_sections_are_unreferenced(root, payload)?;
    for reference in &payload.content.refs {
        if let Action::SectionRevised { section } = payload.action {
            ensure!(
                reference.object != payload.object || reference.section != section,
                EXIT_INVARIANT,
                "section §{section} cannot directly reference itself"
            );
        }
        // The same rule, for the wording a merge produces. A Section cannot rest
        // on something this very operation is about to remove — including the
        // destination itself, which after the merge is what this wording *is*.
        if let Action::SectionMerged { merge } = &payload.action {
            ensure!(
                reference.object != payload.object
                    || !merge.participants().contains(&reference.section),
                EXIT_INVARIANT,
                "the merged wording cannot depend on §{}, which this merge consumes or replaces",
                reference.section
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
        // Content first, seal second, pin third — in that order, because they
        // answer different questions and the order is what keeps them from
        // covering for each other. A stored seal is a claim about what was
        // confirmed; recomputing is the only thing that establishes what the
        // target actually says now. Comparing the pin against the seal first
        // would let a target rewritten behind the gate pass the comparison that
        // matters most, since neither side of it moved.
        let actual = section.recomputed_sha256()?;
        ensure!(
            actual == section.sha256,
            EXIT_INVARIANT,
            "reference target {} §{} does not match its own confirmed hash; its wording was changed outside the gate, and pinning it would make that change look agreed",
            reference.object,
            reference.section
        );
        ensure!(
            actual == reference.sha256,
            EXIT_INVARIANT,
            "reference to {} §{} pins {} but that section is now {}",
            reference.object,
            reference.section,
            &reference.sha256[..8.min(reference.sha256.len())],
            &actual[..8.min(actual.len())]
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
            discard_locked(root, code)?;
            return Ok(Admitted {
                event: *applied,
                object,
            });
        }
        CandidateState::Stale { current_rev } => {
            crate::confirmation::classify_retry(
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
    crate::confirmation::classify_retry(
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
    store::append_event_locked(root, &event)?;
    store::save_object(root, &object)?;
    discard_locked(root, code)?;
    Ok(Admitted { event, object })
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
