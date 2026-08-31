//! Versioned digest scalars: `<contract-version>:<digest>`.
//!
//! A long-lived proof has to survive its own format changing. If a stored digest
//! says only what it is worth, a reader years later cannot tell which
//! calculation produced it — and the two failure modes are both silent: verify
//! it under today's rules and get a mismatch that looks like tampering, or
//! relabel it and claim a guarantee nobody made.
//!
//! So the version travels with the value. What this module owns is deliberately
//! only the **syntax**: parsing, canonical spelling, comparison. It is not a
//! registry of what any version *means*. Contract versions are **field-local**,
//! so `1` under [`REVIEW`] and `1` under [`CANDIDATE`] are unrelated contracts
//! that happen to share a number, and nothing here should tempt a reader into
//! treating them as one namespace.

use crate::{ensure, Error, Result, EXIT_SCHEMA};
use serde::Serialize;
use std::fmt;

/// The largest contract version a scalar may carry.
///
/// `uint32` rather than something wider, so no implementation needs
/// arbitrary-precision integers to parse a persisted value — a difference
/// between languages here would be a difference about whether stored data is
/// readable at all.
pub const MAX_CONTRACT_VERSION: u32 = u32::MAX;

/// One `<contract-version>:<digest>` value.
///
/// Ordering is by the **parsed** tuple, not the persisted string: `2:` precedes
/// `10:` because two is less than ten, which string comparison gets backwards.
/// That matters wherever these participate in a canonical set order.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Versioned {
    version: u32,
    digest: String,
}

impl Versioned {
    /// Build a value for `version` over already-lowercase hex.
    ///
    /// Refuses what [`Versioned::parse`] would refuse, so a value cannot enter
    /// the type by construction that could not have been read from storage.
    pub fn new(version: u32, digest: impl Into<String>) -> Result<Self> {
        let digest = digest.into();
        validate_version(version)?;
        validate_digest(&digest)?;
        Ok(Self { version, digest })
    }

    /// Read a persisted scalar.
    ///
    /// Strict on every axis the canonical form fixes, and deliberately silent
    /// about length: how long a digest should be belongs to the contract that
    /// owns the field, because a future contract may change hash algorithm
    /// without this codec changing at all.
    pub fn parse(value: &str) -> Result<Self> {
        let (version, digest) = value.split_once(':').ok_or_else(|| {
            Error::new(
                EXIT_SCHEMA,
                format!("{value:?} is not a versioned digest; the form is <version>:<digest>"),
            )
        })?;
        ensure!(
            !version.is_empty() && version.bytes().all(|byte| byte.is_ascii_digit()),
            EXIT_SCHEMA,
            "{value:?} has no decimal contract version before the colon"
        );
        // Zero first, and only then the leading-zero rule. Both refuse `0:`, but
        // they say different things, and the specific one is the useful one: a
        // reader who wrote `0:` meant "no version", and telling them about
        // canonical spelling would answer a question they did not ask.
        validate_version(if version == "0" { 0 } else { 1 })?;
        // Checked before parsing, because `01` and `1` parse to the same number
        // and would then be two spellings of one value — the thing canonical
        // form exists to prevent.
        ensure!(
            !version.starts_with('0'),
            EXIT_SCHEMA,
            "{value:?} has a leading zero in its contract version; the canonical spelling is [1-9][0-9]*"
        );
        let version: u32 = version.parse().map_err(|_| {
            Error::new(
                EXIT_SCHEMA,
                format!("{value:?} has a contract version outside 1..{MAX_CONTRACT_VERSION}"),
            )
        })?;
        validate_version(version)?;
        validate_digest(digest)?;
        Ok(Self {
            version,
            digest: digest.to_owned(),
        })
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }
}

/// Zero is not a version, and it is not "unversioned" either.
///
/// Reserving it costs nothing now and stops it becoming the escape hatch every
/// such scheme grows: a legacy value has a legacy compatibility path, and
/// synthesizing `0:` for it would claim the value was written under a contract
/// that never existed.
fn validate_version(version: u32) -> Result<()> {
    ensure!(
        version >= 1,
        EXIT_SCHEMA,
        "contract version 0 is permanently reserved and never means legacy or default"
    );
    Ok(())
}

