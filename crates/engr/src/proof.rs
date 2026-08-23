//! The exact inputs a long-lived proof is computed over.
//!
//! A digest is only as portable as the bytes it was taken of. Two
//! implementations that agree on "hash the operation and the resulting state"
//! and disagree by one omitted member produce different values for the same
//! act, and neither can tell which of them is wrong. So the shapes here are
//! **projections**, written out in full, rather than the persisted resources
//! they are derived from.
//!
//! Three rules follow from that, and all three are the opposite of how the
//! stored representation works:
//!
//! - **Every member is present.** A persisted Section omits `role` when it has
//!   none; a projected one says `"role": null`. Omission is a storage
//!   economy, and a hash contract cannot afford one.
//! - **Representation is excluded.** Integrity seals, `admitted_at`, the
//!   workspace version, Section ids where the operation does not identify by
//!   them — none of it is what was authorized, so none of it is hashed.
//! - **The contract version lives outside.** Nothing here prepends a version or
//!   a field name to the bytes; the version is the `1:` on the scalar.

use crate::model::{Object, Ref, Section};
use crate::semantics::{Admission, ObjectType, Relation, Role, State, Supplement};
use crate::{ensure, Error, Result, EXIT_SCHEMA, EXIT_USAGE};
use serde::Serialize;
use sha2::{Digest, Sha256};

/// The largest magnitude a protocol integer may carry as a JSON number.
///
/// `2^53 - 1`, the cross-language safe-integer bound. Not "whatever this
/// language can hold": the point of a shared contract is that another
/// implementation computes the same bytes, and a value that survives a `u64`
/// but not a JavaScript `number` is a value two conforming readers disagree
/// about.
pub const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

/// Refuse a subject JCS cannot carry without changing it.
///
/// RFC 8785 canonicalizes numbers through binary64, correctly and per the
/// standard — which means an integer past the safe range does not fail, it
/// **quietly becomes a different one**:
///
/// ```text
/// 9007199254740993  ->  9007199254740992
/// 9007199254740992  ->  9007199254740992
/// ```
///
/// Two different subjects, one set of canonical bytes, one hash. For a value
/// whose whole job is naming an exact subject, that is the worst available
/// failure: a proof over one would verify against the other, and nothing
/// anywhere would report it.
///
/// The bound is the protocol's, not this language's. A field with a narrower
/// range of its own keeps it — this is the ceiling every field shares, never a
/// licence to widen one.
pub fn within_safe_integers(value: &serde_json::Value, what: &str) -> Result<()> {
    match value {
        serde_json::Value::Number(number) => {
            let magnitude = match (number.as_u64(), number.as_i64()) {
                (Some(unsigned), _) => unsigned,
                (_, Some(signed)) => signed.unsigned_abs(),
                // Already a double. A protocol integer is never one, so this is
                // a subject that was not an integer to begin with.
                _ => return Ok(()),
            };
            ensure!(
                magnitude <= MAX_SAFE_INTEGER,
                EXIT_USAGE,
                "{what}: {number} is outside the safe integer range every implementation shares, so canonical JSON would turn it into a different number and two different subjects would hash alike"
            );
            Ok(())
        }
        serde_json::Value::Array(items) => {
            for item in items {
                within_safe_integers(item, what)?;
            }
            Ok(())
        }
        serde_json::Value::Object(entries) => {
            for entry in entries.values() {
                within_safe_integers(entry, what)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// The canonical bytes of one value, as **RFC 8785 (JCS)**.
///
/// Not `serde_json::to_string`, and the difference is not academic. JCS orders
/// object members by their **UTF-16** code units, while `serde_json`'s map is
/// ordered by Rust string comparison, which is UTF-8 order. For keys `U+E000`
/// and `U+1F600` the two disagree. JCS also fixes number formatting, which
/// stable serde output does not promise.
///
/// The point of naming a standard is that a second implementation, in another
/// language, computes the same bytes.
pub fn canonical_bytes<T: Serialize>(value: &T, what: &str) -> Result<String> {
    let value = serde_json::to_value(value)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("canonical {what}: {error}")))?;
    within_safe_integers(&value, what)?;
    serde_jcs::to_string(&value)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("canonical {what}: {error}")))
}

fn sha256_of(bytes: &str) -> String {
    format!("{:x}", Sha256::digest(bytes.as_bytes()))
}

