//! What the command line promises the outside world.

use engr::{
    model::{Action, Confirmation, Content, Event, Object, Payload, EVENT_FORMAT},
    store,
};
use serde_json::Value;
use std::fs::OpenOptions;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::Duration;
use tempfile::TempDir;

fn run_engr(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_engr"))
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .expect("run engr")
}

fn prepare(root: &Path, args: &[&str]) -> Value {
    let mut args = args.to_vec();
    args.push("--json");
    let output = run_engr(root, &args);
    assert!(
        output.status.success(),
        "prepare failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("prepare prints a candidate JSON document")
}

fn confirm(root: &Path, candidate: &Value) {
    let challenge = candidate["challenge"]
        .as_str()
        .expect("candidate challenge");
    let response = format!("CONFIRM {challenge}");
    let output = run_engr(root, &["confirm", &response]);
    assert!(
        output.status.success(),
        "confirm failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn dirty_source_requires_an_explicit_repository_basis_choice() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    git(root, &["init", "-q"]);
    std::fs::write(root.join("source.txt"), "committed\n").expect("source");
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
            "baseline",
        ],
    );

    let created = prepare(root, &["prepare", "--new", "--text", "basis choices"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id");
    std::fs::write(root.join("source.txt"), "dirty\n").expect("source");

    let rejected = run_engr(
        root,
        &["prepare", "--object", id, "--add", "--text", "assertion"],
    );
    assert_eq!(rejected.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("--no-based-on"));

    let explicit = prepare(
        root,
        &[
            "prepare",
            "--object",
            id,
            "--add",
            "--text",
            "external assertion",
            "--no-based-on",
        ],
    );
    assert!(
        explicit.get("based_on").is_none(),
        "no basis is represented by an absent field"
    );
}

#[test]
fn revision_candidate_renders_a_contextual_unified_diff() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");
    let created = prepare(root, &["prepare", "--new", "--text", "diffs"]);
    confirm(root, &created);
    let id = created["object"].as_str().expect("object id");
    let old = (1..=20)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let added = prepare(
        root,
        &[
            "prepare",
            "--object",
            id,
            "--add",
            "--text",
            &old,
            "--no-based-on",
        ],
    );
    confirm(root, &added);
    let revised = old.replace("line 10", "line ten");
    let output = run_engr(
        root,
        &[
            "prepare",
            "--object",
            id,
            "--revise",
            "1",
            "--text",
            &revised,
            "--no-based-on",
        ],
    );
    assert!(output.status.success());
    let rendered = String::from_utf8_lossy(&output.stdout);
    assert!(rendered.contains("@@"));
    assert!(rendered.contains("-line 10"));
    assert!(rendered.contains("+line ten"));
    assert!(
        !rendered.contains(" line 1\n"),
        "distant context was not omitted"
    );
}

fn event_workspace() -> (TempDir, std::path::PathBuf, Event) {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path().to_path_buf();
    store::init(&root).expect("init");
    let object = Object::new(engr::model::new_id(), "event validation".to_owned());
    let id = object.id.clone();
    store::save_object(&root, &object).expect("save object");
    let payload = Payload {
        action: Action::SectionAdded,
        object: id,
        content: Content {
            text: "event wording".to_owned(),
            based_on: None,
            refs: Vec::new(),
        },
    };
    let payload_sha256 = payload.sha256().expect("payload hash");
    let event = Event {
        format: EVENT_FORMAT.to_owned(),
        version: engr::FORMAT_VERSION,
        event_id: engr::model::new_id(),
        rev: 1,
        time: "2026-08-13T00:00:00Z".to_owned(),
        payload,
        confirmation: Confirmation {
            challenge: "234567".to_owned(),
            payload_sha256,
        },
    };
    (workspace, root, event)
}

fn assert_event_is_rejected(root: &Path, event: Event) {
    let id = event.payload.object.clone();
    store::append_event(root, &event).expect("write event");
    let output = run_engr(root, &["verify", &id]);
    assert_eq!(
        output.status.code(),
        Some(engr::EXIT_SCHEMA),
        "malformed events must be rejected as stored-data errors: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_event_to(root: &Path, id: &str, event: &Event) {
    let line = serde_json::to_string(event).expect("serialize event");
    std::fs::write(store::events_path(root, id), format!("{line}\n")).expect("write event");
}

/// The installers echo this line back as proof the binary they placed runs, and
/// `latest` never changes — so the part in parentheses is the only thing that
/// says which build it is. The shape is pinned and the contents are not, because
/// `unknown` is the honest answer when there is no git to ask.
#[test]
fn the_version_names_the_commit_it_was_built_from() {
    let output = Command::new(env!("CARGO_BIN_EXE_engr"))
        .arg("--version")
        .output()
        .expect("run engr --version");
    assert!(output.status.success(), "--version did not exit cleanly");
    let line = String::from_utf8(output.stdout).expect("utf8");
    let line = line.trim();
    let commit = line
        .strip_prefix("engr latest (")
        .and_then(|rest| rest.strip_suffix(')'))
        .unwrap_or_else(|| panic!("expected `engr latest (<commit>)`, got {line:?}"));
    assert!(!commit.is_empty(), "nothing was stamped in: {line:?}");
}

#[test]
fn stale_listing_includes_closed_objects_whose_basis_moved() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "tests@example.com"]);
    git(root, &["config", "user.name", "engr tests"]);
    std::fs::write(root.join("basis.txt"), "initial basis\n").expect("write basis");
    git(root, &["add", "basis.txt"]);
    git(root, &["commit", "-qm", "basis"]);

    let init = run_engr(root, &["init"]);
    assert!(
        init.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    let created = prepare(root, &["prepare", "--new", "--text", "closed record"]);
    let object = created["object"].as_str().expect("object id").to_owned();
    confirm(root, &created);

    let added = prepare(
        root,
        &[
            "prepare",
            "--add",
            "--object",
            &object,
            "--text",
            "basis wording",
        ],
    );
    confirm(root, &added);

    let closed = prepare(root, &["prepare", "--close", "--object", &object]);
    confirm(root, &closed);
    git(root, &["add", ".engr"]);
    git(root, &["commit", "-qm", "record closed object"]);

    std::fs::write(root.join("basis.txt"), "changed basis\n").expect("change basis");
    git(root, &["add", "basis.txt"]);
    git(root, &["commit", "-qm", "basis moved"]);

    let output = run_engr(root, &["ls", "--stale"]);
    assert!(
        output.status.success(),
        "ls --stale failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let listing = String::from_utf8(output.stdout).expect("utf8 listing");
    assert!(
        listing.contains("closed"),
        "a closed object whose basis moved must be surfaced by `ls --stale`; got {listing:?}"
    );
}

/// `confirm` asks for the object file to be committed. Counting that commit
/// made every section stale the moment its own record was saved, so the tool's
/// instructions broke the tool's signal and the only way back to zero was to
/// re-confirm every section — until the next commit.
#[test]
fn committing_the_record_does_not_move_its_own_basis() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "tests@example.com"]);
    git(root, &["config", "user.name", "engr tests"]);
    std::fs::create_dir(root.join("src")).expect("mkdir src");
    std::fs::write(root.join("src/audit.go"), "package audit\n").expect("write source");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "the basis"]);

    let init = run_engr(root, &["init"]);
    assert!(init.status.success());

    let created = prepare(root, &["prepare", "--new", "--text", "reason codes"]);
    let object = created["object"].as_str().expect("object id").to_owned();
    confirm(root, &created);
    let added = prepare(
        root,
        &[
            "prepare",
            "--add",
            "--object",
            &object,
            "--text",
            "Ruling: expose the reason code.",
        ],
    );
    confirm(root, &added);

    // Exactly what `confirm` tells the user to do next.
    git(root, &["add", ".engr"]);
    git(root, &["commit", "-qm", "record the ruling"]);

    let output = run_engr(root, &["show", &object]);
    let shown = String::from_utf8(output.stdout).expect("utf8");
    assert!(
        !shown.contains("basis moved"),
        "saving the record moved the record's own basis: {shown}"
    );
    assert!(shown.contains("1 ok"), "{shown}");

    // A real change to the code the ruling was made against still counts.
    std::fs::write(root.join("src/audit.go"), "package audit\n// reworked\n").expect("edit source");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "rework the audit package"]);

    let output = run_engr(root, &["show", &object]);
    let shown = String::from_utf8(output.stdout).expect("utf8");
    assert!(
        shown.contains("basis moved"),
        "a change outside the record must still be reported: {shown}"
    );
    assert!(
        shown.contains("1 commits and 1 files"),
        "the two halves of the sentence have to be filtered the same way: {shown}"
    );
}

