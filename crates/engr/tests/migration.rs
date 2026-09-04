//! The published release's workspace, brought forward under this binary.
//!
//! Everything here runs against `tests/fixtures/released-v1`, which the
//! published `latest` release wrote through its own Human Gate. That is the
//! point: a hand-authored file proves the migrator accepts a shape somebody
//! believed version 1 had, and only the release's own output proves it accepts
//! what the release actually wrote. See the fixture's `PROVENANCE.md`.
//!
//! The suite is arranged as the claim it has to support. First that the fixture
//! is what it says it is, then that it is confirmed rather than simply run, then
//! that the result preserves every semantic the predecessor proved and invents
//! none, then that each way of being wrong is still refused, and last that what
//! comes out is an ordinary current workspace rather than a structure that
//! passes one assertion.

use engr::store::WorkspaceFormat;
use engr::{dependency, integrity, ops, proof, store};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

/// The Objects the release admitted, by what they are about.
const AUTHORITY: &str = "01a049f0-16fb-7c03-a75d-97980cc8c613";
const MODEL: &str = "01a049f0-1d33-7912-9e57-a67b06866805";
const PROVENANCE_OBJECT: &str = "01a049f0-271b-7971-aee7-674dbbadb7f0";
const PROJECTION: &str = "01a049f0-3711-7ca2-8438-7e1f7b620b7a";

/// The commits its two predecessor references pin, and the tip they were taken
/// at.
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
        "git {args:?}: {}",
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
fn released() -> (TempDir, PathBuf) {
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

fn predecessor_object(root: &Path, id: &str) -> Value {
    serde_json::from_str(&read(&store::object_path(root, id))).expect("stored object")
}

fn predecessor_events(root: &Path, id: &str) -> PathBuf {
    store::engr_dir(root)
        .join("events")
        .join(format!("{id}.jsonl"))
}

/// Reseal one predecessor Section the way the released build would have.
///
/// Its seal is over `{based_on, refs, text}` with `based_on` always present,
/// hashed the way that build hashed — `serde_json` over a `Value`, whose map is
/// key-sorted. Reproducing the number means reproducing the input, so this is
/// written out rather than borrowed from the current contract.
fn reseal_predecessor_section(section: &mut Value) {
    let sealed = serde_json::json!({
        "based_on": section["based_on"].clone(),
        "refs": section["refs"].clone(),
        "text": section["text"].clone(),
    });
    let canonical = serde_json::to_string(&sealed).expect("json");
    section["sha256"] = Value::String(proof::sha256_of(&canonical));
}

/// Reseal one predecessor Event line the way the released build would have.
///
/// Its `payload_sha256` is over the mutation alone — the action and its
/// parameters beside `object`, `text`, `refs` and an always-present `based_on` —
/// so the envelope members are dropped before hashing.
fn reseal_predecessor_event(event: &mut Value) {
    let mut payload = event.clone();
    let members = payload.as_object_mut().expect("event");
    for envelope in [
        "format",
        "version",
        "event_id",
        "rev",
        "time",
        "confirmation",
    ] {
        members.remove(envelope);
    }
    members.entry("based_on").or_insert(Value::Null);
    let canonical = serde_json::to_string(&payload).expect("json");
    event["confirmation"]["payload_sha256"] = Value::String(proof::sha256_of(&canonical));
}

/// Rewrite one predecessor Object, the way a hand edit or another tool would.
fn edit_predecessor(root: &Path, id: &str, change: impl FnOnce(&mut Value)) {
    let mut value = predecessor_object(root, id);
    change(&mut value);
    write(
        &store::object_path(root, id),
        &format!("{}\n", serde_json::to_string_pretty(&value).expect("json")),
    );
}

/// Prepare the migration and answer its code, the way a person would.
fn migrate(root: &Path) -> engr::migration::Report {
    let proposed = engr::migration::prepare(root).expect("prepare the migration");
    match engr::confirm(root, &format!("CONFIRM {}", proposed.challenge)).expect("confirm") {
        engr::Confirmed::Migration(report) => report,
        engr::Confirmed::Object(_) => panic!("a migration subject is not an Object confirmation"),
    }
}

/// Accept the Human response, then stop at one named boundary inside the real
/// apply path.
///
/// `stage` is `destination`, `version` or `challenge`: after the destination is
/// durable and before anything is published, after publication and before
/// `VERSION`, and after `VERSION` and before the spent Challenge and the stage
/// are swept up. Each is a window a crash can land in, and each has to converge.
fn interrupt_at(root: &Path, stage: &str, challenge: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_engr"))
        .arg("--root")
        .arg(root)
        .arg("confirm")
        .arg(format!("CONFIRM {challenge}"))
        .env("ENGR_TEST_STOP_MIGRATION", format!("{stage}:{challenge}"))
        .output()
        .expect("run interrupted confirmation");
    assert!(!output.status.success(), "the test boundary must interrupt");
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains(&format!("test interruption at migration stage {stage}")),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn interrupt_after_confirmed_destination(root: &Path, challenge: &str) {
    interrupt_at(root, "destination", challenge);
}

/// Every relative path under `.engr`, with the digest of its bytes.
///
/// `local/` is left out: it is this machine's alone — the writer lock, a live
/// challenge whose filename is its code, and a resumable plan — and taking the
/// lock is what any command does before it decides whether it may do anything
/// at all. Counting it would make "preflight wrote nothing" fail on a migration
/// that wrote nothing.
///
/// `lock` is left out for the same reason and is the predecessor's own: the
/// released build creates `.engr/lock` on demand and its own `.gitignore` names
/// it, and this build now takes it too so a released writer and a migration
/// cannot both think they hold the workspace. Taking a lock is not publishing.
fn fingerprint(root: &Path) -> std::collections::BTreeMap<String, String> {
    let base = store::engr_dir(root);
    let mut found = std::collections::BTreeMap::new();
    let mut pending = vec![base.clone()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).expect("read dir") {
            let entry = entry.expect("entry");
            let path = entry.path();
            let relative = path
                .strip_prefix(&base)
                .expect("inside .engr")
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            if entry.file_type().expect("file type").is_dir() {
                if relative != "local" {
                    pending.push(path);
                }
                continue;
            }
            if relative == "lock" {
                continue;
            }
            found.insert(relative, proof::sha256_of(&read(&path)));
        }
    }
    found
}

// ---------------------------------------------------------------- the fixture

/// The readable copy and the history are the same workspace.
///
/// `workspace/` exists so the released bytes can be reviewed in a diff instead
/// of only inside a pack file, which is worth nothing if the two can drift. So
/// they are compared rather than trusted, and the comparison is byte for byte:
/// every seal in the record is over exact octets.
#[test]
fn the_readable_fixture_is_the_history_it_was_taken_from() {
    let (_temp, root) = released();
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
    ] {
        assert_eq!(
            std::fs::read(root.join(relative)).expect("cloned"),
            std::fs::read(readable.join(relative)).expect("readable"),
            "{relative} differs between the bundle and the readable copy"
        );
        checked += 1;
    }
    assert_eq!(checked, 10);
}

/// The released bootstrap is recognized as the predecessor, and is read-only
/// until an explicit migration.
#[test]
fn a_released_workspace_reads_as_the_predecessor_and_writes_nothing() {
    let (_temp, root) = released();
    assert_eq!(
        store::validate_format(&root).expect("recognized"),
        WorkspaceFormat::Predecessor
    );
    let error = store::require_current(&root).expect_err("read-only until migrated");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("engr migrate"), "{}", error.message);

    // And no current-generation read pretends to answer for it.
    let error = store::load_object(&root, AUTHORITY).expect_err("not a current Object");
    assert!(error.message.contains("migrate"), "{}", error.message);
}

/// A generation this build has no route from is refused without being offered
/// one, which is a different fact from "run `engr migrate`".
#[test]
fn a_generation_with_no_route_is_refused_without_offering_migration() {
    let (_temp, root) = released();
    for version in ["2", "3", "99"] {
        write(
            &store::engr_dir(&root).join("format.json"),
            &format!("{{\"format\":\"engr-workspace\",\"version\":{version}}}"),
        );
        let error = store::validate_format(&root).expect_err(version);
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{version}");
        assert!(
            error.message.contains("released version 1"),
            "{version}: {}",
            error.message
        );
    }
}

// --------------------------------------------------------- the confirmation

/// Migration is confirmed through the ordinary Challenge primitive, and its
/// subject names the exact predecessor it was derived from.
#[test]
fn migration_is_a_human_confirmation_of_one_exact_plan() {
    let (_temp, root) = released();
    let before = fingerprint(&root);

    let proposed = engr::migration::prepare(&root).expect("prepare");
    assert!(!proposed.resumed);
    assert_eq!(proposed.subject.from.version, 1);
    assert_eq!(proposed.subject.to, engr::WORKSPACE_GENERATION);
    assert_eq!(proposed.subject.objects.len(), 4);
    assert_eq!(
        proposed.subject.source.len(),
        9,
        "the bootstrap, four projections and four histories"
    );

    // Preparing publishes nothing, and nothing means every tracked byte: a
    // person who asks what a migration would do and then declines is left
    // holding no change they did not confirm. The live code still has to be
    // kept out of git, and that is done through git's own local exclude —
    // same instant, same effect, nothing anybody commits.
    let after = fingerprint(&root);
    let moved: Vec<&String> = after
        .iter()
        .filter(|(path, digest)| before.get(*path) != Some(*digest))
        .map(|(path, _)| path)
        .collect();
    assert!(moved.is_empty(), "preflight published {moved:?}");
    assert!(
        !read(&store::engr_dir(&root).join(".gitignore")).contains("/local/"),
        "and the tracked ignore file is not the place it was kept out"
    );
    let exclude = read(&root.join(".git").join("info").join("exclude"));
    assert!(
        exclude.contains(".engr/local/"),
        "the live code is excluded locally instead: {exclude}"
    );

    // The subject is a Challenge like any other, and it seals over itself.
    let challenge: engr::confirmation::Challenge = serde_json::from_str(&read(
        &store::challenge_path(&root, &proposed.challenge).expect("path"),
    ))
    .expect("challenge");
    assert_eq!(
        challenge.subject.kind,
        engr::confirmation::SubjectType::Migration
    );
    challenge.validate().expect("a well formed challenge");

    // Preparing again re-renders the plan a person is already holding rather
    // than minting a second code for the same migration.
    let again = engr::migration::prepare(&root).expect("resume");
    assert!(again.resumed);
    assert_eq!(again.challenge, proposed.challenge);

    let report = migrate(&root);
    assert_eq!(report.objects.len(), 4);
    assert_eq!(report.sections, 7);
    assert_eq!(
        store::validate_format(&root).expect("current"),
        WorkspaceFormat::Current
    );
    assert_eq!(
        read(&store::version_path(&root)),
        engr::WORKSPACE_VERSION_FILE
    );

    // A spent code resolves to nothing, and the plan is gone with it.
    assert!(
        !store::challenge_path(&root, &proposed.challenge)
            .expect("path")
            .exists(),
        "a successful confirmation removes the Challenge"
    );
    assert!(!engr::migration::stage_dir(&root).exists());
}

/// A predecessor edited between the question and the answer changes what the
/// answer would mean, so the confirmation is refused rather than applied.
#[test]
fn a_predecessor_that_moved_after_the_plan_stops_the_confirmation() {
    let (_temp, root) = released();
    let proposed = engr::migration::prepare(&root).expect("prepare");

    // Reformatted, not reworded: the same value, different bytes. That is the
    // narrow case the source digests exist for — a file whose meaning is
    // unchanged is still not the file the plan was derived from, and the plan
    // has to say so rather than quietly deriving from whatever is there now.
    let path = predecessor_events(&root, AUTHORITY);
    let reformatted: Vec<String> = read(&path)
        .lines()
        .map(|line| {
            let value: Value = serde_json::from_str(line).expect("event");
            // Through a `Value`, whose map is a `BTreeMap`: the members come
            // back in a different order, so the bytes move and the meaning
            // does not.
            serde_json::to_string(&value).expect("json")
        })
        .collect();
    write(&path, &(reformatted.join("\n") + "\n"));

    let error = engr::confirm(&root, &format!("CONFIRM {}", proposed.challenge))
        .expect_err("the predecessor moved");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("moved after"), "{}", error.message);
    assert!(
        !store::version_path(&root).exists(),
        "nothing was published"
    );
}

