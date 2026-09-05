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
//! - **Representation is excluded.** Integrity seals, `admitted.at`, the
//!   workspace generation, Section ids where the operation does not identify by
//!   them — none of it is what was authorized, so none of it is hashed.
//! - **The contract version lives outside.** Nothing here prepends a version or
//!   a field name to the bytes; the version is the `1:` on the scalar.

use crate::model::{Action, Object, Payload, Ref, Section};
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
    walk_safe_integers(EXIT_USAGE, value, what)
}

/// The same walk over material that was already **stored**.
///
/// One traversal, two fault classes. A number a caller just typed is a usage
/// error; the identical number found inside a persisted Object, Event, Backlog
/// item, Collection or Work sidecar is not something the caller did. It is a
/// file outside the schema, and saying "usage" there sends a reader to fix
/// their command line instead of the record.
pub fn stored_within_safe_integers(value: &serde_json::Value, what: &str) -> Result<()> {
    walk_safe_integers(EXIT_SCHEMA, value, what)
}

fn walk_safe_integers(code: i32, value: &serde_json::Value, what: &str) -> Result<()> {
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
                code,
                "{what}: {number} is outside the safe integer range every implementation shares, so canonical JSON would turn it into a different number and two different subjects would hash alike; carry it as a string"
            );
            Ok(())
        }
        serde_json::Value::Array(items) => {
            for item in items {
                walk_safe_integers(code, item, what)?;
            }
            Ok(())
        }
        serde_json::Value::Object(entries) => {
            for entry in entries.values() {
                walk_safe_integers(code, entry, what)?;
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

/// Put a persisted set in its protocol order, refusing canonical duplicates.
///
/// The protocol classifies every persisted array as `ordered` or `set`. A set
/// is ordered by the JCS bytes of its own elements — **not** by `derive(Ord)`,
/// struct declaration order, insertion order or anything else the host language
/// happens to offer. Those all order by something a second implementation in
/// another language cannot see, which is the one thing a canonical form may not
/// depend on.
///
/// Sorting by one chosen field is the trap this replaces, and it is not
/// obviously wrong: `based_on` sorted by `path` is deterministic and stable. It
/// is still a different order, because canonical JSON sorts keys — so a basis's
/// bytes begin with `commit`, and two conforming implementations that each
/// picked a "natural" field would disagree on the same set.
///
/// Duplicates are rejected by the same bytes that order them, so two members
/// that are canonically equal are refused even when the Rust values differ in
/// some way the canonical form does not carry.
pub fn canonical_set<T: Serialize>(items: &mut Vec<T>, what: &str) -> Result<()> {
    let mut keyed: Vec<(String, T)> = Vec::with_capacity(items.len());
    for item in std::mem::take(items) {
        let canonical = canonical_bytes(&item, what)?;
        keyed.push((canonical, item));
    }
    keyed.sort_by(|left, right| left.0.cmp(&right.0));
    for pair in keyed.windows(2) {
        ensure!(
            pair[0].0 != pair[1].0,
            EXIT_SCHEMA,
            "the same {what} appears twice"
        );
    }
    items.extend(keyed.into_iter().map(|(_, item)| item));
    Ok(())
}
/// SHA-256 of some text, lowercase hex.
///
/// The crate has one of these on purpose: a second spelling is a second place
/// for a digest contract to be computed slightly differently.
pub fn sha256_of(bytes: &str) -> String {
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
/// `id`, `admitted.at` and the integrity seal are deliberately absent. An id
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
    pub header: Option<String>,
    pub role: Option<Role>,
    pub text: String,
    pub content: Vec<Supplement>,
    pub based_on: Option<crate::semantics::BasedOn>,
    pub refs: Vec<Ref>,
    pub relations: Vec<Relation>,
}

impl SectionSemantic {
    /// The one canonical Section semantic projection.
    ///
    /// #35 says there is exactly one, shared by dependency selection, admission
    /// and Rule Review semantics, and drift. `crate::dependency` reads this
    /// rather than building its own, so "shared" is a fact about the build
    /// instead of a claim two definitions have to keep agreeing on.
    ///
    /// `refs` and `relations` are canonicalized here, and that is the whole
    /// reason this returns a `Result`. They were copied verbatim, which made
    /// every proof over a Section sensitive to the order its array happened to
    /// be in — a set ordered by whatever `derive(Ord)` gives its members
    /// disagrees with the protocol set rule, which orders by each element's JCS
    /// bytes. Those two disagree for the same set, so one Section could hash one
    /// way through a Challenge subject and another way through dependency
    /// semantics.
    pub fn of(section: &Section) -> Result<Self> {
        let mut refs = section.refs.clone();
        let mut relations = section.relations.clone();
        canonical_set(&mut refs, "reference")?;
        canonical_set(&mut relations, "relation")?;
        Ok(Self {
            admission: section.admitted.by,
            header: section.header.clone(),
            role: section.role,
            text: section.text.clone(),
            content: section.content.clone(),
            based_on: section.based_on.clone(),
            refs,
            relations,
        })
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
    pub fn of(object: &Object) -> Result<Self> {
        let mut sections: Vec<SectionEntry> = Vec::with_capacity(object.sections.len());
        for section in &object.sections {
            sections.push(SectionEntry {
                id: section.id,
                semantic: SectionSemantic::of(section)?,
            });
        }
        sections.sort_by_key(|entry| entry.id);
        Ok(Self {
            object_type: object.object_type,
            state: object.state,
            sections,
        })
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

/// The frozen projection of one Object-domain operation.
///
/// Which projection each operation uses for `after`, and what its parameters
/// carry, is fixed here and nowhere else. It may not be inferred from whatever
/// the host language's structs happen to look like, because the whole point is
/// that another implementation reaches the same bytes without seeing this code.
///
/// `after` is taken from the Object the reducer produced rather than
/// re-derived, so the projection describes the transition that actually
/// happened — including a lifecycle the same act moved. Re-deriving it would be
/// a second opinion about the thing being proved.
///
/// There is no `before` here. Rule Review's predecessor is a concurrency fact
/// and lives in [`ReviewPrecondition`]; the transition a human authorized is the
/// Challenge subject, which is the frozen payload itself rather than a
/// projection of it.
#[derive(Debug)]
pub struct ObjectProjection {
    pub operation: Operation,
    pub target: String,
    pub after: serde_json::Value,
}

/// Build the frozen projection for one operation, from the states either side.
pub fn object_projection(
    before: &Object,
    after: &Object,
    payload: &Payload,
) -> Result<ObjectProjection> {
    // The identities first, because everything below formats them into a target
    // that a proof then names. A payload naming `not-an-id`, or naming an
    // Object the states either side are not, would otherwise produce a
    // perfectly well-formed digest over a target that denotes nothing.
    crate::model::validate_object_id(&payload.object)?;
    ensure!(
        payload.object == before.id && payload.object == after.id,
        EXIT_INVARIANT,
        "the payload names object {} and the states either side are {} and {}",
        payload.object,
        before.id,
        after.id
    );
    let id = &payload.object;
    let becomes = match payload.becomes() {
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
            section: match object.section(section) {
                Ok(section) => Some(SectionSemantic::of(section)?),
                Err(_) => None,
            },
        })
    };
    let json = |value: &dyn erased_json::Erased| value.to_json();

    let (target, parameters, after_state) =
        match &payload.action {
            Action::ObjectCreated { .. } => (
                object_target(id)?,
                serde_json::json!({}),
                json(&ObjectCreation {
                    title: after.title.clone(),
                    object_type: after.object_type,
                    state: after.state,
                    sections: Vec::new(),
                })?,
            ),
            Action::ObjectRenamed { .. } => (
                object_target(id)?,
                serde_json::json!({ "becomes": becomes }),
                json(&TitleLifecycle::of(after))?,
            ),
            Action::ObjectStateChanged { state } => (
                object_target(id)?,
                serde_json::json!({ "state": state }),
                json(&Lifecycle::of(after))?,
            ),
            Action::ObjectClassified { object_type, state } => (
                object_target(id)?,
                serde_json::json!({ "type": object_type, "state": state }),
                json(&Lifecycle::of(after))?,
            ),
            Action::SectionCreated { .. } => {
                // The resulting id is the protocol's answer, not the caller's, so
                // it is read off what the reducer produced rather than predicted.
                let added = added_section(before, after)?;
                (
                    object_target(id)?,
                    serde_json::json!({ "section": added, "becomes": becomes }),
                    json(&section_state(after, added)?)?,
                )
            }
            Action::SectionUpdated { section, .. } => (
                section_target(id, *section)?,
                serde_json::json!({ "becomes": becomes }),
                json(&section_state(after, *section)?)?,
            ),
            Action::SectionDeleted { section, .. } => (
                section_target(id, *section)?,
                serde_json::json!({ "becomes": becomes }),
                json(&SectionOperation {
                    lifecycle: Lifecycle::of(after),
                    section: None,
                })?,
            ),
            Action::SectionMerged { merge, .. } => (
                section_target(id, merge.destination)?,
                serde_json::json!({ "sources": merge.sources, "becomes": becomes }),
                json(&ObjectInvariant::of(after)?)?,
            ),
            Action::ObjectSuperseded { .. } => {
                let added = added_section(before, after)?;
                (
                    object_target(id)?,
                    serde_json::json!({ "rationale_section": added }),
                    json(&ObjectInvariant::of(after)?)?,
                )
            }
            // The whole Object, and no parameters at all.
            //
            // Repair restores exactly the replay-derived projection, so the
            // integrity-invalid stored bytes are deliberately not an input: they are
            // diagnostic material shown beside the proposal, and a digest that bound
            // them could not be recomputed from history later.
            Action::ObjectRepaired {} => (
                object_target(id)?,
                serde_json::json!({}),
                json(&ObjectInvariant::of(after)?)?,
            ),
            // Not a reviewable mutation. Migration is confirmed as a whole, by a
            // Human, through its own Challenge subject family — there is no Object
            // Rule that governs it and nothing here to project.
            Action::ObjectMigrated { .. } => return Err(Error::new(
                EXIT_SCHEMA,
                "object.migrated.v1 is not an Object-domain mutation and has no review projection"
                    .to_owned(),
            )),
        };

    // And the formatted target parses back as the identity it claims to be.
    // A Section id of 0, or one past the shared safe-integer ceiling, formats
    // into a string like any other and denotes no Section at all.
    check_canonical_target(&target)?;
    Ok(ObjectProjection {
        operation: Operation {
            name: payload.action.command().to_owned(),
            parameters,
        },
        target,
        after: after_state,
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
pub fn object_target(id: &str) -> Result<String> {
    crate::model::validate_object_id(id)?;
    Ok(format!("obj:{}", crate::reference::encode_uuid_str(id)?))
}

/// A formatted target must parse back as the identity it claims to be.
///
/// ReviewDigest input accepts the same compact reference grammar as every
/// persisted Ref. A second raw-UUID target dialect would make the digest
/// contract non-portable even if both strings happened to name the same bits.
fn check_canonical_target(target: &str) -> Result<()> {
    let reference = crate::reference::canonical_embedded(
        target,
        &[crate::reference::ResourceKind::Object],
        "Review target",
    )?;
    if let Some(section) = reference.section() {
        ensure!(
            section <= MAX_SAFE_INTEGER,
            EXIT_SCHEMA,
            "section id {section} is outside the shared safe-integer domain"
        );
    }
    Ok(())
}
/// The canonical target of an operation that names one Section.
pub fn section_target(id: &str, section: u64) -> Result<String> {
    ensure!(section > 0, EXIT_SCHEMA, "section ids start at 1");
    ensure!(
        section <= MAX_SAFE_INTEGER,
        EXIT_SCHEMA,
        "section id {section} is outside the shared safe-integer domain"
    );
    Ok(format!("{}:{section}", object_target(id)?))
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
            header: None,
            role: None,
            text: "wording".to_owned(),
            content: Vec::new(),
            based_on: None,
            refs: Vec::new(),
            relations: Vec::new(),
        };
        assert_eq!(
            canonical_bytes(&section, "section").expect("bytes"),
            concat!(
                r#"{"admission":"human","based_on":null,"content":[],"header":null,"#,
                r#""refs":[],"relations":[],"role":null,"text":"wording"}"#
            )
        );
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
}

#[cfg(test)]
mod table_tests {
    use super::*;
    use crate::model::{Content, Destination, Payload, SectionValue};
    use crate::semantics::Admitted;

    const AT: &str = "2026-08-23T00:00:00Z";

    fn value(text: &str) -> SectionValue {
        SectionValue::new(
            Admitted::new(Admission::Human, AT),
            Content {
                text: text.to_owned(),
                ..Content::default()
            },
        )
    }

    fn section(id: u64, text: &str) -> Section {
        Section::from_value(id, value(text)).expect("section")
    }

    /// The whole frozen table, checked for the two things a reader of another
    /// implementation needs: which target each operation names, and which
    /// projection describes the state it arrives at.
    #[test]
    fn every_operation_projects_the_shape_the_contract_freezes() {
        let id = crate::model::new_id();
        let mut before = Object::new(id.clone(), "before".to_owned()).expect("object");
        before.rev = 1;
        before.next_section_id = 3;
        before.sections = vec![section(1, "one"), section(2, "two")];
        before.reseal().expect("seal");
        let mut after = before.clone();
        after.rev = 2;
        after.title = "after".to_owned();
        after.reseal().expect("seal");

        let cases: Vec<(Action, String)> = vec![
            (
                Action::ObjectRenamed {
                    title: "after".to_owned(),
                    becomes: None,
                },
                object_target(&id).expect("object target"),
            ),
            (
                Action::ObjectStateChanged {
                    state: State::Closed,
                },
                object_target(&id).expect("object target"),
            ),
            (
                Action::ObjectClassified {
                    object_type: Some(ObjectType::Design),
                    state: State::Draft,
                },
                object_target(&id).expect("object target"),
            ),
            (
                Action::SectionUpdated {
                    section: 1,
                    value: value("one, revised"),
                    becomes: None,
                },
                section_target(&id, 1).expect("section target"),
            ),
            (
                Action::SectionDeleted {
                    section: 2,
                    becomes: None,
                },
                section_target(&id, 2).expect("section target"),
            ),
            (
                Action::SectionMerged {
                    merge: crate::model::Merge {
                        destination: 1,
                        sources: vec![2],
                    },
                    value: value("merged"),
                    becomes: None,
                },
                section_target(&id, 1).expect("section target"),
            ),
            (
                Action::ObjectRepaired {},
                object_target(&id).expect("object target"),
            ),
        ];
        for (action, target) in cases {
            let name = action.command().to_owned();
            let projected = object_projection(&before, &after, &Payload::new(id.clone(), action))
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(projected.target, target, "{name} names its own target");
            assert_eq!(projected.operation.name, name);
        }
    }

    /// The resulting Section id is read off what the reducer produced, never
    /// predicted — and creation therefore names the Object, not a Section.
    #[test]
    fn creation_names_the_object_and_reports_the_allocated_section() {
        let id = crate::model::new_id();
        let mut before = Object::new(id.clone(), "before".to_owned()).expect("object");
        before.rev = 1;
        before.reseal().expect("seal");
        let mut after = before.clone();
        after.rev = 2;
        after.next_section_id = 2;
        after.sections = vec![section(1, "new wording")];
        after.reseal().expect("seal");

        let projected = object_projection(
            &before,
            &after,
            &Payload::new(
                id.clone(),
                Action::SectionCreated {
                    value: value("new wording"),
                    becomes: None,
                },
            ),
        )
        .expect("projection");
        assert_eq!(projected.target, object_target(&id).expect("object target"));
        assert_eq!(projected.operation.parameters["section"], 1);
    }

    /// A destination is named in the parameters, because it is part of what the
    /// operation does rather than a fact about the Object it happens to have.
    #[test]
    fn a_destination_is_named_in_the_parameters() {
        let id = crate::model::new_id();
        let mut before = Object::new(id.clone(), "before".to_owned()).expect("object");
        before.rev = 1;
        before.next_section_id = 2;
        before.sections = vec![section(1, "one")];
        before.state = State::Closed;
        before.reseal().expect("seal");
        let mut after = before.clone();
        after.rev = 2;
        after.state = State::Open;
        after.sections = vec![section(1, "one, revised")];
        after.reseal().expect("seal");

        let projected = object_projection(
            &before,
            &after,
            &Payload::new(
                id,
                Action::SectionUpdated {
                    section: 1,
                    value: value("one, revised"),
                    becomes: Some(Destination {
                        object_type: None,
                        state: State::Open,
                    }),
                },
            ),
        )
        .expect("projection");
        assert_eq!(
            projected.operation.parameters["becomes"],
            serde_json::json!({ "state": "open", "type": null })
        );
    }

    /// Migration is confirmed as a whole through its own Challenge family, so it
    /// is not an Object-domain mutation and has no review projection.
    #[test]
    fn a_migration_bootstrap_has_no_review_projection() {
        let id = crate::model::new_id();
        let mut before = Object::new(id.clone(), String::new()).expect("object");
        before.rev = 0;
        before.reseal().expect("seal");
        let error = object_projection(
            &before,
            &before,
            &Payload::new(
                id,
                Action::ObjectMigrated {
                    snapshot: Box::new(crate::model::Snapshot {
                        title: "migrated".to_owned(),
                        object_type: None,
                        state: State::Open,
                        next_section_id: 1,
                        sections: Vec::new(),
                    }),
                },
            ),
        )
        .expect_err("migration is not an Object-domain mutation");
        assert_eq!(error.code, EXIT_SCHEMA);
    }
}

/// The Object-domain ReviewDigest `mutation`.
///
/// Three members and no others. The operation descriptor and target are the
/// frozen ones — `object_projection`, read rather than rebuilt — so that two
/// implementations describing one operation reach one shape.
///
/// `before` is deliberately absent. The review's predecessor is a concurrency
/// fact and lives in [`ReviewPrecondition`].
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct ReviewMutation {
    operation: Operation,
    target: String,
    /// The after projection **under the admission path that actually ran**.
    ///
    /// Not normalized to Human semantics: an Agent-reviewed Section projects
    /// `admission=agent`, and a Human candidate projects the Human-authorized
    /// result including promotion where that applies. Forcing one of them would
    /// make an Agent review and a Human confirmation of the same wording share
    /// a proof, which is the one thing mixed authority must not do.
    after: serde_json::Value,
}

impl ReviewMutation {
    pub fn operation(&self) -> &Operation {
        &self.operation
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn after(&self) -> &serde_json::Value {
        &self.after
    }
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
/// Reads [`object_projection`] rather than rebuilding its table, so "the
/// operation descriptor is the frozen one" is a fact about the build instead of
/// a promise in a comment.
pub fn object_review_mutation(
    before: &Object,
    after: &Object,
    payload: &Payload,
) -> Result<ReviewMutation> {
    let projection = object_projection(before, after, payload)?;
    Ok(ReviewMutation {
        operation: projection.operation,
        target: projection.target,
        after: projection.after,
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
#[derive(Serialize, serde::Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
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
#[derive(Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ReviewResult {
    Passed,
    Failed,
    Exhausted,
}

/// Check a stored Rule Review block against itself.
///
/// A Challenge digest covering these bytes proves only that nobody edited the
/// envelope after it was written. It says nothing about whether the block was
/// *coherent when written*, and a review block that names a review identity its
/// own contents do not produce is exactly what a human must not be shown as
/// settled. This is required on load and render, not only at confirmation, and
/// it fails closed.
///
/// This is the half that needs no workspace: everything here is decidable from
/// the candidate's own bytes. Recomputing the review *identity* additionally
/// needs the mutation descriptor projection, which is [`check_review_identity`].
pub fn check_review_report(review: &CandidateReview) -> Result<()> {
    let mut canonical_rules = review.rules.clone();
    canonical_set(&mut canonical_rules, "rule")?;
    ensure!(
        canonical_rules == review.rules,
        EXIT_SCHEMA,
        "candidate Rule snapshots must use canonical set order"
    );
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
    check_named_identity(review, &binding)
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
    let binding = crate::rules::rebind_object(mutation, expected_rev, review.rules.clone())?;
    check_named_identity(review, &binding)
}

/// Compare a stored review identity against a binding rebuilt from snapshots.
fn check_named_identity(
    review: &CandidateReview,
    binding: &crate::rules::ReviewBinding,
) -> Result<()> {
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
            commit: None,
            dirty: true,
            content_sha256: Some("e".repeat(64)),
        }
    }

    /// The frozen Object-domain descriptor of #25 §4, built the way a caller
    /// must build it. An ad-hoc mutation is refused at the binding boundary now
    /// — which is the point, and which is why these fixtures cannot be a
    /// convenient blob.
    /// The frozen descriptor, built the way the only entry point builds it.
    /// The untyped route is closed for this domain now, so a test that wants an
    /// Object binding has to ask for one the same way production does.
    fn binding_inputs() -> (ReviewMutation, u64) {
        let mutation = ReviewMutation {
            operation: Operation {
                name: "section_revised".to_owned(),
                parameters: serde_json::json!({"section": 1}),
            },
            target: "obj:01jbrcg6hbfgyrwkttddy8v7gf:1".to_owned(),
            after: serde_json::json!({"admission": "human", "text": "revised"}),
        };
        (mutation, 7)
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
        let (mutation, expected_rev) = binding_inputs();
        let rules = vec![rule("a", 5)];
        let binding =
            crate::rules::rebind_object(&mutation, expected_rev, rules.clone()).expect("rebind");
        let honest = CandidateReview {
            review_digest: crate::digest::REVIEW
                .emit(binding.digest_under(1).expect("digest"))
                .expect("emit")
                .to_string(),
            ..review(ReviewResult::Passed, 1, rules)
        };
        check_object_review_identity(&honest, &mutation, expected_rev)
            .expect("its own snapshots produce the name it gives itself");

        // Same digest, a ceiling quietly loosened. The review identity covers
        // the effective policy, so this cannot pass as the same review.
        let mut edited = honest.clone();
        edited.rules = vec![rule("a", 50)];
        let caught = check_object_review_identity(&edited, &mutation, expected_rev)
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
        let (mutation, expected_rev) = binding_inputs();
        let one =
            crate::rules::rebind_object(&mutation, expected_rev, vec![rule("a", 5), rule("b", 2)])
                .expect("rebind");
        let other =
            crate::rules::rebind_object(&mutation, expected_rev, vec![rule("b", 2), rule("a", 5)])
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
        Section::from_value(
            id,
            crate::model::SectionValue::new(
                crate::semantics::Admitted::new(admission, "2026-08-24T00:00:00Z"),
                Content {
                    text: text.to_owned(),
                    ..Content::default()
                },
            ),
        )
        .expect("section")
    }

    fn value(admission: Admission, text: &str) -> crate::model::SectionValue {
        crate::model::SectionValue::new(
            crate::semantics::Admitted::new(admission, "2026-08-24T00:00:00Z"),
            Content {
                text: text.to_owned(),
                ..Content::default()
            },
        )
    }

    fn revised(admission: Admission) -> (Object, Object, Payload) {
        let mut before =
            Object::new(object_id(), "the append boundary".to_owned()).expect("object");
        before.rev = 7;
        before.next_section_id = 2;
        before.sections = vec![section(1, admission, "as first written")];
        before.reseal().expect("seal");
        let mut after = before.clone();
        after.rev = 8;
        after.sections = vec![section(1, admission, "as revised")];
        after.reseal().expect("seal");
        let payload = Payload::new(
            object_id(),
            Action::SectionUpdated {
                section: 1,
                value: value(admission, "as revised"),
                becomes: None,
            },
        );
        (before, after, payload)
    }

    /// The review mutation is the frozen projection's operation, target and
    /// after — read off it rather than rebuilt beside it.
    #[test]
    fn the_review_mutation_is_the_frozen_projection() {
        let (before, after, payload) = revised(Admission::Human);
        let projected = object_projection(&before, &after, &payload).expect("projection");
        let mutation = object_review_mutation(&before, &after, &payload).expect("mutation");

        assert_eq!(mutation.operation, projected.operation);
        assert_eq!(mutation.target, projected.target);
        assert_eq!(mutation.after, projected.after);

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
        let mutation = object_review_mutation(&before, &after, &payload).expect("mutation");
        let binding =
            crate::rules::rebind_object(&mutation, before.rev, Vec::new()).expect("rebind");

        let canonical = canonical_bytes(&binding, "review binding").expect("canonical");
        assert_eq!(
            canonical,
            r#"{"domain":"object","mutation":{"after":{"lifecycle":{"state":"open","type":null},"section":{"admission":"human","based_on":null,"content":[],"header":null,"refs":[],"relations":[],"role":null,"text":"as revised"}},"operation":{"name":"section.update","parameters":{"becomes":null}},"target":"obj:01jbrcg6hbfgyrwkttddy8v7gf:1"},"precondition":{"expected_rev":7},"rules":[]}"#,
            "the frozen JCS bytes of an object review binding"
        );
        assert_eq!(
            binding.digest().expect("digest").to_string(),
            "1:339dff0725eda8ac29ff2893fa3fb2f15a7a1dc57e6c40c2cb17fc85bb9d82ba"
        );
    }

    /// The untyped route is closed for this domain, and closed by name.
    ///
    /// Checking the outer member names was never enough. An unfrozen operation
    /// name, an extra member inside `operation`, a target naming nothing and an
    /// arbitrary `after` all passed that check and produced a scalar labelled
    /// `ReviewDigestContract 1` for a descriptor #25 does not define. Nor could
    /// a shape check have fixed it: whether `after` is *that operation's*
    /// projection is knowable only from the projection.
    ///
    /// So the guarantee comes from the type instead. An Object binding can only
    /// be built from a `ReviewMutation`, and a `ReviewMutation` can only come
    /// from `object_review_mutation` reading a real transition through the
    /// frozen table.
    #[test]
    fn an_object_review_cannot_be_bound_from_arbitrary_json() {
        for (why, mutation) in [
            (
                "the pre-freeze prototype shape",
                serde_json::json!({"action": "section_revised"}),
            ),
            (
                "an operation the table does not define",
                serde_json::json!({
                    "operation": {"name": "not.a.frozen.operation", "parameters": {}},
                    "target": "obj:01jbrcg6hbfgyrwkttddy8v7gf:1",
                    "after": serde_json::Value::Null
                }),
            ),
            (
                "an operation carrying a member of its own",
                serde_json::json!({
                    "operation": {"name": "section.revised", "parameters": {}, "extra": 1},
                    "target": "obj:01jbrcg6hbfgyrwkttddy8v7gf:1",
                    "after": serde_json::Value::Null
                }),
            ),
            (
                "a target naming nothing",
                serde_json::json!({
                    "operation": {"name": "section.revised", "parameters": {}},
                    "target": "not a target",
                    "after": serde_json::Value::Null
                }),
            ),
        ] {
            let refused = crate::rules::rebind(
                crate::rules::Domain::Object,
                mutation,
                serde_json::json!({"expected_rev": 7}),
                Vec::new(),
            )
            .expect_err(why);
            assert!(
                refused.to_string().contains("bind_object"),
                "{why}: {refused}"
            );
        }
    }

    /// Creation binds `expected_rev = 0`, which is a predecessor rather than a
    /// missing one, so it must survive the boundary rather than read as absent.
    #[test]
    fn creation_binds_the_zero_predecessor() {
        let before = Object::new(object_id(), "the append boundary".to_owned()).expect("object");
        let mut after = before.clone();
        after.rev = 1;
        let payload = Payload::new(
            object_id(),
            Action::ObjectCreated {
                title: "the append boundary".to_owned(),
            },
        );
        let mutation = object_review_mutation(&before, &after, &payload).expect("mutation");
        crate::rules::rebind_object(&mutation, 0, Vec::new())
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
