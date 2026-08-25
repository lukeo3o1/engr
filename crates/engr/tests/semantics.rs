//! What an Object is, what state it is in, and what a Section may carry.
//!
//! The rules here are the ones a reader has to be able to trust without opening
//! the code: that `state` means something specific for the type it sits on, that
//! nothing leaves the attention set quietly, that a superseded record always
//! says what replaced it, and that a Section stays one bounded assertion.

use engr::model::{Action, Content, Object, Payload, Ref};
use engr::semantics::{ObjectType, Relation, RelationType, Role, State, Supplement, Target};
use engr::{gate, ops, store};
use serde_json::Value;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn workspace() -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().to_path_buf();
    store::init(&root).expect("init");
    (dir, root)
}

fn git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A repository with one committed source file, so `implemented_by` has
/// something real to pin.
fn repository(root: &Path) -> String {
    git(root, &["init", "-q"]);
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::write(root.join("src/verifier.rs"), "fn verify() {}\n").expect("source");
    git(root, &["add", "."]);
    git(
        root,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "source",
        ],
    );
    engr::git::head(root).expect("HEAD")
}

fn payload(action: Action, object: &str, content: Content) -> Payload {
    Payload {
        action,
        object: object.to_owned(),
        becomes: None,
        content,
    }
}

fn wording(text: &str) -> Content {
    Content {
        text: text.to_owned(),
        ..Content::default()
    }
}

fn admit(root: &Path, payload: Payload) -> Object {
    let prepared = gate::prepare(root, payload).expect("prepare");
    let response = format!("CONFIRM {}", prepared.candidate.challenge);
    gate::confirm(root, &response).expect("confirm").object
}

fn new_object(root: &Path, title: &str) -> String {
    let id = engr::model::new_id();
    admit(root, payload(Action::ObjectCreated, &id, wording(title)));
    id
}

fn text_ref(root: &Path, object: &str, section: u64, commit: &str) -> Ref {
    let target = ops::effective(root, object).expect("reference target");
    Ref::selective(
        engr::dependency::admit(
            root,
            &target,
            target.sha256.as_deref().expect("aggregate seal"),
            section,
            &[engr::dependency::SemanticField::Text],
            commit,
        )
        .expect("admit selective reference"),
    )
}

fn classify(object_type: Option<ObjectType>, state: State) -> Action {
    Action::ObjectClassified { object_type, state }
}

fn classified(root: &Path, id: &str, object_type: Option<ObjectType>, state: State) -> Object {
    // Classifying to the state an Object already stands in is refused as a
    // no-op, so step through a different valid classification first when the
    // destination is where it already is. What these tests prove is that the
    // destination is admissible, not that confirming nothing is.
    let current = ops::effective(root, id).expect("object");
    if (current.object_type, current.state) == (object_type, state) {
        let away = if object_type.is_none() && state == State::Open {
            (None, State::Closed)
        } else {
            (None, State::Open)
        };
        admit(
            root,
            payload(classify(away.0, away.1), id, Content::default()),
        );
    }
    admit(
        root,
        payload(classify(object_type, state), id, Content::default()),
    )
}

/// Edit stored authority the way a text editor would.
fn rewrite(root: &Path, id: &str, edit: impl FnOnce(&mut Value)) {
    let path = store::object_path(root, id);
    let mut value: Value = store::read_json(&path).expect("read");
    edit(&mut value);
    store::write_json(&path, &value).expect("write");
}

/// Adding semantic fields must not have moved any digest that already exists.
///
/// The section hash is what `verify` recomputes, what a ref pins, and what
/// tamper detection compares — so a change to the canonical form would report
/// every section confirmed before this release as forged. The vector is the
/// literal canonical JSON from before the fields existed, hashed outside engr.
#[test]
fn a_section_carrying_none_of_the_new_fields_hashes_exactly_as_it_did_before() {
    let content = Content {
        text: "the confirmed wording".to_owned(),
        based_on: None,
        refs: Vec::new(),
        ..Content::default()
    };
    assert_eq!(
        content.sha256().expect("hash"),
        "6a2607e8a12be0e0a74527dc2c6a9109c1d04f1048e3915a3b67ee4c2c449f1d",
        "role, content and relations are skipped when empty, so the canonical form is unchanged"
    );
}

#[test]
fn every_state_is_valid_for_exactly_the_types_the_protocol_gives_it() {
    let (_dir, root) = workspace();
    let every_state = [
        State::Open,
        State::Closed,
        State::Draft,
        State::Proposed,
        State::Accepted,
        State::Rejected,
        State::Superseded,
        State::Identified,
        State::Mitigated,
        State::Invalidated,
    ];
    let table: [(Option<ObjectType>, &[State]); 4] = [
        (None, &[State::Open, State::Closed]),
        (
            Some(ObjectType::Design),
            &[
                State::Draft,
                State::Proposed,
                State::Accepted,
                State::Rejected,
            ],
        ),
        (
            Some(ObjectType::Decision),
            &[State::Proposed, State::Accepted, State::Rejected],
        ),
        (
            Some(ObjectType::Risk),
            &[
                State::Identified,
                State::Accepted,
                State::Mitigated,
                State::Invalidated,
            ],
        ),
    ];
    // `superseded` is legal for design and decision but is deliberately absent
    // from the reachable set here: it is coupled to a replacement relation, so
    // classification alone can never produce it. Its own test covers that.
    for (object_type, valid) in table {
        for state in every_state {
            let id = new_object(&root, "state table");
            // A fresh object is already untyped and open, so proving that
            // combination is admissible means arriving at it rather than
            // confirming nothing — the no-op refusal is a different rule.
            if (object_type, state) == (None, State::Open) {
                classified(&root, &id, None, State::Closed);
            }
            let proposal = payload(classify(object_type, state), &id, Content::default());
            if valid.contains(&state) {
                let object = admit(&root, proposal);
                assert_eq!(object.object_type, object_type);
                assert_eq!(object.state, state);
            } else {
                let error = gate::prepare(&root, proposal)
                    .expect_err("a state outside the type's vocabulary is refused");
                assert!(
                    error.code == engr::EXIT_USAGE || error.code == engr::EXIT_INVARIANT,
                    "{object_type:?} {state:?}: {error}"
                );
            }
        }
    }
}

