//! The published release's workspace, brought forward under this binary.
//!
//! Everything here runs against `tests/fixtures/released-v1`, which the
//! published `latest` release wrote through its own Human Gate. That is the
//! point: a hand-authored file proves the migrator accepts a shape somebody
//! believed version 1 had, and only the release's own output proves it accepts
//! what the release actually wrote. See the fixture's `PROVENANCE.md`.
//!
//! The suite is arranged as the claim it has to support. First that the fixture
//! is what it says it is, then that it migrates, then that migration preserved
//! every semantic the predecessor proved and invented none, then that each way
//! of being wrong is still refused, then that the coordinated window still
//! behaves, and last that the result is an ordinary current workspace rather
//! than a structure that passes one assertion.

use engr::model::{
    Action, Content, Event, HumanConfirmation, Payload, Provenance, TaggedAdmission,
};
use engr::semantics::Admission;
use engr::store::WorkspaceFormat;
use engr::{backlog, collection, dependency, gate, integrity, proof, rules, store, work};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

/// The Objects the release admitted, by what they are about.
const AUTHORITY: &str = "01a049f0-16fb-7c03-a75d-97980cc8c613";
const MODEL: &str = "01a049f0-1d33-7912-9e57-a67b06866805";
const PROVENANCE_OBJECT: &str = "01a049f0-271b-7971-aee7-674dbbadb7f0";
const PROJECTION: &str = "01a049f0-3711-7ca2-8438-7e1f7b620b7a";

