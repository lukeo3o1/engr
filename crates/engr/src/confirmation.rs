//! The Human Gate Challenge: the domain-neutral half of asking a person.
//!
//! A Challenge is short-lived local state. It lives under `.engr/local/`, it is
//! never committed, and a successful confirmation removes it. Its whole job is
//! to carry one exact question from the moment it is asked to the moment it is
//! answered, unchanged.
//!
//! **The common layer does not understand domains.** `subject.type` selects a
//! broad family and `subject.data` belongs to that family — so adding a family
//! later (a ChangeSet, say) changes nothing here. That separation is what keeps
//! this file from slowly becoming a second copy of the Object action schemas.
//!
//! **The digest is local integrity, not provenance.** It covers the whole
//! Challenge except itself, so a file rewritten on disk cannot present or admit
//! a different question and still check out. It is deliberately *not* copied
//! into the durable Event: by the time anyone reads that Event the Challenge is
//! gone, so a hash of it would be a proof of something unverifiable. What the
//! record keeps is the spent code — which question was answered.
//!
//! This is not a boundary against someone who controls the machine: the file it
//! protects is on that machine, and so is this binary.

use rand::Rng;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::sync::OnceLock;

use crate::{ensure, Error, Result, EXIT_SCHEMA, EXIT_STALE, EXIT_USAGE};

const ALPHABET: &[u8] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";
pub const CHALLENGE_LEN: usize = 6;

/// The families a `subject.type` may name.
///
/// Closed, because an unknown family is a Challenge this build cannot interpret
/// and must not act on. ChangeSet is the family expected to arrive next, and it
/// is deliberately not here yet — a value reserved before its shape is frozen is
/// a value two builds can disagree about.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SubjectType {
    Object,
    Migration,
}

impl SubjectType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Object => "object",
            Self::Migration => "migration",
        }
    }
}

/// Which build produced this Challenge, and whether this one can read it.
///
/// The two members answer different questions and neither substitutes for the
/// other. `version` is for a person reading the file. `fingerprint` is the
/// opaque compatibility identity: it is derived from the Challenge contract this
/// build implements, so a build whose contract differs produces a different
/// value and refuses the pending Challenge rather than interpreting it under
/// rules it was not written against.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Generator {
    pub version: String,
    pub fingerprint: String,
}

impl Generator {
    pub fn current() -> Result<Self> {
        Ok(Self {
            version: crate::IMPLEMENTATION_VERSION.to_owned(),
            fingerprint: fingerprint()?.to_owned(),
        })
    }
}

/// Everything about this build that decides how a pending Challenge is read.
///
/// Deliberately a *declaration* rather than a hash of the binary. It names the
/// subject schemas and the confirmation semantics that can change what a
/// pending question means; unrelated implementation changes stay compatible.
#[derive(Serialize, Clone)]
struct Contract {
    challenge: &'static str,
    subject_envelope: &'static str,
    object: ObjectContract,
    migration: MigrationContract,
    digest_contract: u32,
    alphabet: &'static str,
    length: usize,
    response: &'static str,
}

#[derive(Serialize, Clone)]
struct ObjectContract {
    family: &'static str,
    subject: &'static str,
    commands: Vec<&'static str>,
    action_data: Vec<&'static str>,
    section_value: &'static str,
    vocabularies: &'static str,
    frozen_review: &'static str,
    confirmation: &'static str,
    /// The two digest contracts whose scalars a frozen Object subject carries
    /// literally: a Ref's own digest inside the Section value, and the
    /// ReviewDigest inside the frozen review.
    ///
    /// Neither is the Challenge's own digest, and both decide what the frozen
    /// bytes *mean*. A pending Challenge holding a `1:` Ref digest under an
    /// implementation that has moved to contract 2 is not a Challenge this build
    /// can interpret, so it is one this build must decline to interpret.
    ref_digest_contract: u32,
    review_digest_contract: u32,
}

#[derive(Serialize, Clone)]
struct MigrationContract {
    family: &'static str,
    subject: &'static str,
    planned_object: &'static str,
    interpretation: &'static str,
    confirmation: &'static str,
}

