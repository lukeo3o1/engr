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

use crate::model::{Action, Merge, Object, Payload, Ref, Section};
use crate::semantics::{Admission, ObjectType, Relation, Role, State, Supplement};
use crate::{ensure, Error, Result, EXIT_INVARIANT, EXIT_SCHEMA, EXIT_USAGE};
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
                "{what}: {number} is outside the safe integer range every implementation shares, so canonical JSON would turn it into a different number and two different subjects would hash alike; carry it as a string"
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

/// Build the subject for one operation, from the states either side of it.
///
/// The table this implements is frozen: which projection each operation uses
/// for `before` and `after`, and what its parameters carry. A projection may
/// not be inferred from whatever the host language's structs happen to look
/// like, because the whole point is that another implementation reaches the
/// same bytes without seeing this code.
///
/// `before` and `after` are the Object either side of the reducer. Taking both
/// rather than recomputing one here is deliberate: the projection must describe
/// the transition that actually happened, including a lifecycle the same
/// confirmed act moved, and re-deriving it would be a second opinion about the
/// very thing being proved.
pub fn candidate_subject(
    before: &Object,
    after: &Object,
    payload: &Payload,
    review_digest: Option<String>,
) -> Result<CandidateSubject> {
    let id = &payload.object;
    let becomes = match &payload.becomes {
        Some(destination) => serde_json::to_value(Lifecycle {
            object_type: destination.object_type,
            state: destination.state,
        }),
        None => Ok(serde_json::Value::Null),
    }
    .map_err(|error| Error::new(EXIT_SCHEMA, format!("becomes: {error}")))?;

    let section_state = |object: &Object, section: u64| -> Result<SectionOperation> {
        Ok(SectionOperation {
            lifecycle: Lifecycle::of(object),
            section: object.section(section).ok().map(SectionSemantic::of),
        })
    };
    let json = |value: &dyn erased_json::Erased| value.to_json();

    let (target, parameters, before_state, after_state) = match &payload.action {
        Action::ObjectCreated => (
            object_target(id),
            serde_json::json!({}),
            serde_json::Value::Null,
            json(&ObjectCreation {
                title: after.title.clone(),
                object_type: after.object_type,
                state: after.state,
                sections: Vec::new(),
            })?,
        ),
        Action::ObjectRenamed => (
            object_target(id),
            serde_json::json!({ "becomes": becomes }),
            json(&TitleLifecycle::of(before))?,
            json(&TitleLifecycle::of(after))?,
        ),
        Action::ObjectClosed | Action::ObjectReopened => (
            object_target(id),
            serde_json::json!({}),
            json(&Lifecycle::of(before))?,
            json(&Lifecycle::of(after))?,
        ),
        Action::ObjectClassified { object_type, state } => (
            object_target(id),
            serde_json::json!({ "type": object_type, "state": state }),
            json(&Lifecycle::of(before))?,
            json(&Lifecycle::of(after))?,
        ),
        Action::SectionAdded => {
            // The resulting id is the protocol's answer, not the caller's, so
            // it is read off what the reducer produced rather than predicted.
            let added = added_section(before, after)?;
            (
                object_target(id),
                serde_json::json!({ "section": added, "becomes": becomes }),
                json(&SectionOperation {
                    lifecycle: Lifecycle::of(before),
                    section: None,
                })?,
                json(&section_state(after, added)?)?,
            )
        }
        Action::SectionRevised { section } => (
            section_target(id, *section),
            serde_json::json!({ "becomes": becomes }),
            json(&section_state(before, *section)?)?,
            json(&section_state(after, *section)?)?,
        ),
        Action::SectionDeleted { section } => (
            section_target(id, *section),
            serde_json::json!({ "becomes": becomes }),
            json(&section_state(before, *section)?)?,
            json(&SectionOperation {
                lifecycle: Lifecycle::of(after),
                section: None,
            })?,
        ),
        Action::SectionMerged { merge } => {
            // Only the representation that names its survivor has a frozen
            // projection. The retained shape allocated a fresh id and named no
            // destination, so there is no target for it to have — and this
            // contract was written for the generation that replaced it.
            let Merge::Into {
                destination,
                sources,
            } = merge
            else {
                return Err(Error::new(
                    EXIT_SCHEMA,
                    "the retained merge representation has no candidate projection: it names no surviving section".to_owned(),
                ));
            };
            (
                section_target(id, *destination),
                serde_json::json!({ "sources": sources, "becomes": becomes }),
                json(&ObjectInvariant::of(before))?,
                json(&ObjectInvariant::of(after))?,
            )
        }
        Action::ObjectSuperseded => {
            let added = added_section(before, after)?;
            (
                object_target(id),
                serde_json::json!({ "rationale_section": added }),
                json(&ObjectInvariant::of(before))?,
                json(&ObjectInvariant::of(after))?,
            )
        }
    };

    Ok(CandidateSubject {
        operation: Operation {
            name: payload.action.label().to_owned(),
            parameters,
        },
        target,
        before: before_state,
        after: after_state,
        review_digest,
    })
}

