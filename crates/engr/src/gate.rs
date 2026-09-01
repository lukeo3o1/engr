//! The two admission paths: Human Alignment Gate and Agent Rule Review.
//!
//! `prepare` puts a proposal up and mints a Challenge; `confirm` admits it only
//! against the exact response. `admit_agent` writes directly only after the
//! exact mutation passes every applicable usable Object Rule.
//!
//! What a Challenge stores is deliberately narrow: the subject, and nothing
//! else. Previous wording and the Object's title are derived at render time
//! from the workspace, because a pending Challenge is actionable only while the
//! Object still stands at `expected_rev`. A Rule Review is different: the exact
//! review a human is being asked to overrule is frozen inside the subject and
//! rendered from there, while live Rules are used only to decide staleness.

use crate::confirmation::{Challenge, Subject, SubjectType};
use crate::model::{
    canonical_object_id, project, Action, Content, Event, EventAdmission, HumanConfirmation,
    Object, Payload, ReviewOutcome, ReviewProvenance, SectionValue,
};
use crate::semantics::{self, Admission, Admitted as SectionAdmitted, RelationType, Target};
use crate::{
    ensure, git, ops, store, tool_error, Error, Result, EXIT_INVARIANT, EXIT_NOT_FOUND,
    EXIT_SCHEMA, EXIT_USAGE,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// The Object family's `subject.data`.
///
/// Four members plus one. `action`, `object`, `expected_rev` and `value` are the
/// frozen question. `review` is present only where the human is being asked to
/// stand behind a specific Rule Review outcome — which is the one piece of
/// admission provenance that cannot be recomputed at confirm: the digest can be
/// rebound from live Rules, but whether the agent's own review *passed* is an
/// attestation, and an Event that recorded `passed` for a review the human was
/// shown as failed would be a record of a different act.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct ObjectSubject {
    /// The command vocabulary name, not the Event type. What is being asked for
    /// and what enters history are two statements, and the Challenge makes the
    /// first.
    pub action: String,
    pub object: String,
    pub expected_rev: u64,
    /// The action-specific frozen Object-domain payload, in exactly the shape
    /// the Event's `data` carries. One schema, not two that have to agree.
    pub value: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<FrozenReview>,
}

/// The Rule Review a human is being asked to stand behind, frozen with the rest
/// of the question.
///
/// The Event keeps the interpretable result as `{ outcome, result, attempts }`.
/// The binding digest, exact Rule list and explanation are decision-time
/// material: they are meaningful while this Challenge can rebind them, not
/// after the artifacts have moved and the Challenge is gone.
///
/// `result` preserves the distinction between a review that **failed** and one
/// that was **exhausted**, even though both have outcome `overridden` in
/// history. `attempts` says which try this was. `rules` names the exact Rule set that was reviewed, so the
/// screen renders what was actually considered rather than recomputing the list
/// from live state — a Rule edited afterwards would otherwise change what a
/// frozen Challenge appears to say. `explanation` is the agent's own account of
/// why it could not pass, which nothing can reconstruct.
///
/// All of it is inside `subject`, so all of it is under `Challenge.digest`. A
/// human asked to overrule something is entitled to have the thing they are
/// overruling be part of what their answer is bound to.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct FrozenReview {
    /// The identity the review was of. It lives here rather than in history
    /// because this is where it still means something: while the Challenge is
    /// pending it binds the exact mutation against the exact Rule artifacts, and
    /// confirmation rebinds and compares it. Afterwards there is nothing left
    /// for it to be compared against.
    pub digest: String,
    pub result: crate::proof::ReviewResult,
    pub attempts: u32,
    pub rules: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
}

impl FrozenReview {
    /// What history keeps: how it got in, what the review said, and which try.
    ///
    /// The digest and the explanation stop here. Both are decision-time
    /// material — one binds artifacts that will have moved, the other is an
    /// argument made to a person in a moment — and the Challenge is where that
    /// moment lived.
    pub fn admitted(&self) -> ReviewProvenance {
        ReviewProvenance {
            // Only a human can overrule a review, so reaching history with
            // anything but `passed` means one did.
            outcome: match self.result {
                crate::proof::ReviewResult::Passed => ReviewOutcome::Passed,
                crate::proof::ReviewResult::Failed | crate::proof::ReviewResult::Exhausted => {
                    ReviewOutcome::Overridden
                }
            },
            result: self.result,
            attempts: self.attempts,
        }
    }
}

impl ObjectSubject {
    pub fn of(payload: &Payload, expected_rev: u64, review: Option<FrozenReview>) -> Result<Self> {
        let value = serde_json::to_value(&payload.action)
            .map_err(|error| Error::new(EXIT_SCHEMA, format!("challenge subject: {error}")))?;
        let value = value
            .get("data")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        Ok(Self {
            action: payload.action.command().to_owned(),
            object: payload.object.clone(),
            expected_rev,
            value,
            review,
        })
    }

    /// The payload this subject freezes, rebuilt from its own members.
    pub fn payload(&self) -> Result<Payload> {
        let action = Action::from_command(&self.action, self.value.clone())?;
        Ok(Payload::new(self.object.clone(), action))
    }
}

/// One pending Human Gate question, with its Object-family subject decoded.
///
/// The decoded half is kept beside the envelope rather than instead of it: the
/// envelope is what the digest covers, and a caller that reached only the
/// decoded form could act on a subject it never checked.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub challenge: Challenge,
    pub subject: ObjectSubject,
    pub payload: Payload,
}

impl Candidate {
    pub fn code(&self) -> &str {
        &self.challenge.id
    }

    pub fn expected_rev(&self) -> u64 {
        self.subject.expected_rev
    }

    pub fn object(&self) -> &str {
        &self.subject.object
    }
}

/// What a confirmation produced: the Event that entered the record and the
/// Object it produced.
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