/// The command vocabulary an Object subject may name.
///
/// Here rather than in `model`, because it is part of what the generator
/// fingerprint covers: adding a command changes what a pending Challenge can
/// mean, so it must change the compatibility identity.
pub const OBJECT_COMMANDS: &[&str] = &[
    "create",
    "rename",
    "classify",
    "change_state",
    "supersede",
    "repair",
    "section.create",
    "section.update",
    "section.delete",
    "section.merge",
];

fn contract() -> Contract {
    Contract {
        challenge: "Challenge{id,generator:{version,fingerprint},created_at,subject,digest}; deny unknown members; id is six characters from the declared alphabet; created_at is RFC3339; digest is over id+generator+created_at+subject; a foreign fingerprint is re-prepared, never reinterpreted",
        subject_envelope: "Subject{type,data}; deny unknown members; type is the closed object|migration vocabulary; data is interpreted only by that family",
        object: ObjectContract {
            family: SubjectType::Object.as_str(),
            subject: "ObjectSubject{action,object,expected_rev,value,review?}; deny unknown members; action+value reconstruct exactly one Action; object is canonical UUIDv7; expected_rev is the stale/replay precondition",
            commands: OBJECT_COMMANDS.to_vec(),
            action_data: vec![
                "create:{title}",
                "rename:{title,becomes?}",
                "classify:{type?,state}",
                "change_state:{state}",
                "supersede:{value:SectionValue}",
                "repair:{}",
                "section.create:{value:SectionValue,becomes?}",
                "section.update:{section,value:SectionValue,becomes?}",
                "section.delete:{section,becomes?}",
                "section.merge:{destination,sources,value:SectionValue,becomes?}",
            ],
            section_value: "SectionValue{admitted:{by,at},header?,role?,text,content?,based_on?,refs?,relations?}; optional empty members are omitted; content entries are ordered {type,body}; based_on is {commit}; refs are canonical sets of {target,fields,commit,digest} with compact target identity; relations are canonical sets of {type,target}; admitted.by is human|agent and is frozen; admitted.at is RFC3339 and is the unassigned placeholder 0001-01-01T00:00:00Z until confirmation stamps the actual instant",
            vocabularies: "type/state is null:{open|closed}, design:{draft|proposed|accepted|rejected|superseded}, decision:{proposed|accepted|rejected|superseded}, or risk:{identified|accepted|mitigated|invalidated}; role is decision|risk|supersession|acceptance_criterion; destination is {type?,state}; relation and Ref vocabularies are the workspace-generation-1 schemas; section ids and all JSON integers are positive/shared-safe where applicable",
            frozen_review: "FrozenReview{digest,result,attempts,rules,explanation?}; deny unknown members; digest binds the exact mutation and Rule artifacts; result is passed|failed|exhausted; attempts is positive; rules is the exact canonical Rule-id set rendered to the human; explanation is decision-time material; live rebind must match before confirmation",
            confirmation: "Human confirmation revalidates the frozen subject and review, stamps one actual confirmation instant into Event.metadata.admitted.at and every ordinary changed Section.admitted.at, records review {outcome,result,attempts} without digest/explanation, then applies exactly once; Agent admission has no Challenge and requires a live passing review",
            ref_digest_contract: crate::digest::REF.current,
            review_digest_contract: crate::digest::REVIEW.current,
        },
        migration: MigrationContract {
            family: SubjectType::Migration.as_str(),
            subject: "MigrationSubject{from,to,objects,source}; deny unknown members; objects is canonical by object identity; source maps each predecessor .engr-relative path to the digest of its exact bytes",
            planned_object: "PlannedObject{object,title,sections,predecessor_rev,digest}; object is canonical UUIDv7; sections and predecessor_rev are safe integers; digest is the exact destination Object digest",
            interpretation: "the sole released predecessor format is validated and deterministically converted to workspace generation 1; predecessor history is discarded; one object.migrated.v1 bootstrap Event recreates each destination Object; migrated Sections preserve predecessor admission provenance",
            confirmation: "Human confirmation rederives and matches the frozen plan, then stamps one actual migration confirmation/apply instant into every migration Event; no destination containing final Event admission provenance exists before that confirmation; an intact post-confirm destination may only resume forward for the same Challenge",
        },
        digest_contract: crate::digest::CHALLENGE.current,
        alphabet: std::str::from_utf8(ALPHABET).expect("the alphabet is ASCII"),
        length: CHALLENGE_LEN,
        response: "CONFIRM <code>",
    }
}