/// Which Section an operation brought into existence.
///
/// Read from the counter rather than from the list, because the list is sorted
/// by id and the newest is not necessarily last — and because the counter is
/// the protocol's own record of what it handed out.
fn added_section(before: &Object, after: &Object) -> Result<u64> {
    ensure!(
        after.next_section_id == before.next_section_id + 1,
        EXIT_SCHEMA,
        "this operation is defined to allocate exactly one section id"
    );
    Ok(before.next_section_id)
}

/// Serializing a projection to JSON without naming its type twice.
mod erased_json {
    use super::{Error, Result, EXIT_SCHEMA};
    use serde::Serialize;

    pub trait Erased {
        fn to_json(&self) -> Result<serde_json::Value>;
    }

    impl<T: Serialize> Erased for T {
        fn to_json(&self) -> Result<serde_json::Value> {
            serde_json::to_value(self)
                .map_err(|error| Error::new(EXIT_SCHEMA, format!("projection: {error}")))
        }
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

#[cfg(test)]
mod table_tests {
    use super::*;
    use crate::model::{Content, Destination, Payload};

    fn payload(action: Action, object: &str, text: &str) -> Payload {
        Payload {
            action,
            object: object.to_owned(),
            becomes: None,
            content: Content {
                text: text.to_owned(),
                ..Content::default()
            },
        }
    }

    fn section(id: u64, text: &str) -> Section {
        Section {
            id,
            admission: Admission::Human,
            role: None,
            text: text.to_owned(),
            content: Vec::new(),
            based_on: None,
            refs: Vec::new(),
            relations: Vec::new(),
            sha256: String::new(),
            admitted_at: "2026-08-23T00:00:00Z".to_owned(),
        }
    }

    /// The whole frozen table, checked for the two things a reader of another
    /// implementation needs: which target each operation names, and which
    /// projection sits either side of it.
    #[test]
    fn every_operation_projects_the_shape_the_contract_freezes() {
        let id = crate::model::new_id();
        let mut before = Object::new(id.clone(), "before".to_owned()).expect("object");
        before.rev = 1;
        before.next_section_id = 3;
        before.sections = vec![section(1, "one"), section(2, "two")];
        let mut after = before.clone();
        after.rev = 2;

        let cases: Vec<(Action, String, &str, &str)> = vec![
            (Action::ObjectRenamed, object_target(&id), "title", "title"),
            (Action::ObjectClosed, object_target(&id), "state", "state"),
            (Action::ObjectReopened, object_target(&id), "state", "state"),
            (
                Action::SectionRevised { section: 1 },
                section_target(&id, 1),
                "lifecycle",
                "lifecycle",
            ),
            (
                Action::SectionDeleted { section: 2 },
                section_target(&id, 2),
                "lifecycle",
                "lifecycle",
            ),
            (
                Action::ObjectSuperseded,
                object_target(&id),
                "sections",
                "sections",
            ),
        ];

        for (action, target, before_member, after_member) in cases {
            let label = action.label();
            let mut ending = after.clone();
            if matches!(action, Action::ObjectSuperseded) {
                // Superseding appends its rationale, so the counter moves.
                ending.next_section_id = before.next_section_id + 1;
            }
            let subject = candidate_subject(&before, &ending, &payload(action, &id, "w"), None)
                .unwrap_or_else(|error| panic!("{label}: {}", error.message));
            assert_eq!(subject.target, target, "{label}");
            assert_eq!(subject.operation.name, label);
            assert!(
                subject.before.get(before_member).is_some(),
                "{label}: before projection"
            );
            assert!(
                subject.after.get(after_member).is_some(),
                "{label}: after projection"
            );
        }
    }

    /// Creation has no before, because there was nothing there — and its after
    /// is the neutral shape the operation is defined to arrive at.
    #[test]
    fn creation_has_no_before() {
        let id = crate::model::new_id();
        let before = Object::new(id.clone(), String::new()).expect("object");
        let mut after = before.clone();
        after.title = "a title".to_owned();
        after.rev = 1;

        let subject = candidate_subject(
            &before,
            &after,
            &payload(Action::ObjectCreated, &id, "a title"),
            None,
        )
        .expect("subject");
        assert_eq!(subject.before, serde_json::Value::Null);
        assert_eq!(subject.after["title"], "a title");
        assert_eq!(subject.after["type"], serde_json::Value::Null);
        assert_eq!(subject.after["state"], "open");
        assert_eq!(subject.after["sections"], serde_json::json!([]));
    }

    /// A destination is part of what was authorized, so it is a parameter of the
    /// operation rather than something the after-state is left to imply.
    #[test]
    fn a_destination_is_named_in_the_parameters() {
        let id = crate::model::new_id();
        let mut before = Object::new(id.clone(), "t".to_owned()).expect("object");
        before.rev = 1;
        before.state = State::Closed;
        let mut after = before.clone();
        after.state = State::Open;
        after.next_section_id = 2;
        after.sections = vec![section(1, "w")];

        let mut added = payload(Action::SectionAdded, &id, "w");
        added.becomes = Some(Destination {
            object_type: None,
            state: State::Open,
        });
        let subject = candidate_subject(&before, &after, &added, None).expect("subject");
        assert_eq!(
            subject.operation.parameters,
            serde_json::json!({ "section": 1, "becomes": { "type": null, "state": "open" } })
        );
        assert_eq!(subject.before["section"], serde_json::Value::Null);
        assert_eq!(subject.after["section"]["text"], "w");
    }