#[test]
fn an_object_file_must_match_its_embedded_id() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    store::init(root).expect("init");

    let object = Object::new(engr::model::new_id(), "mismatched storage key".to_owned());
    store::write_json(&store::object_path(root, "wrong"), &object).expect("write object");

    let output = run_engr(root, &["show", "wrong"]);
    assert_eq!(
        output.status.code(),
        Some(engr::EXIT_SCHEMA),
        "a filename/id mismatch is malformed stored data, not a usable object: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn show_waits_for_the_workspace_writer_lock_before_reconciling() {
    use fs2::FileExt;

    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    let init = run_engr(root, &["init"]);
    assert!(init.status.success(), "init");
    let created = prepare(root, &["prepare", "--new", "--text", "locked read"]);
    let object = created["object"].as_str().expect("object id").to_owned();
    confirm(root, &created);

    let payload = Payload {
        action: Action::SectionAdded,
        object: object.clone(),
        content: Content {
            text: "reconcile under lock".to_owned(),
            based_on: None,
            refs: Vec::new(),
        },
    };
    let payload_sha256 = payload.sha256().expect("payload hash");
    store::append_event(
        root,
        &Event {
            format: EVENT_FORMAT.to_owned(),
            version: engr::FORMAT_VERSION,
            event_id: engr::model::new_id(),
            rev: 2,
            time: "2026-08-13T00:00:00Z".to_owned(),
            payload,
            confirmation: Confirmation {
                challenge: "234567".to_owned(),
                payload_sha256,
            },
        },
    )
    .expect("append unprojected event");

    let lock_path = store::engr_dir(root).join("lock");
    let lock = OpenOptions::new()
        .write(true)
        .open(lock_path)
        .expect("open workspace lock");
    lock.lock_exclusive().expect("hold workspace lock");

    let mut child = Command::new(env!("CARGO_BIN_EXE_engr"))
        .arg("--root")
        .arg(root)
        .args(["show", &object])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start show");
    std::thread::sleep(Duration::from_millis(200));
    let while_locked = child.try_wait().expect("inspect show process");
    FileExt::unlock(&lock).expect("release workspace lock");

    assert!(
        while_locked.is_none(),
        "show must not reconcile and write while another writer holds the lock"
    );
    let output = child.wait_with_output().expect("wait for show");
    assert!(
        output.status.success(),
        "show failed after the lock released: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        store::load_object(root, &object)
            .expect("load reconciled object")
            .rev,
        2,
        "show must have reconciled the pending event after acquiring the lock"
    );
}