#[test]
fn changing_type_carries_an_explicit_destination_state_and_never_a_mapped_one() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "reclassified");

    // Untyped to typed, typed to another type, and back to untyped: each hop
    // states both halves, and none of them inherits a state from the last.
    let object = classified(&root, &id, Some(ObjectType::Design), State::Draft);
    assert_eq!(object.state, State::Draft);
    // `draft` exists for design and not for decision, so a hop that tried to
    // carry the old state over is exactly what this refuses.
    let error = gate::prepare(
        &root,
        payload(
            classify(Some(ObjectType::Decision), State::Draft),
            &id,
            Content::default(),
        ),
    )
    .expect_err("a design state does not carry over to a decision");
    assert_eq!(error.code, engr::EXIT_USAGE);
    let object = classified(&root, &id, Some(ObjectType::Decision), State::Proposed);
    assert_eq!(object.object_type, Some(ObjectType::Decision));
    let object = classified(&root, &id, None, State::Open);
    assert_eq!(object.object_type, None);
    assert_eq!(object.state, State::Open);

    let stored = store::load_object(&root, &id).expect("stored");
    let raw: Value = store::read_json(&store::object_path(&root, &id)).expect("raw");
    assert!(
        raw["type"].is_null(),
        "an untyped object stores type=null: {raw}"
    );
    assert!(
        raw.get("status").is_none(),
        "there is one lifecycle field, and it is state: {raw}"
    );
    assert_eq!(stored.state, State::Open);
}

#[test]
fn the_default_listing_follows_derived_attention_rather_than_open_and_closed() {
    let (_dir, root) = workspace();
    let attention = [
        (None, State::Open),
        (Some(ObjectType::Design), State::Draft),
        (Some(ObjectType::Design), State::Proposed),
        (Some(ObjectType::Decision), State::Proposed),
        (Some(ObjectType::Risk), State::Identified),
    ];
    let quiet = [
        (None, State::Closed),
        (Some(ObjectType::Design), State::Accepted),
        (Some(ObjectType::Design), State::Rejected),
        (Some(ObjectType::Decision), State::Accepted),
        (Some(ObjectType::Decision), State::Rejected),
        (Some(ObjectType::Risk), State::Accepted),
        (Some(ObjectType::Risk), State::Mitigated),
        (Some(ObjectType::Risk), State::Invalidated),
    ];
    for (object_type, state) in attention {
        let id = new_object(&root, "loud");
        let object = classified(&root, &id, object_type, state);
        assert!(
            object.needs_attention(),
            "{object_type:?} {state:?} belongs in the default set"
        );
    }
    for (object_type, state) in quiet {
        let id = new_object(&root, "quiet");
        let object = classified(&root, &id, object_type, state);
        assert!(
            !object.needs_attention(),
            "{object_type:?} {state:?} is out of the default set"
        );
        // Nothing on disk says so. Attention is a function of the pair, and a
        // stored copy would be a second truth that drifts the moment somebody
        // edits `state`.
        let raw: Value = store::read_json(&store::object_path(&root, &id)).expect("raw");
        assert!(raw.get("attention").is_none(), "{raw}");
    }
}

#[test]
fn an_object_outside_the_attention_set_refuses_a_content_revision() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "accepted design");
    admit(
        &root,
        payload(Action::SectionAdded, &id, wording("the original wording")),
    );
    classified(&root, &id, Some(ObjectType::Design), State::Accepted);

    for action in [
        Action::SectionAdded,
        Action::SectionRevised { section: 1 },
        Action::SectionDeleted { section: 1 },
        Action::ObjectRenamed,
    ] {
        let content = if action.carries_content() {
            wording("work resumed")
        } else {
            Content::default()
        };
        let error = gate::prepare(&root, payload(action.clone(), &id, content))
            .expect_err("an accepted design is not revised while it stays accepted");
        assert_eq!(error.code, engr::EXIT_INVARIANT, "{}", action.label());
        assert!(
            error.message.contains("draft | proposed"),
            "the refusal names the way back into the attention set: {}",
            error.message
        );
    }

    // The way through is a classification of its own, confirmed on its own.
    classified(&root, &id, Some(ObjectType::Design), State::Draft);
    let object = admit(
        &root,
        payload(
            Action::SectionRevised { section: 1 },
            &id,
            wording("work resumed"),
        ),
    );
    assert_eq!(object.sections[0].text, "work resumed");
}

#[test]
fn refs_and_relations_are_sets_whose_order_is_not_a_semantic_change() {
    let (_dir, root) = workspace();
    let commit = repository(&root);
    let target = new_object(&root, "the target");
    admit(
        &root,
        payload(Action::SectionAdded, &target, wording("depended upon")),
    );
    admit(
        &root,
        payload(Action::SectionAdded, &target, wording("also depended upon")),
    );
    git(&root, &["add", "-A"]);
    git(
        &root,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "record",
        ],
    );
    let commit = engr::git::head(&root).unwrap_or(commit);
    let pin = |section: u64| text_ref(&root, &target, section, &commit);
    let implemented = |symbol: &str| Relation {
        relation: RelationType::ImplementedBy,
        target: Target::Symbol {
            path: "src/verifier.rs".to_owned(),
            symbol: symbol.to_owned(),
            commit: commit.clone(),
        },
    };

    let id = new_object(&root, "the source");
    let object = admit(
        &root,
        payload(
            Action::SectionAdded,
            &id,
            Content {
                text: "stands on both".to_owned(),
                refs: vec![pin(2), pin(1)],
                relations: vec![implemented("verify"), implemented("check")],
                based_on: Some(commit.clone()),
                ..Content::default()
            },
        ),
    );
    let stored = object.section(1).expect("section").clone();
    let mut canonical = Content {
        refs: vec![pin(1), pin(2)],
        relations: vec![implemented("check"), implemented("verify")],
        ..Content::default()
    };
    canonical.canonicalize_order().expect("canonical set order");
    assert_eq!(
        stored.refs, canonical.refs,
        "the gate puts a set in one order before it is hashed"
    );
    assert_eq!(stored.relations, canonical.relations);

    // The same members written the other way round are the same assertion, so
    // there is nothing to confirm and the gate says exactly that.
    let error = gate::prepare(
        &root,
        payload(
            Action::SectionRevised { section: 1 },
            &id,
            Content {
                text: "stands on both".to_owned(),
                refs: vec![pin(1), pin(2)],
                relations: vec![implemented("verify"), implemented("check")],
                based_on: Some(commit.clone()),
                ..Content::default()
            },
        ),
    )
    .expect_err("a reordering is not a change");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("nothing to confirm"), "{error}");

    for duplicated in [
        Content {
            text: "twice".to_owned(),
            refs: vec![pin(1), pin(1)],
            based_on: Some(commit.clone()),
            ..Content::default()
        },
        Content {
            text: "twice".to_owned(),
            relations: vec![implemented("verify"), implemented("verify")],
            based_on: Some(commit.clone()),
            ..Content::default()
        },
    ] {
        let error = gate::prepare(&root, payload(Action::SectionAdded, &id, duplicated))
            .expect_err("a set cannot hold the same member twice");
        assert_eq!(error.code, engr::EXIT_SCHEMA);
        assert!(error.message.contains("twice"), "{error}");
    }
}

