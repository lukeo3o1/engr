//! What the Phase-3 seals protect, stated as the consequences #31 names.
//!
//! Every test here asks the same question from a different side: can a
//! schema-valid edit to persisted state survive with its old seal? The answer
//! has to be no for each protected field, and yes — deliberately — for the
//! incidental encoding choices that carry no meaning.

use engr::integrity::{
    check_mechanical_reseal, check_object_integrity, check_object_seal, check_section_seal, mutate,
    reseal, sealed_section,
};
mod common;

use engr::model::{Content, Object, Ref, Section, SectionValue};
use engr::semantics::{Admission, Admitted, ObjectType, Relation, Role, State, Supplement};

const AT: &str = "2026-08-24T00:00:00Z";

fn object_id() -> String {
    "0192f0c8-1a2b-7c3d-8e4f-5a6b7c8d9e0f".to_owned()
}

fn value(content: Content) -> SectionValue {
    SectionValue::new(Admitted::new(Admission::Human, AT), content)
}

fn section(id: u64) -> Section {
    Section::from_value(
        id,
        value(Content {
            role: Some(Role::Decision),
            text: "the store appends under a lock".to_owned(),
            ..Content::default()
        }),
    )
    .expect("section")
}

/// The same Object, for a test that has already shadowed the name.
fn fresh() -> Object {
    object()
}

/// An Object whose Sections carry the seals their own contents produce — the
/// state a coherent workspace is in, and the only state the consequence table
/// is written about.
fn object() -> Object {
    let mut object = Object::new(object_id(), "the append boundary".to_owned()).expect("object");
    object.rev = 3;
    object.next_section_id = 3;
    object.sections = vec![section(1), section(2)];
    object.reseal().expect("seal");
    object
}

fn seal_of(object: &Object) -> String {
    object.recomputed_digest().expect("seal")
}

fn seal_of_section(section: &Section) -> String {
    section.recomputed_digest().expect("seal")
}

/// The three consequences #31 spells out, in one place.
#[test]
fn a_change_anywhere_under_the_seal_is_visible_from_the_object() {
    let original = object();
    let sealed = seal_of(&original);

    // Change Section data. Its own seal no longer follows from it, and the
    // aggregate — which carries both — moves as well.
    let mut edited = original.clone();
    edited.sections[0].text = "the store appends without a lock".to_owned();
    check_section_seal(&edited.sections[0]).expect_err("the section seal says so");
    assert_ne!(sealed, seal_of(&edited), "change section data");
    check_object_seal(&edited).expect_err("and so does the object seal");

    // Change only a Section's stored seal, leaving every semantic field alone.
    // Both consequences must hold: the Section fails verification, and the
    // aggregate fails too — because it hashes the stored seal, not a fresh one.
    let mut retagged = original.clone();
    retagged.sections[0].digest = format!("1:{}", "f".repeat(64));
    check_section_seal(&retagged.sections[0]).expect_err("the contents do not produce that seal");
    assert_ne!(sealed, seal_of(&retagged), "change only Section.digest");

    // Rewrite the contents and the seal together. The aggregate is internally
    // coherent, every Section verifies, and only the stored aggregate value
    // says anything is wrong — which is why the aggregate exists.
    let mut consistent = original.clone();
    consistent.sections[0].text = "quietly different".to_owned();
    consistent.sections[0].digest = seal_of_section(&consistent.sections[0]);
    let mut resealed = consistent.clone();
    resealed.reseal().expect("seal");
    check_object_integrity(&resealed).expect("nothing internal disagrees");
    check_object_integrity(&consistent).expect_err("but it is not what was sealed");

    // Remove a section.
    let mut shortened = original.clone();
    shortened.sections.pop();
    assert_ne!(sealed, seal_of(&shortened), "remove a section");

    // Reassign one section's id to another's.
    let mut swapped = original.clone();
    swapped.sections[1].id = 1;
    swapped
        .validate()
        .expect_err("two sections cannot claim one id");
}