#[test]
fn unsupported_event_versions_are_rejected() {
    let (_workspace, root, mut event) = event_workspace();
    event.version += 1;
    assert_event_is_rejected(&root, event);
}

#[test]
fn events_must_belong_to_their_object_file() {
    let (_workspace, root, mut event) = event_workspace();
    let path_id = event.payload.object.clone();
    event.payload.object = engr::model::new_id();
    event.confirmation.payload_sha256 = event.payload.sha256().expect("payload hash");
    write_event_to(&root, &path_id, &event);
    let output = run_engr(&root, &["verify", &path_id]);
    assert_eq!(output.status.code(), Some(engr::EXIT_SCHEMA));
}

#[test]
fn invalid_event_payloads_are_rejected() {
    let (_workspace, root, mut event) = event_workspace();
    event.payload.content.text.clear();
    event.confirmation.payload_sha256 = event.payload.sha256().expect("payload hash");
    assert_event_is_rejected(&root, event);
}

#[test]
fn event_confirmation_hashes_are_verified() {
    let (_workspace, root, mut event) = event_workspace();
    event.confirmation.payload_sha256 = "0".repeat(64);
    assert_event_is_rejected(&root, event);
}

#[test]
fn duplicate_event_revisions_are_rejected() {
    let (_workspace, root, event) = event_workspace();
    store::append_event(&root, &event).expect("write first event");
    store::append_event(&root, &event).expect("write duplicate event");
    let output = run_engr(&root, &["verify", &event.payload.object]);
    assert_eq!(output.status.code(), Some(engr::EXIT_SCHEMA));
}