#[test]
fn supersession_confirms_the_state_the_replacement_and_the_reason_together() {
    let (_dir, root) = workspace();
    let replacement = new_object(&root, "the replacement");
    let compact =
        engr::reference::encode_uuid_str(&replacement).expect("compact reference spelling");
    let superseded_by = Relation::superseded_by(format!("obj:{compact}"));

    let id = new_object(&root, "the original");
    classified(&root, &id, Some(ObjectType::Decision), State::Proposed);

    // Neither half alone. The state cannot be reached by classification, and
    // the relation cannot be added by an ordinary section.
    let error = gate::prepare(
        &root,
        payload(
            classify(Some(ObjectType::Decision), State::Superseded),
            &id,
            Content::default(),
        ),
    )
    .expect_err("superseded is not a state you can simply declare");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    let error = gate::prepare(
        &root,
        payload(
            Action::SectionAdded,
            &id,
            Content {
                text: "replaced".to_owned(),
                role: Some(Role::Supersession),
                relations: vec![superseded_by.clone()],
                ..Content::default()
            },
        ),
    )
    .expect_err("a replacement relation does not enter through an ordinary section");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("object.superseded"), "{error}");

    // One confirmation does all three.
    let object = admit(
        &root,
        payload(
            Action::ObjectSuperseded,
            &id,
            Content {
                text: "Replaced: the new decision removes the extra dependency.".to_owned(),
                role: Some(Role::Supersession),
                relations: vec![superseded_by.clone()],
                ..Content::default()
            },
        ),
    );
    assert_eq!(object.state, State::Superseded);
    assert_eq!(object.rev, 3, "one semantic action, one revision");
    let section = object.sections.last().expect("the rationale");
    assert_eq!(section.role, Some(Role::Supersession));
    assert_eq!(section.relations, vec![superseded_by]);
    assert_eq!(
        object.replacements().expect("replacements"),
        vec![replacement]
    );
}

#[test]
fn the_superseded_state_and_its_replacement_cannot_be_separated_afterwards() {
    let (_dir, root) = workspace();
    let replacement = new_object(&root, "the replacement");
    let compact = engr::reference::encode_uuid_str(&replacement).expect("compact");
    let id = new_object(&root, "the original");
    classified(&root, &id, Some(ObjectType::Design), State::Proposed);
    let object = admit(
        &root,
        payload(
            Action::ObjectSuperseded,
            &id,
            Content {
                text: "replaced, and here is why".to_owned(),
                role: Some(Role::Supersession),
                relations: vec![Relation::superseded_by(format!("obj:{compact}"))],
                ..Content::default()
            },
        ),
    );
    let rationale = object.sections.last().expect("rationale").id;

    // Every route out of the pair is closed, and closed for the same reason:
    // the state and the relation are one fact.
    for (what, proposal) in [
        (
            "reclassifying away from superseded",
            payload(
                classify(Some(ObjectType::Design), State::Accepted),
                &id,
                Content::default(),
            ),
        ),
        (
            "deleting the section that holds the relation",
            payload(
                Action::SectionDeleted { section: rationale },
                &id,
                Content::default(),
            ),
        ),
        (
            "revising the rationale without the relation",
            payload(
                Action::SectionRevised { section: rationale },
                &id,
                Content {
                    text: "replaced, and here is a better why".to_owned(),
                    role: Some(Role::Supersession),
                    ..Content::default()
                },
            ),
        ),
    ] {
        let error = gate::prepare(&root, proposal).expect_err(what);
        assert_eq!(error.code, engr::EXIT_INVARIANT, "{what}");
    }

    // A hand-edited file that breaks the pair is refused as stored data too.
    rewrite(&root, &id, |value| {
        value["state"] = Value::String("accepted".to_owned());
    });
    let error = store::load_object(&root, &id).expect_err("a broken pair is not valid authority");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
}

#[test]
fn a_replacement_cannot_point_at_itself_or_close_a_cycle() {
    let (_dir, root) = workspace();
    let first = new_object(&root, "first");
    let second = new_object(&root, "second");
    let missing = engr::model::new_id();
    let reference = |id: &str| {
        Relation::superseded_by(format!(
            "obj:{}",
            engr::reference::encode_uuid_str(id).expect("compact")
        ))
    };
    let supersede = |id: &str, target: &str| {
        payload(
            Action::ObjectSuperseded,
            id,
            Content {
                text: "replaced".to_owned(),
                role: Some(Role::Supersession),
                relations: vec![reference(target)],
                ..Content::default()
            },
        )
    };
    classified(&root, &first, Some(ObjectType::Design), State::Proposed);
    classified(&root, &second, Some(ObjectType::Design), State::Proposed);

    assert_eq!(
        gate::prepare(&root, supersede(&first, &first))
            .expect_err("an object cannot replace itself")
            .code,
        engr::EXIT_INVARIANT
    );
    assert_eq!(
        gate::prepare(&root, supersede(&first, &missing))
            .expect_err("a replacement has to exist")
            .code,
        engr::EXIT_NOT_FOUND
    );

    admit(&root, supersede(&first, &second));
    let error = gate::prepare(&root, supersede(&second, &first))
        .expect_err("a chain that closes leads a reader nowhere");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("cycle"), "{error}");
}

#[test]
fn implemented_by_pins_a_path_that_was_really_in_that_commit() {
    let (_dir, root) = workspace();
    let commit = repository(&root);
    let id = new_object(&root, "implementation provenance");

    let relation = |path: &str, commit: &str| Relation {
        relation: RelationType::ImplementedBy,
        target: Target::File {
            path: path.to_owned(),
            commit: commit.to_owned(),
        },
    };
    let object = admit(
        &root,
        payload(
            Action::SectionAdded,
            &id,
            Content {
                text: "the verifier implements this".to_owned(),
                relations: vec![relation("src/verifier.rs", &commit)],
                based_on: Some(commit.clone()),
                ..Content::default()
            },
        ),
    );
    assert_eq!(object.sections[0].relations.len(), 1);

    for (what, target) in [
        ("a path that was not in that commit", {
            Target::File {
                path: "src/absent.rs".to_owned(),
                commit: commit.clone(),
            }
        }),
        ("a commit that is not in this repository", {
            Target::File {
                path: "src/verifier.rs".to_owned(),
                commit: "0".repeat(40),
            }
        }),
        ("an engr resource, which is not an artifact", {
            Target::engr("obj:01h47kwz2mfk0v47mffcnstqva")
        }),
    ] {
        let error = gate::prepare(
            &root,
            payload(
                Action::SectionAdded,
                &id,
                Content {
                    text: "bad provenance".to_owned(),
                    relations: vec![Relation {
                        relation: RelationType::ImplementedBy,
                        target,
                    }],
                    based_on: Some(commit.clone()),
                    ..Content::default()
                },
            ),
        )
        .expect_err(what);
        assert!(
            error.code == engr::EXIT_INVARIANT || error.code == engr::EXIT_SCHEMA,
            "{what}: {error}"
        );
    }

    // The symbol itself is not resolved. v0 does not parse the language, and a
    // check that only worked for languages engr could parse would be worse than
    // none: it would fail on real code and pass on the rest.
    admit(
        &root,
        payload(
            Action::SectionAdded,
            &id,
            Content {
                text: "a symbol nothing verifies".to_owned(),
                relations: vec![Relation {
                    relation: RelationType::ImplementedBy,
                    target: Target::Symbol {
                        path: "src/verifier.rs".to_owned(),
                        symbol: "NoSuchThing::at_all".to_owned(),
                        commit: commit.clone(),
                    },
                }],
                based_on: Some(commit.clone()),
                ..Content::default()
            },
        ),
    );
}

