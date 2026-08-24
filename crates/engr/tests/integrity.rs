//! What the Phase-3 seals protect, stated as the consequences #31 names.
//!
//! Every test here asks the same question from a different side: can a
//! schema-valid edit to persisted state survive with its old seal? The answer
//! has to be no for each protected field, and yes — deliberately — for the
//! incidental encoding choices that carry no meaning.

use engr::integrity::{
    check_mechanical_reseal, check_object_integrity, check_object_seal, check_section_seal, mutate,
    reseal, sealed_object, sealed_section,
};
use engr::model::{Object, Ref, Section};
use engr::semantics::{Admission, ObjectType, Relation, Role, State, Supplement};

fn object_id() -> String {
    "0192f0c8-1a2b-7c3d-8e4f-5a6b7c8d9e0f".to_owned()
}

fn section(id: u64) -> Section {
    Section {
        id,
        admission: Admission::Human,
        role: Some(Role::Decision),
        text: "the store appends under a lock".to_owned(),
        content: Vec::new(),
        based_on: None,
        refs: Vec::new(),
        relations: Vec::new(),
        sha256: String::new(),
        admitted_at: "2026-08-24T00:00:00Z".to_owned(),
    }
}

/// An Object whose Sections carry the seals their own contents produce — the
/// state a coherent workspace is in, and the only state the consequence table
/// is written about.
fn object() -> Object {
    let mut object = Object::new(object_id(), "the append boundary".to_owned()).expect("object");
    object.rev = 3;
    object.next_section_id = 3;
    object.sections = vec![sealed(section(1)), sealed(section(2))];
    object
}

fn sealed(mut section: Section) -> Section {
    section.sha256 = seal_of_section(&section);
    section
}

fn seal_of(object: &Object) -> String {
    sealed_object(object)
        .expect("project")
        .seal()
        .expect("seal")
}

fn seal_of_section(section: &Section) -> String {
    sealed_section(section)
        .expect("project")
        .seal()
        .expect("seal")
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
    check_section_seal(&edited.sections[0], &edited.sections[0].sha256.clone())
        .expect_err("the section seal says so");
    assert_ne!(sealed, seal_of(&edited), "change section data");
    check_object_seal(&edited, &sealed).expect_err("and so does the object seal");

    // Change only a Section's stored seal, leaving every semantic field alone.
    // Both consequences must hold: the Section fails verification, and the
    // aggregate fails too — because it hashes the stored seal, not a fresh one.
    let mut retagged = original.clone();
    retagged.sections[0].sha256 = "f".repeat(64);
    check_section_seal(&retagged.sections[0], &"f".repeat(64))
        .expect_err("the contents do not produce that seal");
    assert_ne!(sealed, seal_of(&retagged), "change only Section.sha256");

    // Rewrite the contents and the seal together. The aggregate is internally
    // coherent, every Section verifies, and only the stored aggregate value
    // says anything is wrong — which is why the aggregate exists.
    let mut consistent = original.clone();
    consistent.sections[0].text = "quietly different".to_owned();
    consistent.sections[0].sha256 = seal_of_section(&consistent.sections[0]);
    check_object_integrity(&consistent, &seal_of(&consistent)).expect("nothing internal disagrees");
    check_object_integrity(&consistent, &sealed).expect_err("but it is not what was sealed");

    // Remove a section.
    let mut shortened = original.clone();
    shortened.sections.pop();
    assert_ne!(sealed, seal_of(&shortened), "remove a section");

    // Reassign one section's id to another's.
    let mut swapped = original.clone();
    swapped.sections[1].id = 1;
    sealed_object(&swapped).expect_err("two sections cannot claim one id");
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
    moved.admission = Admission::Agent;
    assert_ne!(
        sealed,
        seal_of_section(&moved),
        "admission — the field the v2 seal does not carry at all"
    );

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
    moved.based_on = Some("a".repeat(40));
    assert_ne!(sealed, seal_of_section(&moved), "based_on");

    let mut moved = original.clone();
    moved.refs = vec![reference(9)];
    assert_ne!(sealed, seal_of_section(&moved), "refs");

    let mut moved = original.clone();
    moved.relations = vec![Relation::superseded_by(object_id())];
    assert_ne!(sealed, seal_of_section(&moved), "relations");

    let mut moved = original.clone();
    moved.admitted_at = "2026-08-25T00:00:00Z".to_owned();
    assert_ne!(sealed, seal_of_section(&moved), "admitted_at");

    // And the one field that must not participate, because it is the answer.
    let mut moved = original.clone();
    moved.sha256 = "e".repeat(64);
    assert_eq!(
        sealed,
        seal_of_section(&moved),
        "a seal cannot cover itself"
    );
}

