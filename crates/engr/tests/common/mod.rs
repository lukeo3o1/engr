//! What every test file needs to put something in the record.
//!
//! Not a second implementation of the gate: everything here goes through
//! `gate::prepare` and `engr::confirm`, so a test that admits an Object has
//! exercised the same door a person would. What it removes is the boilerplate —
//! building a payload, reading the minted code back, formatting the response —
//! which is the part that says nothing about the property being pinned.

#![allow(dead_code)]

use engr::model::{Action, Content, Destination, Merge, Object, Payload, SectionValue};
use engr::semantics::{Admission, Admitted, BasedOn, Relation, Role, Supplement};
use engr::{gate, store};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// A bare workspace with no repository behind it.
pub fn workspace() -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().to_path_buf();
    store::init(&root).expect("init");
    (dir, root)
}

/// A workspace inside a repository with one commit, for anything that pins a
/// basis or a reference.
pub fn repository() -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().to_path_buf();
    git(&root, &["init", "-q", "."]);
    git(&root, &["config", "user.email", "test@example.com"]);
    git(&root, &["config", "user.name", "test"]);
    std::fs::write(root.join("README.md"), "a repository\n").expect("write");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "first"]);
    store::init(&root).expect("init");
    (dir, root)
}

pub fn git(root: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

pub fn head(root: &Path) -> String {
    git(root, &["rev-parse", "HEAD"])
}

/// Prepare and confirm one payload through the Human Gate.
pub fn admit(root: &Path, payload: Payload) -> Object {
    let prepared = gate::prepare(root, payload).expect("prepare");
    confirm(root, prepared.candidate.code())
}

/// Answer a minted code exactly.
pub fn confirm(root: &Path, code: &str) -> Object {
    match engr::confirm(root, &format!("CONFIRM {code}")).expect("confirm") {
        engr::Confirmed::Object(admitted) => admitted.object,
        engr::Confirmed::Migration(_) => panic!("expected an Object confirmation"),
    }
}

/// The same, keeping the Event as well as the Object.
pub fn admitted(root: &Path, payload: Payload) -> gate::Admitted {
    let prepared = gate::prepare(root, payload).expect("prepare");
    match engr::confirm(root, &format!("CONFIRM {}", prepared.candidate.code())).expect("confirm") {
        engr::Confirmed::Object(admitted) => *admitted,
        engr::Confirmed::Migration(_) => panic!("expected an Object confirmation"),
    }
}

/// A Section value admitted through the Human door.
pub fn value(content: Content) -> SectionValue {
    SectionValue::new(Admitted::new(Admission::Human, now()), content)
}

/// The same, through the Agent door.
pub fn agent_value(content: Content) -> SectionValue {
    SectionValue::new(Admitted::new(Admission::Agent, now()), content)
}

pub fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("timestamp")
}

/// Plain wording with no basis, references or relations.
pub fn wording(text: &str) -> Content {
    Content {
        text: text.to_owned(),
        ..Content::default()
    }
}

/// Wording written against a commit.
pub fn based_on(text: &str, commit: &str) -> Content {
    Content {
        text: text.to_owned(),
        based_on: Some(BasedOn::new(commit)),
        ..Content::default()
    }
}

pub fn create(object: &str, title: &str) -> Payload {
    Payload::new(
        object,
        Action::ObjectCreated {
            title: title.to_owned(),
        },
    )
}

pub fn rename(object: &str, title: &str) -> Payload {
    Payload::new(
        object,
        Action::ObjectRenamed {
            title: title.to_owned(),
            becomes: None,
        },
    )
}

pub fn add(object: &str, content: Content) -> Payload {
    Payload::new(
        object,
        Action::SectionCreated {
            value: value(content),
            becomes: None,
        },
    )
}

pub fn update(object: &str, section: u64, content: Content) -> Payload {
    Payload::new(
        object,
        Action::SectionUpdated {
            section,
            value: value(content),
            becomes: None,
        },
    )
}

pub fn delete(object: &str, section: u64) -> Payload {
    Payload::new(
        object,
        Action::SectionDeleted {
            section,
            becomes: None,
        },
    )
}

pub fn merge(object: &str, destination: u64, sources: Vec<u64>, content: Content) -> Payload {
    Payload::new(
        object,
        Action::SectionMerged {
            merge: Merge {
                destination,
                sources,
            },
            value: value(content),
            becomes: None,
        },
    )
}

pub fn change_state(object: &str, state: engr::semantics::State) -> Payload {
    Payload::new(object, Action::ObjectStateChanged { state })
}

pub fn classify(
    object: &str,
    object_type: Option<engr::semantics::ObjectType>,
    state: engr::semantics::State,
) -> Payload {
    Payload::new(object, Action::ObjectClassified { object_type, state })
}

pub fn supersede(object: &str, replacement: &str, reason: &str) -> Payload {
    let compact = engr::reference::encode_uuid_str(replacement).expect("compact");
    let content = Content {
        role: Some(Role::Supersession),
        text: reason.to_owned(),
        relations: vec![Relation::superseded_by(format!("obj:{compact}"))],
        ..Content::default()
    };
    Payload::new(
        object,
        Action::ObjectSuperseded {
            value: value(content),
        },
    )
}

/// Put a destination on an action that takes one.
pub fn becoming(mut payload: Payload, destination: Destination) -> Payload {
    payload
        .action
        .set_becomes(Some(destination))
        .expect("this action takes a destination");
    payload
}

/// Create an Object and return its id.
pub fn new_object(root: &Path, title: &str) -> String {
    let id = engr::model::new_id();
    admit(root, create(&id, title));
    id
}

