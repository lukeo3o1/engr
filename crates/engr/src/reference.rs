//! Canonical syntax for engr resource references.

use crate::{ensure, Error, Result, EXIT_SCHEMA};
use serde::{Deserialize, Serialize};

const CROCKFORD: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceKind {
    Object,
    Backlog,
    Collection,
}

impl ResourceKind {
    fn token(self) -> &'static str {
        match self {
            Self::Object => "obj",
            Self::Backlog => "backlog",
            Self::Collection => "collection",
        }
    }
}

/// Parsed reference input. Its invariant-bearing state cannot be constructed or
/// mutated directly by callers.
///
/// ```compile_fail
/// use engr::reference::{EngrRef, ResourceKind};
///
/// let _ = EngrRef {
///     kind: ResourceKind::Object,
///     id: "not-a-validated-id".to_owned(),
///     section: None,
///     snapshot_selector: None,
/// };
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngrRef {
    kind: ResourceKind,
    id: String,
    section: Option<u64>,
    /// Input selector only. It may be abbreviated or symbolic and must be
    /// resolved before a canonical reference can be emitted.
    snapshot_selector: Option<String>,
}

/// A reference that has passed canonicalization.
///
/// All invariant-bearing fields are private. A canonical reference can only be
/// created by parsing validated input and resolving any snapshot selector.
///
/// ```compile_fail
/// use engr::reference::{CanonicalEngrRef, ResourceKind};
///
/// let _ = CanonicalEngrRef {
///     kind: ResourceKind::Object,
///     id: "not-a-compact-uuid".to_owned(),
///     section: None,
///     snapshot: Some("abc123".to_owned()),
/// };
/// ```
///
/// Its resolved snapshot cannot be replaced after construction:
///
/// ```compile_fail
/// use engr::reference::EngrRef;
///
/// let mut reference = EngrRef::parse_standalone(
///     "engr:obj:01h47kwz2mfk0v47mffcnstqva@main",
/// )
/// .unwrap()
/// .canonicalize(|_| Some("0123456789abcdef0123456789abcdef01234567".to_owned()))
/// .unwrap();
/// reference.snapshot = Some("abc123".to_owned());
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalEngrRef {
    kind: ResourceKind,
    id: String,
    section: Option<u64>,
    snapshot: Option<String>,
}

impl EngrRef {
    pub fn parse_standalone(input: &str) -> Result<Self> {
        let relative = input
            .strip_prefix("engr:")
            .ok_or_else(|| Error::new(EXIT_SCHEMA, "an engr reference must begin with `engr:`"))?;
        Self::parse_embedded(relative)
    }

    pub fn parse_embedded(input: &str) -> Result<Self> {
        let (body, snapshot) = match input.split_once('@') {
            Some((body, snapshot)) => {
                ensure!(
                    !snapshot.is_empty() && !snapshot.contains('@'),
                    EXIT_SCHEMA,
                    "invalid Git snapshot selector"
                );
                (body, Some(snapshot.to_owned()))
            }
            None => (input, None),
        };
        let mut parts = body.split(':');
        let token = parts.next().unwrap_or_default();
        let id = parts.next().unwrap_or_default();
        let section = parts.next();
        ensure!(
            parts.next().is_none() && !id.is_empty(),
            EXIT_SCHEMA,
            "invalid engr reference {input:?}"
        );
        let kind = match token {
            "obj" => ResourceKind::Object,
            "backlog" => ResourceKind::Backlog,
            "collection" => ResourceKind::Collection,
            _ => {
                return Err(Error::new(
                    EXIT_SCHEMA,
                    format!("unknown engr resource {token:?}"),
                ))
            }
        };
        validate_id(kind, id)?;
        let section = section
            .map(|value| {
                value
                    .parse::<u64>()
                    .map_err(|_| Error::new(EXIT_SCHEMA, "section selector must be an integer"))
            })
            .transpose()?;
        Ok(Self {
            kind,
            id: id.to_owned(),
            section,
            snapshot_selector: snapshot,
        })
    }

    pub fn kind(&self) -> ResourceKind {
        self.kind
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn section(&self) -> Option<u64> {
        self.section
    }

    pub fn snapshot_selector(&self) -> Option<&str> {
        self.snapshot_selector.as_deref()
    }

    pub fn canonicalize(
        self,
        resolve_snapshot: impl FnOnce(&str) -> Option<String>,
    ) -> Result<CanonicalEngrRef> {
        let Self {
            kind,
            id,
            section,
            snapshot_selector,
        } = self;

        // Keep canonicalization independently defensive even though the parser
        // already validates ids: future constructors inside this module must not
        // become a way around the canonical type invariant.
        validate_id(kind, &id)?;

        let snapshot = snapshot_selector
            .as_deref()
            .map(|selector| {
                resolve_snapshot(selector).ok_or_else(|| {
                    Error::new(
                        EXIT_SCHEMA,
                        format!("Git snapshot {selector:?} could not be resolved"),
                    )
                })
            })
            .transpose()?
            .map(|snapshot| snapshot.to_ascii_lowercase());
        if let Some(snapshot) = &snapshot {
            ensure!(
                is_full_git_oid(snapshot),
                EXIT_SCHEMA,
                "resolved Git snapshot must be a full 40- or 64-character object id"
            );
        }
        Ok(CanonicalEngrRef {
            kind,
            id,
            section,
            snapshot,
        })
    }
}

impl CanonicalEngrRef {
    pub fn kind(&self) -> ResourceKind {
        self.kind
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn section(&self) -> Option<u64> {
        self.section
    }

    pub fn snapshot(&self) -> Option<&str> {
        self.snapshot.as_deref()
    }

    pub fn embedded(&self) -> String {
        let mut value = format!("{}:{}", self.kind.token(), self.id);
        if let Some(section) = self.section {
            value.push_str(&format!(":{section}"));
        }
        if let Some(snapshot) = &self.snapshot {
            value.push('@');
            value.push_str(snapshot);
        }
        value
    }
}

impl std::fmt::Display for CanonicalEngrRef {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "engr:{}", self.embedded())
    }
}

