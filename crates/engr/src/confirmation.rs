//! Mechanical confirmation rules shared by authority-sensitive domains.

use rand::Rng;
use std::fmt::Display;

use crate::{ensure, Result, EXIT_STALE};

const ALPHABET: &[u8] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";
pub const CHALLENGE_LEN: usize = 6;

#[derive(Debug, PartialEq, Eq)]
pub enum Response<'a> {
    Exact(&'a str),
    Qualified(&'a str),
    Invalid,
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
            if words.next().is_some() {
                return Response::Qualified(code);
            }
        }
    }
    Response::Invalid
}

fn valid_challenge(code: &str) -> bool {
    code.len() == CHALLENGE_LEN && code.bytes().all(|byte| ALPHABET.contains(&byte))
}

/// Refuse a prepared mutation when the authority it was reviewed against has
/// moved. Domains choose their own revision or fingerprint type; the gate only
/// owns the compare-before-apply rule.
pub fn ensure_fresh<T: Eq + Display>(expected: &T, current: &T, authority: &str) -> Result<()> {
    ensure!(
        expected == current,
        EXIT_STALE,
        "{authority} moved to {current} after this candidate was prepared at {expected}; prepare it again"
    );
    Ok(())
}
