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

fn classify(object_type: Option<ObjectType>, state: State) -> Action {
    Action::ObjectClassified { object_type, state }
}

fn classified(root: &Path, id: &str, object_type: Option<ObjectType>, state: State) -> Object {
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
        raw.get("type").is_none(),
        "an untyped object stores no type at all: {raw}"
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
    let pin = |section: u64| Ref {
        object: target.clone(),
        section,
        sha256: ops::effective_section(&root, &target, section)
            .expect("section")
            .sha256,
        commit: commit.clone(),
    };
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
    assert_eq!(
        stored.refs,
        vec![pin(1), pin(2)],
        "the gate puts a set in one order before it is hashed"
    );
    assert_eq!(
        stored.relations,
        vec![implemented("check"), implemented("verify")]
    );

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
        .payload_sha256
        .clone();
    for variant in variants {
        let candidate = gate::prepare(&root, payload(Action::SectionAdded, &id, variant))
            .expect("prepare")
            .candidate;
        assert_ne!(
            candidate.payload_sha256, plain,
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
    rewrite(&root, &id, |value| {
        let object = value.as_object_mut().expect("object");
        object.insert("format".to_owned(), Value::String("engr-object".to_owned()));
        object.insert("version".to_owned(), Value::from(1));
        let state = object.remove("state").expect("state");
        object.insert("status".to_owned(), state);
    });
    std::fs::remove_file(store::engr_dir(&root).join("format.json")).expect("format");

    assert_eq!(
        store::validate_format(&root).expect("detect"),
        store::WorkspaceFormat::LegacyV0
    );
    // Readable before migration, and unchanged on disk.
    let before = ops::effective(&root, &id).expect("legacy read");
    assert_eq!(before.state, State::Closed);
    assert_eq!(before.object_type, None);

    store::with_lock(&root, || store::migrate(&root)).expect("migrate");
    let after = store::load_object(&root, &id).expect("migrated");
    assert_eq!(after.state, State::Closed);
    assert_eq!(
        after.object_type, None,
        "migration classifies nothing: the old record does not say what kind of thing it is"
    );
    let raw: Value = store::read_json(&store::object_path(&root, &id)).expect("raw");
    assert!(raw.get("status").is_none(), "{raw}");
    assert!(raw.get("type").is_none(), "{raw}");
    assert_eq!(raw["state"], Value::String("closed".to_owned()));

    // And the old vocabulary still works on it, because that is what its own
    // confirmed history is written in.
    let object = admit(
        &root,
        payload(Action::ObjectReopened, &id, Content::default()),
    );
    assert_eq!(object.state, State::Open);
}