/// Every challenge code a file currently claims.
///
/// Read from the filenames, without loading anything. Minting a fresh code has
/// to avoid every code on disk, including one this build refuses to admit —
/// otherwise a Challenge left by an older generator would either be silently
/// overwritten, or would make preparing anything at all fail with a message
/// about some unrelated code.
pub fn pending_codes(root: &Path) -> Result<Vec<String>> {
    let dir = store::challenges_dir(root);
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

/// Every pending Challenge, fully loaded and checked.
///
/// The strict half of the pair: one file this build refuses fails the whole
/// call. That is right for a caller that wants candidates, and wrong for a
/// listing — which is why the listing walks [`pending_codes`] and loads each one
/// on its own, so a file it cannot read becomes a line rather than the absence
/// of every other line.
pub fn pending(root: &Path) -> Result<Vec<Candidate>> {
    let mut candidates = Vec::new();
    for code in pending_codes(root)? {
        candidates.push(find(root, &code)?);
    }
    Ok(candidates)
}

/// Which object a Challenge file is about, without holding it to this build's
/// rules. Superseding has to work against a Challenge this build would refuse,
/// because leaving that one live is what would give one object two codes.
fn challenge_object(root: &Path, code: &str) -> Option<String> {
    let path = store::challenge_path(root, code).ok()?;
    let value: serde_json::Value = store::read_json(&path).ok()?;
    value
        .get("subject")?
        .get("data")?
        .get("object")?
        .as_str()
        .map(str::to_owned)
}

pub fn find(root: &Path, code: &str) -> Result<Candidate> {
    let path = store::challenge_path(root, code)?;
    ensure!(
        path.exists(),
        EXIT_NOT_FOUND,
        "no challenge awaiting {code}"
    );
    let text =
        std::fs::read_to_string(&path).map_err(|error| crate::tool_error(path.display(), error))?;
    let stored: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))?;
    // A Challenge is a current-generation resource, so its bytes are the one
    // representation this generation writes. That also settles a duplicate
    // member name — two `id` keys, say, with the second the one this reader
    // picks — because the collapsed value no longer re-serializes to the bytes
    // that had both, and a duplicate is precisely where two conforming JSON
    // readers may disagree about what a person was shown.
    store::check_canonical_bytes(&path, &text, &stored)?;
    let challenge: Challenge = serde_json::from_value(stored)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))?;
    // The code a Challenge names is the code it will be admitted by, so a file
    // that names a different one is not a question to render — it is a redirect.
    // Rendering it would show one mutation and ask for the answer to another,
    // which is the exact thing the gate exists to prevent, and both files would
    // pass every check of their own.
    ensure!(
        challenge.id == code,
        EXIT_SCHEMA,
        "challenge file {code} names {}; it would show one change and admit another",
        challenge.id
    );
    challenge.validate()?;
    ensure!(
        challenge.subject.kind == SubjectType::Object,
        EXIT_SCHEMA,
        "challenge {code} is a {} subject, not an Object mutation",
        challenge.subject.kind.as_str()
    );
    let subject: ObjectSubject = serde_json::from_value(challenge.subject.data.clone())
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("challenge {code}: {error}")))?;
    let payload = subject.payload()?;
    payload.validate()?;
    validate_title_context(&payload)?;
    if let Some(value) = payload.action.value() {
        value.content.require_canonical_order()?;
    }
    // The frozen review, if any, has to be one this generation could have
    // recorded. A `passed` outcome the human never saw and an `overridden` one
    // the record has no digest for are both admission provenance that nothing
    // produced.
    if let Some(review) = &subject.review {
        crate::digest::REVIEW.verify(&review.digest)?;
    }
    Ok(Candidate {
        challenge,
        subject,
        payload,
    })
}

/// Refuse, before anything is minted, a transition no numeric identity can carry.
///
/// The allocation boundary is *before* the Challenge. An Object at
/// `rev == MAX_SAFE_INTEGER` is itself entirely valid, and so is one at
/// `next_section_id == MAX_SAFE_INTEGER`; what is not representable is the
/// transition out of either. Some of those numbers appear nowhere in the
/// payload — the reducer allocates them — so a per-payload check passes and a
/// person is handed a code for a mutation the durable Event boundary can never
/// admit. Asking the projection covers the explicit and the allocated alike.
fn check_projection_is_representable(projected: &Object) -> Result<()> {
    let value = serde_json::to_value(projected)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("object {}: {error}", projected.id)))?;
    crate::proof::within_safe_integers(&value, &format!("object {}", projected.id))
}

/// Prove that this durable Event came from an admission path, not from a caller
/// who assembled a well-formed record.
///
/// The shape checks establish that a record is schema-exact, contiguous and
/// replayable. None of that is admission. Review provenance is three
/// self-consistent facts — `outcome`, `result`, `attempts` — and self-consistent
/// is all the envelope can check: it cannot tell an attested review from an
/// asserted one, and it cannot tell a confirmation a person gave from one nobody
/// was ever shown. Without this, a direct library caller could append
/// `by: agent` with a review it simply declares passed, or a Human record naming
/// a challenge that was never minted, and let recovery project either into
/// current authority.
///
/// Asked at the boundary rather than at the two callers, because the point is
/// the boundary and not the route to it — and asked under the writer lock, so
/// the material it recomputes against is the material the append lands on.
pub(crate) fn check_admission(root: &Path, id: &str, event: &Event) -> Result<()> {
    let admitted = &event.metadata.admitted;
    match admitted.by {
        Admission::Human => {
            let confirmation = admitted.confirmation.as_ref().ok_or_else(|| {
                Error::new(
                    EXIT_SCHEMA,
                    "a human Event carries the confirmation it was admitted by".to_owned(),
                )
            })?;
            // The Challenge is still on disk here: `confirm` appends before it
            // discards, precisely so this window exists. A code is minted by
            // `prepare` and is the one thing a caller cannot invent, so
            // requiring the exact prepared subject is what makes a Human Event
            // unforgeable rather than merely well spelled.
            let candidate = find(root, &confirmation.challenge).map_err(|error| {
                Error::new(
                    EXIT_INVARIANT,
                    format!(
                        "no prepared challenge stands for {}, so this Event was not admitted through the gate: {}",
                        confirmation.challenge, error.message
                    ),
                )
            })?;
            ensure!(
                is_admission_of(id, event, &candidate, event.rev),
                EXIT_INVARIANT,
                "challenge {} does not describe the transition this Event admits",
                confirmation.challenge
            );
            Ok(())
        }
        Admission::Agent => {
            let payload = event.payload(id);
            let before = match ops::effective(root, id) {
                Ok(object) => object,
                Err(error) if error.code == EXIT_NOT_FOUND => {
                    Object::new(id.to_owned(), String::new())?
                }
                Err(error) => return Err(error),
            };
            let mut after = before.clone();
            project(&mut after, event)?;
            let Some(review) = admitted.review.as_ref() else {
                // The one non-authoritative exception, and it is narrow: a title
                // asserts nothing about the project, so there is no policy for a
                // review to be of.
                ensure!(
                    payload.action.carries_title(),
                    EXIT_INVARIANT,
                    "Agent semantic Object admission needs at least one applicable usable Object Rule"
                );
                // And narrow in the other direction too: the exception is that
                // there is no *applicable* Rule, not that titles are exempt from
                // Rules. Where a workspace does govern the Object domain, a title
                // mutation reviews against it like any other — so the absence of
                // a review has to be established, not assumed from the shape of
                // the action.
                let mutation = crate::proof::object_review_mutation(&before, &after, &payload)?;
                ensure!(
                    !crate::rules::bind_object(root, &mutation, before.rev)?.has_rules(),
                    EXIT_INVARIANT,
                    "a project rule governs this Object, so even a title mutation carries its review"
                );
                return Ok(());
            };
            ensure!(
                review.outcome == ReviewOutcome::Passed,
                EXIT_INVARIANT,
                "an Agent mutation is admitted only by a passing Rule Review"
            );
            // What this boundary can still establish, now that durable
            // provenance keeps interpretable facts rather than a binding token:
            // that a Rule Review is a thing this mutation could have had. A
            // record claiming one where no Rule governs the mutation describes a
            // review that could not have happened, and is refused.
            //
            // It no longer establishes *which* review, and that is a real
            // narrowing rather than a rewording. The ReviewDigest used to sit in
            // the Event and be recomputed here against the live applicable set,
            // which caught a record naming a review of some other mutation. It
            // is not durable provenance — once the Challenge is gone and the
            // Rule files have moved there is nothing left to compare it against
            // — so it now lives where it is checkable, which is the Challenge
            // and the admission that answers one.
            //
            // The narrowing is smaller than it first looks. That digest is
            // derived from workspace state anybody holding the workspace can
            // read, so it was never a secret and never an obstacle to somebody
            // who wanted to write one; it caught mismatch, not forgery. What
            // stands in the way of forgery is that there is no public append at
            // all, and that `admit_agent` verifies the attestation against the
            // live binding before it builds anything.
            let mutation = crate::proof::object_review_mutation(&before, &after, &payload)?;
            ensure!(
                crate::rules::bind_object(root, &mutation, before.rev)?.has_rules(),
                EXIT_INVARIANT,
                "this Event records a Rule Review, and no Rule governs the mutation it carries"
            );
            Ok(())
        }
    }
}

