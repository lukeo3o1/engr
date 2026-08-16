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
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Candidate<M, B> {
    pub challenge: String,
    pub created_at: String,
    #[serde(flatten)]
    pub binding: B,
    #[serde(flatten)]
    pub payload: M,
    pub payload_sha256: String,
}

impl<M: Serialize, B> Candidate<M, B> {
    pub fn prepare(payload: M, binding: B, taken: &[String], created_at: String) -> Result<Self> {
        Self::prepare_with(payload, binding, taken, created_at, fingerprint)
    }

    pub fn prepare_with(
        payload: M,
        binding: B,
        taken: &[String],
        created_at: String,
        fingerprint: impl FnOnce(&M) -> Result<String>,
    ) -> Result<Self> {
        let payload_sha256 = fingerprint(&payload)?;
        Ok(Self {
            challenge: mint(taken),
            created_at,
            binding,
            payload,
            payload_sha256,
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

    #[test]
    fn another_domain_can_prepare_verify_and_admit_without_an_object_gate() {
        let candidate = Candidate::prepare(
            AliasMutation {
                alias: "auth",
                target: "obj:abc",
            },
            AliasBinding {
                resolver_fingerprint: "before".to_owned(),
            },
            &[],
            "2026-08-16T00:00:00Z".to_owned(),
        )
        .expect("prepare alias mutation");
        candidate.verify_payload().expect("bound payload");
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