// ------------------------------------------------------- what it preserves

/// Every semantic the release admitted survives, and nothing it never said is
/// invented.
#[test]
fn migration_preserves_what_the_release_admitted_and_invents_nothing() {
    let (_temp, root) = released();
    let predecessors: Vec<Value> = [AUTHORITY, MODEL, PROVENANCE_OBJECT, PROJECTION]
        .iter()
        .map(|id| predecessor_object(&root, id))
        .collect();
    migrate(&root);

    for (index, id) in [AUTHORITY, MODEL, PROVENANCE_OBJECT, PROJECTION]
        .iter()
        .enumerate()
    {
        let before = &predecessors[index];
        let after = store::load_object(&root, id).expect("migrated object");

        assert_eq!(after.id, *id, "identity is preserved");
        assert_eq!(after.title, before["title"].as_str().expect("title"));
        assert_eq!(
            after.state.as_str(),
            before["state"].as_str().expect("state"),
            "{id}: the lifecycle it stood in"
        );
        assert_eq!(
            after.object_type, None,
            "{id}: the released generation had no type, so migration classifies nothing"
        );
        assert_eq!(
            after.rev, 1,
            "{id}: a migrated Object is at revision 1, whatever the predecessor counted to"
        );
        assert_eq!(
            after.next_section_id,
            before["next_section_id"].as_u64().expect("counter"),
            "{id}: the id counter is carried, so a consumed id is never handed out again"
        );

        let sections = before["sections"].as_array().expect("sections");
        assert_eq!(after.sections.len(), sections.len());
        for (section, stored) in after.sections.iter().zip(sections) {
            assert_eq!(section.id, stored["id"].as_u64().expect("id"));
            assert_eq!(section.text, stored["text"].as_str().expect("text"));
            assert_eq!(
                section.admitted.by,
                engr::semantics::Admission::Human,
                "every predecessor Section came through the Human Gate"
            );
            assert_eq!(
                section.admitted.at,
                stored["confirmed_at"].as_str().expect("confirmed_at"),
                "the instant the predecessor recorded, not the migration's"
            );
            assert_eq!(
                section.based_on.as_ref().map(|basis| basis.commit.as_str()),
                stored["based_on"].as_str(),
                "an absent basis stays absent; it does not become a commit"
            );
            assert_eq!(section.header, None, "nothing invents a header");
            assert_eq!(section.role, None, "nor a role");
            assert!(section.content.is_empty(), "nor literal content");
            assert!(section.relations.is_empty(), "nor a relation");
        }
        integrity::check_stored_object_integrity(&after).expect("and it seals");
    }
}

/// The predecessor's own history is discarded rather than translated, and each
/// Object gets exactly one bootstrap at revision 1.
#[test]
fn history_is_replaced_by_one_bootstrap_and_never_translated() {
    let (_temp, root) = released();
    let before = read(&predecessor_events(&root, PROVENANCE_OBJECT));
    assert_eq!(before.lines().count(), 7, "the release admitted seven");

    migrate(&root);

    assert!(
        !store::engr_dir(&root).join("events").exists(),
        "the predecessor's own log is gone, not rewritten"
    );
    let events = store::load_events(&root, PROVENANCE_OBJECT).expect("history");
    assert_eq!(events.len(), 1, "one bootstrap, and nothing else");
    let bootstrap = &events[0];
    assert_eq!(bootstrap.rev, 1);
    assert_eq!(bootstrap.action.event_type(), "object.migrated.v1");
    assert_eq!(
        bootstrap.metadata.admitted.by,
        engr::semantics::Admission::Human,
        "a migration is confirmed by a person"
    );
    assert!(
        bootstrap.metadata.admitted.confirmation.is_some(),
        "and records the code they answered"
    );

    // The Event's admission is the migration's; the Sections inside it keep
    // theirs. This is the case #66 keeps the two facts apart for.
    let engr::model::Action::ObjectMigrated { snapshot } = &bootstrap.action else {
        panic!("a migrated stream begins with its bootstrap");
    };
    for section in &snapshot.sections {
        assert_ne!(
            section.value.admitted.at, bootstrap.metadata.admitted.at,
            "a migrated Section keeps the instant it was really admitted at"
        );
    }

    // And the projection is exactly what that history derives.
    let derived = ops::provable(&root, PROVENANCE_OBJECT).expect("replay");
    let stored = store::load_object(&root, PROVENANCE_OBJECT).expect("stored");
    assert_eq!(derived, stored, "the record is what its history says");
}

/// A predecessor reference becomes a selective one over exactly the facts the
/// original attested — no more — and still resolves.
#[test]
fn predecessor_references_convert_to_what_they_attested_and_still_resolve() {
    let (_temp, root) = released();
    migrate(&root);

    let object = store::load_object(&root, PROVENANCE_OBJECT).expect("object");
    let section = object.section(4).expect("§4");
    assert_eq!(section.refs.len(), 1);
    let reference = &section.refs[0];

    assert_eq!(
        reference.target(),
        proof::section_target(MODEL, 1).expect("section target"),
        "the same target the predecessor named"
    );
    assert_eq!(
        reference.commit(),
        MODEL_COMMIT,
        "pinned at the same commit"
    );
    let selected: Vec<&str> = reference
        .fields()
        .iter()
        .map(|field| field.as_str())
        .collect();
    assert_eq!(
        selected,
        vec!["based_on", "refs", "text"],
        "exactly what the released whole-content seal covered, and nothing else"
    );
    for later in ["admission", "content", "header", "relations", "role"] {
        assert!(
            !selected.contains(&later),
            "{later} did not exist in the released contract, so the original Ref \
             cannot have attested it and the migrated one must not claim to"
        );
    }

    // And it verifies against the predecessor commit it pins, which means the
    // conversion the read path performs is the one the migration performed.
    let target = ops::effective(&root, MODEL).expect("target");
    assert_eq!(
        dependency::evaluate(&root, &target, reference).expect("evaluate"),
        dependency::Dependency::Unchanged,
        "a migrated reference is not born stale"
    );
}

/// A migrated Ref depends on what the predecessor Ref actually attested, and on
/// nothing the predecessor could not have said.
///
/// The released Section was `{based_on, refs, text}`; `role`, `content`,
/// `relations`, `header` and `admission` did not exist in that contract. So
/// giving the target one of them afterwards is somebody adding a fact, not the
/// dependency moving — and a Ref that reported drift for it would be reporting
/// a dependency the original reference never declared. This is the observable
/// consequence of the selected field set, which is why it is checked here
/// rather than only in the list of field names.
#[test]
fn a_migrated_reference_does_not_drift_on_fields_the_predecessor_never_had() {
    let (_temp, root) = released();
    migrate(&root);

    let source = store::load_object(&root, PROVENANCE_OBJECT).expect("source");
    let reference = source.section(4).expect("§4").refs[0].clone();

    // Restate the target's whole semantic value, changing only members the
    // released contract had no way to express.
    let target = store::load_object(&root, MODEL).expect("target");
    let mut content = target.section(1).expect("§1").value().content;
    assert!(
        content.role.is_none() && content.content.is_empty() && content.relations.is_empty(),
        "the migrated target starts without any of the later fields"
    );
    content.role = Some(engr::semantics::Role::Decision);
    content.content = vec![engr::semantics::Supplement {
        content_type: "code.rs".to_owned(),
        body: "// a supplement the predecessor contract could not hold\n".to_owned(),
    }];

    let payload = engr::model::Payload::new(
        MODEL,
        engr::model::Action::SectionUpdated {
            section: 1,
            value: engr::gate::value(content, engr::semantics::Admission::Human),
            becomes: None,
        },
    );
    let prepared = engr::gate::prepare(&root, payload).expect("prepare");
    match engr::confirm(&root, &format!("CONFIRM {}", prepared.candidate.code())).expect("confirm")
    {
        engr::Confirmed::Object(_) => {}
        engr::Confirmed::Migration(_) => panic!("an Object confirmation"),
    }

    let moved = ops::effective(&root, MODEL).expect("target");
    assert_eq!(
        moved.section(1).expect("§1").role,
        Some(engr::semantics::Role::Decision),
        "the target really did gain the field"
    );
    assert_eq!(
        dependency::evaluate(&root, &moved, &reference).expect("evaluate"),
        dependency::Dependency::Unchanged,
        "a field the predecessor Ref could not have attested is not drift"
    );
}

// ------------------------------------------------------------ what it refuses

/// A predecessor whose stored Section no longer matches its own seal is not
/// migrated, and nothing is published.
#[test]
fn a_predecessor_changed_outside_its_own_gate_stops_the_migration() {
    let (_temp, root) = released();
    let before = fingerprint(&root);
    edit_predecessor(&root, AUTHORITY, |value| {
        value["sections"][0]["text"] = Value::String("edited outside the gate".to_owned());
    });

    let error = engr::migration::prepare(&root).expect_err("a broken seal");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("seal"), "{}", error.message);
    let after = fingerprint(&root);
    assert_eq!(
        after.len(),
        before.len(),
        "a refused preflight publishes nothing"
    );
    assert!(!store::version_path(&root).exists());
}

/// A predecessor reference whose historical target was rewritten cannot be
/// converted, because converting it would mint a fresh valid proof over an
/// out-of-band change.
#[test]
fn a_reference_to_a_tampered_historical_target_is_refused() {
    let (_temp, root) = released();

    // Rewrite the target as it stood at the pinned commit, keeping its seal.
    git(&root, &["checkout", "-q", MODEL_COMMIT]);
    edit_predecessor(&root, MODEL, |value| {
        value["sections"][0]["text"] = Value::String("history, rewritten".to_owned());
    });
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
            "rewritten",
        ],
    );
    let rewritten = git(&root, &["rev-parse", "HEAD"]);
    git(&root, &["checkout", "-q", HEAD]);
    edit_predecessor(&root, PROVENANCE_OBJECT, |value| {
        value["sections"][0]["refs"][0]["commit"] = Value::String(rewritten.clone());
    });

    let error = engr::migration::prepare(&root).expect_err("a rewritten historical target");
    assert!(
        error.code == engr::EXIT_INVARIANT || error.code == engr::EXIT_SCHEMA,
        "{error}"
    );
    assert!(!store::version_path(&root).exists());
}

/// Repointing a reference by hand breaks the Section that carries it.
///
/// The predecessor seal covers `refs`, so this never reaches the question of
/// whether the new pin resolves — which is the right order: a Section changed
/// outside its own gate is not material to migrate, whatever it now points at.
#[test]
fn a_reference_repointed_by_hand_breaks_its_own_section_seal() {
    let (_temp, root) = released();
    edit_predecessor(&root, PROVENANCE_OBJECT, |value| {
        value["sections"][0]["refs"][0]["commit"] = Value::String("f".repeat(40));
    });

    let error = engr::migration::prepare(&root).expect_err("a repointed reference");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("own seal"), "{}", error.message);
    assert!(!store::version_path(&root).exists());
}