/// The admission provenance a Human confirmation records.
fn human_admission(challenge: &str, at: &str, review: Option<ReviewProvenance>) -> EventAdmission {
    EventAdmission {
        by: Admission::Human,
        at: at.to_owned(),
        confirmation: Some(HumanConfirmation {
            challenge: challenge.to_owned(),
        }),
        review,
    }
}

fn agent_admission(at: &str, review: Option<ReviewProvenance>) -> EventAdmission {
    EventAdmission {
        by: Admission::Agent,
        at: at.to_owned(),
        confirmation: None,
        review,
    }
}

/// The trial Event a preflight projects, so a proposal that cannot possibly
/// apply never reaches a person.
///
/// It carries the admission path but no identity and no seal, because nothing
/// about the projection depends on either — and minting a real Event id for a
/// probe would put an id into the world that never entered history.
fn probe(payload: &Payload, rev: u64, admitted: Admission, at: &str) -> Event {
    Event {
        id: crate::model::new_id(),
        action: payload.action.clone(),
        rev,
        metadata: crate::model::Metadata {
            admitted: EventAdmission {
                by: admitted,
                at: at.to_owned(),
                confirmation: match admitted {
                    Admission::Human => Some(HumanConfirmation {
                        challenge: "AAAAAA".to_owned(),
                    }),
                    Admission::Agent => None,
                },
                review: None,
            },
        },
        digest: String::new(),
    }
}

/// A trial Event a read surface can project to see what a proposal produces.
///
/// Public because rendering a pending Challenge has to answer "which Rules
/// govern this", and that question is asked of the *resulting* projection. It
/// publishes nothing: there is no public append, and the Event it hands back
/// carries no admission a store would accept.
pub fn preview_event(payload: &Payload, rev: u64) -> Result<Event> {
    Ok(probe(payload, rev, Admission::Human, &now()))
}

/// The actionable state of an outstanding Challenge.
///
/// A Challenge can survive the narrow crash window after its Event/projection
/// are durable but before its file is deleted. That is retryable, not stale: the
/// same exact confirmation must finish cleanup without applying again.
#[derive(Debug)]
pub enum CandidateState {
    Pending,
    AlreadyApplied(Box<Event>),
    Stale { current_rev: u64 },
}

/// Whether `event` is the durable admission of exactly this Challenge.
///
/// One predicate for both the reader and the classifier, because two answers to
/// the same question drift. The Event names the spent code, so it proves
/// confirmation of *that* question; matching the mutation alone would let an
/// older identical Challenge — restored or copied after a later one was
/// applied — be reported and cleaned up as though a person had answered for it.
/// Nobody ever did.
/// Whether this Event is the admission of exactly this Challenge.
///
/// Everything is compared but the instant the Section value was admitted at, and
/// that exception is the whole of what the stamp costs. A Challenge freezes
/// *which* act, against *which* Object, at *which* revision, through *which*
/// door — and the Event must match all of it. It cannot also freeze *when* the
/// act was admitted, because that had not happened yet when the question was
/// asked.
///
/// Nothing is loosened by leaving it out. `admitted.at` is not a fact a person
/// assents to; it is a fact the admission creates.
fn is_admission_of(id: &str, event: &Event, candidate: &Candidate, applied_rev: u64) -> bool {
    event.rev == applied_rev
        && id == candidate.object()
        && same_act(&event.action, &candidate.payload.action)
        && event
            .human_confirmation()
            .is_some_and(|confirmation| confirmation.challenge == candidate.challenge.id)
}

/// Two actions that differ in nothing a person was asked about.
fn same_act(left: &Action, right: &Action) -> bool {
    let (mut left, mut right) = (left.clone(), right.clone());
    for action in [&mut left, &mut right] {
        if let Some(value) = action.value_mut() {
            value.admitted.at = String::new();
        }
    }
    left == right
}

/// Stamp the instant the Section value is being admitted at.
///
/// `admitted.by` is frozen with the rest of the question, because which door a
/// value comes through is part of what a person assents to. `admitted.at` is
/// not: it is when the admission happened, and at prepare it has not happened.
///
/// A Challenge can sit for hours before somebody answers it, so carrying the
/// preparation instant into the record would persist an admission time that
/// predates the admission — a false statement in the one place the record exists
/// to be true. The Challenge keeps its own copy as "when this was put up",
/// beside `created_at` which says the same thing about the envelope.
/// The instant is passed in rather than read here, because a Section's
/// `admitted.at` and the Event's `metadata.admitted.at` are **the same
/// admission instant** for an ordinary admission. Two clock reads microseconds
/// apart would have the record say the Section was admitted at a different
/// moment from the Event that admitted it — a distinction the record does not
/// have and could not defend. Migration is the one place the two legitimately
/// differ, and it says so where it does it.
fn admitted_at(mut action: Action, at: &str) -> Action {
    if let Some(value) = action.value_mut() {
        value.admitted.at = at.to_owned();
    }
    action
}

/// Classify a Challenge from the same effective projection and durable Event
/// evidence used by confirmation. Read surfaces and admission must agree about
/// whether a code is still actionable.
pub fn candidate_state(root: &Path, candidate: &Candidate) -> Result<CandidateState> {
    let id = candidate.object();
    match ops::effective(root, id) {
        Ok(object) => {
            if object.rev > candidate.expected_rev() {
                let applied_rev = candidate.expected_rev().checked_add(1).ok_or_else(|| {
                    Error::new(EXIT_INVARIANT, "challenge revision cannot advance")
                })?;
                if let Some(event) = store::load_events(root, id)?
                    .into_iter()
                    .find(|event| is_admission_of(id, event, candidate, applied_rev))
                {
                    return Ok(CandidateState::AlreadyApplied(Box::new(event)));
                }
            }
            if object.rev == candidate.expected_rev() {
                Ok(CandidateState::Pending)
            } else {
                Ok(CandidateState::Stale {
                    current_rev: object.rev,
                })
            }
        }
        Err(error) if error.code == EXIT_NOT_FOUND => {
            if candidate.expected_rev() == 0 && store::load_events(root, id)?.is_empty() {
                Ok(CandidateState::Pending)
            } else {
                Ok(CandidateState::Stale { current_rev: 0 })
            }
        }
        Err(error) => Err(error),
    }
}

/// Whether a pending Challenge can still be acted on. An already-applied one
/// remains live only for its idempotent cleanup retry.
pub fn is_live(root: &Path, candidate: &Candidate) -> bool {
    matches!(
        candidate_state(root, candidate),
        Ok(CandidateState::Pending | CandidateState::AlreadyApplied(_))
    )
}