/// The commits its two legacy references pin, and the tip they were taken at.
const AUTHORITY_COMMIT: &str = "cce71a0d95c24780dcc7f71b20b5160c5dd3b477";
const MODEL_COMMIT: &str = "cda4069e13d750b84be0b91d6d605ce526ecc194";
const HEAD: &str = "7140a349b81c34fd7027a9d81f04e5ea6e0dfcf6";

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("released-v1")
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git");
    assert!(
        output.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// A working copy of the released workspace, history and all.
///
/// Cloned rather than copied, because the record's references pin commits and
/// resolving one reads the Object out of the commit it names. A `.engr` without
/// its history is a different fixture that happens to have the same files in it.
///
/// The line-ending settings are not defensive tidiness. Every Section carries a
/// seal over its exact octets, so a checkout that helpfully rewrote them would
/// turn the whole fixture into a forgery report on a platform that configures
/// `core.autocrlf` globally.
fn released_v1() -> (TempDir, PathBuf) {
    let temp = TempDir::new().expect("temp dir");
    let root = temp.path().join("project");
    let output = Command::new("git")
        .args([
            "clone",
            "--quiet",
            "-c",
            "core.autocrlf=false",
            "-c",
            "core.eol=lf",
        ])
        .arg(fixture().join("history.bundle"))
        .arg(&root)
        .output()
        .expect("git clone");
    assert!(
        output.status.success(),
        "clone the released fixture: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), HEAD);
    (temp, root)
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn write(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
}

fn stored(root: &Path, id: &str) -> Value {
    serde_json::from_str(&read(&store::object_path(root, id))).expect("stored object")
}

/// Rewrite one predecessor Object, the way a hand edit or another tool would.
fn edit_stored(root: &Path, id: &str, change: impl FnOnce(&mut Value)) {
    let mut value = stored(root, id);
    change(&mut value);
    write(
        &store::object_path(root, id),
        &format!("{}\n", serde_json::to_string_pretty(&value).expect("json")),
    );
}

fn stage(root: &Path) -> PathBuf {
    store::engr_dir(root).join("migration-v3")
}

/// A Backlog subject and a Collection member name a resource by its compact
/// reference, not by the UUID the file is called after.
fn compact_object_ref(id: &str) -> String {
    format!(
        "obj:{}",
        engr::reference::encode_uuid_str(id).expect("compact")
    )
}

/// What the declared authority says, without going through the read path that
/// refuses to answer while a stage exists.
fn declared_version(root: &Path) -> u64 {
    let format: Value =
        serde_json::from_str(&read(&store::engr_dir(root).join("format.json"))).expect("format");
    format["version"].as_u64().expect("version")
}

/// One named way of breaking a legacy reference, the exit code it has to be
/// refused with, and a phrase its message has to carry.
type BrokenReference = (&'static str, Box<dyn Fn(&mut Value)>, i32, &'static str);

/// A named way to place a later resource domain beside a v1 workspace.
type PlacedDomain = (&'static str, Box<dyn Fn(&Path)>);

/// One named way a predecessor workspace can fail to be one, and the exit code
/// it has to be refused with.
type Malformation = (&'static str, Box<dyn Fn(&Path)>, i32);

/// Every relative path under `.engr`, with the digest of its bytes.
///
/// `lock` is left out: it is a mutex for this machine, the predecessor
/// generation gitignored it, and taking the workspace lock is what any command
/// does before it decides whether it may do anything at all. Counting it would
/// make "preflight wrote nothing" fail on a migration that wrote nothing.
fn fingerprint(root: &Path) -> std::collections::BTreeMap<String, String> {
    let base = store::engr_dir(root);
    let mut found = std::collections::BTreeMap::new();
    let mut pending = vec![base.clone()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).expect("read dir") {
            let entry = entry.expect("entry");
            let path = entry.path();
            if entry.file_type().expect("file type").is_dir() {
                pending.push(path);
                continue;
            }
            if path.file_name().is_some_and(|name| name == "lock") {
                continue;
            }
            let relative = path
                .strip_prefix(&base)
                .expect("inside .engr")
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            found.insert(relative, proof::sha256_of(&read(&path)));
        }
    }
    found
}

// ---------------------------------------------------------------- the fixture

/// The readable copy and the history are the same workspace.
///
/// `workspace/` exists so the version 1 bytes can be reviewed in a diff instead
/// of only inside a pack file, which is worth nothing if the two can drift. So
/// they are compared rather than trusted, and the comparison is byte for byte:
/// every seal in the record is over exact octets.
#[test]
fn the_readable_fixture_is_the_history_it_was_taken_from() {
    let (_temp, root) = released_v1();
    let readable = fixture().join("workspace");

    let mut checked = 0;
    for relative in [
        ".engr/.gitignore",
        ".engr/format.json",
        ".engr/objects/01a049f0-16fb-7c03-a75d-97980cc8c613.json",
        ".engr/objects/01a049f0-1d33-7912-9e57-a67b06866805.json",
        ".engr/objects/01a049f0-271b-7971-aee7-674dbbadb7f0.json",
        ".engr/objects/01a049f0-3711-7ca2-8438-7e1f7b620b7a.json",
        ".engr/events/01a049f0-16fb-7c03-a75d-97980cc8c613.jsonl",
        ".engr/events/01a049f0-1d33-7912-9e57-a67b06866805.jsonl",
        ".engr/events/01a049f0-271b-7971-aee7-674dbbadb7f0.jsonl",
        ".engr/events/01a049f0-3711-7ca2-8438-7e1f7b620b7a.jsonl",
        "README.md",
        "ARCHITECTURE.md",
    ] {
        let from_history = std::fs::read(root.join(relative)).expect("cloned file");
        let from_readable = std::fs::read(readable.join(relative)).expect("readable file");
        assert_eq!(from_history, from_readable, "{relative}");
        checked += 1;
    }
    assert_eq!(checked, 12);

    let format: Value =
        serde_json::from_str(&read(&store::engr_dir(&root).join("format.json"))).expect("format");
    assert_eq!(format["format"], "engr-workspace");
    assert_eq!(format["version"], 1, "the released generation");
}

/// The released generation is recognized, readable, and not writable.
///
/// Before this, all three of `ls`, `show` and `verify` failed the same way as a
/// corrupt workspace: `workspace version 1 is not supported`. A person holding
/// a record the shipped binary wrote could not tell from the message whether
/// their workspace was too old, broken, or needed a tool they did not have.
#[test]
fn a_released_v1_workspace_reads_and_says_what_it_needs() {
    let (_temp, root) = released_v1();

    assert_eq!(
        store::validate_format(&root).expect("recognized"),
        WorkspaceFormat::OlderVersion(1)
    );
    let object = store::load_object(&root, AUTHORITY).expect("v1 Objects still read");
    assert_eq!(object.title, "Human authority and the admission boundary");

    let error = store::require_current(&root).expect_err("but nothing may be written to it");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(
        error.message.contains("engr migrate"),
        "the refusal names the next action: {error}"
    );
}

/// A version this build has never heard of is a different refusal.
///
/// Recognized-but-older has a next action. A version above this build's does
/// not, and saying "run `engr migrate`" to somebody holding a newer workspace
/// would send them to a command that cannot help.
#[test]
fn an_unknown_generation_is_refused_without_offering_migration() {
    let (_temp, root) = released_v1();
    write(
        &store::engr_dir(&root).join("format.json"),
        "{\"format\":\"engr-workspace\",\"version\":9}",
    );

    let error = store::validate_format(&root).expect_err("a future generation");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("newer engr"), "{error}");
    assert!(!error.message.contains("migrate"), "{error}");
}

// --------------------------------------------------------------- the migration

/// The path the compatibility ruling requires: released v1, one command, v3.
#[test]
fn a_released_v1_workspace_migrates_to_the_current_generation() {
    let (_temp, root) = released_v1();

    store::migrate(&root).expect("released v1 migrates directly");

    assert_eq!(
        store::validate_format(&root).expect("format"),
        WorkspaceFormat::Current
    );
    assert_eq!(
        read(&store::engr_dir(&root).join("format.json")),
        "{\"format\":\"engr-workspace\",\"version\":3}",
        "the authority is itself a current resource, so it is JCS"
    );
    assert!(!stage(&root).exists(), "nothing is left staged");

    for id in [AUTHORITY, MODEL, PROVENANCE_OBJECT, PROJECTION] {
        let object = store::load_object(&root, id).expect("every Object loads as current");
        integrity::check_stored_object_integrity(&object).expect("and verifies");
        let effective = engr::ops::effective(&root, id).expect("and its history still projects");
        assert_eq!(effective.rev, object.rev);
    }
}

/// Everything the predecessor proved survives, and nothing it did not is added.
#[test]
fn migration_preserves_what_the_release_admitted_and_invents_nothing() {
    let (_temp, root) = released_v1();
    let before: Vec<Value> = [AUTHORITY, MODEL, PROVENANCE_OBJECT, PROJECTION]
        .iter()
        .map(|id| stored(&root, id))
        .collect();

    store::migrate(&root).expect("migrate");

    for predecessor in &before {
        let id = predecessor["id"].as_str().expect("id");
        let object = store::load_object(&root, id).expect("migrated");

        assert_eq!(object.id, id, "Object identity");
        assert_eq!(object.title, predecessor["title"], "title");
        assert_eq!(object.state.as_str(), predecessor["state"], "lifecycle");
        assert_eq!(object.rev, predecessor["rev"].as_u64().expect("rev"), "rev");
        assert_eq!(
            object.next_section_id,
            predecessor["next_section_id"].as_u64().expect("counter"),
            "the counter that keeps deleted Section ids from being handed out again"
        );
        assert_eq!(
            object.object_type, None,
            "the predecessor classified nothing, so the migration classifies nothing"
        );

        let sections = predecessor["sections"].as_array().expect("sections");
        assert_eq!(object.sections.len(), sections.len());
        for (migrated, predecessor) in object.sections.iter().zip(sections) {
            assert_eq!(
                migrated.id,
                predecessor["id"].as_u64().expect("section id"),
                "Section identity"
            );
            assert_eq!(migrated.text, predecessor["text"], "durable wording");
            assert_eq!(
                migrated.admission,
                Admission::Human,
                "§{}: the release had one door, and migrating is not walking through another",
                migrated.id
            );
            assert_eq!(
                migrated.admitted_at, predecessor["confirmed_at"],
                "§{}: the instant a human confirmed it, under the authority-neutral name",
                migrated.id
            );
            assert_eq!(
                migrated.based_on.as_deref(),
                predecessor["based_on"].as_str(),
                "§{}: the committed basis, including its absence",
                migrated.id
            );
            assert_eq!(
                migrated.refs.len(),
                predecessor["refs"].as_array().unwrap().len()
            );
            assert!(migrated.relations.is_empty());
            assert!(migrated.content.is_empty());
            assert_eq!(migrated.role, None);
        }
    }

    // The one Section the release admitted with no repository basis at all. v1
    // left the member out of the file; v3 spells absence as an explicit null,
    // and neither is a basis.
    let model = store::load_object(&root, MODEL).expect("model");
    assert_eq!(model.sections[1].based_on, None);
    assert_eq!(stored(&root, MODEL)["sections"][1]["based_on"], Value::Null);

    // Section ids are the record's own claim about identity surviving deletion:
    // four were added, two were merged into a fifth, and the third was removed.
    let provenance = store::load_object(&root, PROVENANCE_OBJECT).expect("provenance");
    assert_eq!(
        provenance
            .sections
            .iter()
            .map(|section| section.id)
            .collect::<Vec<_>>(),
        [4, 5]
    );
    assert_eq!(provenance.next_section_id, 6);

    let projection = store::load_object(&root, PROJECTION).expect("projection");
    assert_eq!(projection.state, engr::semantics::State::Closed);
}

/// Retained history stays under the contract that wrote it.
///
/// Migration reads Event v1 to prove the projection is derivable, and that is
/// all it does with it. Rewriting those bytes to look current would turn an
/// immutable record of what happened into a second statement of what is true
/// now, which is exactly the thing the generation split exists to prevent.
#[test]
fn admitted_history_proves_the_projection_and_is_not_rewritten() {
    let (_temp, root) = released_v1();
    let history: Vec<(PathBuf, String)> = [AUTHORITY, MODEL, PROVENANCE_OBJECT, PROJECTION]
        .iter()
        .map(|id| {
            let path = store::events_path(&root, id);
            let text = read(&path);
            (path, text)
        })
        .collect();

    store::migrate(&root).expect("migrate");

    for (path, before) in &history {
        assert_eq!(&read(path), before, "{}", path.display());
    }
    let events = store::load_events(&root, PROVENANCE_OBJECT).expect("history still reads");
    assert_eq!(events.len(), 7);
    for event in &events {
        assert_eq!(event.version, 1, "history keeps its own generation");
        assert!(matches!(event.provenance, Provenance::Confirmed { .. }));
    }
}

// -------------------------------------------------------------- legacy refs

/// A whole-Section reference becomes a selective one over the same material.
///
/// Both of the fixture's references are checked, and the second is the one that
/// matters most: its target carries a reference of its own, so converting it
/// rewrites the very member the predecessor seal covers.
#[test]
fn legacy_references_convert_and_still_resolve() {
    let (_temp, root) = released_v1();

    store::migrate(&root).expect("migrate");

    for (holder, section, target_id, commit) in [
        (MODEL, 1, AUTHORITY, AUTHORITY_COMMIT),
        (PROVENANCE_OBJECT, 4, MODEL, MODEL_COMMIT),
    ] {
        let object = store::load_object(&root, holder).expect("holder");
        let held = object
            .sections
            .iter()
            .find(|candidate| candidate.id == section)
            .expect("section");
        let selective = held.refs[0].as_selective().expect("selective now");
        assert_eq!(
            selective.commit(),
            commit,
            "the commit the release pinned is the commit that is still pinned"
        );
        assert_eq!(
            selective.target(),
            proof::section_target(target_id, 1),
            "and the same Section"
        );
        let fields: Vec<_> = selective
            .fields()
            .iter()
            .map(|field| field.as_str())
            .collect();
        assert_eq!(
            fields,
            ["based_on", "content", "refs", "relations", "role", "text"],
            "a whole-Section pin selects every semantic field, and no more"
        );

        let target = store::load_object(&root, target_id).expect("target");
        assert_eq!(
            dependency::evaluate(
                &root,
                &target,
                target.sha256.as_deref().expect("object seal"),
                selective,
            )
            .expect("evaluate"),
            dependency::Dependency::Unchanged,
            "{holder} §{section} still depends on exactly what it depended on"
        );
    }
}

/// A pin whose target says something else at the commit it names is refused,
/// and refused as a broken pin rather than as broken material.
///
/// The commit is rewritten to hold a version of the target that is internally
/// perfect — resealed against its own new wording — so nothing about it is
/// detectably damaged. What is wrong is only that it is not what the reference
/// claimed, which is the whole job of pinning a seal.
#[test]
fn a_reference_to_a_tampered_historical_target_is_refused() {
    let (_temp, root) = released_v1();
    let commit_as_test = |root: &Path, message: &str| {
        git(root, &["add", ".engr"]);
        git(
            root,
            &[
                "-c",
                "user.name=engr test",
                "-c",
                "user.email=engr@example.invalid",
                "commit",
                "-m",
                message,
            ],
        );
        git(root, &["rev-parse", "HEAD"])
    };

    edit_stored(&root, AUTHORITY, |object| {
        object["sections"][0]["text"] = Value::String("wording nobody confirmed".to_owned());
    });
    reseal_predecessor(&root, AUTHORITY, 0);
    let rewritten = commit_as_test(&root, "a past that says something else");
    // Put the working tree back, so the only thing that is not what it was is
    // what the newly pinned commit holds.
    git(&root, &["checkout", HEAD, "--", ".engr"]);
    edit_stored(&root, MODEL, |object| {
        object["sections"][0]["refs"][0]["commit"] = Value::String(rewritten.clone());
    });
    reseal_predecessor(&root, MODEL, 0);

    let error = store::migrate(&root).expect_err("a pin the target does not support");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(
        error.message.contains("not the legacy reference seal"),
        "{error}"
    );
    assert_eq!(
        store::validate_format(&root).expect("format"),
        WorkspaceFormat::OlderVersion(1),
        "and the workspace is still the predecessor"
    );
}

/// The three ways a legacy reference can fail to resolve stay three answers.
///
/// #35 keeps these apart at read time — absence is not integrity failure, a
/// lost commit is not drift — and migration is where a pin stops being a claim
/// and becomes a v3 digest, so it is the last place they may be collapsed.
/// None of them converts into a reference that resolves to nothing.
#[test]
fn unresolvable_references_stay_distinguishable_and_stop_the_migration() {
    let cases: Vec<BrokenReference> = vec![
        (
            // The commit is gone, so there is no workspace to inspect there.
            "provenance unavailable",
            Box::new(|object: &mut Value| {
                object["sections"][0]["refs"][0]["commit"] = Value::String("f".repeat(40));
            }),
            engr::EXIT_SCHEMA,
            "historical workspace at commit",
        ),
        (
            "target missing",
            Box::new(|object: &mut Value| {
                object["sections"][0]["refs"][0]["object"] =
                    Value::String("018f7d58-4ca7-7a2e-98f1-9b3014681848".to_owned());
            }),
            engr::EXIT_NOT_FOUND,
            "is absent at",
        ),
        (
            "the Section is not in the target",
            Box::new(|object: &mut Value| {
                object["sections"][0]["refs"][0]["section"] = Value::from(97u64);
            }),
            engr::EXIT_NOT_FOUND,
            "does not exist",
        ),
    ];

    for (what, break_it, code, says) in cases {
        let (_temp, root) = released_v1();
        edit_stored(&root, MODEL, break_it);
        reseal_predecessor(&root, MODEL, 0);

        let error = store::migrate(&root).expect_err(what);
        assert_eq!(error.code, code, "{what}: {}", error.message);
        assert!(error.message.contains(says), "{what}: {error}");
        assert_eq!(
            store::validate_format(&root).expect("format"),
            WorkspaceFormat::OlderVersion(1),
            "{what}: nothing was published"
        );
    }
}

/// Reseal one predecessor Section against its edited content, and re-derive the
/// history that has to agree with it.
///
/// A forgery that skips this is not testing what it thinks it is. A Section
/// whose seal no longer covers it is caught by the seal check, and a projection
/// its history does not derive is caught by the derivation check — both of
/// which have tests of their own. To find out whether the *generation boundary*
/// holds, the workspace has to be internally perfect and wrong only about which
/// generation it belongs to. That means resealing the Section and carrying the
/// same edit into the Event that admitted it, exactly as a build with those
/// members would have written both.
fn reseal_predecessor(root: &Path, id: &str, index: usize) {
    let mut object = stored(root, id);
    let content: Content =
        serde_json::from_value(object["sections"][index].clone()).expect("legacy content");
    let seal = content.sha256().expect("seal");
    object["sections"][index]["sha256"] = Value::String(seal);
    write(
        &store::object_path(root, id),
        &format!("{}\n", serde_json::to_string_pretty(&object).expect("json")),
    );

    // Every semantic member, not just `refs`: the derivation check compares the
    // whole Section, so a forgery that moved `role` and left the Event behind
    // would be refused for the wrong reason.
    let path = store::events_path(root, id);
    let section = &object["sections"][index];
    let id_of = section["id"].as_u64().expect("section id");
    let mut lines = Vec::new();
    for line in read(&path).lines() {
        let mut event: Value = serde_json::from_str(line).expect("event");
        let names_it = event["action"] == "section_added"
            || event["action"] == "section_revised" && event["section"] == id_of;
        if names_it && event["text"] == section["text"] {
            let members = event.as_object_mut().expect("event object");
            for member in ["role", "content", "based_on", "refs", "relations"] {
                match section.get(member) {
                    Some(value) => {
                        members.insert(member.to_owned(), value.clone());
                    }
                    None => {
                        members.remove(member);
                    }
                }
            }
            let payload: Payload =
                serde_json::from_value(event.clone()).expect("payload from event");
            event["confirmation"]["payload_sha256"] =
                Value::String(payload.sha256().expect("payload seal"));
        }
        lines.push(serde_json::to_string(&event).expect("event json"));
    }
    write(&path, &format!("{}\n", lines.join("\n")));
}

/// Append an Event to a predecessor history and advance the projection with it,
/// as the generation that wrote both would have.
fn append_predecessor_event(root: &Path, id: &str, mut event: Value) {
    let path = store::events_path(root, id);
    let history = read(&path);
    let last: Value =
        serde_json::from_str(history.lines().last().expect("history")).expect("event");
    let rev = last["rev"].as_u64().expect("rev") + 1;
    event["format"] = Value::String(engr::model::EVENT_FORMAT.to_owned());
    event["version"] = Value::from(1u64);
    event["event_id"] = Value::String(engr::model::new_id());
    event["rev"] = Value::from(rev);
    event["time"] = Value::String("2026-08-29T00:00:00Z".to_owned());
    event["object"] = Value::String(id.to_owned());
    seal_event(&mut event);
    write(
        &path,
        &format!(
            "{history}{}\n",
            serde_json::to_string(&event).expect("event json")
        ),
    );
    edit_stored(root, id, |object| object["rev"] = Value::from(rev));
}

/// Rewrite the last Event of a predecessor history in place, resealing it.
fn edit_last_predecessor_event(root: &Path, id: &str, change: impl FnOnce(&mut Value)) {
    let path = store::events_path(root, id);
    let mut lines: Vec<String> = read(&path).lines().map(str::to_owned).collect();
    let last = lines.pop().expect("history");
    let mut event: Value = serde_json::from_str(&last).expect("event");
    change(&mut event);
    seal_event(&mut event);
    lines.push(serde_json::to_string(&event).expect("event json"));
    write(&path, &format!("{}\n", lines.join("\n")));
}

fn seal_event(event: &mut Value) {
    let payload: Payload = serde_json::from_value(event.clone()).expect("payload from event");
    event["confirmation"] = serde_json::json!({
        "challenge": "234567",
        "payload_sha256": payload.sha256().expect("payload seal"),
    });
}

// -------------------------------------------------- malformed v1 predecessors

/// A v1 Object that still carries the pre-release per-resource envelope is
/// named as an unfinished conversion, not as an unrecognizable file.
#[test]
fn a_v1_object_still_in_the_v0_envelope_is_named_as_such() {
    let (_temp, root) = released_v1();
    edit_stored(&root, AUTHORITY, |object| {
        let map = object.as_object_mut().expect("object");
        let state = map.remove("state").expect("state");
        map.insert("status".to_owned(), state);
        map.insert("format".to_owned(), Value::String("engr-object".to_owned()));
        map.insert("version".to_owned(), Value::from(1u64));
    });

    let error = store::migrate(&root).expect_err("a half-converted predecessor");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(
        error.message.contains("legacy v0 per-resource envelope"),
        "{error}"
    );
}

/// A v1 workspace cannot carry members the v1 generation never had.
///
/// This is the boundary the whole change rests on, and it is not the same
/// question as "is this file damaged". Each forgery below is *internally
/// perfect*: the Section is resealed against its new content, the Event that
/// admitted it carries the same members, and the stored projection is exactly
/// derivable from that history. Every check the migrator makes about integrity
/// and derivation passes. The only thing wrong is the generation.
///
/// The members are real v3 semantics, and each is a different way of smuggling
/// meaning past a human. `type` and `state` classify an Object; `role` says
/// what a Section *is*, and `supersession` is the reason an Object was retired;
/// `relations` carries `superseded_by` and `implemented_by`, which are claims
/// the record then acts on; `content` carries verbatim supplementary bodies.
/// None existed when a v1 build wrote a workspace, so a v1 file carrying one is
/// a later generation's semantics wearing an older generation's label — and
/// admitting it means the migration, not a human, decided the record says
/// something new.
#[test]
fn a_v1_predecessor_cannot_carry_members_its_generation_never_had() {
    let cases: Vec<Malformation> = vec![
        (
            "an Object classified, which v1 had no vocabulary for",
            Box::new(|root: &Path| {
                append_predecessor_event(
                    root,
                    AUTHORITY,
                    serde_json::json!({
                        "action": "object_classified",
                        "type": "decision",
                        "state": "accepted",
                        "text": "",
                        "refs": [],
                    }),
                );
                edit_stored(root, AUTHORITY, |object| {
                    object["type"] = Value::String("decision".to_owned());
                    object["state"] = Value::String("accepted".to_owned());
                });
            }),
            engr::EXIT_SCHEMA,
        ),
        (
            "a Section with a role",
            Box::new(|root: &Path| {
                edit_stored(root, AUTHORITY, |object| {
                    object["sections"][0]["role"] = Value::String("supersession".to_owned());
                });
                reseal_predecessor(root, AUTHORITY, 0);
            }),
            engr::EXIT_SCHEMA,
        ),
        (
            "a Section carrying relations",
            Box::new(|root: &Path| {
                edit_stored(root, AUTHORITY, |object| {
                    object["sections"][0]["relations"] = serde_json::json!([{
                        "type": "implemented_by",
                        "target": {
                            "kind": "file",
                            "path": "src/lib.rs",
                            "commit": "a".repeat(40),
                        },
                    }]);
                });
                reseal_predecessor(root, AUTHORITY, 0);
            }),
            engr::EXIT_SCHEMA,
        ),
        (
            "a Section carrying supplementary bodies",
            Box::new(|root: &Path| {
                edit_stored(root, AUTHORITY, |object| {
                    object["sections"][0]["content"] = serde_json::json!([{
                        "type": "code.rust",
                        "body": "fn smuggled() {}",
                    }]);
                });
                reseal_predecessor(root, AUTHORITY, 0);
            }),
            engr::EXIT_SCHEMA,
        ),
        (
            "an Event carrying a destination, which v1 had no member for",
            Box::new(|root: &Path| {
                edit_last_predecessor_event(root, AUTHORITY, |event| {
                    event["becomes"] = serde_json::json!({
                        "type": "decision",
                        "state": "proposed",
                    });
                });
                edit_stored(root, AUTHORITY, |object| {
                    object["type"] = Value::String("decision".to_owned());
                    object["state"] = Value::String("proposed".to_owned());
                });
            }),
            engr::EXIT_SCHEMA,
        ),
    ];

    for (what, forge, code) in cases {
        let (_temp, root) = released_v1();
        forge(&root);
        let after_forgery = fingerprint(&root);

        let error = store::migrate(&root).expect_err(what);
        assert_eq!(error.code, code, "{what}: {}", error.message);
        assert_eq!(
            declared_version(&root),
            1,
            "{what}: the generation did not advance"
        );
        assert!(!stage(&root).exists(), "{what}: nothing was staged");
        assert_eq!(
            fingerprint(&root),
            after_forgery,
            "{what}: preflight wrote nothing"
        );
    }
}

/// Each way a released v1 workspace can fail to be one, refused before
/// anything is written.
///
/// One test rather than nine, because the assertion is the same in every case
/// and only the malformation varies: it is refused with the fault class it
/// actually is, nothing is published, and the workspace is left exactly as it
/// was found. The list is what a predecessor can be wrong about — its Object
/// envelope, a Section's shape, a seal, the derivation from admitted history,
/// the generation of that history, a number outside the shared domain, and
/// material that is already current.
#[test]
fn malformed_v1_predecessors_are_refused_and_publish_nothing() {
    let cases: Vec<Malformation> = vec![
        (
            "an Object claiming both lifecycle spellings",
            Box::new(|root: &Path| {
                edit_stored(root, AUTHORITY, |object| {
                    object["status"] = Value::String("open".to_owned());
                })
            }),
            engr::EXIT_SCHEMA,
        ),
        (
            "an Object already carrying a v3 aggregate seal",
            Box::new(|root: &Path| {
                edit_stored(root, AUTHORITY, |object| {
                    object["sha256"] = Value::String("a".repeat(64));
                })
            }),
            engr::EXIT_SCHEMA,
        ),
        (
            "a Section carrying an admission member the generation had no door for",
            Box::new(|root: &Path| {
                edit_stored(root, AUTHORITY, |object| {
                    object["sections"][0]["admission"] = Value::String("agent".to_owned());
                })
            }),
            engr::EXIT_SCHEMA,
        ),
        (
            "a Section timestamp spelled the current way",
            Box::new(|root: &Path| {
                edit_stored(root, AUTHORITY, |object| {
                    let section = object["sections"][0].as_object_mut().expect("section");
                    let at = section.remove("confirmed_at").expect("confirmed_at");
                    section.insert("admitted_at".to_owned(), at);
                })
            }),
            engr::EXIT_SCHEMA,
        ),
        (
            "a Section seal that does not cover its own content",
            Box::new(|root: &Path| {
                edit_stored(root, AUTHORITY, |object| {
                    object["sections"][0]["text"] =
                        Value::String("wording nobody confirmed".to_owned());
                })
            }),
            engr::EXIT_INVARIANT,
        ),
        (
            "a projection that admitted history does not derive",
            Box::new(|root: &Path| {
                edit_stored(root, AUTHORITY, |object| {
                    object["title"] = Value::String("a title nobody confirmed".to_owned());
                })
            }),
            engr::EXIT_INVARIANT,
        ),
        (
            "an Event from a generation this workspace never had",
            Box::new(|root: &Path| {
                let path = store::events_path(root, AUTHORITY);
                let text = read(&path);
                write(&path, &text.replace("\"version\":1", "\"version\":2"));
            }),
            engr::EXIT_SCHEMA,
        ),
        (
            "a Section id outside the shared safe-integer domain",
            Box::new(|root: &Path| {
                edit_stored(root, AUTHORITY, |object| {
                    object["next_section_id"] = Value::from(9_007_199_254_740_993u64);
                })
            }),
            engr::EXIT_SCHEMA,
        ),
        (
            "a reference already in the current spelling",
            Box::new(|root: &Path| {
                edit_stored(root, MODEL, |object| {
                    object["sections"][0]["refs"] = serde_json::json!([{
                        "target": proof::section_target(AUTHORITY, 1),
                        "fields": ["text"],
                        "commit": AUTHORITY_COMMIT,
                        "digest": format!("1:{}", "0".repeat(64)),
                    }]);
                });
                // Resealed, so the reference *spelling* is what refuses rather
                // than the seal that no longer covers it. A predecessor that
                // already holds current material is not a predecessor.
                reseal_predecessor(root, MODEL, 0);
            }),
            engr::EXIT_SCHEMA,
        ),
    ];

    for (what, malform, code) in cases {
        let (_temp, root) = released_v1();
        let before = fingerprint(&root);
        malform(&root);
        let after_edit = fingerprint(&root);

        let error = store::migrate(&root).expect_err(what);
        assert_eq!(error.code, code, "{what}: {}", error.message);
        assert_eq!(
            store::validate_format(&root).expect("format"),
            WorkspaceFormat::OlderVersion(1),
            "{what}: the generation did not advance"
        );
        assert!(!stage(&root).exists(), "{what}: nothing was staged");
        assert_eq!(
            fingerprint(&root),
            after_edit,
            "{what}: preflight wrote nothing"
        );
        assert_ne!(
            before, after_edit,
            "{what}: the fixture was actually changed"
        );
    }
}

/// A later resource domain in a version 1 workspace fails closed.
///
/// #32's ruling `5460403574` settles what "workspace version 1" means for
/// compatibility: the generation the published release shipped, which held
/// `format.json`, `objects/`, `events/` and `candidates/` and nothing else.
/// Rules, Backlog, Work and Collection all arrived afterwards, in builds that
/// were never published, so none of them is version 1 engr state.
///
/// Every resource below is *valid* — a rule that parses, resources the current
/// loaders accept. Being well-formed is not the question. The question is which
/// generation they belong to, and the answer for all four is: not this one.
///
/// `rules/` is the case with teeth, and it is checked twice. A rule file is
/// authored by a human and never by engr, so its presence beside a released
/// workspace says nothing about which build wrote that workspace — and
/// migrating it would hand a rule the released build never recognized authority
/// over agent admission, purchased with nothing but somebody running `engr
/// migrate`.
#[test]
fn a_later_resource_domain_in_a_v1_workspace_fails_closed() {
    let domains: Vec<PlacedDomain> = vec![
        (
            "rules",
            Box::new(|root: &Path| {
                std::fs::create_dir_all(rules::dir(root)).expect("rules dir");
                write(
                    &rules::dir(root).join("agent-policy.md"),
                    "---\nid: agent-policy\napplies:\n  domains:\n    - object\n---\n\n# Agent policy\n\nAnything an agent admits cites the commit it came from.\n",
                );
            }),
        ),
        (
            "backlog",
            Box::new(|root: &Path| {
                std::fs::create_dir_all(backlog::dir(root)).expect("backlog dir");
                let id = "01a04a06-0000-7000-8000-000000000001";
                write(
                    &backlog::item_path(root, id),
                    &serde_json::to_string(&serde_json::json!({
                        "id": id,
                        "topic": "placed",
                        "next_section_id": 2,
                        "sections": [{
                            "id": 1,
                            "text": "an unresolved point beside a released v1 record",
                            "updated_at": "2026-08-29T00:00:00Z",
                            "subjects": [],
                            "produced": [],
                        }],
                    }))
                    .expect("item"),
                );
            }),
        ),
        (
            "work",
            Box::new(|root: &Path| {
                std::fs::create_dir_all(work::dir(root).join("objects")).expect("work dir");
                write(
                    &work::path(root, AUTHORITY),
                    &serde_json::to_string(&serde_json::json!({
                        "object": AUTHORITY,
                        "state": "active",
                        "updated_at": "2026-08-29T00:00:00Z",
                        "items": [],
                    }))
                    .expect("work"),
                );
            }),
        ),
        (
            "collections",
            Box::new(|root: &Path| {
                std::fs::create_dir_all(collection::dir(root)).expect("collections dir");
                write(
                    &collection::path(root, "qevf6gz4ce"),
                    &serde_json::to_string(&serde_json::json!({
                        "id": "qevf6gz4ce",
                        "name": "placed",
                        "state": "open",
                        "members": [],
                    }))
                    .expect("collection"),
                );
            }),
        ),
    ];

    for (domain, place) in domains {
        let (_temp, root) = released_v1();
        place(&root);
        let before = fingerprint(&root);

        let error = store::migrate(&root).expect_err(domain);
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{domain}: {}", error.message);
        assert!(
            error
                .message
                .contains("workspace version 1 is the generation"),
            "{domain}: the refusal names the generation mismatch: {error}"
        );
        assert!(
            error.message.contains(domain),
            "{domain}: and names what it found: {error}"
        );

        assert_eq!(
            declared_version(&root),
            1,
            "{domain}: format.json is intact"
        );
        assert!(!stage(&root).exists(), "{domain}: nothing was staged");
        assert!(
            !store::engr_dir(&root).join("migration-v3.tmp").exists(),
            "{domain}: and nothing was half-staged either"
        );
        // Byte-identical, which covers the authoritative Objects, their history,
        // and `.engr/.gitignore` — migration adds its staging entries to that
        // file, so an unchanged one proves the refusal came before any write at
        // all rather than after a first side effect.
        assert_eq!(fingerprint(&root), before, "{domain}: nothing was written");
    }
}

/// The rule that would have governed is not merely uncarried — it never gets
/// the chance to be read as policy.
#[test]
fn a_v1_rule_never_becomes_governing_policy_by_migration() {
    let (_temp, root) = released_v1();
    std::fs::create_dir_all(rules::dir(&root)).expect("rules dir");
    write(
        &rules::dir(&root).join("agent-policy.md"),
        "---\nid: agent-policy\napplies:\n  domains:\n    - object\n    - backlog\n---\n\n# Agent policy\n\nAnything an agent admits cites the commit it came from.\n",
    );

    let error = store::migrate(&root).expect_err("a rule the released build never had");
    assert!(
        error.message.contains("govern agent admission"),
        "the refusal says what activating it would have done: {error}"
    );
    assert_eq!(
        store::validate_format(&root).expect("format"),
        WorkspaceFormat::OlderVersion(1),
        "the workspace is still its own generation"
    );
    assert!(
        rules::load_all(&root).is_err(),
        "and nothing here is current policy"
    );
}

// ------------------------------------------------- the coordinated window

/// Reads are unavailable while a stage exists, whatever generation it came from.
#[test]
fn the_maintenance_window_fails_closed_for_a_v1_source() {
    let (_temp, root) = released_v1();
    std::fs::create_dir_all(stage(&root)).expect("marker");

    let error = store::validate_format(&root).expect_err("no reads during migration");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("engr migrate"), "{error}");
    assert!(
        store::load_object(&root, AUTHORITY).is_err(),
        "and no mixed-generation reading of the Objects underneath"
    );
}

/// Build the stage a crashed v1 migration would have left behind.
///
/// `interrupted` keeps its predecessor `format.json`, which is what makes it a
/// crash rather than a finished migration: the plan is installed, the authority
/// has not moved, and every Object still on disk is the one preflight read.
fn staged_v1_plan(interrupted: &Path, published: &[&str]) -> PathBuf {
    let (_reference_temp, reference) = released_v1();
    store::migrate(&reference).expect("reference migration");

    let staged = stage(interrupted);
    std::fs::create_dir_all(staged.join("objects")).expect("stage objects");
    let mut objects = serde_json::Map::new();
    let mut source = serde_json::Map::new();
    for id in [AUTHORITY, MODEL, PROVENANCE_OBJECT, PROJECTION] {
        let migrated = read(&store::object_path(&reference, id));
        write(
            &staged.join("objects").join(format!("{id}.json")),
            &migrated,
        );
        objects.insert(id.to_owned(), Value::String(proof::sha256_of(&migrated)));
        source.insert(
            format!("objects/{id}.json"),
            Value::String(proof::sha256_of(&read(&store::object_path(
                interrupted,
                id,
            )))),
        );
        source.insert(
            format!("events/{id}.jsonl"),
            Value::String(proof::sha256_of(&read(&store::events_path(
                interrupted,
                id,
            )))),
        );
        // A crash can leave any prefix of the copy loop installed. Re-copying
        // the sealed plan has to be idempotent, so the ones named here are
        // already at their target while the authority still says version 1.
        if published.contains(&id) {
            write(&store::object_path(interrupted, id), &migrated);
        }
    }
    write(
        &staged.join("manifest.json"),
        &proof::canonical_bytes(
            &serde_json::json!({
                "source_version": 1,
                "target_version": engr::WORKSPACE_VERSION,
                "objects": objects,
                "resources": {},
                "source": source,
            }),
            "manifest",
        )
        .expect("manifest"),
    );
    staged
}

/// An interruption partway through publication resumes and finishes.
#[test]
fn an_interrupted_publication_resumes_from_its_own_stage() {
    let (_temp, root) = released_v1();
    let staged = staged_v1_plan(&root, &[AUTHORITY, MODEL]);

    assert!(
        store::validate_format(&root).is_err(),
        "the window is closed while the stage stands"
    );
    store::migrate(&root).expect("resume");

    assert_eq!(
        store::validate_format(&root).expect("format"),
        WorkspaceFormat::Current
    );
    assert!(!staged.exists());
    for id in [AUTHORITY, MODEL, PROVENANCE_OBJECT, PROJECTION] {
        let object = store::load_object(&root, id).expect("published");
        integrity::check_stored_object_integrity(&object).expect("verifies");
    }
}

/// A resume derives the published Object again rather than trusting the stage.
///
/// The staging directory outlives a crash inside the repository, and Section
/// and aggregate seals are unkeyed — so a staged Object can be rewritten,
/// resealed and re-digested into a plan that agrees with itself everywhere.
/// What it cannot agree with is the predecessor it claims to have come from.
#[test]
fn a_staged_v1_object_that_the_predecessor_does_not_derive_is_not_published() {
    let (_temp, root) = released_v1();
    let staged = staged_v1_plan(&root, &[]);

    let path = staged.join("objects").join(format!("{AUTHORITY}.json"));
    let staged_object: engr::model::Object =
        serde_json::from_str(&read(&path)).expect("the stage holds a current v3 Object");
    let seal = staged_object.sha256.clone().expect("aggregate seal");
    let forged = integrity::mutate(&staged_object, &seal, |next| {
        next.title = "a title nobody admitted".to_owned();
        Ok(())
    })
    .expect("reseal")
    .object;
    integrity::check_stored_object_integrity(&forged)
        .expect("the substitution is self-consistent, which is the point");
    let bytes = proof::canonical_bytes(&forged, "forged object").expect("bytes");
    write(&path, &bytes);
    let manifest_path = staged.join("manifest.json");
    let mut manifest: Value = serde_json::from_str(&read(&manifest_path)).expect("manifest");
    manifest["objects"][AUTHORITY] = Value::String(proof::sha256_of(&bytes));
    write(
        &manifest_path,
        &proof::canonical_bytes(&manifest, "manifest").expect("manifest bytes"),
    );

    let before = read(&store::object_path(&root, AUTHORITY));
    let error = store::migrate(&root).expect_err("a stage is not authority");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(
        error
            .message
            .contains("not the canonical migration of the captured predecessor"),
        "{error}"
    );
    assert_eq!(declared_version(&root), 1, "the authority did not advance");
    assert_eq!(
        read(&store::object_path(&root, AUTHORITY)),
        before,
        "and the predecessor the stage claimed to come from never moved"
    );
}

/// A manifest cannot relabel the generation it was prepared from.
///
/// The stage is durable operational state sitting in the repository. Letting it
/// say the workspace is a version it is not would decide which decoder runs
/// from a file anyone who can reach the workspace can edit.
#[test]
fn a_manifest_cannot_relabel_the_generation_it_was_prepared_from() {
    let (_temp, root) = released_v1();
    store::migrate(&root).expect("first migration");

    let staged = stage(&root);
    std::fs::create_dir_all(&staged).expect("stage");
    write(
        &staged.join("manifest.json"),
        &serde_json::to_string(&serde_json::json!({
            "source_version": 2,
            "target_version": engr::WORKSPACE_VERSION,
            "objects": {},
            "resources": {},
            "source": {},
        }))
        .expect("manifest"),
    );
    // Put the workspace back to the generation the plan claims not to be from.
    write(
        &store::engr_dir(&root).join("format.json"),
        "{\"format\":\"engr-workspace\",\"version\":1}\n",
    );

    let error = store::migrate(&root).expect_err("a plan cannot choose its own source");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(
        error.message.contains("prepared from workspace version"),
        "{error}"
    );
}

/// A predecessor that moved between preflight and publication is caught.
///
/// The window is real: the plan was validated against bytes that were on disk
/// then, and the publication happens later — possibly after a crash, a reboot
/// and someone's hand edit. Resuming over a source that has since changed would
/// publish a migration of something the workspace no longer says.
#[test]
fn a_source_edited_after_preflight_stops_the_publication() {
    let (_temp, root) = released_v1();
    staged_v1_plan(&root, &[]);

    edit_stored(&root, AUTHORITY, |object| {
        object["title"] = Value::String("a title that arrived after the plan".to_owned());
    });

    let error = store::migrate(&root).expect_err("the predecessor is not what was validated");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(
        error.message.contains("after migration preflight"),
        "{error}"
    );
    assert_eq!(declared_version(&root), 1, "and nothing was published");
}

/// Two independent migrations of the same predecessor publish the same bytes.
#[test]
fn publication_is_deterministic() {
    let (_first_temp, first) = released_v1();
    let (_second_temp, second) = released_v1();

    store::migrate(&first).expect("first");
    store::migrate(&second).expect("second");

    assert_eq!(fingerprint(&first), fingerprint(&second));
}

// ------------------------------------------------------------ continuation

/// The migrated workspace is an ordinary current workspace.
///
/// The strongest thing this suite can say is not that migration produced a
/// structure that passes an assertion, but that the same `.engr` then does
/// everything a workspace created today does — a Human admission through the
/// gate, an Agent admission under a reviewed Rule, and each of the
/// non-authoritative domains — with the record the release wrote still in it.
#[test]
fn the_migrated_workspace_carries_on_as_a_current_one() {
    let (_temp, root) = released_v1();
    store::migrate(&root).expect("migrate");

    // A Human mutation on an Object the release admitted, through the gate.
    let human = Payload {
        action: Action::SectionAdded,
        object: AUTHORITY.to_owned(),
        becomes: None,
        content: Content {
            text: "Admission is unchanged across the generation boundary: a human still reads the exact proposal and answers its exact challenge.".to_owned(),
            ..Content::default()
        },
    };
    let prepared = gate::prepare(&root, human).expect("prepare");
    let admitted = gate::confirm(&root, &format!("CONFIRM {}", prepared.candidate.challenge))
        .expect("confirm");
    assert_eq!(admitted.event.version, engr::EVENT_ENVELOPE_VERSION);
    assert_eq!(admitted.object.rev, 4, "the release left it at rev 3");
    assert_eq!(admitted.object.sections.len(), 3);
    assert_eq!(admitted.object.sections[0].admission, Admission::Human);

    // An Agent mutation on another, under a passing Rule Review. The Rule is
    // written now rather than at the top, because a governed mutation is
    // governed whoever is making it: adding it first would make the Human
    // admission above a review-bearing one, and that is a different test.
    std::fs::create_dir_all(rules::dir(&root)).expect("rules dir");
    write(
        &rules::dir(&root).join("object-policy.md"),
        "---\nid: object-policy\napplies:\n  domains:\n    - object\n---\n\n# Object policy\n\nAn agent-admitted section states one settled fact.\n",
    );
    let agent = Payload {
        action: Action::SectionAdded,
        object: MODEL.to_owned(),
        becomes: None,
        content: Content {
            text: "A migrated section keeps the admission it was admitted under, and a new one carries its own.".to_owned(),
            ..Content::default()
        },
    };
    let review = attestation(&root, &agent, Admission::Agent);
    let agent_admitted = gate::admit_agent(&root, agent, Some(review)).expect("agent admit");
    let model = store::load_object(&root, MODEL).expect("model");
    assert_eq!(agent_admitted.object.rev, 6);
    assert_eq!(
        model.sections[0].admission,
        Admission::Human,
        "§1 was the release's"
    );
    assert_eq!(
        model.sections[2].admission,
        Admission::Agent,
        "§3 is the agent's"
    );

    // The non-authoritative domains, which a released v1 workspace never had a
    // directory for.
    let item = backlog::create(
        &root,
        "cumulative-migration",
        "whether a fourth generation should migrate from v1 as directly as this one did",
        vec![backlog::Subject::engr(compact_object_ref(PROJECTION))],
        &backlog::Prepared::attempt(rules::Attempt::FIRST),
    )
    .expect("backlog item");
    backlog::add_section(
        &root,
        &item.id,
        "the released generation is the floor, and the floor is what has to keep working",
        Vec::new(),
        &backlog::Prepared::attempt(rules::Attempt::FIRST)
            .against(backlog::Precondition::section_absent(&root, &item.id).expect("observe")),
    )
    .expect("backlog section");

    let plan = collection::create(
        &root,
        "generation boundary",
        None,
        None,
        rules::Attempt::FIRST,
    )
    .expect("collection");
    collection::add_member(
        &root,
        &plan.id,
        &compact_object_ref(AUTHORITY),
        Some(10),
        None,
        rules::Attempt::FIRST,
    )
    .expect("member");

    work::start(
        &root,
        MODEL,
        Some("carrying the record forward"),
        rules::Attempt::FIRST,
    )
    .expect("work");
    work::add_item(
        &root,
        MODEL,
        "rerun the dogfood against the migrated record",
        rules::Attempt::FIRST,
    )
    .expect("work item");

    // The release's own wording is still exactly what it admitted, still says
    // when it was confirmed, and has not been dragged forward by anything that
    // happened above it.
    assert_eq!(
        model.sections[0].admitted_at,
        "2026-08-28T19:54:29.096654598Z"
    );
    assert_eq!(
        model.sections[0].text,
        "An object is the durable knowledge aggregate and holds coherent sections. A section's text is its current confirmed wording: there is no competing current-text field and no stored staleness verdict."
    );
    assert_eq!(
        model.sections[0].refs[0]
            .as_selective()
            .expect("selective")
            .commit(),
        AUTHORITY_COMMIT
    );

    // And everything still verifies, including the four Objects the release
    // admitted and the history behind them.
    for id in store::object_ids(&root).expect("ids") {
        let object = store::load_object(&root, &id).expect("loads");
        integrity::check_stored_object_integrity(&object).expect("verifies");
        let effective = engr::ops::effective(&root, &id).expect("projects");
        assert_eq!(effective.rev, object.rev, "{id}");
    }
    assert_eq!(store::object_ids(&root).expect("ids").len(), 4);
}

/// A passing review of exactly the mutation being admitted.
fn attestation(root: &Path, payload: &Payload, admission: Admission) -> gate::ReviewAttestation {
    let before = store::load_object(root, &payload.object).expect("before");
    let mut after = before.clone();
    let event = Event {
        format: engr::model::EVENT_FORMAT.to_owned(),
        version: engr::EVENT_ENVELOPE_VERSION,
        event_id: engr::model::new_id(),
        rev: before.rev + 1,
        time: "2026-08-29T00:00:00Z".to_owned(),
        payload: payload.clone(),
        provenance: Provenance::Tagged {
            admission: TaggedAdmission {
                kind: admission,
                confirmation: (admission == Admission::Human).then(|| HumanConfirmation {
                    challenge: "ABCD-EFGH".to_owned(),
                    candidate_digest: format!("1:{}", "0".repeat(64)),
                }),
                rule_review: None,
            },
        },
    };
    engr::model::project(&mut after, &event).expect("project");
    let mutation = proof::object_review_mutation(&before, &after, payload).expect("mutation");
    let binding = rules::bind_object(root, &mutation, before.rev).expect("binding");
    gate::ReviewAttestation {
        review_digest: binding.digest().expect("digest").to_string(),
        reviewed_rules: binding.rule_ids(),
        attempt: 1,
        result: proof::ReviewResult::Passed,
        explanation: None,
    }
}