/// Every field the contract protects, one edit at a time.
///
/// A list rather than one representative case, because the defect this guards
/// against is a field quietly left out of the projection — and a projection
/// missing one field passes every test written about the others.
#[test]
fn each_protected_section_field_moves_the_seal() {
    let original = section(1);
    let sealed = seal_of_section(&original);

    let mut moved = original.clone();
    moved.id = 2;
    assert_ne!(sealed, seal_of_section(&moved), "id");

    let mut moved = original.clone();
    moved.admitted.by = Admission::Agent;
    assert_ne!(
        sealed,
        seal_of_section(&moved),
        "admitted.by — a Section that changed door is a different Section"
    );

    let mut moved = original.clone();
    moved.header = Some("a label".to_owned());
    assert_ne!(sealed, seal_of_section(&moved), "header");

    let mut moved = original.clone();
    moved.role = Some(Role::Risk);
    assert_ne!(sealed, seal_of_section(&moved), "role");

    let mut moved = original.clone();
    moved.role = None;
    assert_ne!(sealed, seal_of_section(&moved), "role removed");

    let mut moved = original.clone();
    moved.text.push('.');
    assert_ne!(sealed, seal_of_section(&moved), "text");

    let mut moved = original.clone();
    moved.content = vec![Supplement::new("data.note", "a note")];
    assert_ne!(sealed, seal_of_section(&moved), "content");

    let mut moved = original.clone();
    moved.based_on = Some(engr::semantics::BasedOn::new("a".repeat(40)));
    assert_ne!(sealed, seal_of_section(&moved), "based_on");

    let mut moved = original.clone();
    moved.refs = vec![reference(9)];
    assert_ne!(sealed, seal_of_section(&moved), "refs");

    let mut moved = original.clone();
    moved.relations = vec![Relation::superseded_by(object_id())];
    assert_ne!(sealed, seal_of_section(&moved), "relations");

    let mut moved = original.clone();
    moved.admitted.at = "2026-08-25T00:00:00Z".to_owned();
    assert_ne!(sealed, seal_of_section(&moved), "admitted.at");

    // And the one field that must not participate, because it is the answer.
    let mut moved = original.clone();
    moved.digest = format!("1:{}", "e".repeat(64));
    assert_eq!(
        sealed,
        seal_of_section(&moved),
        "a seal cannot cover itself"
    );
}

fn reference(section: u64) -> Ref {
    engr::dependency::SelectiveRef::stored(
        engr::proof::section_target(&object_id(), section),
        vec![engr::dependency::SemanticField::Text],
        "d".repeat(40),
        format!("1:{}", "c".repeat(64)),
    )
    .expect("a well formed stored reference")
}

/// One persisted order, enforced where the bytes are read rather than smoothed
/// over where they are hashed.
///
/// The seal is taken over the persisted value, so a reordered set really does
/// hash differently — which is the point. What stops two spellings of one value
/// from both being valid authority is the shape boundary: a stored set that is
/// not in canonical order, or a Section list that is not in increasing id order,
/// is refused as schema before any seal is consulted.
#[test]
fn one_persisted_order_is_enforced_where_the_bytes_are_read() {
    let (_dir, root) = common::workspace();
    let id = common::new_object(&root, "ordering");
    common::admit(
        &root,
        common::add(&id, common::wording("wording with no references")),
    );
    let path = engr::store::object_path(&root, &id);
    let sound: serde_json::Value = engr::store::read_json(&path).expect("read");

    // Two Sections, written out of id order. Every seal in the file is still
    // the one its own contents produce; only the array order moved.
    let mut object = engr::store::load_object(&root, &id).expect("object");
    let second = Section::from_value(
        2,
        value(Content {
            text: "a second point".to_owned(),
            ..Content::default()
        }),
    )
    .expect("section");
    object.next_section_id = 3;
    object.sections = vec![second, object.sections[0].clone()];
    object.reseal().expect("seal");
    std::fs::write(
        &path,
        engr::proof::canonical_bytes(&object, "object").expect("canonical"),
    )
    .expect("write");
    let error = engr::store::load_object(&root, &id).expect_err("out of id order");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(
        error.message.contains("increasing id order"),
        "{}",
        error.message
    );

    // And the same for a set inside one Section.
    std::fs::write(
        &path,
        engr::proof::canonical_bytes(&sound, "object").expect("canonical"),
    )
    .expect("restore");
    let mut object = engr::store::load_object(&root, &id).expect("object");
    let mut refs = vec![reference(4), reference(2), reference(9)];
    engr::proof::canonical_set(&mut refs, "reference").expect("canonical");
    refs.reverse();
    object.sections[0].refs = refs;
    object.sections[0] = sealed_section(&object.sections[0]).expect("seal");
    object.reseal().expect("seal");
    std::fs::write(
        &path,
        engr::proof::canonical_bytes(&object, "object").expect("canonical"),
    )
    .expect("write");
    let error = engr::store::load_object(&root, &id).expect_err("out of canonical set order");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(
        error.message.contains("canonical set order"),
        "{}",
        error.message
    );
}

