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

/// Every relative path under `.engr`, with the digest of its bytes.
///
/// `local/` is left out: it is this machine's alone — the writer lock, a live
/// challenge whose filename is its code, and a resumable plan — and taking the
/// lock is what any command does before it decides whether it may do anything
/// at all. Counting it would make "preflight wrote nothing" fail on a migration
/// that wrote nothing.
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
        proof::section_target(MODEL, 1),
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
        vec!["based_on", "content", "refs", "relations", "role", "text"],
        "exactly what the whole-content seal covered"
    );
    assert!(
        !selected.contains(&"admission") && !selected.contains(&"header"),
        "the original Ref never attested either, so the migrated one does not claim to"
    );

    // And it verifies against the predecessor commit it pins, which means the
    // conversion the read path performs is the one the migration performed.
    let target = ops::effective(&root, MODEL).expect("target");
    assert_eq!(
        dependency::evaluate(&root, &target, reference).expect("evaluate"),
        dependency::Dependency::Unchanged,
        "a migrated reference is not born stale"
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
    let steps: Vec<(&str, Damage)> = vec![
        ("nothing published", |_root| {}),
        ("the first stream landed", |root| {
            let path = store::events_path(root, AUTHORITY);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            write(&path, "a partial write\n");
        }),
        ("the first Object was overwritten", |root| {
            write(
                &store::object_path(root, AUTHORITY),
                "no longer a predecessor\n",
            );
        }),
        ("every Object was overwritten", |root| {
            for id in [AUTHORITY, MODEL, PROVENANCE_OBJECT, PROJECTION] {
                write(&store::object_path(root, id), "no longer a predecessor\n");
            }
        }),
        ("the predecessor's own directories are gone", |root| {
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
        engr::migration::stage_destination_only(&root, &proposed.challenge).expect("stage");
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
    engr::migration::stage_destination_only(&root, &proposed.challenge).expect("stage");

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