/// A reference whose pinned commit holds no such Object is unresolvable, and
/// that is absence rather than drift.
#[test]
fn an_unresolvable_reference_stops_the_migration() {
    let (_temp, root) = released();
    // The first commit of the fixture predates the target Object, so the pin
    // resolves to a commit that genuinely holds nothing to read.
    let earliest = git(&root, &["rev-list", "--max-parents=0", "HEAD"]);
    edit_predecessor(&root, PROVENANCE_OBJECT, |value| {
        value["sections"][0]["refs"][0]["commit"] = Value::String(earliest.clone());
        reseal_predecessor_section(&mut value["sections"][0]);
    });
    // And the history that derives it, or the projection would disagree with its
    // own admitted past before anything reached the pin.
    let path = predecessor_events(&root, PROVENANCE_OBJECT);
    let rewritten: Vec<String> = read(&path)
        .lines()
        .map(|line| {
            let mut event: Value = serde_json::from_str(line).expect("event");
            if event["refs"]
                .as_array()
                .is_some_and(|refs| !refs.is_empty())
            {
                event["refs"][0]["commit"] = Value::String(earliest.clone());
                reseal_predecessor_event(&mut event);
            }
            serde_json::to_string(&event).expect("json")
        })
        .collect();
    write(
        &path,
        &(rewritten.join(
            "
",
        ) + "
"),
    );

    let error = engr::migration::prepare(&root).expect_err("a pin that holds nothing");
    assert!(
        error.code == engr::EXIT_NOT_FOUND || error.code == engr::EXIT_SCHEMA,
        "an unresolvable pin is absent or unreadable, never drift: {error}"
    );
    assert!(!store::version_path(&root).exists());
}

/// A declared predecessor holding a domain the release never had is not the
/// workspace this migration is defined over.
#[test]
fn a_later_unreleased_domain_is_refused_by_name() {
    for domain in ["rules", "backlog", "work", "collections", "eventstore"] {
        let (_temp, root) = released();
        std::fs::create_dir_all(store::engr_dir(&root).join(domain)).expect("domain");
        let error = engr::migration::prepare(&root).expect_err(domain);
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{domain}");
        assert!(
            error.message.contains(domain) && error.message.contains("never had"),
            "{domain}: {}",
            error.message
        );
    }
}

/// A predecessor Object whose stored projection is not what its own history
/// derives is a contradiction, not a state to carry forward.
#[test]
fn a_projection_that_its_history_does_not_derive_is_refused() {
    let (_temp, root) = released();
    edit_predecessor(&root, AUTHORITY, |value| {
        value["title"] = Value::String("a title nobody confirmed".to_owned());
    });
    let error = engr::migration::prepare(&root).expect_err("the projection disagrees");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(
        error.message.contains("derivable from admitted history"),
        "{}",
        error.message
    );
}

/// An Object whose projection never landed is still an Object its own history
/// establishes, and it migrates with the rest.
#[test]
fn a_projection_that_never_landed_is_rebuilt_from_history() {
    let (_temp, root) = released();
    std::fs::remove_file(store::object_path(&root, AUTHORITY)).expect("remove the projection");

    migrate(&root);
    let recovered = store::load_object(&root, AUTHORITY).expect("rebuilt from history");
    assert_eq!(recovered.rev, 1);
    assert_eq!(
        recovered.title,
        "Human authority and the admission boundary"
    );
    assert_eq!(recovered.sections.len(), 2);
}

/// A predecessor Event carrying a member its generation never had is refused
/// before anything decodes it, so it cannot reconstruct a state nobody admitted.
#[test]
fn a_predecessor_event_outside_its_generation_is_refused() {
    let (_temp, root) = released();
    let path = predecessor_events(&root, AUTHORITY);
    let mut lines: Vec<String> = read(&path).lines().map(str::to_owned).collect();
    let mut first: Value = serde_json::from_str(&lines[0]).expect("event");
    first["type"] = Value::String("design".to_owned());
    lines[0] = serde_json::to_string(&first).expect("json");
    write(&path, &format!("{}\n", lines.join("\n")));

    let error = engr::migration::prepare(&root).expect_err("a member that generation never had");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(
        error.message.contains("released Event"),
        "{}",
        error.message
    );
}

/// An interrupted publication is resumable, and until `VERSION` lands the
/// workspace is still the predecessor.
#[test]
fn an_incomplete_migration_is_named_and_resumed_rather_than_read() {
    let (_temp, root) = released();
    let proposed = engr::migration::prepare(&root).expect("prepare");

    // A stage exists and nothing has been published, which is exactly the
    // window a crash leaves behind.
    let error = store::validate_format(&root).expect_err("an incomplete migration");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(
        error.message.contains("incomplete coordinated migration"),
        "{}",
        error.message
    );

    // Resuming re-renders the same question, and answering it finishes.
    let again = engr::migration::prepare(&root).expect("resume");
    assert_eq!(again.challenge, proposed.challenge);
    migrate(&root);
    assert_eq!(
        store::validate_format(&root).expect("current"),
        WorkspaceFormat::Current
    );
}

// ------------------------------------------------------ and then it carries on

/// What comes out is an ordinary current workspace: it takes new work through
/// the gate, and the migrated Object's next mutation is revision 2.
#[test]
fn the_migrated_workspace_carries_on_as_a_current_one() {
    let (_temp, root) = released();
    migrate(&root);

    let before = store::load_object(&root, AUTHORITY).expect("migrated");
    assert_eq!(before.rev, 1);

    let payload = engr::model::Payload::new(
        AUTHORITY,
        engr::model::Action::SectionCreated {
            value: engr::model::SectionValue::new(
                engr::semantics::Admitted::new(
                    engr::semantics::Admission::Human,
                    time::OffsetDateTime::now_utc()
                        .format(&time::format_description::well_known::Rfc3339)
                        .expect("timestamp"),
                ),
                engr::model::Content {
                    text: "wording admitted after the migration".to_owned(),
                    ..engr::model::Content::default()
                },
            ),
            becomes: None,
        },
    );
    let prepared = engr::gate::prepare(&root, payload).expect("prepare");
    let after = match engr::confirm(&root, &format!("CONFIRM {}", prepared.candidate.code()))
        .expect("confirm")
    {
        engr::Confirmed::Object(admitted) => admitted.object,
        engr::Confirmed::Migration(_) => panic!("an Object confirmation"),
    };

    assert_eq!(
        after.rev, 2,
        "the next ordinary mutation of a migrated Object emits revision 2"
    );
    assert_eq!(after.sections.len(), 3);
    let events = store::load_events(&root, AUTHORITY).expect("history");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].action.event_type(), "object.migrated.v1");
    assert_eq!(events[1].action.event_type(), "section.created.v1");
    integrity::check_stored_object_integrity(&after).expect("and it still seals");

    // Every other domain is there and empty, so the workspace is complete
    // rather than merely converted.
    for path in [
        engr::backlog::dir(&root),
        engr::collection::dir(&root),
        engr::rules::dir(&root),
        store::challenges_dir(&root),
    ] {
        assert!(path.is_dir(), "{} should exist", path.display());
    }
    assert!(read(&store::engr_dir(&root).join(".gitignore")).contains("/local/"));

    // And the predecessor's writer lock is gone. Migration has to take it, so
    // that a released build cannot be writing to the workspace underneath it,
    // and taking it creates it — but #66's destination root has one lock and it
    // is `local/lock`. Left behind it would be residue the predecessor's own
    // `/lock` ignore rule then hides.
    assert!(
        !store::predecessor_lock_path(&root).exists(),
        "the compatibility lock is migration-only and does not outlive it"
    );
    assert!(
        store::lock_path(&root).starts_with(store::local_dir(&root)),
        "the one lock a generation-1 workspace has is under local/"
    );
}

// ------------------------------------- what the released contract really said

/// A pruned history prefix is a shape the release wrote, so it migrates.
///
/// The released `load_events` checked only that consecutive records held
/// consecutive revisions — never that a file began at rev 1 — and its own
/// reducer accepted "retained evidence at or below the projection" with a
/// missing prefix. Somebody who trimmed an old log therefore has a workspace the
/// shipped tool called sound, and refusing it here would be this build
/// strengthening a published contract after the fact.
///
/// What comes out is the projection the predecessor already held, because with
/// the beginning gone that projection is the only thing that can say what the
/// Object is.
#[test]
fn a_pruned_history_prefix_still_migrates() {
    let (_temp, root) = released();
    let stored = predecessor_object(&root, AUTHORITY);
    let title = stored["title"].as_str().expect("title").to_owned();
    let sections = stored["sections"].as_array().expect("sections").len();

    let path = predecessor_events(&root, AUTHORITY);
    let history = read(&path);
    let kept: Vec<&str> = history.lines().skip(1).collect();
    assert!(!kept.is_empty(), "the fixture needs a prefix to lose");
    write(&path, &format!("{}\n", kept.join("\n")));

    migrate(&root);
    let object = store::load_object(&root, AUTHORITY).expect("migrated");
    assert_eq!(object.title, title, "the projection is what it was");
    assert_eq!(object.sections.len(), sections);
    assert_eq!(object.rev, 1, "and it starts this generation at rev 1");
    assert!(ops::verify(&root, AUTHORITY).expect("verify").passed());
}

/// The same, for a history file that is gone entirely.
///
/// `load_events` returned an empty list rather than an error for a missing file,
/// so an Object with a projection and no log at all is a released workspace too.
#[test]
fn a_missing_history_file_still_migrates_a_projection() {
    let (_temp, root) = released();
    let title = predecessor_object(&root, AUTHORITY)["title"]
        .as_str()
        .expect("title")
        .to_owned();
    std::fs::remove_file(predecessor_events(&root, AUTHORITY)).expect("remove history");

    migrate(&root);
    let object = store::load_object(&root, AUTHORITY).expect("migrated");
    assert_eq!(object.title, title);
    assert_eq!(object.rev, 1);
    assert!(ops::verify(&root, AUTHORITY).expect("verify").passed());
}

/// A projection with no history is one thing; a history that says nothing, with
/// no projection to fall back on, is another — and it establishes no Object.
///
/// Removing both files instead would prove nothing: enumeration walks the two
/// directories, so an Object with neither is one the workspace does not have.
/// What has to fail closed is the shape that still looks like an Object and
/// cannot answer for itself.
#[test]
fn an_object_whose_history_establishes_nothing_is_refused() {
    let (_temp, root) = released();
    std::fs::remove_file(store::object_path(&root, AUTHORITY)).expect("remove projection");
    write(&predecessor_events(&root, AUTHORITY), "");

    let error = engr::migration::prepare(&root).expect_err("nothing establishes it");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(!store::version_path(&root).exists());
}

/// A pruned prefix does not buy a free pass on the tail.
///
/// What the release checked about a retained file was adjacency, and what it
/// checked about recovery was that the tail began at exactly the next revision.
/// Both still hold, so a log with a hole in it is refused rather than replayed
/// over the gap.
#[test]
fn a_pruned_prefix_still_requires_an_adjacent_tail() {
    let (_temp, root) = released();
    let path = predecessor_events(&root, PROVENANCE_OBJECT);
    let history = read(&path);
    let lines: Vec<&str> = history.lines().collect();
    assert!(lines.len() > 3, "the fixture needs a history to hole");
    // Drop the prefix, then drop one more from the middle of what is left.
    let mut kept: Vec<&str> = lines.iter().skip(1).copied().collect();
    kept.remove(1);
    write(&path, &format!("{}\n", kept.join("\n")));

    let error = engr::migration::prepare(&root).expect_err("a gap is a gap");
    assert_ne!(error.code, 0);
    assert!(!store::version_path(&root).exists());
}

/// A projection the retained history contradicts is refused, and being able to
/// say that is exactly what a complete history buys.
///
/// The release had no Object-level seal, so a hand-edited title was invisible to
/// it. Migration is where the first aggregate seal is minted, and minting one
/// over an edit nothing can establish would launder it into permanent authority.
/// So where the history *can* answer the question, it is asked — which is why
/// accepting a pruned prefix above is a narrowing of the claim rather than an
/// abandonment of it.
#[test]
fn a_complete_history_still_refuses_a_projection_it_does_not_derive() {
    let (_temp, root) = released();
    edit_predecessor(&root, AUTHORITY, |value| {
        value["title"] = Value::String("a title nobody confirmed".to_owned());
    });

    let error = engr::migration::prepare(&root).expect_err("the projection disagrees");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(!store::version_path(&root).exists());
}

/// What a crash at one publication step leaves behind, named so the list below
/// reads as a sequence of moments rather than a sequence of function pointers.
type Damage = fn(&Path);

/// One way of putting a bootstrap in place that this transaction did not earn.
type Forge = fn(&Path, &str);