/// Something worth weighing before typing the code, but not grounds to refuse.
/// Rendered with the Challenge, because the moment to reconsider is while the
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
    /// The Rule Review this preparation composed, for a caller that wants it in
    /// hand without reopening the Challenge.
    ///
    /// **Not the authoritative copy.** That one is frozen inside
    /// `candidate.subject.review`, and it is what rendering and confirmation
    /// both read: the digest, the result, the attempt, the exact Rule ids and
    /// the agent's explanation, fixed at the moment the question was asked. An
    /// earlier design kept only the outcome there and rebuilt the rest from live
    /// Rules whenever the Challenge was shown, which meant a Rule edited in the
    /// meantime changed what a frozen Challenge appeared to say. Live Rules now
    /// decide staleness and nothing else.
    pub review: Option<crate::proof::CandidateReview>,
}

/// What an Agent attests after reviewing the exact binding engr surfaced.
///
/// The digest and Rule ids are checked against a fresh binding inside the writer
/// lock. None of the rest alters the ReviewDigest — the attempt, the result and
/// the explanation describe the review process, not the material reviewed — but
/// all of it is frozen into the Challenge subject, because all of it is what a
/// human is being asked to weigh. What history keeps afterwards is narrower
/// still: `{outcome, result, attempts}`, without the digest that binds artifacts
/// which will have moved, and without an argument written to persuade somebody
/// in a particular moment.
#[derive(Clone, Debug)]
pub struct ReviewAttestation {
    pub review_digest: String,
    pub reviewed_rules: Vec<String>,
    pub attempt: u32,
    pub result: crate::proof::ReviewResult,
    pub explanation: Option<String>,
}

/// A title is a label, not a body.
///
/// It is the line `ls` prints, so a paragraph pasted in here degrades the
/// listing for every other object as well as its own. `--rename` makes the
/// mistake recoverable rather than fatal, which is a reason to keep this check
/// and not a reason to drop it — the listing is wrecked either way until someone
/// notices. 120 characters is wide for a label and nowhere near a paragraph.
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
/// every semantic identifier before the subject is frozen, so the Challenge the
/// human reads is byte-for-byte the one that becomes the Event.
fn canonicalize_payload(root: &Path, payload: &mut Payload) -> Result<()> {
    payload.object = canonical_object_id(&payload.object)?;
    if let Some(value) = payload.action.value_mut() {
        if let Some(based_on) = value.content.based_on.clone() {
            value.content.based_on = Some(crate::semantics::BasedOn::new(resolve_commit(
                root,
                "based_on",
                &based_on.commit,
            )?));
        }
        for relation in &mut value.content.relations {
            if let Target::File { commit, .. } | Target::Symbol { commit, .. } =
                &mut relation.target
            {
                *commit = resolve_commit(root, "relation commit", commit)?;
            }
        }
        // After resolution, not before: two spellings of the same commit are the
        // same relation, and sorting has to see the resolved values or the order
        // would depend on what the caller happened to type.
        value.content.canonicalize_order()?;
    }
    // `sources[]` is a protocol-defined set like any other, so it takes the
    // shared canonical-set order — JCS each element, then sort by those bytes —
    // rather than a field-local numeric rule. The two disagree as soon as the
    // ids have different digit counts: `[2, 10]` is ascending, canonical is
    // `[10, 2]`.
    if let Action::SectionMerged { merge, .. } = &mut payload.action {
        crate::proof::canonical_set(&mut merge.sources, "merge source")?;
    }
    payload.validate()
}

/// Titles name the Object; unlike section wording they are not assertions made
/// against repository context or another section. Keeping that rule at admission
/// prevents the renderer from hiding semantic fields a human never had a chance
/// to read.
fn validate_title_context(payload: &Payload) -> Result<()> {
    if payload.action.carries_title() {
        ensure!(
            payload.action.value().is_none(),
            EXIT_INVARIANT,
            "{} titles carry no repository basis, references, role, supplementary content or relations",
            payload.action.event_type()
        );
    }
    Ok(())
}