    /// Only the representation that names its survivor has a frozen projection.
    /// The retained shape named no destination, so there is no target it could
    /// have — this contract was written for the generation that replaced it.
    #[test]
    fn the_retained_merge_representation_has_no_projection() {
        let id = crate::model::new_id();
        let mut before = Object::new(id.clone(), "t".to_owned()).expect("object");
        before.rev = 1;
        before.next_section_id = 3;
        before.sections = vec![section(1, "one"), section(2, "two")];
        let after = before.clone();

        let retained = payload(
            Action::SectionMerged {
                merge: Merge::Absorbing {
                    absorbs: vec![1, 2],
                },
            },
            &id,
            "together",
        );
        assert!(candidate_subject(&before, &after, &retained, None).is_err());

        let named = payload(
            Action::SectionMerged {
                merge: Merge::Into {
                    destination: 1,
                    sources: vec![2],
                },
            },
            &id,
            "together",
        );
        let subject = candidate_subject(&before, &after, &named, None).expect("subject");
        assert_eq!(subject.target, section_target(&id, 1));
        assert_eq!(
            subject.operation.parameters["sources"],
            serde_json::json!([2])
        );
    }
}

/// The Object-domain ReviewDigest `mutation`, exactly as #25 §4 freezes it.
///
/// Three members and no others. The operation descriptor and target are the
/// *same* projection CandidateDigest uses — not a parallel one that resembles
/// it — so that two implementations describing one operation reach one shape.
/// That sharing is enforced by construction in [`object_review_mutation`],
/// which reads them off the candidate subject rather than rebuilding them.
///
/// `before` is deliberately absent. The two contracts answer different
/// questions: CandidateDigest's `before`/`after` say what transition a human is
/// authorizing, while the review's predecessor is a concurrency fact and lives
/// in [`ReviewPrecondition`].
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct ReviewMutation {
    pub operation: Operation,
    pub target: String,
    /// The after projection **under the admission path that actually ran**.
    ///
    /// Not normalized to Human semantics: an Agent-reviewed Section projects
    /// `admission=agent`, and a Human candidate projects the Human-authorized
    /// result including promotion where that applies. Forcing one of them would
    /// make an Agent review and a Human confirmation of the same wording share
    /// a proof, which is the one thing mixed authority must not do.
    pub after: serde_json::Value,
}

/// The Object-domain ReviewDigest `precondition` (#25 §4): one member.
///
/// `expected_rev = 0` is the creation predecessor. Creation additionally
/// revalidates target absence and history emptiness at admission — those are
/// apply-time checks and are deliberately not folded in here, for the same
/// reason Ref soundness is not: a precondition that quietly stood for several
/// invariants would let one of them be dropped without the digest noticing.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReviewPrecondition {
    pub expected_rev: u64,
}

/// Build the frozen Object-domain review mutation from the states either side.
///
/// Takes the same arguments as [`candidate_subject`] and reads its result,
/// which is what makes "the projection schema is shared with CandidateDigest"
/// a fact about the build rather than a promise in a comment. Change the frozen
/// operation table and both digests move together, because there is one table.
pub fn object_review_mutation(
    before: &Object,
    after: &Object,
    payload: &Payload,
) -> Result<ReviewMutation> {
    let subject = candidate_subject(before, after, payload, None)?;
    Ok(ReviewMutation {
        operation: subject.operation,
        target: subject.target,
        after: subject.after,
    })
}

/// The member names an Object-domain review binds over, and exactly those.
const OBJECT_MUTATION_MEMBERS: &[&str] = &["after", "operation", "target"];
const OBJECT_PRECONDITION_MEMBERS: &[&str] = &["expected_rev"];

/// Refuse an Object-domain binding whose descriptor is not the frozen shape.
///
/// `bind` and `rebind` take caller JSON because the Backlog domain describes its
/// mutations differently. That generality is correct and it is also how a
/// descriptor the protocol no longer permits would get a perfectly ordinary
/// review digest — one that verifies against itself and against nothing anyone
/// else computes. So the Object domain is checked at the boundary rather than
/// trusted to arrive canonical.
pub fn check_object_review_shape(
    mutation: &serde_json::Value,
    precondition: &serde_json::Value,
) -> Result<()> {
    check_members(mutation, OBJECT_MUTATION_MEMBERS, "review mutation")?;
    check_members(
        precondition,
        OBJECT_PRECONDITION_MEMBERS,
        "review precondition",
    )?;
    ensure!(
        mutation["operation"].is_object() && mutation["operation"]["name"].is_string(),
        EXIT_SCHEMA,
        "an object review mutation names the operation it reviewed"
    );
    ensure!(
        mutation["target"].is_string(),
        EXIT_SCHEMA,
        "an object review mutation names the target it reviewed"
    );
    ensure!(
        precondition["expected_rev"].as_u64().is_some(),
        EXIT_SCHEMA,
        "an object review binds the exact revision it was reviewed against"
    );
    Ok(())
}

fn check_members(value: &serde_json::Value, expected: &[&str], what: &str) -> Result<()> {
    let members = value
        .as_object()
        .ok_or_else(|| Error::new(EXIT_SCHEMA, format!("an object {what} is a JSON object")))?;
    let mut found: Vec<&str> = members.keys().map(String::as_str).collect();
    found.sort_unstable();
    ensure!(
        found == expected,
        EXIT_SCHEMA,
        "an object {what} carries exactly {}, and this one carries {}",
        expected.join(", "),
        if found.is_empty() {
            "nothing".to_owned()
        } else {
            found.join(", ")
        }
    );
    Ok(())
}
/// What Rule Review produced, as the candidate shows it to a human.
///
/// A presentation record with teeth: the exact Rule snapshots are here so the
/// human sees what was reviewed against, and so a reader can recompute the
/// review identity from the candidate alone rather than trusting the name it
/// gives itself.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct CandidateReview {
    pub review_digest: String,
    pub attempt: u32,
    pub result: ReviewResult,
    pub rules: Vec<crate::rules::BoundRule>,
    /// Null for a pass. A failed or exhausted review reaching a human for
    /// override carries the exact wording the agent offered for it, because the
    /// human is being asked to overrule something and must read what.
    pub explanation: Option<String>,
}