/// A set holding one member twice is not a set, and is refused rather than
/// deduplicated — silently dropping one would reseal a file whose contents the
/// caller never agreed to change.
#[test]
fn a_canonical_duplicate_is_refused_rather_than_collapsed() {
    let mut section = section(1);
    section.refs = vec![reference(2), reference(2)];
    let refused = section.validate().expect_err("the same reference twice");
    assert!(refused.to_string().contains("twice"), "{refused}");
}

/// #31 and #35 both say it: no Unicode normalization before hashing. Two
/// spellings that render identically are different persisted values.
#[test]
fn visually_equal_text_is_not_the_same_integrity_input() {
    let mut composed = section(1);
    composed.text = "café".to_owned();
    let mut decomposed = section(1);
    decomposed.text = "cafe\u{301}".to_owned();
    assert_ne!(composed.text, decomposed.text);
    assert_ne!(
        seal_of_section(&composed),
        seal_of_section(&decomposed),
        "no normalization pass stands between the file and the hash"
    );
}

/// An Object carries no per-resource schema markers at all, so there is nothing
/// for the aggregate to have to exclude.
#[test]
fn an_object_carries_no_per_resource_format_markers() {
    let value = serde_json::to_value(object()).expect("json");
    let members: Vec<&String> = value.as_object().expect("object").keys().collect();
    assert!(!members.contains(&&"format".to_owned()), "{members:?}");
    assert!(!members.contains(&&"version".to_owned()), "{members:?}");
    assert_eq!(
        members,
        vec![
            "digest",
            "id",
            "next_section_id",
            "rev",
            "sections",
            "state",
            "title"
        ],
        "the generation is the workspace's, not each file's"
    );
}

/// Every stable Object field participates, including the counter.
#[test]
fn each_protected_object_field_moves_the_aggregate() {
    let original = object();
    let sealed = seal_of(&original);

    let mut moved = original.clone();
    moved.title = "the append boundary, revised".to_owned();
    assert_ne!(sealed, seal_of(&moved), "title");

    let mut moved = original.clone();
    moved.object_type = Some(ObjectType::Decision);
    assert_ne!(sealed, seal_of(&moved), "type");

    let mut moved = original.clone();
    moved.state = State::Accepted;
    assert_ne!(sealed, seal_of(&moved), "state");

    let mut moved = original.clone();
    moved.rev += 1;
    assert_ne!(sealed, seal_of(&moved), "rev");

    let mut moved = original.clone();
    moved.next_section_id += 1;
    assert_ne!(
        sealed,
        seal_of(&moved),
        "next_section_id — wind it back by hand and the next section reuses an id"
    );

    check_object_seal(&original).expect("unchanged");
}

/// The nested Section representation carries each field once. #31 warns that a
/// conceptual `id + all fields + sha256` reads like an instruction to write
/// `id` twice; this pins that it does not.
#[test]
fn the_nested_section_representation_holds_each_field_once() {
    let value = serde_json::to_value(object()).expect("json");
    let first = value["sections"][0].as_object().expect("object").clone();
    assert_eq!(first["id"], serde_json::json!(1));
    let mut members: Vec<&str> = first.keys().map(String::as_str).collect();
    members.sort_unstable();
    assert_eq!(
        members,
        vec!["admitted", "digest", "id", "role", "text"],
        "each member once, and everything empty omitted: {first:?}"
    );
    assert!(first.contains_key("digest"), "and the seal is one of them");
}

/// The shared safe-integer domain applies before anything is hashed. An
/// oversized counter is schema-invalid for v3 and cannot be rounded into a
/// seal — `2^53` is an exact binary64 value, which is precisely why refusing it
/// has to be deliberate rather than left to the float conversion.
#[test]
fn an_integer_outside_the_shared_domain_cannot_be_sealed() {
    let mut object = object();
    object.next_section_id = 1 << 53;
    let refused = object
        .recomputed_digest()
        .expect_err("outside the common ceiling");
    assert!(refused.to_string().contains("safe"), "{refused}");

    let mut section = section(1);
    section.id = (1 << 53) - 1;
    seal_of_section(&section);
}