/// Confirmed payloads must already be canonical; retries must never use this to
/// silently change what an earlier human confirmed.
fn validate_persisted_payload(root: &Path, payload: &Payload) -> Result<()> {
    payload.validate()?;
    validate_title_context(payload)?;
    let Some(value) = payload.action.value() else {
        return Ok(());
    };
    if let Some(based_on) = &value.content.based_on {
        ensure!(
            git::resolve(root, &based_on.commit).as_deref() == Some(based_on.commit.as_str()),
            EXIT_INVARIANT,
            "based_on {} is not a commit in this repository",
            based_on.commit
        );
    }
    for reference in &value.content.refs {
        ensure!(
            git::resolve(root, reference.commit()).as_deref() == Some(reference.commit()),
            EXIT_INVARIANT,
            "reference commit {} is not a commit in this repository",
            reference.commit()
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
    if let Some(title) = candidate.payload.action.title() {
        if let Some(object) = object_with_title(root, title, candidate.object()) {
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
    prepare_admitting(root, payload, Allowance::Normal, None)
}

/// Prepare a proposal that broke a normal size threshold, after a first attempt
/// was already refused.
pub fn prepare_oversize(root: &Path, payload: Payload) -> Result<Prepared> {
    prepare_admitting(root, payload, Allowance::Oversize, None)
}

/// Prepare the repair of an Object whose stored projection failed integrity.
///
/// Ordinary mutation refuses an integrity-invalid Object — correctly, or
/// unrelated work would launder an out-of-band edit into valid authority — and
/// without this there is no way back at all: one hand edit would freeze a record
/// permanently, which pushes people toward more hand editing.
///
/// Four things about it are settled elsewhere, not by this function:
///
/// - **Human Gate only.** Repair mints a Challenge like any other Human
///   proposal, whatever admission class the Sections being restored carry. An
///   Agent does not re-establish authority that stopped verifying.
/// - **Exactly the replay-derived projection.** The proposal is
///   [`ops::provable`] and nothing else, so repair cannot carry a change.
///   Anything a person wants to keep from the invalid bytes goes through the
///   normal path *after* the repair, leaving `object.repaired.v1` then
///   `section.updated.v1` in the record instead of one Event that quietly
///   admitted both.
/// - **The invalid bytes are diagnostic, not the proposal.** They are shown
///   beside it and are not part of the frozen subject.
/// - **Fails closed.** If history cannot rebuild the projection, this refuses
///   rather than guessing; that is a different damage class and not this path's
///   to repair.
pub fn prepare_repair(root: &Path, id: &str) -> Result<Prepared> {
    store::require_current(root)?;
    store::with_lock(root, move || prepare_repair_locked(root, id))
}

fn prepare_repair_locked(root: &Path, id: &str) -> Result<Prepared> {
    store::require_current(root)?;
    crate::model::validate_object_id(id)?;
    let stored = store::load_object(root, id)?;
    // Nothing to repair is a refusal, not a no-op. Repair is an exceptional
    // boundary, and one that ran on sound authority would be a general-purpose
    // rewrite with a special name.
    ensure!(
        crate::integrity::check_stored_object_integrity(&stored).is_err(),
        EXIT_INVARIANT,
        "{id} verifies, so there is nothing to repair; ordinary changes go through the normal path"
    );

    let before = ops::provable(root, id)?;
    let payload = Payload::new(id, Action::ObjectRepaired {});
    let at = now();
    {
        let mut trial = before.clone();
        project(
            &mut trial,
            &probe(&payload, before.rev + 1, Admission::Human, &at),
        )?;
        check_projection_is_representable(&trial)?;
    }
    // A repair restores; it does not propose semantics for a Rule to judge, and
    // the projection either side is identical. Binding a review here would ask
    // the Rule set about a change that is not one.
    mint(root, &payload, before.rev, None, None)
}

/// Prepare a Human Challenge carrying the exact Rule Review the Human saw.
pub fn prepare_reviewed(
    root: &Path,
    payload: Payload,
    allowance: Allowance,
    review: ReviewAttestation,
) -> Result<Prepared> {
    prepare_admitting(root, payload, allowance, Some(review))
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
fn prepare_admitting(
    root: &Path,
    payload: Payload,
    allowance: Allowance,
    review: Option<ReviewAttestation>,
) -> Result<Prepared> {
    // Refuse a predecessor workspace before opening the writer lock: even
    // creating a lock file would violate its explicit read-only boundary.
    store::require_current(root)?;
    store::with_lock(root, move || {
        prepare_locked(root, payload, allowance, review)
    })
}

/// Which proposals engr has refused for size, so an explicit retry can prove it
/// is the retry of something that was actually refused.
///
/// A list rather than a slot, because a workspace holds work on many Objects at
/// once. One slot would mean any second proposal considered anywhere revoked the
/// first one's refusal, and the agent that had already done what the rule asks
/// would be sent back to do it again.
///
/// Admission-time only, like the exception itself: nothing here is the record,
/// nothing here survives being consumed, and it is capped because it is scratch
/// memory rather than history.
#[derive(Serialize, Deserialize, Default)]
struct Refusals {
    /// Proposal digests, most recently refused first.
    refused: Vec<String>,
}

const REFUSALS_REMEMBERED: usize = 32;

/// The identity of one proposal, for the size-refusal ledger only.
///
/// Not a protocol digest and never persisted as provenance — it is scratch
/// memory that has to be able to say "this exact proposal, again".
///
/// `admitted` is stripped before hashing, and that is deliberate rather than
/// incidental. What "the same proposal" means here is the same wording against
/// the same basis — which is what a person would say too, and which is a
/// question about the content rather than about the door it comes through or
/// when it is answered. Stripping it also keeps the receipt correct no matter
/// what the pending placeholder is: the retry has to match the proposal it was
/// written for, and a key that could move under it turns the two-stage size rule
/// into a permanent refusal.
fn proposal_key(payload: &Payload) -> Result<String> {
    let mut value = serde_json::to_value(payload)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("proposal: {error}")))?;
    if let Some(section) = value
        .get_mut("data")
        .and_then(|data| data.get_mut("value"))
        .and_then(serde_json::Value::as_object_mut)
    {
        section.remove("admitted");
    }
    Ok(crate::proof::sha256_of(&crate::proof::canonical_bytes(
        &value, "proposal",
    )?))
}

/// The two-stage size allowance enforced, rather than described.
///
/// The first `prepare` above a normal threshold MUST refuse and only an explicit
/// retry may ask for the exception. A flag cannot carry that on its own, because
/// a flag can be passed the first time — so the refusal writes down which
/// proposal it refused and the retry is admitted only if it is that same
/// proposal, byte for byte. The payload is already canonical here, so the same
/// wording written against a different basis is a different proposal and earns
/// its own refusal first.
fn check_allowance(root: &Path, payload: &Payload, allowance: Allowance) -> Result<()> {
    let oversize = allowance == Allowance::Oversize;
    let Some(value) = payload.action.value() else {
        return Ok(());
    };
    let breaches = semantics::exceeded(&value.content.text, &value.content.content);
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
                refusals(root).refused.contains(&proposal_key(payload)?),
                EXIT_INVARIANT,
                "an oversize exception is the retry of a refusal, and engr has not refused this proposal; prepare it without --oversize first and read what that refusal suggests"
            );
        }
    }

    // The refusals themselves stay in one place, so the wording an agent reads
    // and the wording the unit tests pin are the same wording.
    let result = semantics::check_size(&value.content.text, &value.content.content, oversize);
    // Remember what was refused, so the retry has something to be a retry of. A
    // hard-ceiling refusal is deliberately not remembered: it has no retry, and
    // leaving a receipt behind would make `--oversize` look like the answer to a
    // refusal that always says no.
    if result.is_err() && !hard {
        remember_refusal(root, &proposal_key(payload)?)?;
    }
    result.map(|_| ())
}

/// Unreadable or absent is treated as "nothing refused" rather than as an error:
/// this file is one machine's scratch memory, and a corrupted one should cost a
/// second refusal, not make the workspace unusable.
fn refusals(root: &Path) -> Refusals {
    store::read_json::<Refusals>(&store::refusal_path(root)).unwrap_or_default()
}

fn remember_refusal(root: &Path, key: &str) -> Result<()> {
    let mut held = refusals(root);
    held.refused.retain(|held| held != key);
    held.refused.insert(0, key.to_owned());
    held.refused.truncate(REFUSALS_REMEMBERED);
    let path = store::refusal_path(root);
    store::write_json(&path, &held)
}

fn spend_refusal(root: &Path, key: &str) -> Result<()> {
    let mut held = refusals(root);
    let before = held.refused.len();
    held.refused.retain(|held| held != key);
    if held.refused.len() == before {
        return Ok(());
    }
    let path = store::refusal_path(root);
    store::write_json(&path, &held)
}

/// Rebuild the reviewed subject from live state and check the attestation
/// against it.
fn checked_review(
    root: &Path,
    before: &Object,
    after: &Object,
    payload: &Payload,
    expected_rev: u64,
    attestation: Option<ReviewAttestation>,
) -> Result<Option<crate::proof::CandidateReview>> {
    let mutation = crate::proof::object_review_mutation(before, after, payload)?;
    let binding = crate::rules::bind_object(root, &mutation, expected_rev)?;
    let expected_digest = binding.digest()?.to_string();
    let expected_rules = binding.rule_ids();

    if expected_rules.is_empty() {
        ensure!(
            attestation.is_none(),
            EXIT_USAGE,
            "no Object Rule applies to this mutation, so there is no Rule Review to attest"
        );
        return Ok(None);
    }

    let attestation = attestation.ok_or_else(|| {
        Error::new(
            EXIT_USAGE,
            format!(
                "this mutation is governed by {}; review the surfaced Rules, then repeat it with review digest {} and the review outcome",
                expected_rules.join(", "),
                expected_digest
            ),
        )
    })?;
    let mut reviewed = attestation.reviewed_rules;
    reviewed.sort();
    reviewed.dedup();
    ensure!(
        reviewed == expected_rules,
        EXIT_INVARIANT,
        "the review names {}, and this mutation is governed by {}",
        if reviewed.is_empty() {
            "no Rules".to_owned()
        } else {
            reviewed.join(", ")
        },
        expected_rules.join(", ")
    );
    ensure!(
        attestation.review_digest == expected_digest,
        EXIT_INVARIANT,
        "this review was of something else; review the current subject and attest to {expected_digest}"
    );
    let review = crate::proof::CandidateReview {
        review_digest: attestation.review_digest,
        attempt: attestation.attempt,
        result: attestation.result,
        rules: binding.rules().to_vec(),
        explanation: attestation.explanation,
    };
    crate::proof::check_review_report(&review)?;
    crate::proof::check_object_review_identity(&review, &mutation, expected_rev)?;
    Ok(Some(review))
}