#[test]
fn supplementary_content_keeps_its_order_and_allows_repeated_types() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "bounded content");
    let entries = vec![
        Supplement::new("code.rs", "let first = 1;"),
        Supplement::new("data.json", "{\"a\":1}"),
        Supplement::new("code.rs", "let second = 2;"),
    ];
    let object = admit(
        &root,
        payload(
            Action::SectionAdded,
            &id,
            Content {
                text: "one assertion, three excerpts".to_owned(),
                content: entries.clone(),
                ..Content::default()
            },
        ),
    );
    assert_eq!(object.sections[0].content, entries);

    // Order is semantic, so the same entries in another order are a different
    // Section and hash differently.
    let mut reordered = entries.clone();
    reordered.swap(0, 2);
    let shuffled = Content {
        text: "one assertion, three excerpts".to_owned(),
        content: reordered,
        ..Content::default()
    };
    assert_ne!(
        shuffled.sha256().expect("hash"),
        object.sections[0].sha256,
        "moving an entry changes the assertion"
    );
    let object = admit(
        &root,
        payload(Action::SectionRevised { section: 1 }, &id, shuffled),
    );
    assert_eq!(object.sections[0].content[0].body, "let second = 2;");
}

#[test]
fn a_normal_threshold_refuses_once_and_an_explicit_retry_gets_through_the_gate() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "size policy");
    let long = "x".repeat(engr::semantics::TEXT_NORMAL + 1);

    let error = gate::prepare(&root, payload(Action::SectionAdded, &id, wording(&long)))
        .expect_err("the first attempt is refused");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(
        error.message.contains("backlog") && error.message.contains("--oversize"),
        "the refusal says where the material belongs, not only that it is long: {}",
        error.message
    );

    let prepared =
        gate::prepare_oversize(&root, payload(Action::SectionAdded, &id, wording(&long)))
            .expect("an explicit retry goes to the human");
    assert!(
        prepared.candidate.context.oversize,
        "the exception travels with the candidate so the screen can say so"
    );
    let object = gate::confirm(&root, &format!("CONFIRM {}", prepared.candidate.challenge))
        .expect("the normal confirmation flow admits it")
        .object;
    assert_eq!(
        object.sections[0].text.chars().count(),
        long.chars().count()
    );

    // Admission-time only. Nothing about the stored Section remembers the
    // exception, so the next revision is measured from scratch.
    let raw: Value = store::read_json(&store::object_path(&root, &id)).expect("raw");
    assert!(
        !raw.to_string().contains("oversize"),
        "an exception is not a lasting property of a Section: {raw}"
    );
    let error = gate::prepare(
        &root,
        payload(
            Action::SectionRevised { section: 1 },
            &id,
            wording(&format!("{long}y")),
        ),
    )
    .expect_err("a previously admitted Section carries no exemption");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
}

#[test]
fn a_hard_ceiling_refuses_even_an_explicit_retry() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "hard ceilings");
    let body = |length: usize| Supplement::new("code.rs", "x".repeat(length));

    for (what, content) in [
        (
            "text past the hard ceiling",
            Content {
                text: "x".repeat(engr::semantics::TEXT_HARD + 1),
                ..Content::default()
            },
        ),
        (
            "too many entries",
            Content {
                text: "bounded".to_owned(),
                content: (0..=engr::semantics::ENTRIES_HARD)
                    .map(|_| body(1))
                    .collect(),
                ..Content::default()
            },
        ),
        (
            "one body past the hard ceiling",
            Content {
                text: "bounded".to_owned(),
                content: vec![body(engr::semantics::BODY_HARD + 1)],
                ..Content::default()
            },
        ),
        (
            "the bodies together past the hard ceiling",
            Content {
                text: "bounded".to_owned(),
                content: (0..2)
                    .map(|_| body(engr::semantics::BODIES_HARD / 2 + 1))
                    .collect(),
                ..Content::default()
            },
        ),
    ] {
        let error = gate::prepare_oversize(&root, payload(Action::SectionAdded, &id, content))
            .expect_err(what);
        assert_eq!(error.code, engr::EXIT_INVARIANT, "{what}");
        assert!(
            error.message.contains("hold at all"),
            "{what} must not read as a threshold with an override: {}",
            error.message
        );
    }
}

#[test]
fn every_new_semantic_field_is_inside_what_the_human_confirmed() {
    let (_dir, root) = workspace();
    let commit = repository(&root);
    let id = new_object(&root, "hashing");
    let base = Content {
        text: "the assertion".to_owned(),
        based_on: Some(commit.clone()),
        ..Content::default()
    };
    let variants = [
        Content {
            role: Some(Role::Decision),
            ..base.clone()
        },
        Content {
            content: vec![Supplement::new("code.rs", "let x = 1;")],
            ..base.clone()
        },
        Content {
            relations: vec![Relation {
                relation: RelationType::ImplementedBy,
                target: Target::File {
                    path: "src/verifier.rs".to_owned(),
                    commit: commit.clone(),
                },
            }],
            ..base.clone()
        },
    ];
    let plain = gate::prepare(&root, payload(Action::SectionAdded, &id, base.clone()))
        .expect("prepare")
        .candidate
        .candidate_digest
        .clone();
    for variant in variants {
        let candidate = gate::prepare(&root, payload(Action::SectionAdded, &id, variant))
            .expect("prepare")
            .candidate;
        assert_ne!(
            candidate.candidate_digest, plain,
            "a semantic field outside the payload hash is one a human never assented to"
        );
        // And inside the section hash, so `verify` can see it move.
        assert_ne!(
            candidate.payload.content.sha256().expect("hash"),
            base.sha256().expect("hash")
        );
    }
}

#[test]
fn a_candidate_bound_to_a_classification_dies_when_the_object_moves() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "stale classification");
    let prepared = gate::prepare(
        &root,
        payload(
            classify(Some(ObjectType::Risk), State::Identified),
            &id,
            Content::default(),
        ),
    )
    .expect("prepare");

    // Something else lands first. Preparing it supersedes this candidate, which
    // is the ordinary path — so put the file back to reach the case where a code
    // outlives the state it was bound to.
    let code = prepared.candidate.challenge.clone();
    admit(
        &root,
        payload(
            Action::SectionAdded,
            &id,
            wording("work happened meanwhile"),
        ),
    );
    store::write_json(
        &store::candidate_path(&root, &code).expect("candidate path"),
        &prepared.candidate,
    )
    .expect("restore the overtaken candidate");

    let candidate = gate::find(&root, &code).expect("the file is still a valid candidate");
    assert!(
        matches!(
            gate::candidate_state(&root, &candidate).expect("state"),
            gate::CandidateState::Stale { .. }
        ),
        "a classification prepared against an older revision is dead, not applicable"
    );
    let error = gate::confirm(&root, &format!("CONFIRM {code}"))
        .expect_err("the object moved after this was prepared");
    assert_eq!(error.code, engr::EXIT_STALE);
}