fn validate_id(kind: ResourceKind, id: &str) -> Result<()> {
    match kind {
        ResourceKind::Object | ResourceKind::Backlog => {
            let decoded = decode_uuid(id)?;
            ensure!(
                decoded.get_version() == Some(uuid::Version::SortRand),
                EXIT_SCHEMA,
                "Object and Backlog references must encode UUIDv7 identities"
            );
        }
        ResourceKind::Collection => {
            ensure!(
                id.len() == 10 && id.bytes().all(|byte| CROCKFORD.contains(&byte)),
                EXIT_SCHEMA,
                "collection id must be 10 lowercase Crockford Base32 characters"
            );
        }
    }
    Ok(())
}

fn is_full_git_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn encode_uuid(uuid: uuid::Uuid) -> String {
    let value = u128::from_be_bytes(*uuid.as_bytes());
    (0..26)
        .map(|index| {
            let shift = 125usize.saturating_sub(index * 5);
            let digit = if index == 0 {
                (value >> 125) as usize
            } else {
                ((value >> shift) & 31) as usize
            };
            CROCKFORD[digit] as char
        })
        .collect()
}

/// The compact reference spelling of a stored UUID identity.
///
/// Records keep standard UUID text so a file can be identified without a
/// resolver; references use the 26-character codec. Both spellings are the same
/// 128 bits, and this is the one place that says so for callers holding the
/// stored form.
pub fn encode_uuid_str(value: &str) -> Result<String> {
    let uuid = uuid::Uuid::parse_str(value)
        .map_err(|_| Error::new(EXIT_SCHEMA, format!("{value:?} is not a UUID")))?;
    Ok(encode_uuid(uuid))
}

pub fn decode_uuid(compact: &str) -> Result<uuid::Uuid> {
    ensure!(
        compact.len() == 26,
        EXIT_SCHEMA,
        "compact UUID must be exactly 26 characters"
    );
    let mut value = 0u128;
    for (index, byte) in compact.bytes().enumerate() {
        let digit = CROCKFORD
            .iter()
            .position(|candidate| *candidate == byte)
            .ok_or_else(|| {
                Error::new(
                    EXIT_SCHEMA,
                    "compact UUID must use lowercase Crockford Base32",
                )
            })? as u128;
        ensure!(
            index != 0 || digit < 8,
            EXIT_SCHEMA,
            "compact UUID exceeds 128 bits"
        );
        value = (value << 5) | digit;
    }
    Ok(uuid::Uuid::from_bytes(value.to_be_bytes()))
}