/// Verification reports which resource disagreed, and fails closed.
#[test]
fn a_broken_seal_is_an_error_rather_than_a_flag() {
    let mut object = object();
    let wrong = format!("1:{}", "0".repeat(64));
    object.digest = wrong.clone();
    let refused = check_object_seal(&object).expect_err("not this object");
    assert!(refused.to_string().contains(&object.id), "{refused}");

    let mut other = fresh();
    other.sections[0].digest = wrong;
    let refused = check_section_seal(&other.sections[0]).expect_err("not this section");
    assert!(refused.to_string().contains("section 1"), "{refused}");
}

/// A Section whose stored seal is stale is caught by the Section check, not by
/// the aggregate check — the aggregate only knows the bytes it was given.
/// Pinning which check catches what is the point: a caller that ran one and
/// believed it had run both is the failure this pair exists to make impossible.
#[test]
fn the_two_checks_catch_different_lies() {
    let mut object = object();
    let aggregate = seal_of(&object);

    object.sections[1].text = "changed under a valid aggregate".to_owned();
    object.sections[1].digest = seal_of_section(&object.sections[1]);
    let resealed = seal_of(&object);

    let mut coherent = object.clone();
    coherent.reseal().expect("seal");
    check_object_integrity(&coherent).expect("internally coherent");
    assert_ne!(
        coherent.digest, aggregate,
        "but not the object that was sealed"
    );
    let _ = resealed;

    // Now break only the internal agreement, and reseal the aggregate over it.
    object.sections[1].digest = format!("1:{}", "a".repeat(64));
    object.reseal().expect("seal");
    check_object_seal(&object).expect("the aggregate is happy");
    let refused = check_object_integrity(&object).expect_err("the section it covers is not");
    assert!(refused.to_string().contains("section 2"), "{refused}");
}

/// An integrity-invalid resource does not get quietly normalized on the way
/// past. The mutation is refused **before** it is applied, so unrelated work
/// cannot launder an out-of-band edit into a valid-looking seal.
#[test]
fn a_mutation_over_invalid_state_is_refused_before_it_runs() {
    let object = object();
    let _sealed = seal_of(&object);

    // Hand-edit a Section and leave its seal alone: the file still parses and
    // every schema check passes.
    let mut tampered = object.clone();
    tampered.sections[0].text = "quietly rewritten".to_owned();

    let mut ran = false;
    let refused = mutate(&tampered, |object| {
        ran = true;
        object.title = "and mutated on top".to_owned();
        Ok(())
    })
    .expect_err("the predecessor does not verify");
    assert!(!ran, "the mutation must not run at all: {refused}");

    // Same object, same edit, resealing instead of mutating: also refused.
    reseal(&tampered).expect_err("a reseal is not a repair path");
}

/// The order in #35 §12 is not decoration. Sections are resealed first and the
/// aggregate is taken over the fresh values — an aggregate computed first would
/// cover seals that were about to be replaced.
#[test]
fn a_mutation_reseals_the_sections_before_the_aggregate() {
    let object = object();
    let sealed = seal_of(&object);

    let done = mutate(&object, |object| {
        object.sections[0].text = "revised under the gate".to_owned();
        object.rev += 1;
        Ok(())
    })
    .expect("authorized");

    check_object_integrity(&done.object).expect("coherent afterwards");
    assert_ne!(done.seal, sealed, "the object moved");
    assert_ne!(
        done.object.sections[0].digest, object.sections[0].digest,
        "and so did the section it changed"
    );
    assert_eq!(
        done.object.sections[1].digest, object.sections[1].digest,
        "the one it did not touch is untouched"
    );
}

/// A mutation that fails leaves the caller with nothing to write, and the
/// predecessor it was given untouched.
#[test]
fn a_refused_mutation_produces_no_object_at_all() {
    let object = object();
    let _sealed = seal_of(&object);
    let before = object.clone();

    mutate(&object, |object| {
        object.title = "half done".to_owned();
        Err(engr::Error::new(engr::EXIT_INVARIANT, "no".to_owned()))
    })
    .expect_err("the mutation refused itself");

    assert_eq!(object, before, "the predecessor is untouched");
}