/// What a review outcome means for the Event a confirmation will record.
///
/// A pass ratifies; a failure or an exhausted sequence that a human confirms
/// anyway is an override, and the record must not lose the difference.
fn frozen_review(review: Option<&crate::proof::CandidateReview>) -> Option<FrozenReview> {
    review.map(|review| FrozenReview {
        digest: review.review_digest.clone(),
        result: review.result,
        attempts: review.attempt,
        rules: review.rules.iter().map(|rule| rule.id.clone()).collect(),
        explanation: review.explanation.clone(),
    })
}

/// Freeze the subject, mint the code, and supersede any other live Challenge for
/// the same Object.
fn mint(
    root: &Path,
    payload: &Payload,
    expected_rev: u64,
    review: Option<FrozenReview>,
    report: Option<crate::proof::CandidateReview>,
) -> Result<Prepared> {
    let subject = ObjectSubject::of(payload, expected_rev, review)?;
    let data = serde_json::to_value(&subject)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("challenge subject: {error}")))?;
    let taken = pending_codes(root)?;
    let challenge = Challenge::mint(
        Subject {
            kind: SubjectType::Object,
            data,
        },
        &taken,
        now(),
    )?;

    // One live Challenge per object: a second proposal supersedes the first, so
    // a human is never holding two codes for the same thing. Read leniently, so
    // a file this build would refuse still gets superseded rather than being
    // left beside its replacement.
    let mut superseded = Vec::new();
    for code in taken {
        if code != challenge.id
            && challenge_object(root, &code).as_deref() == Some(&*payload.object)
        {
            fs::remove_file(store::challenge_path(root, &code)?)
                .map_err(|error| tool_error("discarding a superseded challenge", error))?;
            superseded.push(code);
        }
    }
    store::write_json(&store::challenge_path(root, &challenge.id)?, &challenge)?;
    let candidate = Candidate {
        challenge,
        subject,
        payload: payload.clone(),
    };
    let notes = notes_for(root, &candidate);
    Ok(Prepared {
        candidate,
        superseded,
        notes,
        review: report,
    })
}

fn prepare_locked(
    root: &Path,
    mut payload: Payload,
    allowance: Allowance,
    review: Option<ReviewAttestation>,
) -> Result<Prepared> {
    store::require_current(root)?;
    validate_title_context(&payload)?;
    canonicalize_payload(root, &mut payload)?;
    // A title is the line a listing prints, so whitespace around it is never
    // meaningful and always visible — it pushes one row out of column. The
    // duplicate check already ignores it; storing what that check ignores is how
    // a listing ends up misaligned underneath a note saying the titles match.
    // Normalised here, before the code is minted, so the human confirms the
    // exact string that will be stored.
    trim_title(&mut payload);
    let object = match ops::reconcile_locked(root, &payload.object) {
        Ok(object) => Some(object),
        Err(error) if error.code == EXIT_NOT_FOUND => None,
        Err(error) => return Err(error),
    };
    check_target_exists(&payload, object.is_some(), "--new", "--rename")?;

    // Every threshold this proposal breaks, refused here unless it is the
    // explicit retry of a refusal. Not in `Payload::validate`, for the same
    // reason the title limit is not: that runs when Events are loaded, and a
    // workspace holding a Section admitted under an exception has to keep being
    // able to replay its own history.
    if payload.action.carries_content() && !payload.action.carries_title() {
        check_allowance(root, &payload, allowance)?;
    } else {
        // Nothing here is measured against a Section threshold, so an exception
        // would be one the Challenge claims and no refusal ever granted.
        ensure!(
            allowance == Allowance::Normal,
            EXIT_USAGE,
            "{} carries no Section content, so there is no size exception to make",
            payload.action.event_type()
        );
    }
    check_is_a_change(&payload, object.as_ref())?;

    // Preflight the reducer so a proposal that cannot possibly apply never
    // reaches a human. The result is kept rather than thrown away: it is the
    // authority this Challenge will produce.
    let before = match &object {
        Some(object) => object.clone(),
        None => Object::new(payload.object.clone(), String::new())?,
    };
    let at = now();
    let projected = {
        let mut trial = before.clone();
        project(
            &mut trial,
            &probe(&payload, before.rev + 1, Admission::Human, &at),
        )?;
        check_projection_is_representable(&trial)?;
        trial
    };

    validate_refs(root, &payload, Admission::Human)?;
    validate_relations(root, &payload, &projected)?;

    let expected_rev = object.as_ref().map(|object| object.rev).unwrap_or(0);
    let report = checked_review(root, &before, &projected, &payload, expected_rev, review)?;
    let prepared = mint(
        root,
        &payload,
        expected_rev,
        frozen_review(report.as_ref()),
        report,
    )?;
    if allowance == Allowance::Oversize {
        spend_refusal(root, &proposal_key(&payload)?)?;
    }
    Ok(prepared)
}

/// Trim the title a create or rename carries.
fn trim_title(payload: &mut Payload) {
    match &mut payload.action {
        Action::ObjectCreated { title } | Action::ObjectRenamed { title, .. } => {
            let trimmed = title.trim().to_owned();
            *title = trimmed;
        }
        _ => {}
    }
}

/// Creation needs an absent Object; everything else needs a present one.
fn check_target_exists(
    payload: &Payload,
    exists: bool,
    new_flag: &str,
    rename_flag: &str,
) -> Result<()> {
    match (&payload.action, exists) {
        (Action::ObjectCreated { .. }, true) => Err(Error::new(
            EXIT_INVARIANT,
            "that object already exists".to_owned(),
        )),
        // Here, not in `Payload::validate`: that runs when Events are *loaded*,
        // so a limit enforced there would make a workspace holding an over-long
        // title unable to replay its own history.
        (Action::ObjectCreated { title }, false) => check_title(new_flag, title),
        (Action::ObjectRenamed { title, .. }, true) => check_title(rename_flag, title),
        (_, false) => Err(Error::new(
            EXIT_NOT_FOUND,
            format!("no object {}", payload.object),
        )),
        (_, true) => Ok(()),
    }
}

/// A confirmation that changes nothing is not a change to confirm.
///
/// It matters for a Section update specifically because `refs` and `relations`
/// are sets: the same proposal written in another order canonicalizes to the
/// same content, and admitting it would spend a confirmation and a revision on a
/// reordering the model says is not a difference. The same rule applies to the
/// Object's own lifecycle, where confirming a move to the state it is already in
/// would append a permanent Event recording no change, spend a `rev`, and void
/// every other live Challenge for this Object.
fn check_is_a_change(payload: &Payload, object: Option<&Object>) -> Result<()> {
    let Some(object) = object else {
        return Ok(());
    };
    match &payload.action {
        Action::SectionUpdated { section, value, .. } => {
            let held = object.section(*section)?;
            let mut current = held.content();
            current.canonicalize_order()?;
            ensure!(
                current != value.content,
                EXIT_INVARIANT,
                "§{} already says exactly this, so there is nothing to confirm",
                held.id
            );
        }
        Action::ObjectClassified { object_type, state } => {
            ensure!(
                (*object_type, *state) != (object.object_type, object.state),
                EXIT_INVARIANT,
                "{} is already {}, so there is nothing to confirm",
                object.id,
                crate::view::classification(object)
            );
        }
        Action::ObjectStateChanged { state } => {
            ensure!(
                *state != object.state,
                EXIT_INVARIANT,
                "{} is already {}, so there is nothing to confirm",
                object.id,
                state.as_str()
            );
        }
        _ => {}
    }
    Ok(())
}