/// How a review ended, as the candidate reports it.
///
/// Wider than what an Event records: `failed` and `exhausted` never become
/// durable provenance, because neither produces an admission on its own. They
/// exist here because a human may be shown one and choose to override it.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ReviewResult {
    Passed,
    Failed,
    Exhausted,
}

/// The concurrency predecessor a candidate is bound to.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct Binding {
    pub expected_rev: u64,
}

/// Everything a human is shown beside the mutation itself.
///
/// Every member is present, filled with its canonical null, empty or false even
/// where the stored envelope omits it. The envelope omits for economy; this
/// projection cannot, for the same reason none of the others can — two
/// implementations that disagree about whether an absent member is written
/// produce different bytes for one candidate.
#[derive(Serialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct PresentedContext {
    pub previous_text: Option<String>,
    pub previous_based_on: Option<String>,
    pub previous_refs: Vec<Ref>,
    pub previous_role: Option<Role>,
    pub previous_content: Vec<Supplement>,
    pub previous_relations: Vec<Relation>,
    pub previous_semantics_recorded: bool,
    pub oversize: bool,
    pub object_title: Option<String>,
    pub rule_review: Option<CandidateReview>,
}

/// Exactly what a candidate envelope's integrity value is taken of.
///
/// **Not** a second semantic identity. `candidate_digest` names the transition
/// that was authorized; this protects the envelope a human is looking at from
/// being edited on disk into something self-consistent but different.
///
/// The mutation payload is deliberately absent: the loader recomputes
/// `candidate_digest` from that payload and its predecessor separately, so
/// duplicating it here would mean two values covering one thing and a future
/// where they can disagree.
///
/// `created_at`, `format` and the envelope version are absent for the opposite
/// reason — they are operational ordering and schema selection, not the subject
/// a human authorized.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct EnvelopeIntegrity {
    pub challenge: String,
    pub candidate_digest: String,
    pub binding: Binding,
    pub context: PresentedContext,
}

impl EnvelopeIntegrity {
    /// The bare hex this envelope's integrity field carries.
    ///
    /// Bare rather than versioned, and that is not an oversight: the envelope's
    /// own version selects this calculation, so a field-local contract version
    /// would be a second answer to a question already answered one level up.
    pub fn digest(&self) -> Result<String> {
        Ok(sha256_of(&canonical_bytes(self, "candidate envelope")?))
    }
}

#[cfg(test)]
mod envelope_tests {
    use super::*;

    /// The exact bytes, for the shape where every optional member is at its
    /// default. This is the case an envelope most often stores as omissions, so
    /// it is the one where an implementation is most likely to disagree.
    #[test]
    fn the_integrity_projection_writes_every_default_out() {
        let integrity = EnvelopeIntegrity {
            challenge: "ABC234".to_owned(),
            candidate_digest: format!("1:{}", "a".repeat(64)),
            binding: Binding { expected_rev: 7 },
            context: PresentedContext::default(),
        };
        assert_eq!(
            canonical_bytes(&integrity, "envelope").expect("bytes"),
            concat!(
                r#"{"binding":{"expected_rev":7},"#,
                r#""candidate_digest":"1:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","#,
                r#""challenge":"ABC234","#,
                r#""context":{"object_title":null,"oversize":false,"previous_based_on":null,"#,
                r#""previous_content":[],"previous_refs":[],"previous_relations":[],"#,
                r#""previous_role":null,"previous_semantics_recorded":false,"previous_text":null,"#,
                r#""rule_review":null}}"#
            )
        );
        let digest = integrity.digest().expect("digest");
        assert_eq!(
            digest.len(),
            64,
            "bare hex, because the envelope version selects the calculation"
        );
        assert!(!digest.contains(':'));
    }

    /// The mutation is not in here, so two candidates differing only in their
    /// mutation share an integrity value — which is correct, because the
    /// loader recomputes `candidate_digest` from the mutation separately and
    /// duplicating it would be two values covering one thing.
    #[test]
    fn integrity_covers_the_envelope_and_the_digest_covers_the_mutation() {
        let one = EnvelopeIntegrity {
            challenge: "ABC234".to_owned(),
            candidate_digest: format!("1:{}", "a".repeat(64)),
            binding: Binding { expected_rev: 7 },
            context: PresentedContext::default(),
        };
        let mut moved = one.clone();
        moved.binding.expected_rev = 8;
        assert_ne!(
            one.digest().expect("digest"),
            moved.digest().expect("digest"),
            "the predecessor it is bound to is part of the envelope"
        );

        let mut shown = one.clone();
        shown.context.previous_text = Some("what it said before".to_owned());
        assert_ne!(
            one.digest().expect("digest"),
            shown.digest().expect("digest"),
            "and so is what the human was shown"
        );
    }
}

