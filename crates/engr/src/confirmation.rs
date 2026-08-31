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
/// Deliberately a *declaration* rather than a hash of the binary. Two builds
/// that differ only in an unrelated bug fix must be able to admit each other's
/// pending Challenges; two that differ in the vocabulary, the digest contract,
/// the code alphabet or the response phrase must not. So the fingerprint is
/// taken over exactly those, and extending any of them changes it.
#[derive(Serialize)]
struct Contract {
    families: Vec<&'static str>,
    object_commands: Vec<&'static str>,
    digest_contract: u32,
    alphabet: &'static str,
    length: usize,
    response: &'static str,
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

fn fingerprint() -> Result<&'static str> {
    static VALUE: OnceLock<String> = OnceLock::new();
    if let Some(value) = VALUE.get() {
        return Ok(value);
    }
    let contract = Contract {
        families: vec![
            SubjectType::Object.as_str(),
            SubjectType::Migration.as_str(),
        ],
        object_commands: OBJECT_COMMANDS.to_vec(),
        digest_contract: crate::digest::CHALLENGE.current,
        alphabet: std::str::from_utf8(ALPHABET).expect("the alphabet is ASCII"),
        length: CHALLENGE_LEN,
        response: "CONFIRM <code>",
    };
    let digest = crate::digest::CHALLENGE
        .emit(crate::proof::sha256_of(&crate::proof::canonical_bytes(
            &contract,
            "challenge generator",
        )?))?
        .to_string();
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
        assert!(fingerprint().expect("value").starts_with("1:"));
    }
}