#[test]
fn event_revisions_must_be_contiguous_within_history() {
    let (_workspace, root, event) = event_workspace();
    store::append_event(&root, &event).expect("write first event");
    let mut skipped = event.clone();
    skipped.rev += 2;
    store::append_event(&root, &skipped).expect("write skipped event");
    let output = run_engr(&root, &["verify", &event.payload.object]);
    assert_eq!(output.status.code(), Some(engr::EXIT_SCHEMA));
}

/// Reversed deliberately. This test used to assert the opposite — that any
/// newer commit, empty or not, means the basis moved — on the reasoning that
/// HEAD moving is not something the tool should second-guess. Excluding the
/// record's own files from the comparison makes that untenable: the same rule
/// that stops `commit .engr` from moving a section's basis also stops a commit
/// that changes nothing at all from moving it. That is the right answer to the
/// question the signal is actually asked — did what I decided against change?
/// — and an empty commit is the clearest case of no.
#[test]
fn an_empty_commit_is_not_the_basis_moving() {
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "tests@example.com"]);
    git(root, &["config", "user.name", "engr tests"]);
    std::fs::write(root.join("basis.txt"), "initial basis\n").expect("write basis");
    git(root, &["add", "basis.txt"]);
    git(root, &["commit", "-qm", "basis"]);

    let init = run_engr(root, &["init"]);
    assert!(init.status.success(), "init");
    let created = prepare(root, &["prepare", "--new", "--text", "closed record"]);
    let object = created["object"].as_str().expect("object id").to_owned();
    confirm(root, &created);
    let added = prepare(
        root,
        &[
            "prepare",
            "--add",
            "--object",
            &object,
            "--text",
            "basis wording",
        ],
    );
    confirm(root, &added);
    let closed = prepare(root, &["prepare", "--close", "--object", &object]);
    confirm(root, &closed);
    git(root, &["add", ".engr"]);
    git(root, &["commit", "-qm", "record closed object"]);
    git(
        root,
        &["commit", "--allow-empty", "-qm", "no source changes"],
    );

    let output = run_engr(root, &["ls", "--stale"]);
    assert!(output.status.success(), "ls --stale");
    let listing = String::from_utf8(output.stdout).expect("utf8 listing");
    assert_eq!(
        listing, "all ok\n",
        "neither saving the record nor an empty commit changed what the ruling was made against"
    );

    // The closed object still surfaces the moment something real moves, which
    // is the guarantee this test was written to protect.
    std::fs::write(root.join("basis.txt"), "changed basis\n").expect("change basis");
    git(root, &["add", "basis.txt"]);
    git(root, &["commit", "-qm", "basis moved"]);
    let output = run_engr(root, &["ls", "--stale"]);
    let listing = String::from_utf8(output.stdout).expect("utf8 listing");
    assert!(
        listing.contains("closed"),
        "a closed object whose basis really moved must still surface; got {listing:?}"
    );
}

/// Installed from a release archive there is no checkout, so the document that
/// says what the tool guarantees would otherwise not be on the machine the tool
/// is on. It also has to work before `init`: the protocol is what someone reads
/// to decide whether to adopt engr at all.
#[test]
fn the_protocol_prints_without_a_workspace_and_byte_for_byte() {
    let empty = TempDir::new().expect("temp dir");
    let output = Command::new(env!("CARGO_BIN_EXE_engr"))
        .current_dir(empty.path())
        .arg("protocol")
        .output()
        .expect("run engr");
    assert!(
        output.status.success(),
        "engr protocol must need no workspace: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Byte for byte, so `engr protocol > PROTOCOL.md` reproduces the document
    // rather than something one newline away from it.
    let printed = String::from_utf8(output.stdout).expect("utf-8");
    assert_eq!(printed, engr::PROTOCOL);
    assert!(printed.starts_with("# engr protocol v0"));
}