/// Check a stored Rule Review block against itself.
///
/// `integrity_sha256` covering these bytes proves only that nobody edited the
/// envelope after it was written. It says nothing about whether the block was
/// *coherent when written*, and a review block that names a review identity its
/// own contents do not produce is exactly what a human must not be shown as
/// settled. #25 §14 requires this on load and render, not only at confirmation,
/// and requires it to fail closed.
///
/// This is the half that needs no workspace: everything here is decidable from
/// the candidate's own bytes. Recomputing the review *identity* additionally
/// needs the mutation descriptor projection, which is [`check_review_identity`].
pub fn check_review_report(review: &CandidateReview) -> Result<()> {
    // Reject attempt 0 through the same type the rest of the crate counts
    // attempts with, so the candidate cannot carry a number no policy question
    // has an answer for.
    let attempt = crate::rules::Attempt::new(review.attempt)?;
    within_safe_integers(&serde_json::json!(review.attempt), "review attempt")?;

    // `exhausted` is the one outcome that is mechanically decidable, so it is
    // the one engr checks. `failed` is an Agent's semantic judgement and is
    // taken as attested — #25 is explicit that engr does not pretend prose
    // comprehension is decidable.
    let ceiling = crate::rules::smallest_ceiling(&review.rules);
    match (review.result, ceiling) {
        (ReviewResult::Exhausted, Some(limit)) => ensure!(
            attempt.get() > limit,
            EXIT_INVARIANT,
            "attempt {} is exhausted against a ceiling of {limit}, which it has not passed",
            attempt.get()
        ),
        (ReviewResult::Exhausted, None) => {
            return Err(Error::new(
                EXIT_INVARIANT,
                "a review with no rules cannot be exhausted".to_owned(),
            ))
        }
        (_, Some(limit)) => ensure!(
            attempt.get() <= limit,
            EXIT_INVARIANT,
            "attempt {} is past the ceiling of {limit} and can only be exhausted, not {}",
            attempt.get(),
            serde_json::to_value(review.result)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default()
        ),
        (_, None) => {}
    }

    // A human overriding a review is being asked to overrule something, and
    // must be able to read what. An empty string is the same absence with a
    // different spelling, so it is refused as one.
    match (review.result, review.explanation.as_deref()) {
        (ReviewResult::Passed, None) => {}
        (ReviewResult::Passed, Some(_)) => {
            return Err(Error::new(
                EXIT_INVARIANT,
                "a passed review carries no explanation".to_owned(),
            ))
        }
        (_, Some(text)) if !text.trim().is_empty() => {}
        _ => {
            return Err(Error::new(
                EXIT_INVARIANT,
                "a review offered for override must say what is being overridden".to_owned(),
            ))
        }
    }
    Ok(())
}

/// Recompute the review identity from the candidate's own Rule snapshots.
///
/// The descriptor is passed in because domains describe their mutations
/// differently — Object under #25 §4, Backlog under #8. It is not passed in
/// because the shape is open: for the Object domain it is frozen, and
/// [`check_object_review_identity`] is the entry point that builds it, so a
/// caller cannot reach this one with an Object descriptor of its own devising.
/// `rules::bind` and `rules::rebind` refuse a non-canonical Object shape
/// regardless of which entry point got there.
///
/// Failing closed is the point: a candidate whose stored snapshots do not
/// reproduce the identity it names is not a candidate with a stale review, it
/// is a candidate making a claim about a review that never happened that way.
pub fn check_review_identity(
    review: &CandidateReview,
    domain: crate::rules::Domain,
    mutation: serde_json::Value,
    precondition: serde_json::Value,
) -> Result<()> {
    let binding = crate::rules::rebind(domain, mutation, precondition, review.rules.clone())?;
    let checked = crate::digest::REVIEW.recheck(&review.review_digest, |version| {
        binding.digest_under(version)
    })?;
    ensure!(
        checked.agrees(),
        EXIT_INVARIANT,
        "the candidate names review {} and its own rule snapshots produce {}",
        checked.attested,
        checked.expected
    );
    Ok(())
}