fn reference(section: u64) -> Ref {
    Ref {
        object: object_id(),
        section,
        sha256: "c".repeat(64),
        commit: "d".repeat(40),
    }
}

/// Sets are ordered by their elements' canonical bytes, and sections by id.
/// Neither is the order the file happened to be written in.
#[test]
fn incidental_order_is_not_integrity_meaning() {
    let mut one = section(1);
    one.refs = vec![reference(4), reference(2), reference(9)];
    let mut other = section(1);
    other.refs = vec![reference(9), reference(4), reference(2)];
    assert_eq!(
        seal_of_section(&one),
        seal_of_section(&other),
        "the same three references are the same assertion"
    );

    let forward = object();
    let mut backward = forward.clone();
    backward.sections.reverse();
    assert_eq!(
        seal_of(&forward),
        seal_of(&backward),
        "sections are canonicalized by id"
    );
}

/// A set holding one member twice is not a set, and is refused rather than
/// deduplicated — silently dropping one would reseal a file whose contents the
/// caller never agreed to change.
#[test]
fn a_canonical_duplicate_is_refused_rather_than_collapsed() {
    let mut section = section(1);
    section.refs = vec![reference(2), reference(2)];
    let refused = sealed_section(&section).expect_err("the same reference twice");
    assert!(refused.to_string().contains("appears twice"), "{refused}");
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

/// The seal is over the current Object state, and legacy per-resource markers
/// are not part of it — the migration removes them, so an Object that still
/// carries them and one that never did are the same current Object.
#[test]
fn legacy_format_markers_do_not_reach_the_aggregate() {
    let clean = object();
    let mut migrated = clean.clone();
    migrated.legacy_format = Some("engr-object".to_owned());
    migrated.legacy_version = Some(1);
    assert_eq!(
        seal_of(&clean),
        seal_of(&migrated),
        "provenance is not current Object state"
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

    check_object_seal(&original, &sealed).expect("unchanged");
}

/// The nested Section representation carries each field once. #31 warns that a
/// conceptual `id + all fields + sha256` reads like an instruction to write
/// `id` twice; this pins that it does not.
#[test]
fn the_nested_section_representation_holds_each_field_once() {
    let projected = sealed_object(&object()).expect("project");
    let first = projected.sections[0].as_object().expect("object");
    assert_eq!(first["id"], serde_json::json!(1));
    assert_eq!(
        first.len(),
        10,
        "nine protected fields plus the seal, each exactly once: {first:?}"
    );
    assert!(first.contains_key("sha256"), "and the seal is one of them");
}

/// The shared safe-integer domain applies before anything is hashed. An
/// oversized counter is schema-invalid for v3 and cannot be rounded into a
/// seal — `2^53` is an exact binary64 value, which is precisely why refusing it
/// has to be deliberate rather than left to the float conversion.
#[test]
fn an_integer_outside_the_shared_domain_cannot_be_sealed() {
    let mut object = object();
    object.next_section_id = 1 << 53;
    let refused = sealed_object(&object)
        .expect("project")
        .seal()
        .expect_err("outside the common ceiling");
    assert!(refused.to_string().contains("safe"), "{refused}");

    let mut section = section(1);
    section.id = (1 << 53) - 1;
    seal_of_section(&section);
}

/// Verification reports which resource disagreed, and fails closed.
#[test]
fn a_broken_seal_is_an_error_rather_than_a_flag() {
    let object = object();
    let wrong = "0".repeat(64);
    let refused = check_object_seal(&object, &wrong).expect_err("not this object");
    assert!(refused.to_string().contains(&object.id), "{refused}");

    let refused = check_section_seal(&object.sections[0], &wrong).expect_err("not this section");
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
    object.sections[1].sha256 = seal_of_section(&object.sections[1]);
    let resealed = seal_of(&object);

    check_object_integrity(&object, &resealed).expect("internally coherent");
    check_object_seal(&object, &aggregate).expect_err("but not the object that was sealed");

    // Now break only the internal agreement, and reseal the aggregate over it.
    object.sections[1].sha256 = "a".repeat(64);
    let over_a_lie = seal_of(&object);
    check_object_seal(&object, &over_a_lie).expect("the aggregate is happy");
    let refused =
        check_object_integrity(&object, &over_a_lie).expect_err("the section it covers is not");
    assert!(refused.to_string().contains("section 2"), "{refused}");
}

/// An integrity-invalid resource does not get quietly normalized on the way
/// past. The mutation is refused **before** it is applied, so unrelated work
/// cannot launder an out-of-band edit into a valid-looking seal.
#[test]
fn a_mutation_over_invalid_state_is_refused_before_it_runs() {
    let object = object();
    let sealed = seal_of(&object);

    // Hand-edit a Section and leave its seal alone: the file still parses and
    // every schema check passes.
    let mut tampered = object.clone();
    tampered.sections[0].text = "quietly rewritten".to_owned();

    let mut ran = false;
    let refused = mutate(&tampered, &sealed, |object| {
        ran = true;
        object.title = "and mutated on top".to_owned();
        Ok(())
    })
    .expect_err("the predecessor does not verify");
    assert!(!ran, "the mutation must not run at all: {refused}");

    // Same object, same edit, resealing instead of mutating: also refused.
    reseal(&tampered, &sealed).expect_err("a reseal is not a repair path");
}

/// The order in #35 §12 is not decoration. Sections are resealed first and the
/// aggregate is taken over the fresh values — an aggregate computed first would
/// cover seals that were about to be replaced.
#[test]
fn a_mutation_reseals_the_sections_before_the_aggregate() {
    let object = object();
    let sealed = seal_of(&object);

    let done = mutate(&object, &sealed, |object| {
        object.sections[0].text = "revised under the gate".to_owned();
        object.rev += 1;
        Ok(())
    })
    .expect("authorized");

    check_object_integrity(&done.object, &done.seal).expect("coherent afterwards");
    assert_ne!(done.seal, sealed, "the object moved");
    assert_ne!(
        done.object.sections[0].sha256, object.sections[0].sha256,
        "and so did the section it changed"
    );
    assert_eq!(
        done.object.sections[1].sha256, object.sections[1].sha256,
        "the one it did not touch is untouched"
    );
}

/// A mutation that fails leaves the caller with nothing to write, and the
/// predecessor it was given untouched.
#[test]
fn a_refused_mutation_produces_no_object_at_all() {
    let object = object();
    let sealed = seal_of(&object);
    let before = object.clone();

    mutate(&object, &sealed, |object| {
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
    let done = reseal(&object, &sealed).expect("nothing moved");
    assert_eq!(done.seal, sealed, "and the seal says so");
    assert_eq!(done.object, object);

    type Forbidden = fn(&mut Object) -> engr::Result<()>;
    let forbidden: [(&str, Forbidden); 5] = [
        ("admission", |object| {
            object.sections[0].admission = Admission::Agent;
            Ok(())
        }),
        ("admitted_at", |object| {
            object.sections[0].admitted_at = "2026-08-25T00:00:00Z".to_owned();
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
        let done = mutate(&object, &sealed, apply).expect("a real mutation may do this");
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
    let sealed = seal_of(&object);

    let admitted = mutate(&object, &sealed, |object| {
        object.sections[0].admission = Admission::Agent;
        object.rev += 1;
        Ok(())
    })
    .expect("the authority path already said yes");

    check_object_integrity(&admitted.object, &admitted.seal).expect("sealed over what it now says");
    reseal(&admitted.object, &admitted.seal).expect("and reseals to itself unchanged");
}

/// P3 renames one persisted member and adds one the v2 shape does not write.
///
/// Listed rather than inferred, because both are exactly the kind of difference
/// that makes a name-based coverage check quietly vacuous.
const RENAMED_IN_PHASE_3: &[(&str, &str)] = &[("confirmed_at", "admitted_at")];

/// Persisted members the seal deliberately does not cover, each with a reason
/// the contract gives.
const NOT_SEALED: &[(&str, &str)] = &[
    ("sha256", "a seal cannot cover itself"),
    (
        "format",
        "#31 removes the legacy per-resource marker from current Objects",
    ),
    (
        "version",
        "the workspace version comes only from .engr/format.json",
    ),
];

fn members(value: &serde_json::Value) -> Vec<String> {
    value
        .as_object()
        .expect("a resource is a JSON object")
        .keys()
        .cloned()
        .collect()
}

fn covered(persisted: &[String], sealed: &[String], what: &str) {
    for member in persisted {
        if let Some((_, reason)) = NOT_SEALED.iter().find(|(name, _)| name == member) {
            let _ = reason;
            continue;
        }
        let expected = RENAMED_IN_PHASE_3
            .iter()
            .find(|(from, _)| from == member)
            .map(|(_, to)| (*to).to_owned())
            .unwrap_or_else(|| member.clone());
        assert!(
            sealed.contains(&expected),
            "the persisted {what} writes {member:?} and the seal projection does not cover it; \
             add it to the projection, or to NOT_SEALED with the reason it is excluded"
        );
    }
}

/// Every member the persisted shape writes is either under the seal or on a
/// list saying why it is not.
///
/// The claim "`Object.sha256` covers every stable persisted Object field except
/// itself" is otherwise enforced by nobody: add a field to `Object` and the
/// projection silently stops covering it, every existing test still passes, and
/// the new field is one a hand-edit can change without detection. #31 says the
/// omission of `next_section_id` from illustrative lists was not an exclusion —
/// this is what stops the next such omission from being one.
///
/// Deliberately a list of exclusions rather than a list of inclusions: forget
/// to extend it and the test fails, which is the direction that fails safe.
#[test]
fn no_persisted_field_escapes_the_seal() {
    // Everything optional is populated, so nothing is missing merely because it
    // was empty and skipped.
    let mut section = section(1);
    section.content = vec![Supplement::new("data.note", "a note")];
    section.based_on = Some("a".repeat(40));
    section.refs = vec![reference(2)];
    section.relations = vec![Relation::superseded_by(object_id())];
    section.sha256 = seal_of_section(&section);

    let persisted = members(&serde_json::to_value(&section).expect("persisted section"));
    let sealed = members(
        &serde_json::to_value(sealed_section(&section).expect("project")).expect("projection"),
    );
    assert!(
        persisted.contains(&"sha256".to_owned()),
        "the fixture must exercise the excluded member too: {persisted:?}"
    );
    covered(&persisted, &sealed, "section");

    let mut object = object();
    object.sections = vec![section];
    object.object_type = Some(ObjectType::Decision);
    object.legacy_format = Some("engr-object".to_owned());
    object.legacy_version = Some(1);

    let persisted = members(&serde_json::to_value(&object).expect("persisted object"));
    let sealed = members(
        &serde_json::to_value(sealed_object(&object).expect("project")).expect("projection"),
    );
    assert!(
        persisted.contains(&"format".to_owned()) && persisted.contains(&"version".to_owned()),
        "the fixture must exercise the legacy markers: {persisted:?}"
    );
    covered(&persisted, &sealed, "object");
}

/// The same question for the nested Section representation the aggregate
/// carries: it holds the Section's own members plus the seal, and nothing else.
#[test]
fn the_nested_representation_covers_the_same_members_plus_the_seal() {
    let mut section = section(1);
    section.based_on = Some("a".repeat(40));
    section.sha256 = seal_of_section(&section);
    let mut object = object();
    object.sections = vec![section.clone()];

    let projected = sealed_object(&object).expect("project");
    let nested = members(&projected.sections[0]);
    let sealed = members(
        &serde_json::to_value(sealed_section(&section).expect("project")).expect("projection"),
    );

    for member in &sealed {
        assert!(
            nested.contains(member),
            "the nested form dropped {member:?}"
        );
    }
    assert_eq!(
        nested.len(),
        sealed.len() + 1,
        "and adds exactly one member, the seal: {nested:?}"
    );
}