/// The first phase of publication, done the way publication does it.
///
/// Every staged Event stream, copied verbatim into the workspace. A crash later
/// in the sequence always has these behind it, so a simulated crash that skips
/// them is describing a state the code never produces.
fn publish_staged_streams(root: &Path) {
    let staged = store::local_dir(root)
        .join("migration")
        .join("destination")
        .join("eventstore");
    for entry in std::fs::read_dir(&staged).expect("staged streams") {
        let entry = entry.expect("entry");
        let id = entry
            .file_name()
            .to_string_lossy()
            .trim_end_matches(".jsonl")
            .to_owned();
        let path = store::events_path(root, &id);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        write(&path, &read(&entry.path()));
    }
}

/// Just the Object projections out of a workspace fingerprint.
///
/// The bootstrap Events are deliberately excluded: each run mints a fresh Event
/// id and stamps the instant it was admitted, so two independent migrations of
/// one fixture differ there by design. What must be identical between runs is
/// what the migration *derives*, and what must be identical across a resume is
/// the record that was staged — two different claims, checked separately.
fn objects_only(
    fingerprint: &std::collections::BTreeMap<String, String>,
) -> std::collections::BTreeMap<String, String> {
    fingerprint
        .iter()
        .filter(|(path, _)| path.starts_with("objects/") || path.as_str() == "VERSION")
        .map(|(path, digest)| (path.clone(), digest.clone()))
        .collect()
}

// --------------------------------------------- crash, at every publication step

/// Publication is resumable wherever it stops, because the whole destination is
/// written where the predecessor cannot be harmed by it before anything
/// canonical moves.
///
/// The failure this closes is specific. Publication overwrites
/// `.engr/objects/<id>.json` — the predecessor's own path — so once the first of
/// those lands, re-deriving from the predecessor is no longer possible: the
/// bytes it would read are the new generation's. Without a staged destination a
/// crash anywhere in that window leaves a workspace with no `VERSION` and no
/// predecessor to rebuild from, which is neither generation and has no way back.
///
/// Each case below stops the transaction after staging, damages the workspace
/// the way a crash at that step would, and requires the retry not merely to
/// succeed but to arrive at the very same bytes a clean run produces.
#[test]
fn a_crash_at_any_publication_step_is_resumable() {
    // The Objects a migration derives are deterministic, so a clean run says
    // what any run must arrive at. The bootstrap Events are not — each carries a
    // fresh id and the instant it was admitted — so those are held to a stricter
    // and more useful standard below: the resumed workspace must publish the
    // record that was *staged*, rather than mint a second one.
    let clean = {
        let (_temp, root) = released();
        migrate(&root);
        objects_only(&fingerprint(&root))
    };

    let objects = [AUTHORITY, MODEL, PROVENANCE_OBJECT, PROJECTION];
    // Each step is a **prefix of the real publication order**, because that is
    // what a crash can actually leave: every Event stream first, then every
    // Object, then the predecessor's own directories, then its bootstrap. Steps
    // that skipped ahead — an overwritten Object with no stream beside it —
    // described a workspace no crash can produce, and a resume is entitled to
    // read that as a source that moved rather than as a publication in progress.
    let steps: Vec<(&str, Damage)> = vec![
        ("nothing published", |_root| {}),
        ("the first stream landed", |root| {
            let path = store::events_path(root, AUTHORITY);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            write(&path, "a partial write\n");
        }),
        ("the first Object was overwritten", |root| {
            publish_staged_streams(root);
            write(
                &store::object_path(root, AUTHORITY),
                "no longer a predecessor\n",
            );
        }),
        ("every Object was overwritten", |root| {
            publish_staged_streams(root);
            for id in [AUTHORITY, MODEL, PROVENANCE_OBJECT, PROJECTION] {
                write(&store::object_path(root, id), "no longer a predecessor\n");
            }
        }),
        ("the predecessor's own directories are gone", |root| {
            publish_staged_streams(root);
            for id in [AUTHORITY, MODEL, PROVENANCE_OBJECT, PROJECTION] {
                write(&store::object_path(root, id), "no longer a predecessor\n");
            }
            std::fs::remove_dir_all(store::engr_dir(root).join("events")).expect("events");
            std::fs::remove_file(store::engr_dir(root).join("format.json")).expect("format");
        }),
    ];

    for (what, damage) in steps {
        let (_temp, root) = released();
        let proposed = engr::migration::prepare(&root).expect("prepare");
        interrupt_after_confirmed_destination(&root, &proposed.challenge);
        let staged: Vec<String> = objects
            .iter()
            .map(|id| {
                read(
                    &store::local_dir(&root)
                        .join("migration")
                        .join("destination")
                        .join("eventstore")
                        .join(format!("{id}.jsonl")),
                )
            })
            .collect();
        damage(&root);

        let report = match engr::confirm(&root, &format!("CONFIRM {}", proposed.challenge)) {
            Ok(engr::Confirmed::Migration(report)) => report,
            Ok(_) => panic!("{what}: expected a migration confirmation"),
            Err(error) => panic!("{what}: the retry must complete: {error}"),
        };
        assert_eq!(report.objects.len(), objects.len(), "{what}");
        assert_eq!(
            objects_only(&fingerprint(&root)),
            clean,
            "{what}: the resumed Objects are not the ones a clean run derives"
        );
        for (id, staged) in objects.iter().zip(&staged) {
            assert_eq!(
                &read(&store::events_path(&root, id)),
                staged,
                "{what}: {id} published a record other than the one it staged"
            );
        }
        assert!(store::version_path(&root).exists(), "{what}");
        for id in objects {
            assert!(
                ops::verify(&root, id).expect("verify").passed(),
                "{what}: {id}"
            );
        }
    }
}

/// A staged destination is not a blank cheque: it is published only where it
/// still matches the digests the confirmed plan names.
#[test]
fn a_rewritten_staged_destination_is_refused_rather_than_published() {
    let (_temp, root) = released();
    let proposed = engr::migration::prepare(&root).expect("prepare");
    interrupt_after_confirmed_destination(&root, &proposed.challenge);

    let staged = store::local_dir(&root)
        .join("migration")
        .join("destination")
        .join("objects")
        .join(format!("{AUTHORITY}.json"));
    let mut object: Value = serde_json::from_str(&read(&staged)).expect("staged object");
    object["title"] = Value::String("a title nobody confirmed".to_owned());
    write(&staged, &serde_json::to_string(&object).expect("json"));

    let error = engr::confirm(&root, &format!("CONFIRM {}", proposed.challenge))
        .expect_err("a rewritten stage publishes nothing");
    assert_ne!(error.code, 0);
    assert!(!store::version_path(&root).exists());
}

