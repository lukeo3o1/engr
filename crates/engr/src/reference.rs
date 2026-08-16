//! Canonical syntax for engr resource references.

use crate::{ensure, Error, Result, EXIT_SCHEMA};

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngrRef {
    pub kind: ResourceKind,
    pub id: String,
    pub section: Option<u64>,
    /// Input selector only. It may be abbreviated or symbolic and must be
    /// resolved before a canonical reference can be emitted.
    pub snapshot_selector: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalEngrRef {
    pub kind: ResourceKind,
    pub id: String,
    pub section: Option<u64>,
    pub snapshot: Option<String>,
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
        match kind {
            ResourceKind::Object | ResourceKind::Backlog => {
                decode_uuid(id)?;
            }
            ResourceKind::Collection => {
                ensure!(
                    id.len() == 10 && id.bytes().all(|byte| CROCKFORD.contains(&byte)),
                    EXIT_SCHEMA,
                    "collection id must be 10 lowercase Crockford Base32 characters"
                );
            }
        }
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

    pub fn canonicalize(
        self,
        resolve_snapshot: impl FnOnce(&str) -> Option<String>,
    ) -> Result<CanonicalEngrRef> {
        let snapshot = self
            .snapshot_selector
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
            kind: self.kind,
            id: self.id,
            section: self.section,
            snapshot,
        })
    }
}

impl CanonicalEngrRef {
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
}