/// Mechanical reseal may recompute seals and nothing else.
///
/// The three MUST NOTs of #35 §11 are checked by comparing the projections
/// either side, not by trusting that some function only assigns to `sha256` —
/// which is why the check is reachable here at all. `reseal` itself applies no
/// mutation, so a guard that lived only inside it could never be exercised, and
/// would go on passing after the edit that broke it.
#[test]
fn a_mechanical_reseal_cannot_reach_a_semantic_field() {
    let object = object();
    let sealed = seal_of(&object);

    // The honest case: nothing semantic moved, so the seal does not move either.
    let done = reseal(&object).expect("nothing moved");
    assert_eq!(done.seal, sealed, "and the seal says so");
    assert_eq!(done.object, object);

    type Forbidden = fn(&mut Object) -> engr::Result<()>;
    let forbidden: [(&str, Forbidden); 5] = [
        ("admitted.by", |object| {
            object.sections[0].admitted.by = Admission::Agent;
            Ok(())
        }),
        ("admitted.at", |object| {
            object.sections[0].admitted.at = "2026-08-25T00:00:00Z".to_owned();
            Ok(())
        }),
        ("text", |object| {
            object.sections[0].text = "resealed into something else".to_owned();
            Ok(())
        }),
        ("title", |object| {
            object.title = "renamed by a reseal".to_owned();
            Ok(())
        }),
        ("a whole section", |object| {
            object.sections.pop();
            Ok(())
        }),
    ];

    for (what, apply) in forbidden {
        let done = mutate(&object, apply).expect("a real mutation may do this");
        let refused = check_mechanical_reseal(&object, &done.object)
            .expect_err("but a mechanical reseal may not");
        assert!(
            refused.to_string().contains("mechanical reseal"),
            "{what}: {refused}"
        );
    }
}

/// Resealing an Object that has legitimately moved is fine; what is refused is
/// calling that movement mechanical.
#[test]
fn a_reseal_after_an_authorized_mutation_is_the_normal_case() {
    let object = object();
    let _sealed = seal_of(&object);

    let admitted = mutate(&object, |object| {
        object.sections[0].admitted.by = Admission::Agent;
        object.rev += 1;
        Ok(())
    })
    .expect("the authority path already said yes");

    check_object_integrity(&admitted.object).expect("sealed over what it now says");
    reseal(&admitted.object).expect("and reseals to itself unchanged");
}

/// Persisted members the seal deliberately does not cover, each with a reason
/// the contract gives.
const NOT_SEALED: &[(&str, &str)] = &[("digest", "a seal cannot cover itself")];

fn members(value: &serde_json::Value) -> Vec<String> {
    value
        .as_object()
        .expect("a resource is a JSON object")
        .keys()
        .cloned()
        .collect()
}

/// Every member the persisted shape writes is either under the seal or on a
/// list saying why it is not.
///
/// The claim "`Object.digest` covers every stable persisted Object field except
/// itself" is otherwise enforced by nobody: add a field to `Object` and the
/// seal input silently stops covering it, every existing test still passes, and
/// the new field is one a hand-edit can change without detection.
///
/// Deliberately a list of exclusions rather than a list of inclusions: forget
/// to extend it and the test fails, which is the direction that fails safe.
#[test]
fn no_persisted_field_escapes_the_seal() {
    // Everything optional is populated, so nothing is missing merely because it
    // was empty and therefore omitted.
    let full = Section::from_value(
        1,
        value(Content {
            header: Some("a label".to_owned()),
            role: Some(Role::Decision),
            text: "the store appends under a lock".to_owned(),
            content: vec![Supplement::new("data.note", "a note")],
            based_on: Some(engr::semantics::BasedOn::new("a".repeat(40))),
            refs: vec![reference(2)],
            relations: vec![Relation::superseded_by(object_id())],
        }),
    )
    .expect("section");

    let persisted = members(&serde_json::to_value(&full).expect("persisted section"));
    assert!(
        persisted.contains(&"digest".to_owned()),
        "the fixture must exercise the excluded member too: {persisted:?}"
    );
    for member in &persisted {
        if NOT_SEALED.iter().any(|(name, _)| name == member) {
            continue;
        }
        let mut edited = serde_json::to_value(&full).expect("json");
        edited[member] = serde_json::Value::String("moved".to_owned());
        edited.as_object_mut().expect("object").remove("digest");
        let moved = engr::proof::sha256_of(
            &engr::proof::canonical_bytes(&edited, "section").expect("canonical"),
        );
        assert_ne!(
            format!("1:{moved}"),
            full.digest,
            "the persisted Section writes {member:?} and the seal does not cover it"
        );
    }

    // And the same for the Object's own members.
    let mut object = object();
    object.object_type = Some(ObjectType::Decision);
    object.reseal().expect("seal");
    let persisted = members(&serde_json::to_value(&object).expect("persisted object"));
    for member in &persisted {
        if NOT_SEALED.iter().any(|(name, _)| name == member) {
            continue;
        }
        let mut edited = serde_json::to_value(&object).expect("json");
        edited[member] = serde_json::Value::String("moved".to_owned());
        edited.as_object_mut().expect("object").remove("digest");
        let moved = engr::proof::sha256_of(
            &engr::proof::canonical_bytes(&edited, "object").expect("canonical"),
        );
        assert_ne!(
            format!("1:{moved}"),
            object.digest,
            "the persisted Object writes {member:?} and the seal does not cover it"
        );
    }
}