/// Stored authority is held to exactly what the write path enforces.
///
/// The same rule Backlog already lives under, and for the same reason: a check
/// that only runs on the way in stops being true after one hand-edit, and these
/// files are meant to be readable and diffable.
#[test]
fn stored_semantic_fields_outside_the_schema_are_refused_rather_than_dropped() {
    let (_dir, root) = workspace();
    let commit = repository(&root);
    let id = new_object(&root, "stored shape");
    admit(
        &root,
        payload(
            Action::SectionAdded,
            &id,
            Content {
                text: "a section with everything".to_owned(),
                role: Some(Role::Decision),
                content: vec![Supplement::new("code.rs", "let x = 1;")],
                relations: vec![Relation {
                    relation: RelationType::ImplementedBy,
                    target: Target::File {
                        path: "src/verifier.rs".to_owned(),
                        commit: commit.clone(),
                    },
                }],
                based_on: Some(commit.clone()),
                ..Content::default()
            },
        ),
    );
    assert!(store::load_object(&root, &id).is_ok(), "the seed is valid");
    let seed: Value = store::read_json(&store::object_path(&root, &id)).expect("seed");

    type Corruption = (&'static str, fn(&mut Value));
    let corruptions: [Corruption; 10] = [
        ("a role outside the vocabulary", |value: &mut Value| {
            value["sections"][0]["role"] = Value::String("rationale".to_owned());
        }),
        (
            "a relation type outside the vocabulary",
            |value: &mut Value| {
                value["sections"][0]["relations"][0]["type"] =
                    Value::String("relates_to".to_owned());
            },
        ),
        ("a content type outside the grammar", |value: &mut Value| {
            value["sections"][0]["content"][0]["type"] = Value::String("text.md".to_owned());
        }),
        ("an empty content body", |value: &mut Value| {
            value["sections"][0]["content"][0]["body"] = Value::String(String::new());
        }),
        (
            "an unknown field on a content entry",
            |value: &mut Value| {
                value["sections"][0]["content"][0]["language"] = Value::String("rust".to_owned());
            },
        ),
        ("an unknown field on a relation", |value: &mut Value| {
            value["sections"][0]["relations"][0]["strength"] = Value::String("weak".to_owned());
        }),
        (
            "an unknown field on a relation target",
            |value: &mut Value| {
                value["sections"][0]["relations"][0]["target"]["line"] = Value::from(12);
            },
        ),
        (
            "a state outside the type's vocabulary",
            |value: &mut Value| {
                value["state"] = Value::String("mitigated".to_owned());
            },
        ),
        (
            "a superseded_by with no superseded state",
            |value: &mut Value| {
                value["sections"][0]["relations"][0] = serde_json::json!({
                    "type": "superseded_by",
                    "target": { "kind": "engr", "ref": "obj:01h47kwz2mfk0v47mffcnstqva" }
                });
            },
        ),
        ("a duplicated relation", |value: &mut Value| {
            let existing = value["sections"][0]["relations"][0].clone();
            value["sections"][0]["relations"] = Value::Array(vec![existing.clone(), existing]);
        }),
    ];
    for (what, corrupt) in corruptions {
        let path = store::object_path(&root, &id);
        let mut value = seed.clone();
        corrupt(&mut value);
        store::write_json(&path, &value).expect("write");
        let error = store::load_object(&root, &id).expect_err(what);
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{what}: {}", error.message);
    }
}

/// A workspace written before Phase 3 keeps loading, and gains nothing it did
/// not say.
#[test]
fn a_migrated_workspace_carries_no_invented_classification() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "legacy object");
    admit(
        &root,
        payload(Action::SectionAdded, &id, wording("confirmed long ago")),
    );
    admit(
        &root,
        payload(Action::ObjectClosed, &id, Content::default()),
    );

    // Put it back into the v0 shape a Phase 0 workspace really had.
    let current = store::load_object(&root, &id).expect("current object");
    rewrite(&root, &id, |value| {
        let object = value.as_object_mut().expect("object");
        object.remove("sha256");
        object.insert("format".to_owned(), Value::String("engr-object".to_owned()));
        object.insert("version".to_owned(), Value::from(1));
        let state = object.remove("state").expect("state");
        object.insert("status".to_owned(), state);
        for stored in object["sections"].as_array_mut().expect("sections") {
            let stored = stored.as_object_mut().expect("section");
            let section_id = stored["id"].as_u64().expect("section id");
            let section = current.section(section_id).expect("current section");
            stored.remove("admission");
            let admitted_at = stored.remove("admitted_at").expect("admitted_at");
            stored.insert("confirmed_at".to_owned(), admitted_at);
            stored.insert(
                "sha256".to_owned(),
                Value::String(section.recomputed_sha256().expect("legacy Section seal")),
            );
        }
    });
    let events_path = store::events_path(&root, &id);
    let mut retained = String::new();
    for line in std::fs::read_to_string(&events_path)
        .expect("events")
        .lines()
    {
        let mut event: engr::model::Event = serde_json::from_str(line).expect("event");
        event.version = engr::EVENT_ENVELOPE_VERSION_V0;
        event.provenance = engr::model::Provenance::confirmed(
            "TEST00",
            event.payload.sha256().expect("payload hash"),
        );
        retained.push_str(&serde_json::to_string(&event).expect("event"));
        retained.push('\n');
    }
    std::fs::write(events_path, retained).expect("retained events");
    std::fs::remove_file(store::engr_dir(&root).join("format.json")).expect("format");

    assert_eq!(
        store::validate_format(&root).expect("detect"),
        store::WorkspaceFormat::LegacyV0
    );
    // Readable before migration, and unchanged on disk.
    let before = ops::effective(&root, &id).expect("legacy read");
    assert_eq!(before.state, State::Closed);
    assert_eq!(before.object_type, None);

    store::migrate(&root).expect("migrate");
    let after = store::load_object(&root, &id).expect("migrated");
    assert_eq!(after.state, State::Closed);
    assert_eq!(
        after.object_type, None,
        "migration classifies nothing: the old record does not say what kind of thing it is"
    );
    let raw: Value = store::read_json(&store::object_path(&root, &id)).expect("raw");
    assert!(raw.get("status").is_none(), "{raw}");
    assert!(raw["type"].is_null(), "{raw}");
    assert_eq!(raw["state"], Value::String("closed".to_owned()));

    // And the old vocabulary still works on it, because that is what its own
    // confirmed history is written in.
    let object = admit(
        &root,
        payload(Action::ObjectReopened, &id, Content::default()),
    );
    assert_eq!(object.state, State::Open);
}