/// Admit an Agent-reviewed Object mutation without minting a Human Challenge.
///
/// The review is recomputed from the exact predecessor, resulting Agent
/// projection, and live Rule material while the writer lock is held. A missing
/// Rule is permitted only for title create/rename, which is non-authoritative
/// navigation metadata.
pub fn admit_agent(
    root: &Path,
    payload: Payload,
    review: Option<ReviewAttestation>,
) -> Result<Admitted> {
    store::require_current(root)?;
    store::with_lock(root, move || admit_agent_locked(root, payload, review))
}

fn admit_agent_locked(
    root: &Path,
    mut payload: Payload,
    review: Option<ReviewAttestation>,
) -> Result<Admitted> {
    store::require_current(root)?;
    // Before anything else, and unconditionally. The reducer refuses an
    // Agent-admitted repair too, but a door that is only closed further in is
    // one somebody has to reason about; this one is shut at the entrance.
    ensure!(
        !matches!(payload.action, Action::ObjectRepaired {}),
        EXIT_INVARIANT,
        "object.repaired.v1 is admitted through the human gate only; prepare it with `engr repair`"
    );
    validate_title_context(&payload)?;
    canonicalize_payload(root, &mut payload)?;
    trim_title(&mut payload);
    let stored = match ops::reconcile_locked(root, &payload.object) {
        Ok(object) => Some(object),
        Err(error) if error.code == EXIT_NOT_FOUND => None,
        Err(error) => return Err(error),
    };
    check_target_exists(&payload, stored.is_some(), "--new", "--rename")?;

    if payload.action.carries_content() && !payload.action.carries_title() {
        check_allowance(root, &payload, Allowance::Normal)?;
    }
    check_is_a_change(&payload, stored.as_ref())?;

    let before = match stored {
        Some(object) => object,
        None => Object::new(payload.object.clone(), String::new())?,
    };
    let at = now();
    let mut projected = before.clone();
    project(
        &mut projected,
        &probe(&payload, before.rev + 1, Admission::Agent, &at),
    )?;
    check_projection_is_representable(&projected)?;
    validate_refs(root, &payload, Admission::Agent)?;
    validate_relations(root, &payload, &projected)?;

    let checked = checked_review(root, &before, &projected, &payload, before.rev, review)?;
    if let Some(review) = &checked {
        ensure!(
            review.result == crate::proof::ReviewResult::Passed,
            EXIT_INVARIANT,
            "an Agent mutation is admitted only by a passing Rule Review"
        );
    } else {
        ensure!(
            payload.action.carries_title(),
            EXIT_INVARIANT,
            "Agent semantic Object admission needs at least one applicable usable Object Rule"
        );
    }

    // An Agent cannot overrule a review, so anything that reached here passed
    // one; the attempt it passed on is the fact worth keeping.
    let reviewed = checked.map(|review| ReviewProvenance {
        outcome: ReviewOutcome::Passed,
        result: review.result,
        attempts: review.attempt,
    });
    // One clock read, for both facts, exactly as the Human path does it. The
    // Agent path has no waiting human, so the two moments are close — but
    // "close" is not a contract, and a record stating them as one fact is the
    // record that cannot be wrong about it.
    let at = now();
    let event = Event::sealed(
        &payload.object,
        crate::model::new_id(),
        admitted_at(payload.action.clone(), &at),
        before.rev + 1,
        agent_admission(&at, reviewed),
    )?;
    let object = crate::integrity::mutate(&before, |next| project(next, &event))?.object;
    validate_relations(root, &payload, &object)?;
    store::append_event_locked(root, &payload.object, &event)?;
    store::save_object(root, &object)?;
    Ok(Admitted { event, object })
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
    let relations = payload
        .action
        .value()
        .map(|value| value.content.relations.clone())
        .unwrap_or_default();
    for relation in &relations {
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
/// and every branch of it must be walked to the end.
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
        // path.
        //
        // Absence is the same failure. `effective` already reconstructs a
        // target whose projection is missing but whose admitted history still
        // establishes it, so NOT_FOUND here means the authority genuinely
        // cannot be established — and v1 has no Object delete, so a
        // `superseded_by` edge pointing at nothing is a broken invariant
        // already, not an ordinary end of chain. Authorizing a new supersession
        // through a graph that cannot be established would bless it.
        match ops::effective(root, &next) {
            Ok(target) => {
                ops::sound(root, &target, None)?;
                frontier.extend(target.replacements()?)
            }
            Err(error) if error.code == EXIT_NOT_FOUND => {
                return Err(Error::new(
                    EXIT_INVARIANT,
                    format!(
                        "the supersession chain passes through {next}, which no longer exists, so the replacement graph this would join cannot be established"
                    ),
                ))
            }
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
    let Action::SectionMerged { merge, .. } = &payload.action else {
        return Ok(());
    };
    let consumed = merge.consumed();
    let participants = merge.participants();
    for id in store::object_ids(root)? {
        let object = ops::effective(root, &id)?;
        ops::sound(root, &object, None)?;
        for section in &object.sections {
            if object.id == payload.object && participants.contains(&section.id) {
                continue;
            }
            for reference in &section.refs {
                let (target, target_section) = crate::dependency::parse_target(reference.target())?;
                ensure!(
                    target != payload.object || !consumed.contains(&target_section),
                    EXIT_INVARIANT,
                    "§{} of {} depends on §{}, which this merge would consume; a consumed id is never reused, so revise that reference first",
                    section.id,
                    object.id,
                    target_section
                );
            }
        }
    }
    Ok(())
}

fn validate_refs(root: &Path, payload: &Payload, admitted_by: Admission) -> Result<()> {
    check_consumed_sections_are_unreferenced(root, payload)?;
    let Some(value) = payload.action.value() else {
        return Ok(());
    };
    for reference in &value.content.refs {
        let (target_id, target_section_id) = crate::dependency::parse_target(reference.target())?;
        if let Action::SectionUpdated { section, .. } = payload.action {
            ensure!(
                target_id != payload.object || target_section_id != section,
                EXIT_INVARIANT,
                "section §{section} cannot directly reference itself"
            );
        }
        // The same rule, for the wording a merge produces. A Section cannot rest
        // on something this very operation is about to remove — including the
        // destination itself, which after the merge is what this wording *is*.
        if let Action::SectionMerged { merge, .. } = &payload.action {
            ensure!(
                target_id != payload.object || !merge.participants().contains(&target_section_id),
                EXIT_INVARIANT,
                "the merged wording cannot depend on §{}, which this merge consumes or replaces",
                target_section_id
            );
        }
        let target = ops::effective(root, &target_id).map_err(|error| {
            if error.code == EXIT_NOT_FOUND {
                Error::new(
                    EXIT_NOT_FOUND,
                    format!("reference target object {target_id} does not exist"),
                )
            } else {
                error
            }
        })?;
        let section = target.section(target_section_id)?;
        if admitted_by == Admission::Human {
            ensure!(
                section.admitted.by == Admission::Human,
                EXIT_INVARIANT,
                "a human section may reference only human-admitted authority; {target_id} §{target_section_id} is {}",
                section.admitted.by.as_str()
            );
        }
        match crate::dependency::evaluate(root, &target, reference)? {
            crate::dependency::Dependency::Unchanged => {}
            crate::dependency::Dependency::SchemaMismatch => {
                return Err(Error::new(
                    EXIT_SCHEMA,
                    format!(
                        "reference target {target_id} §{target_section_id} cannot be interpreted under the workspace contract at its recorded commit"
                    ),
                ));
            }
            crate::dependency::Dependency::TargetMissing
            | crate::dependency::Dependency::ProvenanceUnavailable => {
                return Err(Error::new(
                    EXIT_NOT_FOUND,
                    format!(
                        "reference target {target_id} §{target_section_id} or its recorded provenance is unavailable"
                    ),
                ));
            }
            state => {
                return Err(Error::new(
                    EXIT_INVARIANT,
                    format!(
                        "reference target {target_id} §{target_section_id} is not the unchanged verified dependency it records: {state:?}"
                    ),
                ));
            }
        }
    }
    Ok(())
}
pub fn discard(root: &Path, code: &str) -> Result<()> {
    store::require_current(root)?;
    store::with_lock(root, || discard_locked(root, code))
}

pub(crate) fn discard_locked(root: &Path, code: &str) -> Result<()> {
    store::require_current(root)?;
    let path = store::challenge_path(root, code)?;
    ensure!(
        path.exists(),
        EXIT_NOT_FOUND,
        "no challenge awaiting {code}"
    );
    fs::remove_file(&path).map_err(|error| tool_error(path.display(), error))?;
    Ok(())
}

/// Admit a Challenge against the exact response.
///
/// A response that begins with `CONFIRM ` but is not exactly the phrase is not a
/// near miss to be helpful about — it is hedged assent, and it discards the
/// Challenge. Accepting a bare code instead would put the agent in the position
/// of deciding whether "yes, but reword the second sentence" counted as a yes.
pub fn confirm(root: &Path, response: &str) -> Result<Admitted> {
    store::require_current(root)?;
    store::with_lock(root, || confirm_locked(root, response))
}

pub(crate) fn confirm_locked(root: &Path, response: &str) -> Result<Admitted> {
    store::require_current(root)?;
    let code = crate::confirmation::authorize(
        response,
        |code| {
            store::challenge_path(root, code)
                .map(|path| path.exists())
                .unwrap_or(false)
        },
        |code| discard_locked(root, code),
    )?;

    let candidate = find(root, code)?;
    let id = candidate.object().to_owned();
    validate_persisted_payload(root, &candidate.payload)?;

    let object = match candidate_state(root, &candidate)? {
        CandidateState::AlreadyApplied(applied) => {
            let object = ops::reconcile_locked(root, &id)?;
            discard_locked(root, code)?;
            return Ok(Admitted {
                event: *applied,
                object,
            });
        }
        CandidateState::Stale { current_rev } => {
            crate::confirmation::classify_retry(
                &candidate.expected_rev(),
                &current_rev,
                false,
                "the object revision",
            )?;
            unreachable!("a stale challenge cannot be admitted")
        }
        // Repair cannot come through reconciliation. That path requires the
        // stored seal to verify, which is the condition repair exists to leave —
        // so it rebuilds the predecessor from admitted history instead, and the
        // invalid bytes on disk contribute nothing to what gets admitted.
        CandidateState::Pending
            if matches!(candidate.payload.action, Action::ObjectRepaired {}) =>
        {
            ops::provable(root, &id)?
        }
        CandidateState::Pending => match ops::reconcile_locked(root, &id) {
            Ok(object) => object,
            Err(error) if error.code == EXIT_NOT_FOUND => Object::new(id.clone(), String::new())?,
            Err(error) => return Err(error),
        },
    };
    crate::confirmation::classify_retry(
        &candidate.expected_rev(),
        &object.rev,
        false,
        "the object revision",
    )?;

    // Re-check references at the moment of admission, not only at prepare: a
    // target may have been revised while the human was reading.
    validate_refs(root, &candidate.payload, Admission::Human)?;

    // One clock read, for both facts. This is the moment the mutation was
    // admitted; the Section and the Event are two statements about that one
    // moment rather than two moments that happen to be close.
    let at = now();
    let event = Event::sealed(
        &id,
        crate::model::new_id(),
        admitted_at(candidate.payload.action.clone(), &at),
        object.rev + 1,
        // History keeps the structured facts and nothing else. The digest and
        // the agent's explanation lived in the Challenge, and the Challenge is
        // discarded the moment it is answered.
        human_admission(
            candidate.code(),
            &at,
            candidate
                .subject
                .review
                .as_ref()
                .map(FrozenReview::admitted),
        ),
    )?;

    let before = object;
    let object = crate::integrity::mutate(&before, |next| project(next, &event))?.object;

    // The Rule material is rebound from live state, so a Challenge whose review
    // no longer names the review its mutation would get is refused rather than
    // admitted against a policy that has since moved.
    let mutation = crate::proof::object_review_mutation(&before, &object, &candidate.payload)?;
    let live = crate::rules::bind_object(root, &mutation, candidate.expected_rev())?;
    match &candidate.subject.review {
        Some(review) => ensure!(
            live.digest()?.to_string() == review.digest,
            EXIT_INVARIANT,
            "the Rule Review material moved after challenge {} was prepared",
            candidate.code()
        ),
        None => ensure!(
            !live.has_rules(),
            EXIT_INVARIANT,
            "the applicable Rule set moved after challenge {} was prepared",
            candidate.code()
        ),
    }
    // After the projection, because the acyclic walk has to see the section this
    // Event is adding: the relation being admitted is part of the graph it must
    // not close.
    validate_relations(root, &candidate.payload, &object)?;
    store::append_event_locked(root, &id, &event)?;
    store::save_object(root, &object)?;
    discard_locked(root, code)?;
    Ok(Admitted { event, object })
}

/// Build the semantic content of a proposal, defaulting `based_on` to HEAD so a
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
        based_on: based_on.map(crate::semantics::BasedOn::new),
        refs,
        ..Content::default()
    })
}