fn validate_digest(digest: &str) -> Result<()> {
    ensure!(
        !digest.is_empty(),
        EXIT_SCHEMA,
        "a versioned digest has no digest after the colon"
    );
    // Uppercase is refused rather than folded. Silently normalizing would make
    // two spellings equal on read and different on disk, and every hash over the
    // enclosing structure would depend on which one happened to be stored.
    ensure!(
        digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        EXIT_SCHEMA,
        "{digest:?} is not lowercase hexadecimal; persisted digests have one spelling"
    );
    Ok(())
}

impl fmt::Display for Versioned {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.version, self.digest)
    }
}

impl Serialize for Versioned {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// What a build can do with one contract version.
///
/// Two dimensions, not one, because they retire at different times: a contract
/// stops being the thing new values are written under long before it stops
/// having to read the values already written under it. Collapsing them into a
/// boolean would force a choice between refusing to read history and emitting
/// under several contracts at once.
///
/// Setting `verify: false` is how an experimental contract is retired, and it is
/// legitimate — a version is only owed lasting verification once it has been
/// declared stable for durable use and emitted under that status. That
/// declaration is release policy and lives outside this type, because a
/// stability flag in the data would make the promise a property of the value
/// rather than of the release that made it. What is *never* allowed either way
/// is redefining what a number calculates: that always takes a new version.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Support {
    /// May new values be written under this version?
    pub emit: bool,
    /// Can values already written under it still be verified?
    pub verify: bool,
}

/// One field's digest contract family.
///
/// Field-local by construction: a family knows its own versions and nothing
/// about anyone else's, which is what keeps `1` under two families from becoming
/// an accidental shared meaning.
pub struct Family {
    /// Named in errors, so an unsupported version says which contract it is not
    /// supported by.
    pub name: &'static str,
    /// The one version new values are written under.
    ///
    /// Exactly one, because two concurrent emitters make the contract of a
    /// stored value depend on which code path produced it.
    pub current: u32,
    /// Every version this build understands, with what it may do.
    pub versions: &'static [(u32, Support, usize)],
}

impl Family {
    fn support(&self, version: u32) -> Option<(Support, usize)> {
        self.versions
            .iter()
            .find(|(candidate, _, _)| *candidate == version)
            .map(|(_, support, length)| (*support, *length))
    }

    /// Encode a digest under the current emission contract.
    pub fn emit(&self, digest: impl Into<String>) -> Result<Versioned> {
        let value = Versioned::new(self.current, digest)?;
        let (support, length) = self.support(self.current).ok_or_else(|| {
            Error::new(
                EXIT_SCHEMA,
                format!(
                    "{}: version {} is the emission target but is not a known version",
                    self.name, self.current
                ),
            )
        })?;
        ensure!(
            support.emit,
            EXIT_SCHEMA,
            "{}: version {} does not emit",
            self.name,
            self.current
        );
        self.check_length(&value, length)?;
        Ok(value)
    }

    /// Read a persisted value and confirm this build can verify it.
    ///
    /// The two failures are kept apart on purpose. A scalar whose *grammar* is
    /// wrong is malformed persisted data. A well-formed scalar naming a version
    /// this build does not know is **not** malformed — it is a readable value
    /// this build cannot check, and saying so is what lets an experimental
    /// contract be retired without its verifier having to live forever.
    pub fn verify(&self, value: &str) -> Result<Versioned> {
        let parsed = Versioned::parse(value)?;
        let (support, length) = self.support(parsed.version()).ok_or_else(|| {
            Error::new(
                EXIT_SCHEMA,
                format!(
                    "{}: unsupported contract version {}; the value is well formed but this build cannot verify it",
                    self.name,
                    parsed.version()
                ),
            )
        })?;
        ensure!(
            support.verify,
            EXIT_SCHEMA,
            "{}: unsupported contract version {}; this build no longer verifies it",
            self.name,
            parsed.version()
        );
        self.check_length(&parsed, length)?;
        Ok(parsed)
    }

    /// Check an attestation by recomputing **under the version it names**.
    ///
    /// This is the whole point of carrying the version, and it is easy to build
    /// the metadata without it: a family that only answers "is version 1 still
    /// supported" will happily accept a `1:` attestation and then compare it
    /// against a value recomputed under version 2. The two disagree by
    /// construction — if they did not, version 2 would not have been needed —
    /// so a valid historical proof gets reported as a changed subject, and the
    /// lifetime verification promise exists in the support table and nowhere
    /// else.
    ///
    /// `recompute` is handed the attested version and must answer with what
    /// *that* contract's calculation yields for the subject as it stands now.
    pub fn recheck(
        &self,
        attested: &str,
        recompute: impl FnOnce(u32) -> Result<String>,
    ) -> Result<Attestation> {
        let attested = self.verify(attested)?;
        let version = attested.version();
        let expected = Versioned::new(version, recompute(version)?)?;
        let (_, length) = self.support(version).ok_or_else(|| {
            Error::new(
                EXIT_SCHEMA,
                format!("{}: no contract for version {version}", self.name),
            )
        })?;
        self.check_length(&expected, length)?;
        Ok(Attestation { attested, expected })
    }