fn fingerprint_of(contract: &Contract) -> Result<String> {
    let digest = crate::digest::CHALLENGE
        .emit(crate::proof::sha256_of(&crate::proof::canonical_bytes(
            contract,
            "challenge generator",
        )?))?
        .to_string();
    Ok(digest)
}

fn fingerprint() -> Result<&'static str> {
    static VALUE: OnceLock<String> = OnceLock::new();
    if let Some(value) = VALUE.get() {
        return Ok(value);
    }
    let digest = fingerprint_of(&contract())?;
    Ok(VALUE.get_or_init(|| digest))
}

/// The complete immutable value whose exact confirmation is requested.
///
/// Not merely a resource target. A Challenge that named `obj:…` and an action
/// would be asking a person to assent to a category of change; what a gate has
/// to be able to say afterwards is that *this exact value* was the one somebody
/// read.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Subject {
    #[serde(rename = "type")]
    pub kind: SubjectType,
    /// Owned by the family. Untyped here on purpose: the common layer that
    /// starts interpreting one family's payload is the common layer that ends
    /// up owning every family's schema.
    pub data: serde_json::Value,
}

/// One pending Human Gate question.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Challenge {
    pub id: String,
    pub generator: Generator,
    pub created_at: String,
    pub subject: Subject,
    pub digest: String,
}

impl Challenge {
    /// Mint a Challenge for `subject`, avoiding every code already on disk.
    pub fn mint(subject: Subject, taken: &[String], created_at: String) -> Result<Self> {
        let mut challenge = Self {
            id: mint(taken),
            generator: Generator::current()?,
            created_at,
            subject,
            digest: String::new(),
        };
        challenge.digest = challenge.recomputed_digest()?;
        Ok(challenge)
    }

    pub fn recomputed_digest(&self) -> Result<String> {
        crate::digest::CHALLENGE
            .emit(self.digest_under(crate::digest::CHALLENGE.current)?)
            .map(|value| value.to_string())
    }

    /// `id + generator + created_at + subject`, and nothing else.
    pub fn digest_under(&self, version: u32) -> Result<String> {
        match version {
            1 => {
                let mut value = serde_json::to_value(self).map_err(|error| {
                    Error::new(EXIT_SCHEMA, format!("canonical challenge: {error}"))
                })?;
                if let serde_json::Value::Object(members) = &mut value {
                    members.remove("digest");
                }
                Ok(crate::proof::sha256_of(&crate::proof::canonical_bytes(
                    &value,
                    "challenge",
                )?))
            }
            other => Err(Error::new(
                EXIT_SCHEMA,
                format!("ChallengeDigestContract: no contract for version {other}"),
            )),
        }
    }

    /// Everything decidable from the Challenge's own bytes.
    ///
    /// The generator check comes first and is the one that fails closed on a
    /// contract this build does not implement: pending Challenge compatibility
    /// is not migrated across incompatible generators, and the answer is to
    /// prepare again rather than to guess at what the file meant.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            valid_challenge(&self.id),
            EXIT_SCHEMA,
            "challenge {:?} is not six characters from {}",
            self.id,
            std::str::from_utf8(ALPHABET).expect("the alphabet is ASCII")
        );
        ensure!(
            self.generator.fingerprint == fingerprint()?,
            EXIT_SCHEMA,
            "challenge {} was prepared by a generator this build cannot interpret ({}); prepare it again",
            self.id,
            self.generator.version
        );
        ensure!(
            time::OffsetDateTime::parse(
                &self.created_at,
                &time::format_description::well_known::Rfc3339
            )
            .is_ok(),
            EXIT_SCHEMA,
            "challenge {} has a created_at that is not RFC3339",
            self.id
        );
        let attested =
            crate::digest::CHALLENGE.recheck(&self.digest, |version| self.digest_under(version))?;
        ensure!(
            attested.agrees(),
            EXIT_SCHEMA,
            "challenge {} does not match its own digest, so what it presents is not what was prepared",
            self.id
        );
        Ok(())
    }
}