/// Check a candidate's review identity against the frozen Object-domain
/// binding, built from the mutation and the exact revision it was reviewed at.
///
/// The typed entry point exists so an Object review cannot be checked against a
/// descriptor somebody assembled by hand. Build the mutation with
/// [`object_review_mutation`], which reads the same projection CandidateDigest
/// uses, and the two contracts cannot describe one operation differently.
pub fn check_object_review_identity(
    review: &CandidateReview,
    mutation: &ReviewMutation,
    expected_rev: u64,
) -> Result<()> {
    let mutation = serde_json::to_value(mutation)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("review mutation: {error}")))?;
    let precondition = serde_json::to_value(ReviewPrecondition { expected_rev })
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("review precondition: {error}")))?;
    check_review_identity(review, crate::rules::Domain::Object, mutation, precondition)
}
/// The agreement #25 §14 requires between a candidate's two identities.
///
/// A confirmation over a failed or exhausted review authorizes *that review's
/// result* as well as the mutation, so the semantic identity has to say which
/// review — otherwise the same bytes would prove an override of any review that
/// ever produced this mutation. A pass carries nothing, because a pass adds no
/// claim the human is being asked to overrule.
pub fn check_review_agreement(
    review: Option<&CandidateReview>,
    subject_review_digest: Option<&str>,
) -> Result<()> {
    let expected = match review {
        None => None,
        Some(review) => match review.result {
            ReviewResult::Passed => None,
            ReviewResult::Failed | ReviewResult::Exhausted => Some(review.review_digest.as_str()),
        },
    };
    ensure!(
        subject_review_digest == expected,
        EXIT_INVARIANT,
        "the candidate subject names {} as the review it depends on, and its review context requires {}",
        subject_review_digest.unwrap_or("none"),
        expected.unwrap_or("none")
    );
    Ok(())
}

#[cfg(test)]
mod review_context_tests {
    use super::*;
    use crate::rules::{BoundRule, Domain, OnExhaustion, Review};

    fn rule(id: &str, max_attempts: u32) -> BoundRule {
        BoundRule {
            id: id.to_owned(),
            domains: vec![Domain::Object],
            based_on: Vec::new(),
            review: Review {
                max_attempts,
                on_exhaustion: OnExhaustion::Reject,
            },
            body: "record what happened, not what is planned".to_owned(),
        }
    }

    /// The frozen Object-domain descriptor of #25 §4, built the way a caller
    /// must build it. An ad-hoc mutation is refused at the binding boundary now
    /// — which is the point, and which is why these fixtures cannot be a
    /// convenient blob.
    fn binding_inputs() -> (serde_json::Value, serde_json::Value) {
        let mutation = ReviewMutation {
            operation: Operation {
                name: "section_revised".to_owned(),
                parameters: serde_json::json!({"section": 1}),
            },
            target: "obj:0192f0c8-1a2b-7c3d-8e4f-5a6b7c8d9e0f:1".to_owned(),
            after: serde_json::json!({"admission": "human", "text": "revised"}),
        };
        (
            serde_json::to_value(mutation).expect("mutation"),
            serde_json::to_value(ReviewPrecondition { expected_rev: 7 }).expect("precondition"),
        )
    }
    fn review(result: ReviewResult, attempt: u32, rules: Vec<BoundRule>) -> CandidateReview {
        CandidateReview {
            review_digest: format!("1:{}", "a".repeat(64)),
            attempt,
            result,
            rules,
            explanation: match result {
                ReviewResult::Passed => None,
                _ => Some("the wording is a plan, not a record".to_owned()),
            },
        }
    }

    #[test]
    fn exhaustion_must_agree_with_the_ceilings_it_names() {
        // Two rules, and the smaller ceiling is the one an attempt meets first.
        let rules = vec![rule("a", 5), rule("b", 2)];
        check_review_report(&review(ReviewResult::Exhausted, 3, rules.clone()))
            .expect("past the smallest ceiling");
        let claimed = check_review_report(&review(ReviewResult::Exhausted, 2, rules.clone()))
            .expect_err("attempt 2 has not passed a ceiling of 2");
        assert!(claimed.to_string().contains("ceiling of 2"), "{claimed}");
        let overrun = check_review_report(&review(ReviewResult::Failed, 3, rules))
            .expect_err("attempt 3 is past the ceiling and can only be exhausted");
        assert!(
            overrun.to_string().contains("past the ceiling"),
            "{overrun}"
        );
    }

    #[test]
    fn a_review_with_no_rules_cannot_be_exhausted() {
        let empty = check_review_report(&review(ReviewResult::Exhausted, 9, Vec::new()))
            .expect_err("nothing governs this, so nothing can run out");
        assert!(empty.to_string().contains("no rules"), "{empty}");
        // An ungoverned mutation still passes, and there is no ceiling to meet.
        check_review_report(&review(ReviewResult::Passed, 9, Vec::new())).expect("ungoverned");
    }

    #[test]
    fn there_is_no_attempt_zero_inside_a_candidate() {
        let zero = check_review_report(&review(ReviewResult::Passed, 0, vec![rule("a", 5)]))
            .expect_err("counted from 1");
        assert!(zero.to_string().contains("attempt 0"), "{zero}");
    }

    #[test]
    fn an_override_must_say_what_is_being_overridden() {
        let mut blank = review(ReviewResult::Failed, 1, vec![rule("a", 5)]);
        blank.explanation = Some("   ".to_owned());
        let refused = check_review_report(&blank).expect_err("whitespace is absence respelled");
        assert!(refused.to_string().contains("must say what"), "{refused}");

        blank.explanation = None;
        check_review_report(&blank).expect_err("and so is nothing at all");

        let mut chatty = review(ReviewResult::Passed, 1, vec![rule("a", 5)]);
        chatty.explanation = Some("nothing to overrule".to_owned());
        check_review_report(&chatty).expect_err("a pass overrules nothing");
    }