    fn check_length(&self, value: &Versioned, length: usize) -> Result<()> {
        ensure!(
            value.digest().len() == length,
            EXIT_SCHEMA,
            "{}: version {} carries a {}-character digest, not {}",
            self.name,
            value.version(),
            length,
            value.digest().len()
        );
        Ok(())
    }
}

/// What an attestation claimed, beside what it should have been.
///
/// Both are kept rather than reduced to a boolean, because the caller has to be
/// able to say what the correct value is — an agent told only "wrong" cannot
/// tell a moved subject from a mis-copied digest.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Attestation {
    pub attested: Versioned,
    pub expected: Versioned,
}

impl Attestation {
    pub fn agrees(&self) -> bool {
        self.attested == self.expected
    }
}

/// SHA-256 hex, the length every v1 contract here happens to use.
const SHA256_HEX: usize = 64;

/// The Rule Review proof. See `rules::ReviewBinding`.
pub const REVIEW: Family = Family {
    name: "ReviewDigestContract",
    current: 1,
    versions: &[(
        1,
        Support {
            emit: true,
            verify: true,
        },
        SHA256_HEX,
    )],
};

/// The Human Gate Challenge seal.
///
/// Covers `id + generator + created_at + subject` — the complete Challenge
/// except the value itself. Local-only: a Challenge lives under `.engr/local/`,
/// is spent at confirmation, and its digest is never copied into durable Event
/// provenance. What travels into history is the spent code, because what the
/// record needs to say is *which* question a human answered, not a hash of a
/// file that no longer exists.
pub const CHALLENGE: Family = Family {
    name: "ChallengeDigestContract",
    current: 1,
    versions: &[(
        1,
        Support {
            emit: true,
            verify: true,
        },
        SHA256_HEX,
    )],
};

/// The Object aggregate seal.
///
/// Over the complete canonical persisted Object except its own `digest`. Field
/// local like the rest: sharing the number 1 with SectionDigestContract says
/// nothing about sharing a calculation.
pub const OBJECT: Family = Family {
    name: "ObjectDigestContract",
    current: 1,
    versions: &[(
        1,
        Support {
            emit: true,
            verify: true,
        },
        SHA256_HEX,
    )],
};

/// The Section seal.
///
/// Over the complete canonical persisted Section except its own `digest`, so it
/// covers identity and admission provenance as well as semantics. A seal over
/// wording alone would let a Section be repointed at another id, or its
/// admission rewritten from agent to human, with the seal still verifying.
pub const SECTION: Family = Family {
    name: "SectionDigestContract",
    current: 1,
    versions: &[(
        1,
        Support {
            emit: true,
            verify: true,
        },
        SHA256_HEX,
    )],
};

/// The durable Event seal.
///
/// Version 1 binds the owning Object UUID *beside* the Event rather than
/// hashing the Event alone, so a well-formed Event cannot be moved into another
/// Object's stream and keep verifying. Filesystem layout is a locator; the
/// digest is what says which stream an Event belongs to.
pub const EVENT: Family = Family {
    name: "EventDigestContract",
    current: 1,
    versions: &[(
        1,
        Support {
            emit: true,
            verify: true,
        },
        SHA256_HEX,
    )],
};

/// The selective semantic dependency snapshot a Section Ref pins.
///
/// Field-local, like the others: this version namespace is `refs[].digest`'s
/// alone and shares nothing with ChallengeDigestContract or
/// ReviewDigestContract. Version 1 is **undomained** — it hashes its canonical
/// snapshot bytes with no kind or version prefix mixed in — so a later contract
/// may add domain separation without redefining how a v1 value verifies.
pub const REF: Family = Family {
    name: "RefDigestContract",
    current: 1,
    versions: &[(
        1,
        Support {
            emit: true,
            verify: true,
        },
        SHA256_HEX,
    )],
};