/// Where an Object stands, with both members always spelled out.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct Lifecycle {
    #[serde(rename = "type")]
    pub object_type: Option<ObjectType>,
    pub state: State,
}

impl Lifecycle {
    pub fn of(object: &Object) -> Self {
        Self {
            object_type: object.object_type,
            state: object.state,
        }
    }
}

/// Everything a Section *means*, and nothing about how it is stored.
///
/// `id`, `admitted_at` and the integrity seal are deliberately absent. An id
/// identifies rather than asserts, a timestamp is when rather than what, and a
/// seal is a claim about the bytes — none of the three is part of what somebody
/// authorized.
///
/// `admission` **is** here, and is the actual value on the path being taken: an
/// Agent-reviewed result projects `agent`. Forcing it to `human` would make one
/// projection describe two different admissions.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct SectionSemantic {
    pub admission: Admission,
    pub role: Option<Role>,
    pub text: String,
    pub content: Vec<Supplement>,
    pub based_on: Option<String>,
    pub refs: Vec<Ref>,
    pub relations: Vec<Relation>,
}

impl SectionSemantic {
    pub fn of(section: &Section) -> Self {
        Self {
            admission: section.admission,
            role: section.role,
            text: section.text.clone(),
            content: section.content.clone(),
            based_on: section.based_on.clone(),
            refs: section.refs.clone(),
            relations: section.relations.clone(),
        }
    }
}

/// One Section's state, together with where the Object stands.
///
/// The lifecycle travels with it because a Section operation may legally arrive
/// at a new one — a settled Object returning to attention in the same confirmed
/// act — and a projection that showed only the Section would hash two different
/// authorizations alike.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct SectionOperation {
    pub lifecycle: Lifecycle,
    pub section: Option<SectionSemantic>,
}

/// One Section of an Object-level projection, by identity and meaning.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct SectionEntry {
    pub id: u64,
    pub semantic: SectionSemantic,
}

/// The whole Object, for the operations whose meaning spans Sections.
///
/// `rev` and `next_section_id` are excluded: they are bookkeeping about how the
/// Object got here, not about what it says. `sections` is ordered by numeric id
/// so two implementations produce one order.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct ObjectInvariant {
    #[serde(rename = "type")]
    pub object_type: Option<ObjectType>,
    pub state: State,
    pub sections: Vec<SectionEntry>,
}

impl ObjectInvariant {
    pub fn of(object: &Object) -> Self {
        let mut sections: Vec<SectionEntry> = object
            .sections
            .iter()
            .map(|section| SectionEntry {
                id: section.id,
                semantic: SectionSemantic::of(section),
            })
            .collect();
        sections.sort_by_key(|entry| entry.id);
        Self {
            object_type: object.object_type,
            state: object.state,
            sections,
        }
    }
}

/// What an Object is brought into existence as.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct ObjectCreation {
    pub title: String,
    #[serde(rename = "type")]
    pub object_type: Option<ObjectType>,
    pub state: State,
    pub sections: Vec<SectionEntry>,
}

/// A title, together with where the Object stands.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct TitleLifecycle {
    pub title: String,
    #[serde(rename = "type")]
    pub object_type: Option<ObjectType>,
    pub state: State,
}

impl TitleLifecycle {
    pub fn of(object: &Object) -> Self {
        Self {
            title: object.title.clone(),
            object_type: object.object_type,
            state: object.state,
        }
    }
}

/// What is being done, named by the protocol rather than by a command line.
///
/// `name` is the protocol's operation name — never the CLI spelling, which is
/// presentation and may change without the semantics changing at all.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct Operation {
    pub name: String,
    pub parameters: serde_json::Value,
}

/// Exactly what a candidate digest is taken of.
///
/// All five members are always present. `before` and `after` are the projected
/// semantic states, `null` where the operation defines none — creation has no
/// before, because there was nothing there.
///
/// `review_digest` is non-null only when the confirmation semantically depends
/// on that exact review identity: a human overriding a failed or exhausted
/// review is authorizing *that* review's result as well as the mutation, and a
/// proof that did not say so would be a proof of a different act.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct CandidateSubject {
    pub operation: Operation,
    pub target: String,
    pub before: serde_json::Value,
    pub after: serde_json::Value,
    pub review_digest: Option<String>,
}

impl CandidateSubject {
    /// The versioned scalar this subject hashes to.
    pub fn digest(&self) -> Result<String> {
        let bytes = canonical_bytes(self, "candidate subject")?;
        crate::digest::CANDIDATE
            .emit(sha256_of(&bytes))
            .map(|versioned| versioned.to_string())
    }
}