pub fn mint(taken: &[String]) -> String {
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

pub enum Response<'a> {
    Exact(&'a str),
    Qualified(&'a str),
    Invalid,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Application {
    Apply,
    AlreadyApplied,
}

pub fn response(input: &str) -> Response<'_> {
    let mut words = input.split(' ');
    let head = words.next().unwrap_or_default();
    let code = words.next().unwrap_or_default();
    if head == "CONFIRM" && words.next().is_none() && valid_challenge(code) {
        return Response::Exact(code);
    }
    if let Some(rest) = input.strip_prefix("CONFIRM ") {
        let mut words = rest.split_whitespace();
        if let Some(code) = words.next() {
            if words.next().is_some() && valid_challenge(code) {
                return Response::Qualified(code);
            }
        }
    }
    Response::Invalid
}

/// Enforce the response and qualified-assent rules. Challenge lookup and
/// disposal stay storage-specific closures, while every gated domain gets the
/// same admit-or-discard behavior and diagnostics.
pub fn authorize(
    input: &str,
    challenge_exists: impl FnOnce(&str) -> bool,
    discard: impl FnOnce(&str) -> Result<()>,
) -> Result<&str> {
    match response(input) {
        Response::Exact(code) => Ok(code),
        Response::Qualified(code) if challenge_exists(code) => {
            discard(code)?;
            Err(Error::new(
                EXIT_USAGE,
                format!("`CONFIRM {code}` carried commentary, so it is a qualified yes rather than assent; challenge {code} was discarded"),
            ))
        }
        Response::Qualified(_) | Response::Invalid => Err(Error::new(
            EXIT_USAGE,
            "the response must be exactly `CONFIRM <code>`, with nothing else on the line",
        )),
    }
}

/// The only spelling that may name a Challenge on disk. Keeping the grammar
/// here makes a qualified response unable to turn arbitrary path text into a
/// storage lookup before the domain gets a chance to reject it.
pub fn valid_challenge(code: &str) -> bool {
    code.len() == CHALLENGE_LEN && code.bytes().all(|byte| ALPHABET.contains(&byte))
}

/// Classify retry before applying. A domain supplies its own binding comparison
/// and durable-history lookup; the shared gate owns the never-apply-twice rule.
pub fn classify_retry<B: Eq + Display>(
    expected: &B,
    current: &B,
    already_applied: bool,
    authority: &str,
) -> Result<Application> {
    if already_applied {
        return Ok(Application::AlreadyApplied);
    }
    ensure_fresh(expected, current, authority)?;
    Ok(Application::Apply)
}

pub fn ensure_fresh<T: Eq + Display>(expected: &T, current: &T, authority: &str) -> Result<()> {
    ensure!(
        expected == current,
        EXIT_STALE,
        "{authority} moved to {current} after this challenge was prepared at {expected}; prepare it again"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subject() -> Subject {
        Subject {
            kind: SubjectType::Object,
            data: serde_json::json!({
                "action": "section.update",
                "object": "01a02f75-d750-73c1-8d03-32aa3b1a9fa5",
                "expected_rev": 7,
                "value": {}
            }),
        }
    }

    /// The digest covers the whole envelope except itself, so a file rewritten
    /// on disk cannot present a different question and still check out.
    #[test]
    fn a_rewritten_challenge_no_longer_matches_its_own_digest() {
        let challenge =
            Challenge::mint(subject(), &[], "2026-08-31T00:00:00Z".to_owned()).expect("mint");
        challenge.validate().expect("freshly minted");

        let mut moved = challenge.clone();
        moved.subject.data["expected_rev"] = serde_json::json!(8);
        assert!(
            moved.validate().is_err(),
            "the subject is inside the digest"
        );

        let mut renamed = challenge.clone();
        renamed.id = "ABC234".to_owned();
        assert!(renamed.validate().is_err(), "the code is inside the digest");

        let mut later = challenge.clone();
        later.created_at = "2026-09-01T00:00:00Z".to_owned();
        assert!(
            later.validate().is_err(),
            "the instant is inside the digest"
        );
    }

    /// A Challenge minted by a generator whose contract differs is refused with
    /// an answer — prepare it again — rather than interpreted under this
    /// build's rules.
    #[test]
    fn an_unreadable_generator_fails_closed() {
        let mut challenge =
            Challenge::mint(subject(), &[], "2026-08-31T00:00:00Z".to_owned()).expect("mint");
        challenge.generator.fingerprint = format!("1:{}", "a".repeat(64));
        challenge.digest = challenge.recomputed_digest().expect("reseal");
        let error = challenge
            .validate()
            .expect_err("a foreign generator is not interpretable");
        assert_eq!(error.code, EXIT_SCHEMA);
        assert!(error.message.contains("prepare it again"), "{error}");
    }

    /// The fingerprint is a declaration of the contract, so it is stable across
    /// runs of the same build.
    #[test]
    fn the_generator_fingerprint_is_stable_within_a_build() {
        assert_eq!(fingerprint().expect("first"), fingerprint().expect("again"));
    }

    /// The value is pinned, and the pin is the point.
    ///
    /// A fingerprint that changes is a statement that pending Challenges minted
    /// by the previous build can no longer be interpreted, and every one of them
    /// has to be prepared again. That is the correct outcome whenever the
    /// declaration below genuinely changed meaning, and a bad one when a member
    /// was reworded for taste. Neither can be told apart from the diff, so the
    /// value is written down: changing it is a deliberate act with a reviewer,
    /// rather than a side effect nobody noticed.
    #[test]
    fn the_generator_fingerprint_is_the_value_this_build_publishes() {
        assert_eq!(
            fingerprint().expect("value"),
            "1:e9d22f142ab3b88082095bb47e6e5a9213254fe79a06c496d43cfb52e78339e2"
        );
    }

    /// Compatibility follows interpretation, not just the family and command
    /// names.
    ///
    /// This is the finding the earlier fingerprint failed: `FrozenReview`
    /// changed incompatibly — `attempt`/`outcome` became `attempts`/`result`,
    /// and what history keeps changed with it — while the families, the command
    /// vocabulary, the digest contract, the alphabet and the response phrase all
    /// stayed byte-identical. A pending Challenge from the old build therefore
    /// carried a fingerprint the new build accepted as its own. Every member
    /// that can change what a frozen question *means* is now an input, so the
    /// same class of change invalidates the pending question it would have
    /// silently misread.
    #[test]
    fn the_generator_fingerprint_binds_each_subject_contract() {
        let current = contract();
        let baseline = fingerprint_of(&current).expect("baseline");

        // The exact shape of the earlier miss: a frozen-review schema change
        // under an unchanged command vocabulary.
        let mut object_changed = current.clone();
        object_changed.object.frozen_review =
            "FrozenReview{digest,outcome,attempt,rules,explanation?}; the superseded spelling";
        assert_ne!(
            fingerprint_of(&object_changed).expect("frozen review"),
            baseline
        );

        // A pure semantics change, where no schema moves at all: same members,
        // different meaning at admission.
        let mut semantics_changed = current.clone();
        semantics_changed.object.confirmation =
            "Human confirmation stamps the preparation instant rather than the admission instant";
        assert_ne!(
            fingerprint_of(&semantics_changed).expect("admission mapping"),
            baseline
        );

        // The value schema the human is actually deciding about.
        let mut value_changed = current.clone();
        value_changed.object.section_value =
            "SectionValue with a different admitted placeholder convention";
        assert_ne!(
            fingerprint_of(&value_changed).expect("section value"),
            baseline
        );

        // The digest contracts whose scalars the frozen subject carries
        // literally. These are numbers rather than prose, and they move on their
        // own schedule, so they are checked separately from the wording.
        let mut ref_contract_changed = current.clone();
        ref_contract_changed.object.ref_digest_contract += 1;
        assert_ne!(
            fingerprint_of(&ref_contract_changed).expect("ref digest contract"),
            baseline
        );
        let mut review_contract_changed = current.clone();
        review_contract_changed.object.review_digest_contract += 1;
        assert_ne!(
            fingerprint_of(&review_contract_changed).expect("review digest contract"),
            baseline
        );

        // And the other family, which has its own frozen subject and its own
        // apply-time interpretation.
        let mut migration_changed = current;
        migration_changed.migration.confirmation =
            "a different apply-time interpretation with the same migration family name";
        assert_ne!(
            fingerprint_of(&migration_changed).expect("migration contract"),
            baseline
        );
    }
}