    /// The check that makes the stored snapshots load-bearing rather than
    /// decorative: edit one, and the identity the candidate names no longer
    /// follows from it.
    #[test]
    fn snapshots_that_do_not_produce_the_named_identity_fail_closed() {
        let (mutation, precondition) = binding_inputs();
        let rules = vec![rule("a", 5)];
        let binding = crate::rules::rebind(
            Domain::Object,
            mutation.clone(),
            precondition.clone(),
            rules.clone(),
        )
        .expect("rebind");
        let honest = CandidateReview {
            review_digest: crate::digest::REVIEW
                .emit(binding.digest_under(1).expect("digest"))
                .expect("emit")
                .to_string(),
            ..review(ReviewResult::Passed, 1, rules)
        };
        check_review_identity(
            &honest,
            Domain::Object,
            mutation.clone(),
            precondition.clone(),
        )
        .expect("its own snapshots produce the name it gives itself");

        // Same digest, a ceiling quietly loosened. The review identity covers
        // the effective policy, so this cannot pass as the same review.
        let mut edited = honest.clone();
        edited.rules = vec![rule("a", 50)];
        let caught = check_review_identity(&edited, Domain::Object, mutation.clone(), precondition)
            .expect_err("edited on disk");
        assert!(
            caught.to_string().contains("rule snapshots produce"),
            "{caught}"
        );
    }

    /// The snapshot list is a set, and a candidate that stored it in another
    /// order must still reproduce the same identity — otherwise every reader
    /// would have to preserve an ordering the protocol calls insignificant.
    #[test]
    fn the_stored_snapshot_order_is_not_part_of_the_identity() {
        let (mutation, precondition) = binding_inputs();
        let one = crate::rules::rebind(
            Domain::Object,
            mutation.clone(),
            precondition.clone(),
            vec![rule("a", 5), rule("b", 2)],
        )
        .expect("rebind");
        let other = crate::rules::rebind(
            Domain::Object,
            mutation,
            precondition,
            vec![rule("b", 2), rule("a", 5)],
        )
        .expect("rebind");
        assert_eq!(
            one.digest_under(1).expect("digest"),
            other.digest_under(1).expect("digest")
        );
    }

    #[test]
    fn only_an_override_carries_the_review_into_the_semantic_identity() {
        let digest = format!("1:{}", "a".repeat(64));
        let passed = review(ReviewResult::Passed, 1, vec![rule("a", 5)]);
        let failed = review(ReviewResult::Failed, 1, vec![rule("a", 5)]);

        check_review_agreement(None, None).expect("no review, nothing named");
        check_review_agreement(None, Some(&digest))
            .expect_err("named a review that did not happen");
        check_review_agreement(Some(&passed), None).expect("a pass adds no claim");
        check_review_agreement(Some(&passed), Some(&digest))
            .expect_err("a pass is not an override");
        check_review_agreement(Some(&failed), Some(&digest))
            .expect("the override names its review");
        check_review_agreement(Some(&failed), None).expect_err("an override must say which review");
        check_review_agreement(Some(&failed), Some(&format!("1:{}", "c".repeat(64))))
            .expect_err("and must name the one it actually overrode");
    }
}

#[cfg(test)]
mod object_binding_tests {
    use super::*;
    use crate::model::{Content, Payload};

    fn object_id() -> String {
        "0192f0c8-1a2b-7c3d-8e4f-5a6b7c8d9e0f".to_owned()
    }

    fn section(id: u64, admission: Admission, text: &str) -> Section {
        Section {
            id,
            admission,
            role: None,
            text: text.to_owned(),
            content: Vec::new(),
            based_on: None,
            refs: Vec::new(),
            relations: Vec::new(),
            sha256: String::new(),
            admitted_at: "2026-08-24T00:00:00Z".to_owned(),
        }
    }

    fn revised(admission: Admission) -> (Object, Object, Payload) {
        let mut before =
            Object::new(object_id(), "the append boundary".to_owned()).expect("object");
        before.rev = 7;
        before.next_section_id = 2;
        before.sections = vec![section(1, admission, "as first written")];
        let mut after = before.clone();
        after.rev = 8;
        after.sections[0].text = "as revised".to_owned();
        let payload = Payload {
            action: Action::SectionRevised { section: 1 },
            object: object_id(),
            becomes: None,
            content: Content {
                text: "as revised".to_owned(),
                ..Content::default()
            },
        };
        (before, after, payload)
    }

    /// The review mutation is the candidate's own operation, target and after —
    /// read off the same projection rather than rebuilt beside it.
    #[test]
    fn the_review_mutation_is_the_candidates_projection_without_its_before() {
        let (before, after, mut payload) = revised(Admission::Human);
        payload.content.text = "as revised".to_owned();
        let subject = candidate_subject(&before, &after, &payload, None).expect("subject");
        let mutation = object_review_mutation(&before, &after, &payload).expect("mutation");

        assert_eq!(mutation.operation, subject.operation);
        assert_eq!(mutation.target, subject.target);
        assert_eq!(mutation.after, subject.after);

        let members = serde_json::to_value(&mutation).expect("value");
        let members = members.as_object().expect("object");
        assert_eq!(members.len(), 3, "three members and no before: {members:?}");
        assert!(!members.contains_key("before"));
    }