/// The `admitted.at` a proposal carries while there is no admission instant.
///
/// A Challenge freezes *which* act, against *which* Object, at *which*
/// revision, through *which* door. It cannot freeze *when* the act was
/// admitted, because that has not happened yet — and reading the clock at
/// preparation would put a plausible false answer where the record expects a
/// true one, which is the shape of mistake nothing downstream can detect.
///
/// The member cannot simply be absent: the proposal's value is the same schema
/// the Event's `data` carries, and one schema rather than two that have to
/// agree is worth more than a nullable member. So it holds an instant no
/// admission can have. Two properties follow, and both are wanted. Preparing
/// the same mutation twice produces the same frozen bytes, because nothing in
/// the question is a clock read; and a value that ever reached a record still
/// wearing this would be visibly, unmistakably wrong rather than quietly
/// plausible.
pub const UNASSIGNED_ADMISSION: &str = "0001-01-01T00:00:00Z";

/// The Section value a proposal admits, with the admission path it will come
/// through.
///
/// `admitted.by` is frozen because the door is part of the question.
/// `admitted.at` is [`UNASSIGNED_ADMISSION`] until there is an admission:
/// pending rendering never presents it as provenance, and confirmation replaces
/// it with the one actual Section/Event admission instant. Migration preserves
/// predecessor Section instants separately.
pub fn value(content: Content, by: Admission) -> SectionValue {
    SectionValue::new(SectionAdmitted::new(by, UNASSIGNED_ADMISSION), content)
}
