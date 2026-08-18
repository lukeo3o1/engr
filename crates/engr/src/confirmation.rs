//! Mechanical confirmation rules shared by authority-sensitive domains.

use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Display;

use crate::{ensure, Error, Result, EXIT_SCHEMA, EXIT_STALE, EXIT_USAGE};

const ALPHABET: &[u8] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";
pub const CHALLENGE_LEN: usize = 6;

/// The persisted, domain-neutral part of a prepared mutation. Domains provide
/// their own binding fields and mutation vocabulary; flattening keeps their
/// storage envelope independent without recreating the admission protocol.
///
/// Two fingerprints, because they answer different questions. `payload_sha256`
/// is the mutation's own identity: it travels into the confirmed Event and is
/// what an already-applied retry matches against, so its input may never widen.
/// `integrity_sha256` covers that value together with the challenge, the
/// prepared binding, and whatever context the domain stored — the state that
/// decides what the human is shown and which mutation their answer admits.
/// Without it those fields sit outside every check while still steering the
/// confirmation.
///
/// The challenge is in there because it is the *link* between the two. A
/// candidate that renders one mutation while naming another candidate's code
/// tells a human to type an answer to a question they were never shown, and
/// every other check passes: both files are internally consistent.
///
/// This is not a boundary against someone who controls the machine: the file it
/// protects is on that machine, and so is this binary. It is the narrower
/// guarantee that a candidate rewritten on disk cannot present or bind a
/// materially different confirmation context and still pass its own checks.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Candidate<M, B> {
    pub challenge: String,
    pub created_at: String,
    #[serde(flatten)]
    pub binding: B,
    #[serde(flatten)]
    pub payload: M,
    pub payload_sha256: String,
    /// Defaulted on load rather than required, so an envelope that predates
    /// this value fails the integrity check with an answer — prepare it again —
    /// instead of a deserializer complaining about a missing field. The empty
    /// string is never a digest, so absence can only fail.
    #[serde(default)]
    pub integrity_sha256: String,
}

/// The exact input to `integrity_sha256`. A named struct rather than an ad-hoc
/// map, so the covered set is one declaration a reader can check against.
#[derive(Serialize)]
struct Integrity<'a, B, C> {
    challenge: &'a str,
    payload_sha256: &'a str,
    binding: &'a B,
    context: &'a C,
}

impl<M: Serialize, B: Serialize> Candidate<M, B> {
    pub fn prepare(
        payload: M,
        binding: B,
        context: &impl Serialize,
        taken: &[String],
        created_at: String,
    ) -> Result<Self> {
        Self::prepare_with(payload, binding, context, taken, created_at, fingerprint)
    }

    pub fn prepare_with(
        payload: M,
        binding: B,
        context: &impl Serialize,
        taken: &[String],
        created_at: String,
        fingerprint: impl FnOnce(&M) -> Result<String>,
    ) -> Result<Self> {
        let payload_sha256 = fingerprint(&payload)?;
        let challenge = mint(taken);
        let integrity_sha256 = integrity(&challenge, &payload_sha256, &binding, context)?;
        Ok(Self {
            challenge,
            created_at,
            binding,
            payload,
            payload_sha256,
            integrity_sha256,
        })
    }

    pub fn verify_payload(&self) -> Result<()> {
        self.verify_payload_with(fingerprint)
    }

    pub fn verify_payload_with(
        &self,
        fingerprint: impl FnOnce(&M) -> Result<String>,
    ) -> Result<()> {
        ensure!(
            self.payload_sha256 == fingerprint(&self.payload)?,
            EXIT_SCHEMA,
            "candidate {} does not match its own hash",
            self.challenge
        );
        Ok(())
    }

    /// Check the binding and the domain's stored context against the value
    /// minted at prepare. Call it wherever a candidate is loaded — rendering it
    /// again is as much a use of the prepared context as admitting it is.
    pub fn verify_integrity(&self, context: &impl Serialize) -> Result<()> {
        ensure!(
            self.integrity_sha256
                == integrity(
                    &self.challenge,
                    &self.payload_sha256,
                    &self.binding,
                    context
                )?,
            EXIT_SCHEMA,
            "candidate {} does not match its own integrity hash; prepare it again",
            self.challenge
        );
        Ok(())
    }
}