/// A representation-only rewrite may reorder the stored `sections[]` array,
/// because the protocol canonicalizes it by `Section.id` and says incidental
/// array position is not integrity meaning. The equivalence guard has to prove
/// semantic equality under *that* identity, not under input position — or the
/// one operation it exists to authorize is the one it refuses.
#[test]
fn reordered_sections_are_the_same_object_to_the_equivalence_guard() {
    let forward = object();
    let mut backward = forward.clone();
    backward.sections.reverse();
    check_mechanical_reseal(&forward, &backward).expect("the guard reads identity, not position");

    // And it still catches a real change hiding behind a reorder.
    let mut edited = backward.clone();
    let last = edited.sections.len() - 1;
    edited.sections[last].text = "changed while shuffled".to_owned();
    check_mechanical_reseal(&forward, &edited).expect_err("reordering is not cover");

    // A section replaced by a duplicate of another is not a reorder either.
    let mut duplicated = forward.clone();
    duplicated.sections[1] = duplicated.sections[0].clone();
    check_mechanical_reseal(&forward, &duplicated).expect_err("section 2 is gone");
}

/// The canonical Section omits every optional member it does not carry, and the
/// seal is over exactly those bytes.
///
/// This is a permanent byte contract, so it is pinned as bytes. The failure it
/// exists to catch is a member quietly *losing* its `skip_serializing_if` — a
/// change that reads as tidy, breaks nothing locally, and silently gives every
/// Section with an empty `relations` a different seal from the one another
/// implementation computes.
#[test]
fn the_canonical_section_omits_what_it_does_not_carry() {
    let bare = Section::from_value(
        1,
        value(Content {
            text: "the store appends under a lock".to_owned(),
            ..Content::default()
        }),
    )
    .expect("section");

    let value = serde_json::to_value(&bare).expect("json");
    let members = value.as_object().expect("object");
    for omitted in ["header", "role", "based_on", "content", "refs", "relations"] {
        assert!(
            !members.contains_key(omitted),
            "{omitted} is omitted when empty, never written out: {members:?}"
        );
    }
    assert_eq!(
        members.len(),
        4,
        "id, admitted, text and the seal: {members:?}"
    );

    // The bytes themselves, because the contract is bytes.
    let mut unsealed = value.clone();
    unsealed.as_object_mut().expect("object").remove("digest");
    assert_eq!(
        engr::proof::canonical_bytes(&unsealed, "section").expect("canonical"),
        concat!(
            r#"{"admitted":{"at":"2026-08-24T00:00:00Z","by":"human"},"#,
            r#""id":1,"text":"the store appends under a lock"}"#
        )
    );
    check_section_seal(&bare).expect("and it seals over exactly that");
}

/// An Object with no Sections omits the member rather than writing `[]`, so the
/// two spellings cannot seal differently.
#[test]
fn an_object_with_no_sections_omits_the_member() {
    let mut object = object();
    object.sections.clear();
    object.reseal().expect("seal");
    let value = serde_json::to_value(&object).expect("json");
    assert!(
        !value.as_object().expect("object").contains_key("sections"),
        "{value}"
    );
    check_object_integrity(&object).expect("and it still seals");
}