/// Parse an embedded engr target and hold it to the one canonical spelling.
///
/// Shared because every domain that embeds `{ "kind": "engr", "ref": ... }` has
/// the same three questions — is it parseable, is that resource kind legal
/// *here*, and is it written the one way — while each keeps its own answer to
/// the second. Sharing the parser is not sharing the semantics: `subjects[]`,
/// `produced[]` and `relations[]` all pass through this and mean different
/// things on the other side.
///
/// `what` names the field in the refusal, because a caller who typed the wrong
/// reference needs to know which of several it was.
pub fn canonical_embedded(
    reference: &str,
    allowed: &[ResourceKind],
    what: &str,
) -> Result<CanonicalEngrRef> {
    let parsed = EngrRef::parse_embedded(reference)?;
    ensure!(
        parsed.snapshot_selector().is_none(),
        EXIT_SCHEMA,
        "{what} names a current resource, so it cannot carry a Git snapshot selector"
    );
    ensure!(
        allowed.contains(&parsed.kind()),
        EXIT_SCHEMA,
        "{what} cannot target {reference:?}"
    );
    let canonical = parsed.canonicalize(|_| None)?;
    ensure!(
        canonical.embedded() == reference,
        EXIT_SCHEMA,
        "{what} {reference:?} is not canonical; write it as {:?}",
        canonical.embedded()
    );
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_uuid_vectors_round_trip() {
        let vectors = [
            (
                "00000000-0000-7000-8000-000000000000",
                "0000000000e008000000000000",
            ),
            (
                "01890f3e-7c54-7cc1-b21e-8f7b2b9d5f6a",
                "01h47kwz2mfk0v47mffcnstqva",
            ),
            (
                "ffffffff-ffff-7fff-bfff-ffffffffffff",
                "7zzzzzzzzzfzzvzzzzzzzzzzzz",
            ),
        ];
        for (uuid, compact) in vectors {
            let uuid = uuid::Uuid::parse_str(uuid).expect("fixed UUID");
            assert_eq!(encode_uuid(uuid), compact);
            assert_eq!(decode_uuid(compact).expect("compact UUID"), uuid);
        }
    }

    #[test]
    fn standalone_and_embedded_forms_share_one_parser() {
        let parsed = EngrRef::parse_standalone("engr:obj:01h47kwz2mfk0v47mffcnstqva:3@abc123")
            .expect("reference");
        let canonical = parsed
            .clone()
            .canonicalize(|selector| {
                assert_eq!(selector, "abc123");
                Some("0123456789abcdef0123456789abcdef01234567".to_owned())
            })
            .expect("resolved reference");
        assert_eq!(
            canonical.to_string(),
            "engr:obj:01h47kwz2mfk0v47mffcnstqva:3@0123456789abcdef0123456789abcdef01234567"
        );
        assert_eq!(
            EngrRef::parse_embedded("obj:01h47kwz2mfk0v47mffcnstqva:3@abc123").expect("embedded"),
            parsed,
        );
    }

    #[test]
    fn unresolved_or_abbreviated_snapshots_cannot_be_emitted_canonically() {
        let parsed = EngrRef::parse_standalone("engr:obj:01h47kwz2mfk0v47mffcnstqva@main")
            .expect("input reference");
        assert!(parsed.clone().canonicalize(|_| None).is_err());
        assert!(parsed.canonicalize(|_| Some("abc123".to_owned())).is_err());
    }

    #[test]
    fn invalid_identifiers_and_resolved_snapshots_cannot_be_canonicalized() {
        for input in [
            "engr:obj:not-a-compact-uuid",
            "engr:backlog:01h47kwz2mfk0v47mffcnstqvi",
            "engr:collection:abcdefghij",
        ] {
            assert!(EngrRef::parse_standalone(input).is_err(), "{input}");
        }

        let parsed = EngrRef::parse_standalone("engr:obj:01h47kwz2mfk0v47mffcnstqva@main")
            .expect("input reference");
        assert!(parsed.canonicalize(|_| Some("g".repeat(40))).is_err());
    }

    #[test]
    fn object_and_backlog_references_require_uuidv7() {
        let v7 = "01h47kwz2mfk0v47mffcnstqva";
        for kind in ["obj", "backlog"] {
            EngrRef::parse_standalone(&format!("engr:{kind}:{v7}"))
                .expect("a UUIDv7 compact reference");
        }

        let v4 = encode_uuid(
            uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").expect("fixed UUIDv4"),
        );
        for kind in ["obj", "backlog"] {
            assert!(
                EngrRef::parse_standalone(&format!("engr:{kind}:{v4}")).is_err(),
                "{kind} must not accept a UUIDv4 compact identity"
            );
        }
    }

    #[test]
    fn canonicalization_normalizes_a_full_resolved_snapshot() {
        let canonical = EngrRef::parse_standalone("engr:obj:01h47kwz2mfk0v47mffcnstqva:3@abc123")
            .expect("input")
            .canonicalize(|_| Some("0123456789ABCDEF0123456789ABCDEF01234567".to_owned()))
            .expect("canonical");
        assert_eq!(canonical.kind(), ResourceKind::Object);
        assert_eq!(canonical.id(), "01h47kwz2mfk0v47mffcnstqva");
        assert_eq!(canonical.section(), Some(3));
        assert_eq!(
            canonical.snapshot(),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
        assert_eq!(
            canonical.embedded(),
            "obj:01h47kwz2mfk0v47mffcnstqva:3@0123456789abcdef0123456789abcdef01234567"
        );
    }
}

/// The structural discriminator from the shared embedded reference form. `kind`
/// is structural; `type` stays reserved for semantic classification.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum EngrKind {
    #[serde(rename = "engr")]
    Engr,
}

/// `{ "kind": "engr", "ref": "obj:<id>" }` — the shared embedded engr target.
///
/// Lives here rather than in the first domain that needed it, because more than
/// one now does. What is shared is the *syntax*: every domain still decides for
/// itself which resource kinds it will accept and what pointing at one means.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct EngrTarget {
    pub kind: EngrKind,
    #[serde(rename = "ref")]
    pub reference: String,
}

impl EngrTarget {
    pub fn new(reference: impl Into<String>) -> Self {
        Self {
            kind: EngrKind::Engr,
            reference: reference.into(),
        }
    }
}

/// A fresh workspace-scoped collection id: fifty random bits as ten Crockford
/// Base32 characters.
///
/// Deliberately opaque and deliberately not a uuid. A collection id is short
/// enough to say out loud and carries nothing — no date, no milestone number,
/// no type — because every one of those would be a fact that can stop being
/// true while the id cannot change.
pub fn random_collection_id() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..10)
        .map(|_| CROCKFORD[rng.gen_range(0..32)] as char)
        .collect()
}