/// Publication writes the staged bytes verbatim, so the resume path holds them
/// to exactly the contract the ordinary read path holds them to.
///
/// A rewrite that keeps the value and changes the bytes — member order,
/// whitespace, an explicit null where the writer omits the member — satisfies
/// every digest, because a digest is taken over the value. It does not satisfy
/// the current generation's canonical JCS representation. Publishing it would
/// write `VERSION` over a workspace unable to read its own migrated resources,
/// and `VERSION` is the last thing written, so nothing after it would notice.
#[test]
fn a_rewritten_staged_byte_is_refused_even_when_the_value_survives() {
    let object_path = |root: &Path| {
        store::local_dir(root)
            .join("migration")
            .join("destination")
            .join("objects")
            .join(format!("{AUTHORITY}.json"))
    };
    let event_path = |root: &Path| {
        store::local_dir(root)
            .join("migration")
            .join("destination")
            .join("eventstore")
            .join(format!("{AUTHORITY}.jsonl"))
    };

    /// One way of writing the same value differently, applied to a staged file.
    type Rewrite = (&'static str, Box<dyn Fn(&Path)>);

    // Each rewrite parses to the same value the digests were taken over.
    let rewrites: Vec<Rewrite> = vec![
        (
            "an Object with its members in another order",
            Box::new(move |root: &Path| {
                let value: Value = serde_json::from_str(&read(&object_path(root))).expect("staged");
                let reordered: Vec<String> = value
                    .as_object()
                    .expect("object")
                    .iter()
                    .rev()
                    .map(|(key, value)| format!("{}:{value}", serde_json::to_string(key).unwrap()))
                    .collect();
                write(&object_path(root), &format!("{{{}}}", reordered.join(",")));
            }),
        ),
        (
            "an Object with insignificant whitespace",
            Box::new(move |root: &Path| {
                let value: Value = serde_json::from_str(&read(&object_path(root))).expect("staged");
                write(
                    &object_path(root),
                    &serde_json::to_string_pretty(&value).expect("pretty"),
                );
            }),
        ),
        (
            "an Event with insignificant whitespace",
            Box::new(move |root: &Path| {
                let text = read(&event_path(root));
                let value: Value = serde_json::from_str(text.trim_end()).expect("staged");
                write(
                    &event_path(root),
                    &format!(
                        "{}\n",
                        serde_json::to_string_pretty(&value).expect("pretty")
                    ),
                );
            }),
        ),
        (
            "an Event with its members in another order",
            Box::new(move |root: &Path| {
                let text = read(&event_path(root));
                let value: Value = serde_json::from_str(text.trim_end()).expect("staged");
                let reordered: Vec<String> = value
                    .as_object()
                    .expect("object")
                    .iter()
                    .rev()
                    .map(|(key, value)| format!("{}:{value}", serde_json::to_string(key).unwrap()))
                    .collect();
                write(&event_path(root), &format!("{{{}}}\n", reordered.join(",")));
            }),
        ),
    ];

    for (what, rewrite) in rewrites {
        let (_temp, root) = released();
        let proposed = engr::migration::prepare(&root).expect("prepare");
        interrupt_after_confirmed_destination(&root, &proposed.challenge);
        rewrite(&root);

        let refused = engr::confirm(&root, &format!("CONFIRM {}", proposed.challenge))
            .expect_err(&format!("{what} must be refused"));
        assert_eq!(refused.code, engr::EXIT_SCHEMA, "{what}: {refused}");
        assert!(
            !store::version_path(&root).exists(),
            "{what}: nothing may activate"
        );
        // Still an unfinished transaction, and still saying so: the stage is
        // present and `VERSION` is not, which is exactly the state a refusal
        // naming `engr migrate` describes.
        let refused = store::validate_format(&root).expect_err("the transaction is unfinished");
        assert!(
            refused.message.contains("incomplete coordinated migration"),
            "{what}: {refused}"
        );
    }
}

/// The staged set is the confirmed set, exactly.
///
/// Counting entries and looking each one up in the plan is satisfied by a
/// manifest naming one Object twice and another not at all: every entry finds a
/// plan and the lengths agree. What would then publish is the duplicate, while
/// the missing Object keeps its predecessor bytes at the canonical current path
/// with no stream behind them — under a `VERSION` saying the workspace is
/// current.
#[test]
fn a_staged_destination_that_drops_an_object_activates_nothing() {
    let (_temp, root) = released();
    let proposed = engr::migration::prepare(&root).expect("prepare");
    interrupt_after_confirmed_destination(&root, &proposed.challenge);

    let manifest_path = store::local_dir(&root)
        .join("migration")
        .join("destination")
        .join("destination.json");
    let mut manifest: Value = serde_json::from_str(&read(&manifest_path)).expect("manifest");
    let files = manifest["files"].as_array().expect("files").clone();
    assert_eq!(files.len(), 4, "the fixture is four Objects");
    // The shape the old check could not see: same length, every entry still
    // found in the plan, and one Object silently gone.
    let dropped = vec![
        files[0].clone(),
        files[0].clone(),
        files[2].clone(),
        files[3].clone(),
    ];
    manifest["files"] = Value::Array(dropped);
    write(&manifest_path, &manifest.to_string());

    let refused = engr::confirm(&root, &format!("CONFIRM {}", proposed.challenge))
        .expect_err("a manifest that drops an Object must be refused");
    assert_eq!(refused.code, engr::EXIT_INVARIANT, "{refused}");
    assert!(!store::version_path(&root).exists(), "nothing may activate");
    for id in [AUTHORITY, MODEL, PROVENANCE_OBJECT, PROJECTION] {
        assert!(
            !store::events_path(&root, id).exists(),
            "{id} must have no current stream"
        );
    }
}

/// A crash after `VERSION` leaves residue, not an unfinished transaction.
///
/// `VERSION` is written after the last destination byte, so once it exists the
/// migration is over. What can still be on disk is the spent Challenge and the
/// stage. Treating that stage as an incomplete migration refused every read —
/// and the only command the refusal named could not act, because resuming
/// demanded the Challenge the same code path had already removed. The window
/// has to converge on its own.
#[test]
fn a_crash_after_activation_leaves_residue_that_clears_itself() {
    let stage_dir_of = engr::migration::stage_dir;

    // Before `VERSION`: everything is published but nothing is activated. The
    // stage is still the transaction, so this must resume, not sweep.
    let (_temp, root) = released();
    let proposed = engr::migration::prepare(&root).expect("prepare");
    interrupt_at(&root, "version", &proposed.challenge);
    assert!(!store::version_path(&root).exists());
    assert!(stage_dir_of(&root).exists());
    let refused = store::validate_format(&root).expect_err("not activated yet");
    assert!(
        refused.message.contains("incomplete coordinated migration"),
        "{refused}"
    );
    let resumed = engr::migration::prepare(&root).expect("preparing resumes the same question");
    assert_eq!(resumed.challenge, proposed.challenge, "and the same code");
    match engr::confirm(&root, &format!("CONFIRM {}", proposed.challenge)).expect("resume") {
        engr::Confirmed::Migration(_) => {}
        engr::Confirmed::Object(_) => panic!("a migration confirmation"),
    }
    assert!(!stage_dir_of(&root).exists(), "and it finishes the sweep");

    // After `VERSION`, before the sweep: the migration is over and what is left
    // is residue. It must not refuse a read, and it must clear without anybody
    // deleting a file by hand.
    let (_temp, root) = released();
    let proposed = engr::migration::prepare(&root).expect("prepare");
    interrupt_at(&root, "challenge", &proposed.challenge);

    assert!(store::version_path(&root).exists(), "it did activate");
    assert!(stage_dir_of(&root).exists(), "and the sweep never ran");
    assert_eq!(
        store::validate_format(&root).expect("a current workspace reads"),
        WorkspaceFormat::Current,
        "residue must not make an activated workspace unreadable"
    );
    let listed = engr::store::object_ids(&root).expect("object ids");
    assert_eq!(listed.len(), 4, "every migrated Object is there");
    for id in &listed {
        ops::effective(&root, id).expect("and each one resolves");
        assert!(
            ops::verify(&root, id).expect("verify").passed(),
            "{id} verifies through the residue"
        );
    }

    // The compatibility lock is residue too, and this is the path that leaves
    // it: `apply` never returned, so it never got to retire it.
    assert!(
        store::predecessor_lock_path(&root).exists(),
        "the crash left the legacy lock behind"
    );

    // The supported command converges all of it. Nothing is left to migrate,
    // the residue is gone, the spent code goes with it, and so does the lock.
    let error = engr::migration::prepare(&root).expect_err("there is nothing left to migrate");
    assert_eq!(error.code, engr::EXIT_SCHEMA, "{error}");
    assert!(error.message.contains("nothing to migrate"), "{error}");
    assert!(!stage_dir_of(&root).exists(), "the residue is gone");
    assert!(
        engr::gate::pending_codes(&root)
            .expect("pending codes")
            .is_empty(),
        "and the spent code with it"
    );
    assert!(
        !store::predecessor_lock_path(&root).exists(),
        "and the legacy lock with it"
    );
}

/// `engr migrate` on a workspace that is already current does not invent a
/// predecessor lock.
///
/// Taking the compatibility lock creates the file, and preparation took it
/// before deciding whether there was a predecessor at all — so asking to
/// migrate an ordinary generation-1 workspace materialized exactly the residue
/// the migration exists to have removed, then returned "nothing to migrate"
/// without cleaning it up.
#[test]
fn migrating_a_current_workspace_leaves_no_legacy_lock() {
    let (_temp, root) = released();
    migrate(&root);
    assert!(!store::predecessor_lock_path(&root).exists());

    for _ in 0..2 {
        let error = engr::migration::prepare(&root).expect_err("nothing to migrate");
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{error}");
        assert!(
            !store::predecessor_lock_path(&root).exists(),
            "asking again must not create one either"
        );
    }

    // And one left over from any earlier path is swept by the same command.
    write(&store::predecessor_lock_path(&root), "");
    engr::migration::prepare(&root).expect_err("nothing to migrate");
    assert!(
        !store::predecessor_lock_path(&root).exists(),
        "a legacy lock found on a current workspace is residue"
    );
}

/// A qualified yes withdraws a migration, the same way it withdraws anything
/// else.
///
/// `CONFIRM <code>` followed by commentary is hedged assent, and the documented
/// consequence is that the Challenge is discarded. Routing that through the
/// Object family's disposal meant it hit `require_current` on a workspace that
/// is a predecessor by definition — so the withdrawal quietly did not happen and
/// the code the human had just declined stayed live. The plan goes with it,
/// because a plan whose code is gone can never be applied.
#[test]
fn a_qualified_response_withdraws_a_prepared_migration() {
    let (_temp, root) = released();
    let proposed = engr::migration::prepare(&root).expect("prepare");
    let before = fingerprint(&root);

    let refused = engr::confirm(
        &root,
        &format!(
            "CONFIRM {} but only the first two objects",
            proposed.challenge
        ),
    )
    .expect_err("a qualified yes is not assent");
    assert_eq!(refused.code, engr::EXIT_USAGE, "{refused}");
    assert!(
        refused.message.contains("qualified yes") && refused.message.contains("was discarded"),
        "{refused}"
    );

    assert!(
        !store::challenge_path(&root, &proposed.challenge)
            .expect("challenge path")
            .exists(),
        "the code the human declined must not stay live"
    );
    assert!(
        !engr::migration::stage_dir(&root).exists(),
        "and the plan standing behind it goes too"
    );
    assert!(!store::version_path(&root).exists(), "nothing activated");

    // Not one predecessor byte moved: withdrawing is as local as preparing was.
    assert_eq!(
        fingerprint(&root),
        before,
        "withdrawing a migration changes nothing the record is made of"
    );

    // And the workspace is ready to be asked again, with a new code.
    let again = engr::migration::prepare(&root).expect("prepare again");
    assert_ne!(again.challenge, proposed.challenge, "a fresh question");
    assert!(!again.resumed, "not a resumption of the withdrawn one");
    match engr::confirm(&root, &format!("CONFIRM {}", again.challenge)).expect("confirm") {
        engr::Confirmed::Migration(report) => assert_eq!(report.objects.len(), 4),
        engr::Confirmed::Object(_) => panic!("a migration confirmation"),
    }
    assert!(store::version_path(&root).exists());
}

/// Rewrite a Challenge on disk and keep it self-consistent.
fn restamp_challenge(
    root: &Path,
    code: &str,
    change: impl FnOnce(&mut engr::confirmation::Challenge),
) {
    let path = store::challenge_path(root, code).expect("challenge path");
    let mut challenge: engr::confirmation::Challenge =
        serde_json::from_str(&read(&path)).expect("challenge");
    change(&mut challenge);
    challenge.digest = challenge.recomputed_digest().expect("reseal the challenge");
    let target = store::challenge_path(root, &challenge.id).expect("challenge path");
    write(
        &target,
        &proof::canonical_bytes(&challenge, "challenge").expect("canonical"),
    );
    if target != path {
        std::fs::remove_file(&path).expect("remove the original");
    }
}

/// A pending question this build cannot interpret has to be reachable by the
/// route the protocol names: prepare again.
///
/// `Challenge::validate` refuses a foreign generator fingerprint and says so.
/// But resuming only checked that the file existed, so `engr migrate` handed
/// back the unusable code forever; confirming it failed the fingerprint check,
/// and withdrawing it failed too, because disposal has to read the file to learn
/// whose question it is. The only way out was deleting local files by hand.
#[test]
fn an_uninterpretable_pending_migration_is_asked_again() {
    let (_temp, root) = released();
    let proposed = engr::migration::prepare(&root).expect("prepare");

    // A perfectly well-formed Challenge from a generator whose contract has
    // moved: resealed, so nothing but the fingerprint is wrong with it.
    restamp_challenge(&root, &proposed.challenge, |challenge| {
        challenge.generator.fingerprint = format!("1:{}", "b".repeat(64));
        challenge.generator.version = "latest (something-else)".to_owned();
    });
    let refused = engr::confirm(&root, &format!("CONFIRM {}", proposed.challenge))
        .expect_err("this build cannot interpret it");
    assert!(refused.message.contains("prepare it again"), "{refused}");

    // So preparing again is what it says: the stale question and its plan are
    // retired, and a fresh one is minted under this build's own generator.
    let again = engr::migration::prepare(&root).expect("prepare again");
    assert_ne!(again.challenge, proposed.challenge, "a new question");
    assert!(!again.resumed, "not a resumption of the unusable one");
    assert!(
        !store::challenge_path(&root, &proposed.challenge)
            .expect("path")
            .exists(),
        "and the unusable code is gone rather than lingering"
    );
    assert_eq!(
        engr::gate::pending_codes(&root).expect("pending"),
        vec![again.challenge.clone()],
        "exactly one live question"
    );

    // And it is answerable, which is the whole point.
    match engr::confirm(&root, &format!("CONFIRM {}", again.challenge)).expect("confirm") {
        engr::Confirmed::Migration(report) => assert_eq!(report.objects.len(), 4),
        engr::Confirmed::Object(_) => panic!("a migration confirmation"),
    }
    assert!(store::version_path(&root).exists());
}

/// Migration serializes against the lock the *released* build takes.
///
/// The two generations lock different files — `.engr/lock` and
/// `.engr/local/lock` — so a released process and this one could each hold "the"
/// workspace lock and not contend at all. A migration runs on a workspace a
/// released build is still entitled to write to, and the source revalidation
/// completes before publication begins: in that interval an old writer could
/// admit a predecessor mutation that publication then overwrote and deleted, or
/// overwrite a just-published Object with predecessor bytes before `VERSION`.
#[test]
fn confirmation_waits_for_the_released_writer_lock() {
    use fs2::FileExt;

    let (_temp, root) = released();
    let proposed = engr::migration::prepare(&root).expect("prepare");
    let before = fingerprint(&root);

    // Stand in for a released build that is mid-write.
    let held = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(store::predecessor_lock_path(&root))
        .expect("open the predecessor lock");
    held.lock_exclusive().expect("hold the predecessor lock");

    let mut confirming = Command::new(env!("CARGO_BIN_EXE_engr"))
        .arg("--root")
        .arg(&root)
        .arg("confirm")
        .arg(format!("CONFIRM {}", proposed.challenge))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn the confirmation");

    // It must not get past the lock. Nothing may be published while another
    // writer holds the predecessor.
    std::thread::sleep(std::time::Duration::from_millis(1_500));
    assert!(
        confirming
            .try_wait()
            .expect("poll the confirmation")
            .is_none(),
        "the confirmation ran while a released writer held the predecessor"
    );
    assert!(
        !store::version_path(&root).exists(),
        "and nothing activated while it waited"
    );
    assert_eq!(
        fingerprint(&root),
        before,
        "nor was a single predecessor byte published"
    );

    // Now the old writer commits something and lets go. This is the mutation
    // the interval used to be able to swallow.
    edit_predecessor(&root, AUTHORITY, |value| {
        value["sections"][0]["text"] =
            Value::String("admitted by a released build during the migration".to_owned());
        reseal_predecessor_section(&mut value["sections"][0]);
    });
    let moved = read(&store::object_path(&root, AUTHORITY));
    FileExt::unlock(&held).expect("release the predecessor lock");

    // The migration proceeds, revalidates, and refuses — rather than publishing
    // over a mutation it never saw.
    let finished = confirming.wait_with_output().expect("await confirmation");
    assert!(
        !finished.status.success(),
        "the source moved, so the confirmed plan is not the plan"
    );
    assert!(
        !store::version_path(&root).exists(),
        "nothing activated: {}",
        String::from_utf8_lossy(&finished.stderr)
    );
    assert_eq!(
        read(&store::object_path(&root, AUTHORITY)),
        moved,
        "and the concurrent admission survived untouched"
    );
}

/// Withdrawal is refused by the state of the transaction, not by reading the
/// question.
///
/// The fallback that makes an unreadable Challenge withdrawable is exactly what
/// must not reach a confirmed one. Once a destination is staged somebody has
/// already answered exactly and publication may have begun — at the `version`
/// boundary the predecessor's own `events/` and `format.json` are already gone,
/// so the stage is the only copy of what was confirmed. A qualified response
/// arriving then must not be able to delete it, and it cannot be asked to prove
/// that by interpreting a Challenge it cannot read.
#[test]
fn a_qualified_response_cannot_discard_a_confirmed_migration() {
    for (stage, published) in [("destination", false), ("version", true)] {
        let (_temp, root) = released();
        let proposed = engr::migration::prepare(&root).expect("prepare");
        interrupt_at(&root, stage, &proposed.challenge);
        assert!(
            engr::migration::stage_dir(&root)
                .join("destination")
                .exists(),
            "{stage}: the destination is staged"
        );
        assert_eq!(
            store::engr_dir(&root).join("format.json").exists(),
            !published,
            "{stage}: publication state is what the boundary says"
        );

        // Now make the Challenge unreadable, which is the only way into the
        // fallback: a generator whose contract this build does not share.
        restamp_challenge(&root, &proposed.challenge, |challenge| {
            challenge.generator.fingerprint = format!("1:{}", "c".repeat(64));
        });

        let refused = engr::confirm(
            &root,
            &format!("CONFIRM {} on second thoughts", proposed.challenge),
        )
        .expect_err("a confirmed migration cannot be withdrawn");
        assert_eq!(refused.code, engr::EXIT_INVARIANT, "{stage}: {refused}");
        assert!(
            refused.message.contains("only be finished"),
            "{stage}: {refused}"
        );

        // Everything needed to finish is still there.
        assert!(
            engr::migration::stage_dir(&root)
                .join("destination")
                .exists(),
            "{stage}: the only copy of what was confirmed is retained"
        );
        assert!(
            store::challenge_path(&root, &proposed.challenge)
                .expect("path")
                .exists(),
            "{stage}: and so is the code that finishes it"
        );
    }
}

/// A later domain's name is present whether or not it resolves to anything.
///
/// `exists()` follows links, so a dangling `.engr/rules` answered "absent" for
/// an entry that is plainly there — and the released workspace never had that
/// name under any target. Migration would have read a later build's workspace as
/// the released one.
#[test]
fn a_dangling_later_domain_is_still_a_later_domain() {
    for domain in ["rules", "backlog", "work", "collections", "eventstore"] {
        let (_temp, root) = released();
        let entry = store::engr_dir(&root).join(domain);
        let nowhere = store::engr_dir(&root).join("no-such-target");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&nowhere, &entry).expect("symlink");
        #[cfg(windows)]
        if std::os::windows::fs::symlink_dir(&nowhere, &entry).is_err() {
            // Windows needs privilege for this; the platform is covered by CI.
            return;
        }
        assert!(!entry.exists(), "{domain}: it dangles, so exists() says no");
        assert!(
            std::fs::symlink_metadata(&entry).is_ok(),
            "{domain}: but the entry is there"
        );

        let refused = engr::migration::prepare(&root).expect_err("not the released workspace");
        assert_eq!(refused.code, engr::EXIT_SCHEMA, "{domain}: {refused}");
        assert!(refused.message.contains(domain), "{domain}: {refused}");
        // And it refused before minting or staging anything.
        assert!(
            !engr::migration::stage_dir(&root).exists(),
            "{domain}: nothing staged"
        );
        assert!(
            engr::gate::pending_codes(&root)
                .expect("pending codes")
                .is_empty(),
            "{domain}: no code minted"
        );
    }
}