    /// #25 §4: the values are not forced to Human semantics. An Agent-reviewed
    /// Section projects `admission=agent`, so the two admission paths cannot
    /// share one review proof over the same wording.
    #[test]
    fn the_after_projection_follows_the_path_that_actually_admitted_it() {
        let (before, after, payload) = revised(Admission::Human);
        let human = object_review_mutation(&before, &after, &payload).expect("mutation");
        let (before, after, payload) = revised(Admission::Agent);
        let agent = object_review_mutation(&before, &after, &payload).expect("mutation");

        assert_ne!(
            human.after, agent.after,
            "the same wording admitted two ways is not the same review subject"
        );
        assert_eq!(human.operation, agent.operation, "and the same operation");
    }

    /// The exact bytes, pinned. Two implementations that agree on the operation,
    /// target, after projection, predecessor and Rule set must reach this
    /// scalar; anything else is a private agreement between one build and its
    /// own tests.
    #[test]
    fn the_object_binding_hashes_to_its_pinned_contract_bytes() {
        let (before, after, payload) = revised(Admission::Human);
        let mutation = serde_json::to_value(
            object_review_mutation(&before, &after, &payload).expect("mutation"),
        )
        .expect("value");
        let precondition = serde_json::to_value(ReviewPrecondition {
            expected_rev: before.rev,
        })
        .expect("value");

        let binding = crate::rules::rebind(
            crate::rules::Domain::Object,
            mutation.clone(),
            precondition.clone(),
            Vec::new(),
        )
        .expect("rebind");

        let canonical = canonical_bytes(&binding, "review binding").expect("canonical");
        assert_eq!(
            canonical,
            r#"{"domain":"object","mutation":{"after":{"lifecycle":{"state":"open","type":null},"section":{"admission":"human","based_on":null,"content":[],"refs":[],"relations":[],"role":null,"text":"as revised"}},"operation":{"name":"section.revised","parameters":{"becomes":null}},"target":"obj:0192f0c8-1a2b-7c3d-8e4f-5a6b7c8d9e0f:1"},"precondition":{"expected_rev":7},"rules":[]}"#,
            "the frozen JCS bytes of an object review binding"
        );
        assert_eq!(
            binding.digest().expect("digest").to_string(),
            "1:9b42b2b35dfab7b55ba90a625bd886a3946e9eec89d22797c9484590cf271eae"
        );
    }

    /// A descriptor the protocol no longer permits is refused where it is
    /// bound, not accepted into a digest that verifies against itself and
    /// against nothing anyone else computes.
    #[test]
    fn a_non_canonical_object_descriptor_is_refused_at_the_boundary() {
        let (before, after, payload) = revised(Admission::Human);
        let good = serde_json::to_value(
            object_review_mutation(&before, &after, &payload).expect("mutation"),
        )
        .expect("value");
        let precondition = serde_json::json!({"expected_rev": 7});

        // The pre-freeze prototype shape.
        let refused = crate::rules::rebind(
            crate::rules::Domain::Object,
            serde_json::json!({"action": "section_revised"}),
            precondition.clone(),
            Vec::new(),
        )
        .expect_err("not the frozen descriptor");
        assert!(refused.to_string().contains("exactly"), "{refused}");

        // CandidateDigest's own subject, which carries one member too many.
        let mut with_before = good.as_object().expect("object").clone();
        with_before.insert("before".to_owned(), serde_json::Value::Null);
        crate::rules::rebind(
            crate::rules::Domain::Object,
            serde_json::Value::Object(with_before),
            precondition.clone(),
            Vec::new(),
        )
        .expect_err("the review predecessor is not the candidate's before");

        // A precondition standing for more than the revision.
        crate::rules::rebind(
            crate::rules::Domain::Object,
            good.clone(),
            serde_json::json!({"expected_rev": 7, "refs_are_sound": true}),
            Vec::new(),
        )
        .expect_err("apply-time invariants are not folded into expected_rev");

        // A revision that is not a revision.
        crate::rules::rebind(
            crate::rules::Domain::Object,
            good,
            serde_json::json!({"expected_rev": "7"}),
            Vec::new(),
        )
        .expect_err("expected_rev is the exact revision, not a spelling of it");
    }

    /// Creation binds `expected_rev = 0`, which is a predecessor rather than a
    /// missing one, so it must survive the boundary rather than read as absent.
    #[test]
    fn creation_binds_the_zero_predecessor() {
        let mutation = serde_json::to_value(ReviewMutation {
            operation: Operation {
                name: "object_created".to_owned(),
                parameters: serde_json::json!({}),
            },
            target: format!("obj:{}", object_id()),
            after: serde_json::json!({"state": "open"}),
        })
        .expect("value");
        crate::rules::rebind(
            crate::rules::Domain::Object,
            mutation,
            serde_json::to_value(ReviewPrecondition { expected_rev: 0 }).expect("value"),
            Vec::new(),
        )
        .expect("zero is the creation predecessor");
    }

    /// Backlog describes its mutations under #8 and is not held to the Object
    /// shape — the gate is per-domain, not a single shape imposed on every one.
    #[test]
    fn another_domain_keeps_its_own_descriptor() {
        crate::rules::rebind(
            crate::rules::Domain::Backlog,
            serde_json::json!({"action": "item_opened"}),
            serde_json::json!({"predecessor": "none"}),
            Vec::new(),
        )
        .expect("backlog is not an object");
    }
}