/// A Section with an incidental noncanonical Ref order remains the same
/// assertion, and re-proposing the same members is still not a change.
///
/// Canonicalization is done to the proposal. If the "nothing to confirm" check
/// did not do the same to the stored value only for the comparison, every
/// Section stored before this rule holding two refs the other way round would
/// accept one confirmation and one Event that changed nothing but an array's
/// order — the exact thing declaring it a set was supposed to rule out. The
/// stored Section is not tidied either: set order is outside integrity meaning.
#[test]
fn a_section_stored_before_refs_were_a_set_is_not_revised_into_sorted_order() {
    let (_dir, root) = workspace();
    let commit = repository(&root);
    let target = new_object(&root, "the target");
    admit(
        &root,
        payload(Action::SectionAdded, &target, wording("depended upon")),
    );
    admit(
        &root,
        payload(Action::SectionAdded, &target, wording("also depended upon")),
    );
    git(&root, &["add", "-A"]);
    git(
        &root,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "record",
        ],
    );
    let commit = engr::git::head(&root).unwrap_or(commit);
    let pin = |section: u64| text_ref(&root, &target, section, &commit);

    let id = new_object(&root, "the source");
    let object = admit(
        &root,
        payload(
            Action::SectionAdded,
            &id,
            Content {
                text: "stands on both".to_owned(),
                refs: vec![pin(1), pin(2)],
                based_on: Some(commit.clone()),
                ..Content::default()
            },
        ),
    );

    // Seed the same two refs the other way round. The v3 seal canonicalizes
    // sets, so neither the Section seal nor Object aggregate changes.
    let mut legacy = object.section(1).expect("section").content();
    legacy.refs.reverse();
    let stored_sha256 = object.section(1).expect("section").sha256.clone();
    rewrite(&root, &id, |value| {
        let section = &mut value["sections"][0];
        section["refs"].as_array_mut().expect("refs").reverse();
    });
    let seeded = store::load_object(&root, &id).expect("an unsorted set is valid stored authority");
    engr::integrity::check_stored_object_integrity(&seeded)
        .expect("set order does not change either resource seal");

    // Both spellings of the same membership are the same assertion.
    for refs in [vec![pin(1), pin(2)], vec![pin(2), pin(1)]] {
        let error = gate::prepare(
            &root,
            payload(
                Action::SectionRevised { section: 1 },
                &id,
                Content {
                    text: "stands on both".to_owned(),
                    refs,
                    based_on: Some(commit.clone()),
                    ..Content::default()
                },
            ),
        )
        .expect_err("re-proposing the same set is not a change");
        assert_eq!(error.code, engr::EXIT_INVARIANT);
        assert!(error.message.contains("nothing to confirm"), "{error}");
    }

    let after = store::load_object(&root, &id).expect("load");
    assert_eq!(
        after.sections[0].sha256, stored_sha256,
        "and nothing sorted the stored Section behind its own hash"
    );
    assert_eq!(after.sections[0].refs, legacy.refs);
    assert_eq!(
        store::load_events(&root, &id).expect("events").len(),
        2,
        "created and added — a reordering appends nothing"
    );
}

/// The first refusal is a requirement on engr, not advice to the caller.
///
/// #14 makes the admission two-stage on purpose: above a normal threshold the
/// first `prepare` must refuse, and only then may a retry ask for the
/// exception. A flag alone cannot carry that, because a flag can be passed the
/// first time — so the exception is admitted only as the retry of a refusal
/// that really happened, for that same proposal.
#[test]
fn an_oversize_exception_is_only_ever_the_retry_of_a_refusal() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "admission");
    let long = "x".repeat(engr::semantics::TEXT_NORMAL + 1);
    let another = format!("{long}y");

    let error = gate::prepare_oversize(&root, payload(Action::SectionAdded, &id, wording(&long)))
        .expect_err("the flag is not a way past the first refusal");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("retry of a refusal"), "{error}");
    assert!(
        gate::pending(&root).expect("candidates").is_empty(),
        "and no code is minted for it"
    );

    // An exception over nothing is an agent setting the flag by default.
    let error = gate::prepare_oversize(&root, payload(Action::SectionAdded, &id, wording("brief")))
        .expect_err("there is nothing here to except");
    assert_eq!(error.code, engr::EXIT_USAGE);
    assert!(error.message.contains("no exception to make"), "{error}");

    gate::prepare(&root, payload(Action::SectionAdded, &id, wording(&long)))
        .expect_err("the first attempt is refused");

    // A refusal admits the proposal it refused, and not whatever comes next.
    let error =
        gate::prepare_oversize(&root, payload(Action::SectionAdded, &id, wording(&another)))
            .expect_err("a receipt is not a mode the workspace is now in");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("retry of a refusal"), "{error}");

    let prepared =
        gate::prepare_oversize(&root, payload(Action::SectionAdded, &id, wording(&long)))
            .expect("the retry of what was actually refused");
    assert!(prepared.candidate.context.oversize);

    // One refusal admits one retry. Preparing the same thing again has to be
    // refused again first, or the receipt would be a standing permission.
    let error = gate::prepare_oversize(&root, payload(Action::SectionAdded, &id, wording(&long)))
        .expect_err("the receipt does not survive being used");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("retry of a refusal"), "{error}");
}

/// Supersession is the retirement of a current object, so the state it starts
/// from is the one that needs no attention.
///
/// v0 defines no transition graph: a destination valid for the type with the
/// invariants holding afterwards is the whole test. Sending an `accepted`
/// decision back through `proposed` first would confirm a state it was never
/// in, and would split into two the one operation the protocol requires to be
/// atomic.
#[test]
fn an_accepted_object_is_superseded_without_being_reclassified_first() {
    let (_dir, root) = workspace();
    let replacement = new_object(&root, "the replacement");
    let compact = engr::reference::encode_uuid_str(&replacement).expect("compact");
    let id = new_object(&root, "the original");
    let accepted = classified(&root, &id, Some(ObjectType::Decision), State::Accepted);
    assert!(
        !accepted.needs_attention(),
        "the object supersession exists for is one nobody is looking at"
    );

    let object = admit(
        &root,
        payload(
            Action::ObjectSuperseded,
            &id,
            Content {
                text: "replaced by advisory locks".to_owned(),
                role: Some(Role::Supersession),
                relations: vec![Relation::superseded_by(format!("obj:{compact}"))],
                ..Content::default()
            },
        ),
    );
    assert_eq!(object.state, State::Superseded);
    assert_eq!(
        store::load_events(&root, &id).expect("events").len(),
        3,
        "created, classified, superseded — and no invented state in between"
    );

    // Superseding it again is refused by the coupled invariant counting two
    // replacements, not by a lifecycle sequence. The difference matters: the
    // record refuses to say it was replaced by two different things, and says
    // so in those terms.
    let second = new_object(&root, "a third design");
    let second = engr::reference::encode_uuid_str(&second).expect("compact");
    let error = gate::prepare(
        &root,
        payload(
            Action::ObjectSuperseded,
            &id,
            Content {
                text: "replaced again".to_owned(),
                role: Some(Role::Supersession),
                relations: vec![Relation::superseded_by(format!("obj:{second}"))],
                ..Content::default()
            },
        ),
    )
    .expect_err("one object has one replacement");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(
        error.message.contains("exactly one superseded_by"),
        "{error}"
    );
}