/// A six-character code names a live question, not a moment in history.
///
/// The crash between removing the spent code and removing the plan leaves the
/// plan naming a code with no file behind it — on a workspace that is already
/// current, so ordinary Human questions can be prepared again and one of them
/// may legitimately take that code back. Sweeping by name would then delete
/// somebody else's live question.
#[test]
fn cleanup_does_not_take_a_reused_code_from_an_unrelated_question() {
    let (_temp, root) = released();
    let proposed = engr::migration::prepare(&root).expect("prepare");
    interrupt_at(&root, "challenge", &proposed.challenge);

    // Exactly the crash state: activated, the spent code gone, the plan left.
    let spent = proposed.challenge.clone();
    let path = store::challenge_path(&root, &spent).expect("challenge path");
    if path.exists() {
        std::fs::remove_file(&path).expect("the sweep had removed it");
    }
    assert!(engr::migration::stage_dir(&root).exists());
    assert!(store::version_path(&root).exists());

    // The code is spoken for while the residue stands, so minting cannot take
    // it — which is the first of the two answers to this.
    let payload = engr::model::Payload::new(
        AUTHORITY,
        engr::model::Action::ObjectRenamed {
            title: "A question that has nothing to do with the migration".to_owned(),
            becomes: None,
        },
    );
    let prepared = engr::gate::prepare(&root, payload).expect("prepare an ordinary question");
    assert_ne!(
        prepared.candidate.code(),
        spent,
        "a code the residue still names must not be minted"
    );

    // Force the collision anyway, which is what a build without that
    // reservation would have produced, and require the sweep to leave it alone.
    restamp_challenge(&root, prepared.candidate.code(), |challenge| {
        challenge.id = spent.clone();
    });
    let survivor = store::challenge_path(&root, &spent).expect("challenge path");
    assert!(
        survivor.exists(),
        "the unrelated question now holds the code"
    );

    engr::migration::prepare(&root).expect_err("there is nothing left to migrate");
    assert!(
        !engr::migration::stage_dir(&root).exists(),
        "the residue is swept"
    );
    assert!(
        survivor.exists(),
        "but a Human question that merely reuses the code is not"
    );
    let still_there: engr::confirmation::Challenge =
        serde_json::from_str(&read(&survivor)).expect("and it still reads");
    assert_eq!(still_there.id, spent);
    assert_eq!(
        still_there.subject.kind,
        engr::confirmation::SubjectType::Object,
        "as the Object question it is"
    );
    still_there.validate().expect("and it is still usable");
}

/// Revision 1 of a migrated Object has to be the bootstrap that derives it.
///
/// The Object is pinned to the confirmed plan, but the staged Event was pinned
/// only to its own seal and to `event_digest` — which lives in the same local
/// manifest an interrupted transaction leaves behind, and is no part of what
/// anybody confirmed. So a canonical, self-sealed rev-1 Event for the same
/// Object, with that local digest updated to match, satisfied every check, and
/// the permanent history could stop reproducing the Object under a `VERSION`
/// saying the workspace is current.
#[test]
fn a_restaged_bootstrap_that_does_not_derive_the_object_activates_nothing() {
    let event_path = |root: &Path| {
        store::local_dir(root)
            .join("migration")
            .join("destination")
            .join("eventstore")
            .join(format!("{AUTHORITY}.jsonl"))
    };
    let manifest_path = |root: &Path| {
        store::local_dir(root)
            .join("migration")
            .join("destination")
            .join("destination.json")
    };

    // Two records that are each perfectly valid on their own terms, and neither
    // of which replays to the Object the human confirmed.
    let forgeries = vec![
        (
            "a creation standing where the bootstrap belongs",
            engr::model::Action::ObjectCreated {
                title: "Something else entirely".to_owned(),
            },
        ),
        (
            "a migration bootstrap carrying a different snapshot",
            engr::model::Action::ObjectMigrated {
                snapshot: Box::new(engr::model::Snapshot {
                    title: "A title nobody confirmed".to_owned(),
                    object_type: None,
                    state: engr::semantics::State::Open,
                    next_section_id: 1,
                    sections: Vec::new(),
                }),
            },
        ),
    ];

    for (what, action) in forgeries {
        let (_temp, root) = released();
        let proposed = engr::migration::prepare(&root).expect("prepare");
        interrupt_after_confirmed_destination(&root, &proposed.challenge);

        // Sealed by the library, so it is exactly as canonical and as
        // self-consistent as the record it replaces.
        let forged = engr::model::Event::sealed(
            AUTHORITY,
            engr::model::new_id(),
            action,
            1,
            engr::model::EventAdmission::human("2026-09-02T00:00:00Z", proposed.challenge.clone()),
        )
        .expect("seal the replacement");
        let bytes = proof::canonical_bytes(&forged, "event").expect("canonical");
        write(&event_path(&root), &format!("{bytes}\n"));

        // And the local manifest is updated to agree with it, which is the
        // whole point: nothing in `destination.json` is covered by the
        // confirmation.
        let mut manifest: Value =
            serde_json::from_str(&read(&manifest_path(&root))).expect("manifest");
        for file in manifest["files"].as_array_mut().expect("files") {
            if file["object"] == Value::String(AUTHORITY.to_owned()) {
                file["event_digest"] = Value::String(forged.digest.clone());
            }
        }
        write(&manifest_path(&root), &manifest.to_string());

        let refused = engr::confirm(&root, &format!("CONFIRM {}", proposed.challenge))
            .expect_err(&format!("{what} must be refused"));
        assert_eq!(refused.code, engr::EXIT_INVARIANT, "{what}: {refused}");
        assert!(
            !store::version_path(&root).exists(),
            "{what}: nothing may activate"
        );
        assert!(
            !store::events_path(&root, AUTHORITY).exists(),
            "{what}: and no current stream is written"
        );
    }
}