/// The canonical target of an operation that names a whole Object.
pub fn object_target(id: &str) -> String {
    format!("obj:{id}")
}

/// The canonical target of an operation that names one Section.
pub fn section_target(id: &str, section: u64) -> String {
    format!("obj:{id}:{section}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object() -> Object {
        Object::new(crate::model::new_id(), "a title".to_owned()).expect("object")
    }

    /// Every member is present, including the ones the persisted resource omits
    /// when they carry nothing. Omission is a storage economy; a hash contract
    /// cannot afford one, because the omitting and the spelling-out
    /// implementations would disagree about the bytes.
    #[test]
    fn a_projection_spells_out_what_storage_leaves_unsaid() {
        let lifecycle = Lifecycle::of(&object());
        assert_eq!(
            canonical_bytes(&lifecycle, "lifecycle").expect("bytes"),
            r#"{"state":"open","type":null}"#
        );

        let section = SectionSemantic {
            admission: Admission::Human,
            role: None,
            text: "wording".to_owned(),
            content: Vec::new(),
            based_on: None,
            refs: Vec::new(),
            relations: Vec::new(),
        };
        assert_eq!(
            canonical_bytes(&section, "section").expect("bytes"),
            r#"{"admission":"human","based_on":null,"content":[],"refs":[],"relations":[],"role":null,"text":"wording"}"#
        );
    }

    /// The subject carries all five members whatever the operation is, so a
    /// reader never has to know which ones this operation happened to use.
    #[test]
    fn a_candidate_subject_always_carries_its_five_members() {
        let subject = CandidateSubject {
            operation: Operation {
                name: "object.created".to_owned(),
                parameters: serde_json::json!({}),
            },
            target: object_target("01a02f75-d750-73c1-8d03-32aa3b1a9fa5"),
            before: serde_json::Value::Null,
            after: serde_json::to_value(ObjectCreation {
                title: "a title".to_owned(),
                object_type: None,
                state: State::Open,
                sections: Vec::new(),
            })
            .expect("json"),
            review_digest: None,
        };
        assert_eq!(
            canonical_bytes(&subject, "subject").expect("bytes"),
            concat!(
                r#"{"after":{"sections":[],"state":"open","title":"a title","type":null},"#,
                r#""before":null,"#,
                r#""operation":{"name":"object.created","parameters":{}},"#,
                r#""review_digest":null,"#,
                r#""target":"obj:01a02f75-d750-73c1-8d03-32aa3b1a9fa5"}"#
            )
        );
        let digest = subject.digest().expect("digest");
        assert!(digest.starts_with("1:"), "{digest}");
        assert_eq!(digest.len(), 2 + 64);
    }

    /// An integer past the shared safe range does not fail canonicalization —
    /// it silently becomes a different one, which would make two distinct
    /// subjects hash alike. Refused before the bytes are computed.
    #[test]
    fn a_subject_outside_the_shared_integer_range_is_refused() {
        let safe = serde_json::json!({ "n": MAX_SAFE_INTEGER });
        canonical_bytes(&safe, "subject").expect("the bound itself is carriable");

        let beyond = serde_json::json!({ "n": MAX_SAFE_INTEGER + 1 });
        let error = canonical_bytes(&beyond, "subject")
            .expect_err("past the bound, canonical JSON would change the value");
        assert_eq!(error.code, EXIT_USAGE);

        let negative = serde_json::json!({ "n": -(MAX_SAFE_INTEGER as i64) - 1 });
        assert!(canonical_bytes(&negative, "subject").is_err());
    }

    /// Sections are ordered by numeric id, never by whatever order they
    /// happened to be stored in.
    #[test]
    fn an_object_projection_orders_sections_by_identity() {
        let mut object = object();
        object.next_section_id = 4;
        for id in [3, 1] {
            object.sections.push(Section {
                id,
                admission: Admission::Human,
                role: None,
                text: format!("section {id}"),
                content: Vec::new(),
                based_on: None,
                refs: Vec::new(),
                relations: Vec::new(),
                sha256: String::new(),
                admitted_at: "2026-08-23T00:00:00Z".to_owned(),
            });
        }
        let projected = ObjectInvariant::of(&object);
        assert_eq!(
            projected
                .sections
                .iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>(),
            vec![1, 3]
        );
    }
}