pub fn integrity(
    challenge: &str,
    payload_sha256: &str,
    binding: &impl Serialize,
    context: &impl Serialize,
) -> Result<String> {
    fingerprint(&Integrity {
        challenge,
        payload_sha256,
        binding,
        context,
    })
}

#[derive(Debug, PartialEq, Eq)]
pub enum Response<'a> {
    Exact(&'a str),
    Qualified(&'a str),
    Invalid,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Admission {
    Apply,
    AlreadyApplied,
}

pub fn fingerprint<T: Serialize>(value: &T) -> Result<String> {
    let value = serde_json::to_value(value)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("canonical form: {error}")))?;
    let canonical = serde_json::to_string(&value)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("canonical form: {error}")))?;
    Ok(format!("{:x}", Sha256::digest(canonical.as_bytes())))
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

/// Enforce the response and qualified-assent rules. Candidate lookup and
/// disposal stay storage-specific closures, while every gated domain gets the
/// same admit-or-discard behavior and diagnostics.
pub fn authorize(
    input: &str,
    candidate_exists: impl FnOnce(&str) -> bool,
    discard: impl FnOnce(&str) -> Result<()>,
) -> Result<&str> {
    match response(input) {
        Response::Exact(code) => Ok(code),
        Response::Qualified(code) if candidate_exists(code) => {
            discard(code)?;
            Err(Error::new(
                EXIT_USAGE,
                format!("`CONFIRM {code}` carried commentary, so it is a qualified yes rather than assent; candidate {code} was discarded"),
            ))
        }
        Response::Qualified(_) | Response::Invalid => Err(Error::new(
            EXIT_USAGE,
            "the response must be exactly `CONFIRM <code>`, with nothing else on the line",
        )),
    }
}

/// The only spelling that may name a candidate on disk. Keeping the grammar
/// here makes a qualified response unable to turn arbitrary path text into a
/// storage lookup before the domain gets a chance to reject it.
pub fn valid_challenge(code: &str) -> bool {
    code.len() == CHALLENGE_LEN && code.bytes().all(|byte| ALPHABET.contains(&byte))
}

/// Classify retry before applying. A domain supplies its own binding comparison
/// and durable-history lookup; the shared gate owns the never-apply-twice rule.
pub fn admission<B: Eq + Display>(
    expected: &B,
    current: &B,
    already_applied: bool,
    authority: &str,
) -> Result<Admission> {
    if already_applied {
        return Ok(Admission::AlreadyApplied);
    }
    ensure_fresh(expected, current, authority)?;
    Ok(Admission::Apply)
}

pub fn ensure_fresh<T: Eq + Display>(expected: &T, current: &T, authority: &str) -> Result<()> {
    ensure!(
        expected == current,
        EXIT_STALE,
        "{authority} moved to {current} after this candidate was prepared at {expected}; prepare it again"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct AliasMutation<'a> {
        alias: &'a str,
        target: &'a str,
    }

    #[derive(Serialize, Deserialize, Clone, Debug)]
    struct AliasBinding {
        resolver_fingerprint: String,
    }

    #[derive(Serialize, Clone, Debug)]
    struct AliasContext {
        previous_target: String,
    }

    #[test]
    fn another_domain_can_prepare_verify_and_admit_without_an_object_gate() {
        let mut context = AliasContext {
            previous_target: "obj:old".to_owned(),
        };
        let candidate = Candidate::prepare(
            AliasMutation {
                alias: "auth",
                target: "obj:abc",
            },
            AliasBinding {
                resolver_fingerprint: "before".to_owned(),
            },
            &context,
            &[],
            "2026-08-16T00:00:00Z".to_owned(),
        )
        .expect("prepare alias mutation");
        candidate.verify_payload().expect("bound payload");
        candidate
            .verify_integrity(&context)
            .expect("untouched prepared context");
        context.previous_target = "obj:something-else".to_owned();
        assert!(
            candidate.verify_integrity(&context).is_err(),
            "presentation context is covered by candidate integrity"
        );
        assert_eq!(
            admission(
                &candidate.binding.resolver_fingerprint,
                &"before".to_owned(),
                false,
                "alias registry"
            )
            .expect("fresh"),
            Admission::Apply
        );
        assert_eq!(
            admission(
                &candidate.binding.resolver_fingerprint,
                &"after".to_owned(),
                true,
                "alias registry"
            )
            .expect("retry"),
            Admission::AlreadyApplied
        );
    }
}