/// Admission metadata is provenance, and replay cannot check it.
///
/// The bootstrap must replay to the confirmed Object, but `project` derives
/// Object state and neither the review member nor the admission instant is
/// Object state. So a staged Event could keep its snapshot, take on a
/// structurally valid Rule Review — which no migration has — or its own
/// admission time, be re-sealed, and have the local `event_digest` updated to
/// agree. It would replay identically and publish permanent provenance
/// describing something that never happened.
#[test]
fn a_restaged_bootstrap_with_invented_provenance_activates_nothing() {
    let staged_event = |root: &Path, id: &str| {
        store::local_dir(root)
            .join("migration")
            .join("destination")
            .join("eventstore")
            .join(format!("{id}.jsonl"))
    };
    let manifest_path = |root: &Path| {
        store::local_dir(root)
            .join("migration")
            .join("destination")
            .join("destination.json")
    };

    /// One way of moving a bootstrap's provenance without moving its snapshot.
    type Tamper = (&'static str, Box<dyn Fn(&mut engr::model::EventAdmission)>);

    let cases: Vec<Tamper> = vec![
        (
            "a Rule Review no migration ever had",
            Box::new(|admitted: &mut engr::model::EventAdmission| {
                admitted.review = Some(engr::model::ReviewProvenance {
                    outcome: engr::model::ReviewOutcome::Passed,
                    result: engr::proof::ReviewResult::Passed,
                    attempts: 1,
                });
            }),
        ),
        (
            "an admission instant of its own",
            Box::new(|admitted: &mut engr::model::EventAdmission| {
                admitted.at = "2020-01-01T00:00:00Z".to_owned();
            }),
        ),
    ];

    for (what, tamper) in cases {
        let (_temp, root) = released();
        let proposed = engr::migration::prepare(&root).expect("prepare");
        interrupt_after_confirmed_destination(&root, &proposed.challenge);

        // The snapshot is untouched, so it still replays to the confirmed
        // Object. Only the provenance moves.
        let path = staged_event(&root, AUTHORITY);
        let mut event: engr::model::Event =
            serde_json::from_str(read(&path).trim_end()).expect("staged event");
        tamper(&mut event.metadata.admitted);
        let resealed = engr::model::Event::sealed(
            AUTHORITY,
            event.id.clone(),
            event.action.clone(),
            event.rev,
            event.metadata.admitted.clone(),
        )
        .expect("reseal");
        write(
            &path,
            &format!(
                "{}\n",
                proof::canonical_bytes(&resealed, "event").expect("canonical")
            ),
        );

        let mut manifest: Value =
            serde_json::from_str(&read(&manifest_path(&root))).expect("manifest");
        for file in manifest["files"].as_array_mut().expect("files") {
            if file["object"] == Value::String(AUTHORITY.to_owned()) {
                file["event_digest"] = Value::String(resealed.digest.clone());
            }
        }
        write(&manifest_path(&root), &manifest.to_string());

        let refused = engr::confirm(&root, &format!("CONFIRM {}", proposed.challenge))
            .expect_err(&format!("{what} must be refused"));
        assert_eq!(refused.code, engr::EXIT_INVARIANT, "{what}: {refused}");
        assert!(
            !store::version_path(&root).exists(),
            "{what}: nothing may activate"
        );
        assert!(
            !store::events_path(&root, AUTHORITY).exists(),
            "{what}: and no current stream is written"
        );
    }
}

/// Staging is not admission. No destination Event carrying final provenance
/// exists until the exact Human response has crossed the real apply boundary;
/// a retry publishes that one admitted Event rather than restamping it.
#[test]
fn a_destination_is_staged_only_after_human_confirmation() {
    let (_temp, root) = released();
    let proposed = engr::migration::prepare(&root).expect("prepare");
    let challenge: Value = serde_json::from_str(&read(
        &store::challenge_path(&root, &proposed.challenge).expect("challenge path"),
    ))
    .expect("challenge");
    let created_at = time::OffsetDateTime::parse(
        challenge["created_at"].as_str().expect("created_at"),
        &time::format_description::well_known::Rfc3339,
    )
    .expect("created_at time");
    let destination = store::local_dir(&root)
        .join("migration")
        .join("destination");
    assert!(
        !destination.exists(),
        "preparation must not stage final Event provenance"
    );
    std::thread::sleep(std::time::Duration::from_millis(1_100));
    interrupt_after_confirmed_destination(&root, &proposed.challenge);

    let staged: Value = serde_json::from_str(
        read(
            &destination
                .join("eventstore")
                .join(format!("{AUTHORITY}.jsonl")),
        )
        .trim_end(),
    )
    .expect("staged event");
    let staged_at = staged["metadata"]["admitted"]["at"]
        .as_str()
        .expect("staged admission time")
        .to_owned();
    let admitted_at =
        time::OffsetDateTime::parse(&staged_at, &time::format_description::well_known::Rfc3339)
            .expect("admitted_at time");
    assert!(
        admitted_at > created_at,
        "the final Event timestamp must be acquired after the Human response"
    );

    match engr::confirm(&root, &format!("CONFIRM {}", proposed.challenge)).expect("confirm") {
        engr::Confirmed::Migration(_) => {}
        engr::Confirmed::Object(_) => panic!("migration confirmation"),
    }
    let event = &store::load_events(&root, AUTHORITY).expect("events")[0];
    assert_eq!(
        event.metadata.admitted.at, staged_at,
        "resume must publish the admitted transaction, not mint a second instant"
    );
}

/// The local exclude lands where git actually reads it, including from a linked
/// worktree.
///
/// `--absolute-git-dir` answers with the *per-worktree* administrative
/// directory, while `info/` is resolved through the common directory a linked
/// worktree shares with its parent. Joining `info/exclude` onto the git dir
/// therefore writes, in a linked worktree, a file git never reads — and the
/// failure is silent in the worst possible way: preparing appears to succeed,
/// and `.engr/local/` stays visible to `git add -A` with a live challenge code
/// sitting in it, which is the whole thing the exclude exists to prevent.
///
/// So the path is asked for by name, and this checks the answer from the place
/// that gets it wrong.
#[test]
fn the_local_exclude_lands_where_git_reads_it_from_a_linked_worktree() {
    let (temp, main) = released();
    // A branch to check out, since a linked worktree cannot share the one the
    // parent has.
    git(&main, &["branch", "migration-worktree"]);
    let linked = temp.path().join("linked");
    git(
        &main,
        &[
            "worktree",
            "add",
            linked.to_str().expect("path"),
            "migration-worktree",
        ],
    );
    assert!(
        store::engr_dir(&linked).exists(),
        "the linked worktree has the predecessor workspace"
    );

    let proposed = engr::migration::prepare(&linked).expect("prepare from the linked worktree");
    let code = proposed.challenge;

    // Git's own answer, not ours: whatever it says `info/exclude` is, that is
    // the file it reads.
    let exclude = PathBuf::from(git(
        &linked,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "info/exclude",
        ],
    ));
    let text = std::fs::read_to_string(&exclude).unwrap_or_default();
    assert!(
        text.contains(".engr/local/"),
        "the exclude git reads must name the local directory: {} holds {text:?}",
        exclude.display()
    );

    // And the thing that matters, asked of git rather than inferred: the live
    // code is not something `git add -A` would stage.
    let live = store::challenges_dir(&linked).join(format!("{code}.json"));
    assert!(live.exists(), "the challenge is on disk");
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(&linked)
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output()
        .expect("git status");
    let listing = String::from_utf8_lossy(&status.stdout).to_string();
    assert!(
        !listing.contains(&code),
        "a live challenge code must not be stageable: {listing}"
    );
    assert!(
        !listing.contains("local/"),
        "and nor must anything else under local/: {listing}"
    );

    // Preparation still changed no tracked byte, in the worktree or the parent.
    for root in [&linked, &main] {
        assert!(
            !read(&store::engr_dir(root).join(".gitignore")).contains("/local/"),
            "the tracked ignore file is not where this was done"
        );
    }
}

/// Every exit that hands back a live code has established the exclusion that
/// protects it, and doing so damages nothing the person already had.
///
/// Two halves of one invariant, and both were broken.
///
/// The exclusion was written with a direct in-place `fs::write`: nothing flushed
/// the bytes and nothing flushed the name, while the staged plan and the
/// Challenge minted a moment later were made deliberately durable. So a power
/// failure after a *successful* `engr migrate` could keep the live code and lose
/// the only thing keeping `git add -A` from staging it — the tracked
/// `.gitignore` does not name `/local/` until the migration is confirmed. The
/// same in-place write could truncate a person's existing exclusions, which
/// preparing a migration has no business touching at all.
///
/// And the resume path never called it. It returns the *same live code* as the
/// fresh path, so it needs the same protection to be in place — an earlier
/// prepare having established it once is not the same as it being there now,
/// because the file belongs to git and the person, not to this transaction.
///
/// The lost-write itself is not observable in-process; that half is held
/// structurally by `no_file_that_must_survive_a_crash_is_written_in_place` in
/// the record tests. What is observable is here: the content is preserved, and
/// the exclusion is re-established on the exit that had skipped it.
#[test]
fn a_live_code_is_never_handed_back_without_the_exclusion_that_protects_it() {
    let (_temp, root) = released();
    let exclude = PathBuf::from(git(
        &root,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "info/exclude",
        ],
    ));

    // Exclusions the person already had. Preparing may add to this file; it may
    // not damage it.
    let mine = "# mine\n*.swp\nbuild/\n";
    std::fs::create_dir_all(exclude.parent().expect("parent")).expect("git info");
    write(&exclude, mine);

    let first = engr::migration::prepare(&root).expect("prepare");
    assert!(!first.resumed);
    let after = read(&exclude);
    for kept in ["# mine", "*.swp", "build/"] {
        assert!(
            after.contains(kept),
            "preparing must not take away an exclusion it did not add: {after:?}"
        );
    }
    assert!(after.contains(".engr/local/"), "{after:?}");

    // The resume exit, which hands back the same live question. Whatever the
    // first prepare wrote, this one is exposed by its absence exactly as a fresh
    // one would be.
    std::fs::remove_file(&exclude).expect("remove the exclusion");
    let resumed = engr::migration::prepare(&root).expect("resume");
    assert!(
        resumed.resumed,
        "the staged question is resumed, not re-minted"
    );
    assert_eq!(
        resumed.challenge, first.challenge,
        "and it is the same live code"
    );
    assert!(
        read(&exclude).contains(".engr/local/"),
        "so the exclusion has to be there again: {:?}",
        read(&exclude)
    );

    // Asked of git rather than inferred from the file.
    assert!(
        store::challenges_dir(&root)
            .join(format!("{}.json", resumed.challenge))
            .exists(),
        "the challenge is on disk"
    );
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output()
        .expect("git status");
    let listing = String::from_utf8_lossy(&status.stdout).to_string();
    assert!(
        !listing.contains(&resumed.challenge),
        "a live challenge code must not be stageable: {listing}"
    );
}

/// The source of a migration is the bytes the released workspace persisted, so
/// no part of it may be reached by following a link.
///
/// A migration establishes what the predecessor's repository actually held, and
/// that claim is only as good as the paths it read. A link is a path denoting
/// another path: git records the link, the read returns the target's contents,
/// and the migrated record would be built from bytes the predecessor repository
/// never tracked — while the captured source digest says otherwise. A *dangling*
/// link is worse than refused-and-visible: `exists()` reports it as absence, and
/// absence of a history file is a shape the release legitimately wrote, so an
/// Object would have been migrated as though it had no admitted history at all.
#[test]
#[cfg(unix)]
fn a_predecessor_reached_through_a_link_is_not_the_released_bytes() {
    let (_temp, root) = released();
    let outside = TempDir::new().expect("outside");
    let before = fingerprint(&root);

    let object = store::object_path(&root, AUTHORITY);
    let moved = outside.path().join("authority.json");
    std::fs::rename(&object, &moved).expect("move the projection out");
    std::os::unix::fs::symlink(&moved, &object).expect("symlink");
    let error = engr::migration::prepare(&root).expect_err("a redirected projection");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(
        error.message.contains("link to somewhere else"),
        "{}",
        error.message
    );
    std::fs::remove_file(&object).expect("remove the link");
    std::fs::rename(&moved, &object).expect("put it back");

    // A dangling history link is the one that used to read as absence.
    let history = store::engr_dir(&root)
        .join("events")
        .join(format!("{AUTHORITY}.jsonl"));
    let kept = history.with_extension("jsonl.kept");
    std::fs::rename(&history, &kept).expect("move the history aside");
    std::os::unix::fs::symlink(root.join("nowhere"), &history).expect("symlink");
    let error = engr::migration::prepare(&root).expect_err("a dangling history link");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(
        error.message.contains("link to somewhere else"),
        "{}",
        error.message
    );
    std::fs::remove_file(&history).expect("remove the link");
    std::fs::rename(&kept, &history).expect("put it back");

    // And a whole source directory.
    let objects = store::objects_dir(&root);
    let moved = outside.path().join("objects");
    std::fs::rename(&objects, &moved).expect("move");
    std::os::unix::fs::symlink(&moved, &objects).expect("symlink");
    let error = engr::migration::prepare(&root).expect_err("a redirected objects directory");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(
        error.message.contains("link to somewhere else"),
        "{}",
        error.message
    );
    std::fs::remove_file(&objects).expect("remove the link");
    std::fs::rename(&moved, &objects).expect("put it back");

    assert_eq!(
        fingerprint(&root).len(),
        before.len(),
        "a refused preflight publishes nothing"
    );
    assert!(!store::version_path(&root).exists());
    engr::migration::prepare(&root).expect("and the restored workspace migrates again");
}