/// Create an Object with one Section of plain wording.
pub fn object_with_section(root: &Path, title: &str, text: &str) -> String {
    let id = new_object(root, title);
    admit(root, add(&id, wording(text)));
    id
}

pub fn supplement(content_type: &str, body: &str) -> Supplement {
    Supplement::new(content_type, body)
}

/// The action a test is asking for, before its wording is attached.
///
/// Tests name an operation and then hand it content; the persisted action
/// carries both together. Keeping the selector separate is what lets one
/// `payload` builder serve every operation, and what keeps a test that is about
/// wording from having to spell out an admission structure.
#[derive(Clone)]
pub enum Act {
    Create,
    Rename,
    Add,
    Revise(u64),
    Merge {
        destination: u64,
        sources: Vec<u64>,
    },
    Delete(u64),
    Close,
    Reopen,
    Classify {
        object_type: Option<engr::semantics::ObjectType>,
        state: engr::semantics::State,
    },
    Supersede,
    Repair,
}

/// Build the payload for one action through the Human door.
pub fn payload(act: Act, object: &str, content: Content) -> Payload {
    payload_by(act, object, content, Admission::Human)
}

/// The same, through the Agent door.
pub fn agent_payload(act: Act, object: &str, content: Content) -> Payload {
    payload_by(act, object, content, Admission::Agent)
}

pub fn payload_by(act: Act, object: &str, content: Content, by: Admission) -> Payload {
    let admitted = SectionValue::new(Admitted::new(by, now()), content.clone());
    let action = match act {
        Act::Create => Action::ObjectCreated {
            title: content.text,
        },
        Act::Rename => Action::ObjectRenamed {
            title: content.text,
            becomes: None,
        },
        Act::Add => Action::SectionCreated {
            value: admitted,
            becomes: None,
        },
        Act::Revise(section) => Action::SectionUpdated {
            section,
            value: admitted,
            becomes: None,
        },
        Act::Merge {
            destination,
            sources,
        } => Action::SectionMerged {
            merge: Merge {
                destination,
                sources,
            },
            value: admitted,
            becomes: None,
        },
        Act::Delete(section) => Action::SectionDeleted {
            section,
            becomes: None,
        },
        Act::Close => Action::ObjectStateChanged {
            state: engr::semantics::State::Closed,
        },
        Act::Reopen => Action::ObjectStateChanged {
            state: engr::semantics::State::Open,
        },
        Act::Classify { object_type, state } => Action::ObjectClassified { object_type, state },
        Act::Supersede => Action::ObjectSuperseded { value: admitted },
        Act::Repair => Action::ObjectRepaired {},
    };
    Payload::new(object, action)
}

/// A Ref onto one Section's text, pinned at `commit`.
pub fn text_ref(root: &Path, object: &str, section: u64, commit: &str) -> engr::model::Ref {
    field_ref(
        root,
        object,
        section,
        commit,
        &[engr::dependency::SemanticField::Text],
    )
}

pub fn field_ref(
    root: &Path,
    object: &str,
    section: u64,
    commit: &str,
    fields: &[engr::dependency::SemanticField],
) -> engr::model::Ref {
    let target = engr::ops::effective(root, object).expect("reference target");
    engr::dependency::admit(root, &target, section, fields, commit).expect("admit reference")
}

impl Act {
    /// Whether this action carries wording a human must read.
    pub fn carries_content(&self) -> bool {
        matches!(
            self,
            Self::Create
                | Self::Rename
                | Self::Add
                | Self::Revise(_)
                | Self::Merge { .. }
                | Self::Supersede
        )
    }

    /// The Event type this action becomes.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Create => "object.created.v1",
            Self::Rename => "object.renamed.v1",
            Self::Add => "section.created.v1",
            Self::Revise(_) => "section.updated.v1",
            Self::Merge { .. } => "section.merged.v1",
            Self::Delete(_) => "section.deleted.v1",
            Self::Close | Self::Reopen => "object.state_changed.v1",
            Self::Classify { .. } => "object.classified.v1",
            Self::Supersede => "object.superseded.v1",
            Self::Repair => "object.repaired.v1",
        }
    }
}

/// Answer a minted code, keeping the Event as well as the Object.
pub fn admitted_code(root: &Path, code: &str) -> gate::Admitted {
    match engr::confirm(root, &format!("CONFIRM {code}")).expect("confirm") {
        engr::Confirmed::Object(admitted) => *admitted,
        engr::Confirmed::Migration(_) => panic!("expected an Object confirmation"),
    }
}

/// A working copy of the released predecessor workspace, history and all.
///
/// Cloned rather than copied, because the record's references pin commits and
/// resolving one reads the Object out of the commit it names. A `.engr` without
/// its history is a different fixture that happens to have the same files in it.
///
/// The line-ending settings are not defensive tidiness. Every Section carries a
/// seal over its exact octets, so a checkout that helpfully rewrote them would
/// turn the whole fixture into a forgery report on a platform that configures
/// `core.autocrlf` globally.
pub fn released() -> (TempDir, PathBuf) {
    let temp = TempDir::new().expect("temp dir");
    let root = temp.path().join("project");
    let bundle = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("released-v1")
        .join("history.bundle");
    let output = std::process::Command::new("git")
        .args([
            "clone",
            "--quiet",
            "-c",
            "core.autocrlf=false",
            "-c",
            "core.eol=lf",
        ])
        .arg(bundle)
        .arg(&root)
        .output()
        .expect("git clone");
    assert!(
        output.status.success(),
        "clone the released fixture: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    (temp, root)
}