/// A refusal belongs to the proposal it refused, not to the workspace.
///
/// One slot would mean any second proposal considered anywhere revoked the
/// first one's refusal — so an agent that had already done exactly what the
/// two-stage rule asks would be sent back to do it again, for a reason with
/// nothing to do with its own proposal. A workspace holds work on many Objects
/// at once; the rule has to hold for each of them.
#[test]
fn each_refused_proposal_keeps_its_own_retry() {
    let (_dir, root) = workspace();
    let first = new_object(&root, "one object");
    let second = new_object(&root, "another object");
    let long = "x".repeat(engr::semantics::TEXT_NORMAL + 1);
    let other = format!("{long}y");
    let proposal = |id: &str, text: &str| payload(Action::SectionAdded, id, wording(text));

    // Both are refused, in the order that used to lose the first receipt.
    gate::prepare(&root, proposal(&first, &long)).expect_err("refused for size");
    gate::prepare(&root, proposal(&second, &other)).expect_err("refused for size");

    // The older refusal is still the caller's to retry.
    let earlier =
        gate::prepare_oversize(&root, proposal(&first, &long)).expect("the first refusal stands");
    assert!(earlier.candidate.context.oversize);
    let later = gate::prepare_oversize(&root, proposal(&second, &other))
        .expect("and so does the second, independently");
    assert!(later.candidate.context.oversize);

    // Spending one leaves nothing behind for it, and takes nothing from anyone
    // else — both were spent above, so both now need a fresh refusal.
    for (id, text) in [(&first, &long), (&second, &other)] {
        let error = gate::prepare_oversize(&root, proposal(id, text))
            .expect_err("an exception is spent by the retry that used it");
        assert_eq!(error.code, engr::EXIT_INVARIANT);
    }
}

/// Every byte of a literal body is part of the assertion.
///
/// #14 defines a body as literal non-empty UTF-8 and says nothing about
/// normalising it, so v0 does not: `"x"`, `"x\n"` and `"x   "` are three
/// different Sections with three different hashes, and a body of nothing but
/// spaces is a body somebody wrote. Whether trailing whitespace *should* be
/// insignificant is a real question, and an open one — deciding it here would
/// redefine literal equality, payload identity and no-op semantics without an
/// accepted design ruling. What the gate owes in the meantime is presentation,
/// not normalisation, and that is pinned on the command line.
#[test]
fn a_literal_body_keeps_every_byte_it_was_written_with() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "literal bodies");
    let excerpt = |body: &str| Content {
        text: "the assertion the excerpt supports".to_owned(),
        content: vec![Supplement::new("code.rs", body)],
        ..Content::default()
    };

    let object = admit(
        &root,
        payload(Action::SectionAdded, &id, excerpt("let x = 1;\n")),
    );
    assert_eq!(
        object.sections[0].content[0].body, "let x = 1;\n",
        "the trailing newline is stored, because nothing is entitled to drop it"
    );
    let with_newline = object.sections[0].sha256.clone();

    // The same characters without it are a different assertion, so this is a
    // revision with something to confirm rather than a no-op.
    let object = admit(
        &root,
        payload(
            Action::SectionRevised { section: 1 },
            &id,
            excerpt("let x = 1;"),
        ),
    );
    assert_eq!(object.sections[0].content[0].body, "let x = 1;");
    assert_ne!(
        object.sections[0].sha256, with_newline,
        "two bodies that differ only in a trailing newline hash differently"
    );

    // And the same wording written twice over is still nothing to confirm, so
    // the no-op check has not been loosened along the way.
    let error = gate::prepare(
        &root,
        payload(
            Action::SectionRevised { section: 1 },
            &id,
            excerpt("let x = 1;"),
        ),
    )
    .expect_err("the identical body is identical");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("nothing to confirm"), "{error}");

    // Whitespace alone is a body. Empty is the one that means nothing.
    let object = admit(&root, payload(Action::SectionAdded, &id, excerpt("   ")));
    assert_eq!(object.sections[1].content[0].body, "   ");
    let error = gate::prepare(&root, payload(Action::SectionAdded, &id, excerpt("")))
        .expect_err("an empty body is not content");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("cannot be empty"), "{error}");
}