/// A confirmed migration locks the released build out of its own workspace,
/// durably, before anything is published.
///
/// The predecessor lock is an OS lock held by one process: an interruption
/// frees it, and the released build is then entitled to a workspace whose
/// `format.json` still says version 1. It would admit predecessor state
/// legitimately in that window, and a resume that went straight to publication
/// would overwrite it — or leave a newly created predecessor Object standing as
/// legacy-shaped bytes after its history directory was removed underneath it.
///
/// So the transaction leaves a bootstrap the released build cannot read, and it
/// leaves it before the first published byte. What the released build does with
/// that is its own affair; what this pins is that the file it reads to decide is
/// no longer one it can accept.
#[test]
fn a_confirmed_migration_locks_the_predecessor_out_before_publishing() {
    let (_temp, root) = released();
    let proposed = engr::migration::prepare(&root).expect("prepare");
    let bootstrap = store::engr_dir(&root).join("format.json");
    assert_eq!(
        serde_json::from_str::<Value>(&read(&bootstrap)).expect("json")["format"],
        "engr-workspace",
        "the predecessor can read its own workspace while the question is open"
    );

    interrupt_after_confirmed_destination(&root, &proposed.challenge);

    // Nothing is published yet: the destination is staged and `VERSION` is not
    // there. And the released build is already out.
    assert!(!store::version_path(&root).exists());
    let barrier: Value = serde_json::from_str(&read(&bootstrap)).expect("json");
    assert_eq!(barrier["format"], "engr-migration-in-progress");
    assert_eq!(barrier["migration"], proposed.challenge.as_str());
    assert!(
        engr::store::validate_format(&root).is_err(),
        "and so is every ordinary read, until the migration is finished"
    );

    // Resuming still finishes, and the result is an ordinary current workspace.
    let report = match engr::confirm(&root, &format!("CONFIRM {}", proposed.challenge))
        .expect("resume the confirmed migration")
    {
        engr::Confirmed::Migration(report) => report,
        engr::Confirmed::Object(_) => panic!("a migration subject"),
    };
    assert_eq!(report.objects.len(), 4);
    assert_eq!(
        store::validate_format(&root).expect("current"),
        WorkspaceFormat::Current
    );
    assert!(!bootstrap.exists(), "the barrier goes with the publication");
}

/// A released writer in the window before the barrier is not published over.
///
/// One window remains after the destination is staged and before the barrier is
/// raised, and it is the window the old lock's release opens. A resume that
/// found a confirmed destination used to go straight to `finish`, so anything
/// admitted in the meantime was overwritten and its history deleted. The
/// confirmed subject already pins the digest of every predecessor file it was
/// derived from, so the resume can establish that the source did not move — and
/// nothing has been published yet, so withdrawing and preparing again is a real
/// answer rather than a wedge.
#[test]
fn a_predecessor_write_before_the_barrier_stops_the_resume() {
    let (_temp, root) = released();
    let proposed = engr::migration::prepare(&root).expect("prepare");
    interrupt_at(&root, "barrier", &proposed.challenge);

    // The interruption left the destination staged and the predecessor readable,
    // which is exactly what the released build needs to write again.
    assert!(store::engr_dir(&root)
        .join("local")
        .join("migration")
        .join("destination")
        .exists());
    assert_eq!(
        serde_json::from_str::<Value>(&read(&store::engr_dir(&root).join("format.json")))
            .expect("json")["format"],
        "engr-workspace",
        "the barrier is not up yet, which is the premise of this test"
    );

    // What a released build would do with the workspace it can still read: admit
    // a mutation of its own, through its own gate. Reproduced as the bytes it
    // would have written — the Event first and then the projection, in its own
    // order — because those bytes are all this build can observe about it
    // anyway, and a crude edit would be refused later as a tampered predecessor
    // rather than as the legitimate admission this is.
    rename_as_the_released_build_would(&root, AUTHORITY, "admitted by the old build");

    let error = engr::confirm(&root, &format!("CONFIRM {}", proposed.challenge))
        .expect_err("the resume must not publish over it");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(
        error.message.contains("moved after migration"),
        "{}",
        error.message
    );
    assert!(
        !store::version_path(&root).exists(),
        "nothing was published"
    );
    assert_eq!(
        predecessor_object(&root, AUTHORITY)["title"],
        "admitted by the old build",
        "and what the old build wrote is still there"
    );

    // And the way out is open, because nothing was published: a qualified
    // response withdraws the confirmed migration, and it can be prepared again
    // over the workspace as it now stands.
    engr::confirm(&root, &format!("CONFIRM {} no", proposed.challenge))
        .expect_err("a qualified response is not assent");
    assert!(
        !store::engr_dir(&root)
            .join("local")
            .join("migration")
            .exists(),
        "the withdrawn plan is gone"
    );
    engr::migration::prepare(&root).expect("and the migration can be prepared again");
}

/// One admission the released build would have made, written the way it wrote.
///
/// A rename: the Event appended to its own history first, then the projection —
/// which is the order that generation used, and the reason its migration reads
/// effective state rather than stored state. Nothing here is a hand edit
/// pretending to be an admission: the record is sealed with the predecessor's
/// own payload hash and the projection is exactly what replaying it derives, so
/// the workspace afterwards is one the released build could have produced and
/// one this build's preflight accepts.
fn rename_as_the_released_build_would(root: &Path, id: &str, title: &str) {
    let history = store::engr_dir(root)
        .join("events")
        .join(format!("{id}.jsonl"));
    let mut projection = predecessor_object(root, id);
    let rev = projection["rev"].as_u64().expect("rev") + 1;
    let mut event = serde_json::json!({
        "format": "engr-event",
        "version": 1,
        "event_id": engr::model::new_id(),
        "rev": rev,
        "time": "2026-09-03T00:00:00Z",
        "action": "object_renamed",
        "object": id,
        "text": title,
        "refs": [],
        "confirmation": { "challenge": "K7M3PQ", "payload_sha256": "" },
    });
    reseal_predecessor_event(&mut event);
    let mut stream = read(&history);
    stream.push_str(&serde_json::to_string(&event).expect("json"));
    stream.push('\n');
    write(&history, &stream);

    projection["title"] = Value::String(title.to_owned());
    projection["rev"] = serde_json::json!(rev);
    write(
        &store::object_path(root, id),
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&projection).expect("json")
        ),
    );
}

/// A barrier is a marker, not proof that the source was checked before it went
/// up.
///
/// It says the predecessor is shut out *now*. It cannot say that anything was
/// established before it was written, so a resume that skipped the source
/// comparison whenever the bootstrap merely looked shut had a hole the shape of
/// the whole finding: interrupt before the barrier, let the predecessor write,
/// then put any barrier-shaped bootstrap in place — or simply delete it — and
/// the intervening state is published over.
///
/// What decides now is what publication itself leaves behind, and none of it can
/// be written by the released build or forged by editing one file.
#[test]
fn a_barrier_nobody_earned_does_not_skip_the_source_check() {
    let forgeries: Vec<(&str, Forge)> = vec![
        ("a forged barrier", |root: &Path, _challenge: &str| {
            write(
                    &store::engr_dir(root).join("format.json"),
                    "{\"format\":\"engr-migration-in-progress\",\"migration\":\"ABC234\",\"version\":1}",
                );
        }),
        (
            "a barrier naming this very migration",
            |root: &Path, challenge: &str| {
                write(
                    &store::engr_dir(root).join("format.json"),
                    &format!(
                        "{{\"format\":\"engr-migration-in-progress\",\"migration\":\"{challenge}\",\"version\":1}}"
                    ),
                );
            },
        ),
        ("no bootstrap at all", |root: &Path, _challenge: &str| {
            std::fs::remove_file(store::engr_dir(root).join("format.json")).expect("remove");
        }),
    ];
    for (what, forge) in forgeries {
        let (_temp, root) = released();
        let proposed = engr::migration::prepare(&root).expect("prepare");
        interrupt_at(&root, "barrier", &proposed.challenge);
        rename_as_the_released_build_would(&root, AUTHORITY, "admitted by the old build");
        forge(&root, &proposed.challenge);

        let error = engr::confirm(&root, &format!("CONFIRM {}", proposed.challenge))
            .expect_err(&format!("{what}: the resume must not publish over it"));
        // Either refusal is the right one — a barrier that names another
        // transaction is caught for that before the comparison is reached — and
        // what matters is that no forged bootstrap buys a skip.
        assert!(
            error.message.contains("moved after migration")
                || error.message.contains("is the barrier of migration"),
            "{what}: {}",
            error.message
        );
        assert!(
            !store::version_path(&root).exists(),
            "{what}: nothing was published"
        );
        assert_eq!(
            predecessor_object(&root, AUTHORITY)["title"],
            "admitted by the old build",
            "{what}: and what the old build wrote is still there"
        );
    }
}

/// A barrier that names another transaction is not this transaction's.
///
/// The members exist to say which migration shut the predecessor out and which
/// generation it is on the way to. A resume that read the file for its *shape*
/// alone would accept the barrier of an older, abandoned or fabricated
/// migration as evidence about this one.
#[test]
fn a_barrier_is_held_to_the_migration_and_generation_it_names() {
    for (what, bootstrap, expected) in [
        (
            "another migration's barrier",
            "{\"format\":\"engr-migration-in-progress\",\"migration\":\"ZZZ999\",\"version\":1}",
            "is the barrier of migration ZZZ999",
        ),
        (
            "a barrier on the way to another generation",
            "{\"format\":\"engr-migration-in-progress\",\"migration\":\"REPLACE\",\"version\":9}",
            "on the way to generation 9",
        ),
        (
            "a bootstrap nothing here wrote",
            "{\"format\":\"engr-something-else\",\"version\":1}",
            "neither the released predecessor bootstrap nor a migration barrier",
        ),
    ] {
        let (_temp, root) = released();
        let proposed = engr::migration::prepare(&root).expect("prepare");
        interrupt_at(&root, "barrier", &proposed.challenge);
        write(
            &store::engr_dir(&root).join("format.json"),
            &bootstrap.replace("REPLACE", &proposed.challenge),
        );

        let error = engr::confirm(&root, &format!("CONFIRM {}", proposed.challenge))
            .expect_err(&format!("{what}: this is not this transaction's barrier"));
        assert!(
            error.message.contains(expected),
            "{what}: {}",
            error.message
        );
        assert!(
            !store::version_path(&root).exists(),
            "{what}: nothing was published"
        );
    }
}

/// The ignore line a migration publishes is written to the workspace, not
/// through a link out of it.
///
/// `.engr/.gitignore` is neither an Object nor a history, so the preflight does
/// not pin it and the source-unmoved comparison never looks at it — which is
/// exactly how a link there reached a *confirmed* publication step, where the
/// line that keeps live challenge codes out of git was written through it into a
/// file outside the workspace entirely.
///
/// The refusal happens inside publication, so the transaction is left
/// unactivated rather than half-lying: remove the link, answer again, and it
/// completes.
#[test]
#[cfg(unix)]
fn the_published_ignore_line_is_not_written_through_a_link() {
    let (_temp, root) = released();
    let outside = TempDir::new().expect("outside");
    let elsewhere = outside.path().join("captured-gitignore");
    let ignore = store::engr_dir(&root).join(".gitignore");
    // A redirection of a file that is really there, which is how one arrives.
    std::fs::rename(&ignore, &elsewhere).expect("move the predecessor's own out");
    std::os::unix::fs::symlink(&elsewhere, &ignore).expect("symlink");
    let before = read(&elsewhere);

    let proposed = engr::migration::prepare(&root).expect("the plan does not pin .gitignore");
    let error = engr::confirm(&root, &format!("CONFIRM {}", proposed.challenge))
        .expect_err("publishing through a link is refused");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(
        error.message.contains("link to somewhere else"),
        "{}",
        error.message
    );
    assert_eq!(
        read(&elsewhere),
        before,
        "nothing outside the workspace was written"
    );
    assert!(
        !store::version_path(&root).exists(),
        "and the workspace is not activated while a published step is refused"
    );

    // Remove the redirection and the same confirmation finishes.
    std::fs::remove_file(&ignore).expect("remove the link");
    std::fs::rename(&elsewhere, &ignore).expect("put the predecessor's own back");
    match engr::confirm(&root, &format!("CONFIRM {}", proposed.challenge)).expect("resume") {
        engr::Confirmed::Migration(report) => assert_eq!(report.objects.len(), 4),
        engr::Confirmed::Object(_) => panic!("a migration subject"),
    }
    assert!(read(&ignore).lines().any(|line| line.trim() == "/local/"));
    assert_eq!(
        store::validate_format(&root).expect("current"),
        WorkspaceFormat::Current
    );
}