/// A no-attention Object is revised in **one** confirmation, not two.
///
/// Permitted exactly when the same operation atomically returns the Object to a
/// state that needs attention. The rule it replaces is not relaxed: a bare
/// revision is still refused, and so is one whose destination still needs no
/// attention. What is gone is the artificial intermediate — the `proposed` an
/// object was never really in, confirmed only so the next confirmation could
/// land.
///
/// And it is narrow in the other direction too. A destination is admissible
/// *because* it is what makes the action legal, so an Object that already needs
/// attention cannot carry one: there would be nothing to unblock, and the
/// destination would be a second, unrelated change hidden inside someone else's
/// confirmation.
#[test]
fn a_no_attention_object_is_revised_and_returned_to_attention_in_one_confirmation() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "the auth design");
    admit(
        &root,
        payload(
            Action::SectionAdded,
            &id,
            wording("Use short-lived tokens."),
        ),
    );
    classified(&root, &id, Some(ObjectType::Design), State::Accepted);
    let before = ops::effective(&root, &id).expect("object");
    assert!(!before.needs_attention());

    let revise = || Payload {
        action: Action::SectionRevised { section: 1 },
        object: id.clone(),
        becomes: None,
        content: wording("Use short-lived tokens, capped at 15 minutes."),
    };

    // Still refused on its own: nothing about the guard was loosened.
    let error = gate::prepare(&root, revise()).expect_err("nobody is looking at it");
    assert_eq!(error.code, engr::EXIT_INVARIANT);

    // And refused when the destination would leave it out of the listing, which
    // is the "only if" half of the rule.
    let mut still_hidden = revise();
    still_hidden.becomes = Some(engr::model::Destination {
        object_type: Some(ObjectType::Design),
        state: State::Rejected,
    });
    let error = gate::prepare(&root, still_hidden).expect_err("rejected needs no attention");
    assert_eq!(error.code, engr::EXIT_INVARIANT);

    // One confirmation that does both.
    let mut atomic = revise();
    atomic.becomes = Some(engr::model::Destination {
        object_type: Some(ObjectType::Design),
        state: State::Proposed,
    });
    let object = admit(&root, atomic);
    assert_eq!(object.state, State::Proposed);
    assert!(object.needs_attention());
    assert_eq!(
        object.sections[0].text,
        "Use short-lived tokens, capped at 15 minutes."
    );
    assert_eq!(
        object.rev,
        before.rev + 1,
        "one operation, one event — no intermediate state was ever confirmed"
    );

    // Now that it needs attention, a destination is no longer admissible on it.
    // Nothing is blocked any more, so one would be a second, unrelated change
    // riding along inside someone else's confirmation — and `object_classified`
    // already says that on its own, where a reader can see it.
    let mut piggybacked = Payload {
        action: Action::SectionRevised { section: 1 },
        object: id.clone(),
        becomes: Some(engr::model::Destination {
            object_type: Some(ObjectType::Design),
            state: State::Draft,
        }),
        content: wording("Use short-lived tokens, capped at 10 minutes."),
    };
    let error = gate::prepare(&root, piggybacked.clone()).expect_err("it already needs attention");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("already needs attention"), "{error}");

    // The very same revision without one is admitted, which is what makes the
    // refusal about the destination rather than about the revision.
    piggybacked.becomes = None;
    let object = admit(&root, piggybacked);
    assert_eq!(object.state, State::Proposed);

    // An action that names its own state takes no destination.
    let mut confused = payload(Action::ObjectClosed, &id, engr::model::Content::default());
    confused.becomes = Some(engr::model::Destination {
        object_type: None,
        state: State::Open,
    });
    let error = gate::prepare(&root, confused).expect_err("close sets the state itself");
    assert!(
        error.code == engr::EXIT_INVARIANT || error.code == engr::EXIT_USAGE,
        "{error}"
    );

    // A payload carrying no destination is byte-for-byte what it always was.
    let raw: Value = store::read_json(&store::object_path(&root, &id)).expect("raw");
    assert!(!raw.to_string().contains("becomes"), "{raw}");

    // The wire key is part of the ruling, not an implementation detail, so it is
    // asserted on the confirmed events rather than left to the Rust field name.
    // Exactly one event carries it: the one that needed it to be legal.
    let events = std::fs::read_to_string(store::events_path(&root, &id)).expect("events");
    let carrying: Vec<&str> = events
        .lines()
        .filter(|line| line.contains("\"becomes\""))
        .collect();
    assert_eq!(carrying.len(), 1, "{events}");
    assert!(
        carrying[0].contains("\"section_revised\"")
            && carrying[0].contains("\"state\":\"proposed\""),
        "{}",
        carrying[0]
    );
}

/// The supersession cycle walk fails closed on authority it cannot read.
///
/// The walk existed to prove a replacement does not close a loop. It followed
/// each edge with `if let Ok(target)`, so an intermediate Object that would not
/// load simply ended that branch — and a branch nobody walked can hide the
/// cycle the walk was there to find. Unreadable authoritative state must fail
/// closed rather than be collapsed into "nothing further this way".
#[test]
fn a_supersession_chain_through_unreadable_authority_is_refused() {
    let (_dir, root) = workspace();
    let compact = |id: &str| {
        format!(
            "obj:{}",
            engr::reference::encode_uuid_str(id).expect("compact")
        )
    };

    let far = new_object(&root, "the far end of the chain");
    let middle = new_object(&root, "the middle of the chain");
    let near = new_object(&root, "the near end of the chain");
    let source = new_object(&root, "the object being replaced");

    for id in [&middle, &near, &source] {
        classified(&root, id, Some(ObjectType::Design), State::Accepted);
    }

    // middle is superseded by far, so walking from middle continues to far.
    admit(
        &root,
        payload(
            Action::ObjectSuperseded,
            &middle,
            Content {
                text: "replaced by the far end".to_owned(),
                role: Some(Role::Supersession),
                relations: vec![Relation::superseded_by(compact(&far))],
                ..Content::default()
            },
        ),
    );
    // near is superseded by middle, so walking from near reaches far through it.
    admit(
        &root,
        payload(
            Action::ObjectSuperseded,
            &near,
            Content {
                text: "replaced by the middle".to_owned(),
                role: Some(Role::Supersession),
                relations: vec![Relation::superseded_by(compact(&middle))],
                ..Content::default()
            },
        ),
    );

    let replaces_source = || {
        payload(
            Action::ObjectSuperseded,
            &source,
            Content {
                text: "replaced by the near end".to_owned(),
                role: Some(Role::Supersession),
                relations: vec![Relation::superseded_by(compact(&near))],
                ..Content::default()
            },
        )
    };
    // Intact, the whole chain walks and the supersession is admissible.
    gate::prepare(&root, replaces_source()).expect("an intact chain");
    gate::discard(
        &root,
        &gate::pending_codes(&root).expect("pending")[0].clone(),
    )
    .expect("clear");

    // Now the middle of the chain will not load. Everything beyond it is
    // unreachable, so whether this replacement closes a cycle is no longer
    // establishable — and saying nothing is the one answer that is wrong.
    rewrite(&root, &middle, |value| {
        value["state"] = Value::String("not-a-state".into());
    });
    assert!(
        ops::effective(&root, &middle).is_err(),
        "the middle is unreadable"
    );

    let error = gate::prepare(&root, replaces_source())
        .expect_err("a chain that cannot be walked is not a chain that is clear");
    assert!(
        error.message.contains("will not load"),
        "the refusal must say why it could not be established: {error}"
    );
}

/// A classification that changes nothing is not a change to confirm.
///
/// Confirming it would append a permanent Event recording no change, spend a
/// `rev`, and invalidate every other live candidate for the Object — three
/// lasting consequences for an operation that does nothing, and a human asked
/// to assent to nothing.
#[test]
fn classifying_an_object_into_the_state_it_already_holds_is_refused() {
    let (_dir, root) = workspace();
    let id = new_object(&root, "already there");

    // A fresh object is untyped and open, so that is already a no-op.
    let error = gate::prepare(
        &root,
        payload(classify(None, State::Open), &id, Content::default()),
    )
    .expect_err("nothing to confirm");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("nothing to confirm"), "{error}");

    let before = ops::effective(&root, &id).expect("object").rev;
    let object = classified(&root, &id, Some(ObjectType::Design), State::Draft);
    assert_eq!(object.rev, before + 1, "a real change is one revision");

    // And it holds for a typed classification too, not just the untyped default.
    let error = gate::prepare(
        &root,
        payload(
            classify(Some(ObjectType::Design), State::Draft),
            &id,
            Content::default(),
        ),
    )
    .expect_err("still nothing to confirm");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert_eq!(
        ops::effective(&root, &id).expect("object").rev,
        before + 1,
        "and the refusal spent nothing"
    );
}
