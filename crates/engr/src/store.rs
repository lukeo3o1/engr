//! Filesystem layout, locking, and atomic writes.
//!
//! ```text
//! .engr/
//!   VERSION                          the workspace generation, "1"
//!   .gitignore                       at least /local/
//!   objects/<uuid>.json              the authority
//!   eventstore/objects/<uuid>.jsonl  append-only admitted history
//!   backlog/<uuid>.json              unresolved staging, admitted by nobody
//!   work/objects/<uuid>.json         execution memory, owned by a subject
//!   work/backlog/<uuid>.json         the same, for a backlog subject
//!   collections/<id>.json            planning metadata, admitted by nobody
//!   rules/*.md                       project review policy
//!   local/lock                       one writer at a time
//!   local/challenges/<CODE>.json     awaiting a human
//! ```
//!
//! `local/` is the one non-Git-tracked directory, and everything that must not
//! travel with a `git add -A` lives inside it — so the workspace has one ignore
//! line instead of a list that grows every time something local is added.
//!
//! There is no public writer here, and that is a contract rather than an
//! oversight. A raw serializer or an Object save that validates shape is not an
//! admission boundary: a self-consistent, correctly resealed Object says
//! nothing about whether any Event, Human Gate or Rule Review produced it, and
//! holding the writer lock closes a race rather than that question. So the
//! primitives are crate-private, and a consumer reaches durable state only
//! through a domain API that owns its own authority contract.
//!
//! ```compile_fail
//! # use std::path::Path;
//! fn publish(root: &Path, object: &engr::model::Object) {
//!     engr::store::save_object(root, object).expect("no such public writer");
//! }
//! ```
//!
//! ```compile_fail
//! # use std::path::Path;
//! fn publish(root: &Path, object: &engr::model::Object) {
//!     let path = engr::store::object_path(root, &object.id);
//!     engr::store::write_json(&path, object).expect("no such public writer");
//! }
//! ```
//!
//! The durable Event append is closed for a further reason, and not for
//! visibility's own sake. Event provenance is deliberately minimal — the
//! review's outcome and digest, and none of the transient inputs the decision
//! was made from. The attempt is the one that matters: every mutation carries
//! one Agent-attested attempt, each applicable Rule judges it against its own
//! ceiling, and past any ceiling autonomous admission stops. None of that
//! survives into the record, so re-deriving what the record *does* carry can
//! never ask the question, and a public raw append would be a second Agent
//! admission API holding strictly less state than the gate. [`check_appendable`]
//! is the read-only half.
//!
//! ```compile_fail
//! # use std::path::Path;
//! fn admit(root: &Path, event: &engr::model::Event) {
//!     engr::store::append_event(root, event).expect("no such public writer");
//! }
//! ```

use crate::model::{replay_recoverable_tail, Action, Event, Object};
use crate::{ensure, tool_error, Error, Result, EXIT_NOT_FOUND, EXIT_SCHEMA, EXIT_USAGE};
use fs2::FileExt;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

pub const DIR: &str = ".engr";

pub fn engr_dir(root: &Path) -> PathBuf {
    root.join(DIR)
}
pub fn objects_dir(root: &Path) -> PathBuf {
    engr_dir(root).join("objects")
}
pub fn eventstore_dir(root: &Path) -> PathBuf {
    engr_dir(root).join("eventstore").join("objects")
}
/// Non-Git-tracked, non-authoritative state. One directory, one ignore line.
pub fn local_dir(root: &Path) -> PathBuf {
    engr_dir(root).join("local")
}
pub fn challenges_dir(root: &Path) -> PathBuf {
    local_dir(root).join("challenges")
}
pub fn lock_path(root: &Path) -> PathBuf {
    local_dir(root).join("lock")
}
pub fn version_path(root: &Path) -> PathBuf {
    engr_dir(root).join("VERSION")
}
pub fn object_path(root: &Path, id: &str) -> PathBuf {
    objects_dir(root).join(format!("{id}.json"))
}
pub fn events_path(root: &Path, id: &str) -> PathBuf {
    eventstore_dir(root).join(format!("{id}.jsonl"))
}
pub fn challenge_path(root: &Path, challenge: &str) -> Result<PathBuf> {
    ensure!(
        crate::confirmation::valid_challenge(challenge),
        EXIT_USAGE,
        "challenge code {challenge:?} must be six characters from 23456789ABCDEFGHJKLMNPQRSTUVWXYZ"
    );
    Ok(challenges_dir(root).join(format!("{challenge}.json")))
}

/// Where the last size refusal is written down.
///
/// Under `local/`, which is the whole of what a workspace does not commit — so
/// this cannot travel out of the machine that made it, and it needs no ignore
/// line of its own. [`crate::gate::pending_codes`] reads `challenges/` by
/// filename and keeps only valid challenge codes, so this name is invisible to
/// it.
pub fn refusal_path(root: &Path) -> PathBuf {
    local_dir(root).join("refused-oversize.json")
}

/// Walk up from `start` looking for a workspace, so the tool works from any
/// subdirectory the way git does.
pub fn find_root(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        ensure!(
            engr_dir(path).is_dir(),
            EXIT_NOT_FOUND,
            "no {DIR} workspace at {}",
            path.display()
        );
        return Ok(path.to_path_buf());
    }
    let current =
        std::env::current_dir().map_err(|error| tool_error("current directory", error))?;
    let mut cursor = current.as_path();
    loop {
        if engr_dir(cursor).is_dir() {
            return Ok(cursor.to_path_buf());
        }
        match cursor.parent() {
            Some(parent) => cursor = parent,
            None => {
                return Err(Error::new(
                    EXIT_NOT_FOUND,
                    format!(
                        "no {DIR} workspace here or in any parent; run `engr init` at the repository root"
                    ),
                ))
            }
        }
    }
}

/// Which generation wrote this workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceFormat {
    /// The one supported predecessor: the officially released `latest`
    /// workspace, which bootstraps `{"format":"engr-workspace","version":1}`.
    ///
    /// Well formed and read-only until `engr migrate`, which is a different fact
    /// from a workspace this build cannot read at all — and the reader's next
    /// question has a different answer in each case.
    Predecessor,
    Current,
}

/// Written by [`init`], because `git add -A` is the normal way people stage a
/// workspace and one directory in it must never travel.
///
/// A Challenge's filename *is* its code, so committing a live one hands the code
/// to everyone with repository access — the gate assumes it goes to one human
/// and comes back. Telling people not to do that in a README does not stop `-A`.
///
/// One line, because there is one local directory. Every earlier layout grew
/// this list each time something local was added, and every workspace created
/// before that addition kept the shorter list.
const GITIGNORE: &str = "\
# The record lives in objects/ — commit that; it is where earlier wording is
# recovered from. eventstore/ is safe to commit too: any challenge codes in it
# have already been spent, and a spent code resolves to nothing.
#
# local/ is this machine's alone: the writer lock, live challenges whose
# filenames are their codes, and a resumable migration plan.
/local/
";

pub fn init(root: &Path) -> Result<PathBuf> {
    let dir = engr_dir(root);
    ensure!(
        !dir.exists(),
        EXIT_SCHEMA,
        "{} already exists",
        dir.display()
    );
    let mut layout = vec![
        objects_dir(root),
        eventstore_dir(root),
        challenges_dir(root),
        crate::backlog::dir(root),
        crate::collection::dir(root),
        crate::rules::dir(root),
    ];
    // Work has one directory per subject kind, and asks for them by list so a
    // third kind could not be created here and forgotten everywhere else.
    layout.extend(crate::work::dirs(root));
    for path in layout {
        fs::create_dir_all(&path).map_err(|error| tool_error(path.display(), error))?;
    }
    write_text(&version_path(root), crate::WORKSPACE_VERSION_FILE)?;
    let ignore = dir.join(".gitignore");
    fs::write(&ignore, GITIGNORE).map_err(|error| tool_error(ignore.display(), error))?;
    Ok(dir)
}

pub fn validate_format(root: &Path) -> Result<WorkspaceFormat> {
    let migration = crate::migration::stage_dir(root);
    ensure!(
        !migration.exists(),
        EXIT_SCHEMA,
        "{} marks an incomplete coordinated migration; run `engr migrate` to resume it",
        migration.display()
    );
    if version_path(root).exists() {
        read_generation(root)?;
        return Ok(WorkspaceFormat::Current);
    }
    ensure!(
        predecessor_bootstrap(root)?.is_some(),
        EXIT_SCHEMA,
        "{} has no VERSION and is not the released predecessor workspace this engr migrates",
        engr_dir(root).display()
    );
    Ok(WorkspaceFormat::Predecessor)
}

/// Read `.engr/VERSION`, held to its one spelling.
///
/// Bytes rather than a parsed integer. The marker exists so a build refuses what
/// it cannot read, and a parser that accepts `" 1 "`, `"01"` and `"1"` alike has
/// already conceded that a workspace may say the same thing several ways —
/// which is what makes two implementations able to disagree while each believes
/// it agrees.
pub(crate) fn read_generation(root: &Path) -> Result<u32> {
    let path = version_path(root);
    let text = read_text(&path)?;
    ensure!(
        text == crate::WORKSPACE_VERSION_FILE,
        EXIT_SCHEMA,
        "{}",
        unsupported_generation(&path, &text)
    );
    Ok(crate::WORKSPACE_GENERATION)
}

/// Why a `VERSION` this build does not write is refused, and which refusal it is.
///
/// A generation *above* this build's is a workspace from a newer engr: nothing
/// here will ever read it, and the answer is a newer binary. Anything else is a
/// marker this build has no route from, which is a different fact and a
/// different conversation. Neither is "run `engr migrate`", so neither is
/// allowed to sound like it.
fn unsupported_generation(path: &Path, text: &str) -> String {
    let declared = text.trim_end_matches('\n');
    if let Ok(generation) = declared.parse::<u32>() {
        if generation > crate::WORKSPACE_GENERATION {
            return format!(
                "{}: workspace generation {generation} was written by a newer engr; this build ({}) writes generation {} and cannot read it",
                path.display(),
                crate::IMPLEMENTATION_VERSION,
                crate::WORKSPACE_GENERATION
            );
        }
    }
    format!(
        "{}: {declared:?} is not a workspace generation engr {} recognizes; this build writes generation {}",
        path.display(),
        crate::IMPLEMENTATION_VERSION,
        crate::WORKSPACE_GENERATION
    )
}

/// The predecessor bootstrap, when this workspace is one.
///
/// Returns `None` for a directory that is not the released predecessor rather
/// than failing, so [`validate_format`] can say the one useful thing about a
/// `.engr` that is neither generation.
pub(crate) fn predecessor_bootstrap(root: &Path) -> Result<Option<u32>> {
    let path = engr_dir(root).join("format.json");
    if !path.exists() {
        return Ok(None);
    }
    let text = read_text(&path)?;
    let format: crate::predecessor::Format = serde_json::from_str(&text)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))?;
    ensure!(
        format.format == crate::predecessor::WORKSPACE_FORMAT,
        EXIT_SCHEMA,
        "{}: not an engr workspace",
        path.display()
    );
    // Named exactly, and refused by name. `format.json` said version 1, 2 and 3
    // across a long unreleased development window whose builds wrote domains
    // and shapes the release never had, and the *published* version 1 is the
    // only one #66 defines a route from. Reading any other under its rules would
    // be the silent reinterpretation the generation marker exists to prevent.
    ensure!(
        format.version == crate::PREDECESSOR_WORKSPACE_VERSION,
        EXIT_SCHEMA,
        "{}: workspace version {} is not a generation engr {} migrates; the supported predecessor is the released version {} workspace ({})",
        path.display(),
        format.version,
        crate::IMPLEMENTATION_VERSION,
        crate::PREDECESSOR_WORKSPACE_VERSION,
        crate::PREDECESSOR_RELEASE_COMMIT
    );
    Ok(Some(format.version))
}

pub fn require_current(root: &Path) -> Result<()> {
    match validate_format(root)? {
        WorkspaceFormat::Current => Ok(()),
        WorkspaceFormat::Predecessor => Err(Error::new(
            EXIT_SCHEMA,
            format!(
                "the released predecessor workspace is read-only here; this engr writes generation {}. Run `engr migrate` before mutation",
                crate::WORKSPACE_GENERATION
            ),
        )),
    }
}

/// Decode a current Object, held to the generation's exact persisted shape.
///
/// The members are enumerated ahead of decoding rather than left to serde. Every
/// optional member of the model has a default, so a file carrying a member this
/// generation never defined would deserialize cleanly with the unknown one
/// ignored, and everything downstream would treat the default as something the
/// file said.
pub(crate) fn decode_object(path: &Path, id: &str, value: serde_json::Value) -> Result<Object> {
    crate::proof::stored_within_safe_integers(&value, &path.display().to_string())?;
    check_current_object_shape(path, &value)?;
    let object: Object = serde_json::from_value(value)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))?;
    object.validate()?;
    ensure!(
        object.id == id,
        EXIT_SCHEMA,
        "{}: object id {:?} does not match its filename",
        path.display(),
        object.id
    );
    Ok(object)
}

/// Required members, then the ones that may be omitted.
///
/// Not exact equality. The canonical omission rule means an absent optional, an
/// empty array and an empty object are all simply not written — so "these exact
/// keys" would refuse every Object that legitimately carries none of them. What
/// is still exact is the *vocabulary*: a member outside both lists is not a
/// member of this generation, whatever else about the file reads correctly.
fn shape_members(
    path: &Path,
    what: &str,
    value: &serde_json::Map<String, serde_json::Value>,
    required: &[&str],
    optional: &[&str],
) -> Result<()> {
    for member in required {
        ensure!(
            value.contains_key(*member),
            EXIT_SCHEMA,
            "{}: a {what} carries {member}",
            path.display()
        );
    }
    for member in value.keys() {
        ensure!(
            required.contains(&member.as_str()) || optional.contains(&member.as_str()),
            EXIT_SCHEMA,
            "{}: {member:?} is not a member of a {what}",
            path.display()
        );
    }
    // An omissible member written out as `null` is the other spelling the rule
    // forbids: two files saying the same thing, sealing differently. Refused
    // here rather than by the decoder, which would read it as absent.
    for member in optional.iter().copied() {
        ensure!(
            !value.get(member).is_some_and(serde_json::Value::is_null),
            EXIT_SCHEMA,
            "{}: {what} {member} is absent by omission, never by null",
            path.display()
        );
    }
    Ok(())
}

const OBJECT_REQUIRED: &[&str] = &["id", "title", "state", "rev", "next_section_id", "digest"];
const OBJECT_OPTIONAL: &[&str] = &["type", "sections"];
const SECTION_REQUIRED: &[&str] = &["id", "admitted", "text", "digest"];
const SECTION_OPTIONAL: &[&str] = &["header", "role", "content", "based_on", "refs", "relations"];

fn check_current_object_shape(path: &Path, value: &serde_json::Value) -> Result<()> {
    let object = value.as_object().ok_or_else(|| {
        Error::new(
            EXIT_SCHEMA,
            format!("{}: object must be a JSON object", path.display()),
        )
    })?;
    shape_members(path, "Object", object, OBJECT_REQUIRED, OBJECT_OPTIONAL)?;
    // An empty array is the same value as an absent one, and this generation
    // writes it one way. Checked here because the decoder cannot tell them
    // apart afterwards, and the two seal differently.
    if let Some(sections) = object.get("sections") {
        let sections = sections.as_array().ok_or_else(|| {
            Error::new(
                EXIT_SCHEMA,
                format!("{}: sections must be an array", path.display()),
            )
        })?;
        ensure!(
            !sections.is_empty(),
            EXIT_SCHEMA,
            "{}: an Object with no Sections omits the member rather than writing an empty list",
            path.display()
        );
        let mut previous = None;
        for section in sections {
            let section = section.as_object().ok_or_else(|| {
                Error::new(
                    EXIT_SCHEMA,
                    format!("{}: a section must be a JSON object", path.display()),
                )
            })?;
            shape_members(path, "Section", section, SECTION_REQUIRED, SECTION_OPTIONAL)?;
            let id = section
                .get("id")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| {
                    Error::new(
                        EXIT_SCHEMA,
                        format!("{}: section id must be an integer", path.display()),
                    )
                })?;
            ensure!(
                previous.is_none() || previous.is_some_and(|previous| previous < id),
                EXIT_SCHEMA,
                "{}: current Sections are stored in increasing id order",
                path.display()
            );
            for field in ["content", "refs", "relations"] {
                let Some(items) = section.get(field) else {
                    continue;
                };
                let items = items.as_array().ok_or_else(|| {
                    Error::new(
                        EXIT_SCHEMA,
                        format!("{}: §{id} {field} must be an array", path.display()),
                    )
                })?;
                ensure!(
                    !items.is_empty(),
                    EXIT_SCHEMA,
                    "{}: §{id} omits {field} rather than writing an empty list",
                    path.display()
                );
                // Set-valued arrays have one persisted order too, and it is the
                // shared one: JCS each element, then order by those bytes. The
                // canonical-bytes check cannot see this — JCS fixes member order
                // inside an object and leaves arrays exactly as written — so
                // without it a stored set can be reordered, keep a valid seal,
                // and load: two accepted encodings for one value. `content` is
                // ordered rather than a set, so only its emptiness is checked.
                if field != "content" {
                    let mut canonical = items.clone();
                    crate::proof::canonical_set(&mut canonical, field)?;
                    ensure!(
                        canonical == *items,
                        EXIT_SCHEMA,
                        "{}: §{id} {field} must be in canonical set order",
                        path.display()
                    );
                }
            }
            previous = Some(id);
        }
    }
    Ok(())
}
/// Publish the workspace generation marker.
///
/// The last write of a migration, deliberately: while it is absent the
/// workspace is still the predecessor, and a crash before it leaves something
/// `validate_format` can name rather than a workspace that claims a generation
/// it has not finished becoming.
pub(crate) fn write_generation(root: &Path) -> Result<()> {
    write_text(&version_path(root), crate::WORKSPACE_VERSION_FILE)
}

/// Hold the workspace write lock for the duration of `body`.
pub fn with_lock<T>(root: &Path, body: impl FnOnce() -> Result<T>) -> Result<T> {
    let path = lock_path(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| tool_error(parent.display(), error))?;
    }
    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .map_err(|error| tool_error(path.display(), error))?;
    FileExt::lock_exclusive(&file).map_err(|error| tool_error("workspace lock", error))?;
    let outcome = body();
    let _ = FileExt::unlock(&file);
    outcome
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text = read_text(path)?;
    serde_json::from_str(&text)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))
}

fn read_text(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Error::new(EXIT_NOT_FOUND, format!("{}: not found", path.display()))
        } else {
            tool_error(path.display(), error)
        }
    })
}

/// Current resources have the one spelling their writer emits. JCS alone fixes
/// JSON member order and number text, but it cannot distinguish an omitted
/// optional field from an explicit `null`, or an omitted false from `false`.
pub(crate) fn check_current_resource_shape<T: Serialize>(
    path: &Path,
    text: &str,
    resource: &T,
) -> Result<()> {
    ensure!(
        text == crate::proof::canonical_bytes(resource, &path.display().to_string())?,
        EXIT_SCHEMA,
        "{}: a current resource is not in the exact shape its writer emits",
        path.display()
    );
    Ok(())
}

/// The bytes of a current-generation resource against the one serialization it
/// is allowed to have. Split out so a caller that already holds both can ask.
pub(crate) fn check_canonical_bytes(
    path: &Path,
    text: &str,
    value: &serde_json::Value,
) -> Result<()> {
    // The numeric domain first, because canonicalizing is what would report it —
    // and would report it as a caller mistake. A number found inside a stored
    // file is a fault in the file.
    crate::proof::stored_within_safe_integers(value, &path.display().to_string())?;
    ensure!(
        text == crate::proof::canonical_bytes(value, &path.display().to_string())?,
        EXIT_SCHEMA,
        "{}: a current resource is persisted as its canonical JCS bytes, and these are not them",
        path.display()
    );
    Ok(())
}

/// Read one JSON resource, held to the current generation's canonical
/// representation exactly when the workspace *is* that generation.
///
/// A predecessor workspace is read under its own contract, which did not say
/// there was one persisted spelling. Refusing those bytes here would make a
/// valid old workspace unreadable rather than migratable, which is the opposite
/// of what the generation boundary is for.
pub(crate) fn read_resource<T: DeserializeOwned + Serialize>(
    root: &Path,
    path: &Path,
) -> Result<T> {
    if validate_format(root)? == WorkspaceFormat::Current {
        let text = read_text(path)?;
        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))?;
        check_canonical_bytes(path, &text, &value)?;
        let resource = serde_json::from_value(value)
            .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))?;
        check_current_resource_shape(path, &text, &resource)?;
        Ok(resource)
    } else {
        read_json(path)
    }
}

/// Write via a temporary file and rename, so a reader never sees half a file.
pub(crate) fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| tool_error(parent.display(), error))?;
    }
    let text = crate::proof::canonical_bytes(value, &path.display().to_string())?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, text.as_bytes())
        .map_err(|error| tool_error(temporary.display(), error))?;
    fs::rename(&temporary, path).map_err(|error| tool_error(path.display(), error))?;
    Ok(())
}

/// Write exact text via a temporary file and rename.
///
/// [`write_json`] serializes; this one publishes bytes that were already
/// validated as the thing to write. Migration needs that distinction: the
/// artifact it checked and the artifact it publishes have to be the same value,
/// not two serializations that ought to agree.
pub(crate) fn write_text(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| tool_error(parent.display(), error))?;
    }
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    fs::write(&temporary, text.as_bytes())
        .map_err(|error| tool_error(temporary.display(), error))?;
    fs::rename(&temporary, path).map_err(|error| tool_error(path.display(), error))?;
    Ok(())
}

/// Load a current Object.
///
/// A predecessor workspace has no current Object to load, and this says so
/// rather than reading its bytes under this generation's rules. Migration is the
/// one caller that reads a predecessor, and it goes through
/// [`crate::predecessor`].
pub fn load_object(root: &Path, id: &str) -> Result<Object> {
    require_readable(root)?;
    let path = object_path(root, id);
    let text = read_text(&path)?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))?;
    check_canonical_bytes(&path, &text, &value)?;
    decode_object(&path, id, value)
}

/// Read one pending Challenge, checked as an envelope and nothing more.
///
/// The family is deliberately not interpreted here: the dispatcher needs to know
/// which domain owns the subject *before* that domain reads it, and a loader
/// that understood the Object family would be one the migration family had to
/// route around.
pub(crate) fn load_challenge(root: &Path, code: &str) -> Result<crate::confirmation::Challenge> {
    let path = challenge_path(root, code)?;
    ensure!(
        path.exists(),
        EXIT_NOT_FOUND,
        "no challenge awaiting {code}"
    );
    let text = read_text(&path)?;
    let stored: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))?;
    check_canonical_bytes(&path, &text, &stored)?;
    let challenge: crate::confirmation::Challenge = serde_json::from_value(stored)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))?;
    ensure!(
        challenge.id == code,
        EXIT_SCHEMA,
        "challenge file {code} names {}; it would show one change and admit another",
        challenge.id
    );
    challenge.validate()?;
    Ok(challenge)
}

/// Every read surface asks this first, so a predecessor workspace produces one
/// answer — migrate it — rather than a schema complaint about whichever file the
/// caller happened to open.
fn require_readable(root: &Path) -> Result<()> {
    match validate_format(root)? {
        WorkspaceFormat::Current => Ok(()),
        WorkspaceFormat::Predecessor => Err(Error::new(
            EXIT_SCHEMA,
            format!(
                "this is the released predecessor workspace; engr {} reads generation {}. Run `engr migrate`",
                crate::IMPLEMENTATION_VERSION,
                crate::WORKSPACE_GENERATION
            ),
        )),
    }
}

pub(crate) fn save_object(root: &Path, object: &Object) -> Result<()> {
    require_current(root)?;
    object.validate()?;
    crate::integrity::check_stored_object_integrity(object)?;
    let value = serde_json::to_value(object)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("object {}: {error}", object.id)))?;
    check_current_object_shape(&object_path(root, &object.id), &value)?;
    crate::proof::stored_within_safe_integers(&value, &format!("object {}", object.id))?;
    write_json(&object_path(root, &object.id), object)
}

pub fn object_ids(root: &Path) -> Result<Vec<String>> {
    let dir = objects_dir(root);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut ids = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|error| tool_error(dir.display(), error))? {
        let entry = entry.map_err(|error| tool_error(dir.display(), error))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(id) = name.strip_suffix(".json") {
            ids.push(id.to_owned());
        }
    }
    ids.sort();
    Ok(ids)
}

pub(crate) fn event_ids(root: &Path) -> Result<Vec<String>> {
    let dir = eventstore_dir(root);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut ids = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|error| tool_error(dir.display(), error))? {
        let entry = entry.map_err(|error| tool_error(dir.display(), error))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(id) = name.strip_suffix(".jsonl") {
            ids.push(id.to_owned());
        }
    }
    ids.sort();
    Ok(ids)
}

/// Confirm that any event tail which reconciliation could apply is safe to
/// replay. Events are a recovery buffer rather than a second authority, so a
/// completed projection need not replay its older history here.
fn validate_recoverable_tail(id: &str, object: Option<Object>, events: &[Event]) -> Result<()> {
    let object = match object {
        Some(object) => object,
        None => {
            let created = events.iter().find(|event| event.rev == 1).ok_or_else(|| {
                Error::new(
                    EXIT_SCHEMA,
                    format!("{id}: event buffer cannot reconstruct a missing object"),
                )
            })?;
            ensure!(
                matches!(
                    &created.action,
                    Action::ObjectCreated { .. } | Action::ObjectMigrated { .. }
                ),
                EXIT_SCHEMA,
                "{id}: event rev 1 cannot reconstruct a missing object"
            );
            let mut empty = Object::new(id.to_owned(), String::new())?;
            empty.rev = 0;
            empty.reseal()?;
            empty
        }
    };
    let (projected, _) = replay_recoverable_tail(object, events).map_err(|error| {
        Error::new(
            EXIT_SCHEMA,
            format!("{id}: event tail cannot reconcile: {}", error.message),
        )
    })?;
    // Replaying is not the whole question. Some current-state integers are
    // allocated *by* the replay and appear nowhere in the record: a
    // `section_added` carries no Section id and no counter, so its own
    // safe-integer walk passes while `take_id()` advances `next_section_id` past
    // the shared ceiling. The projection would then be one canonical sealing
    // refuses, and `.engr/events` is append-only — the tail would be durable
    // history its own recovery path can never materialize. So the check is over
    // what the replay produces, not only over what the caller wrote.
    let value = serde_json::to_value(&projected)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{id}: {error}")))?;
    crate::proof::stored_within_safe_integers(&value, &format!("{id}: replayed object"))
}

/// Shortest prefix length at which every id is distinct, floored at 8.
///
/// A uuidv7 begins with a 48-bit millisecond timestamp, so its first twelve hex
/// characters carry no randomness at all — two objects created in the same
/// minute share an eight-character prefix. Abbreviation therefore has to grow
/// with the set, the way `git` widens a short commit, instead of being a fixed
/// width that silently stops distinguishing.
pub fn abbrev_len(ids: &[String]) -> usize {
    let longest = ids.iter().map(String::len).max().unwrap_or(8);
    for length in 8..=longest {
        let mut prefixes: Vec<&str> = ids.iter().map(|id| &id[..length.min(id.len())]).collect();
        prefixes.sort_unstable();
        let count = prefixes.len();
        prefixes.dedup();
        if prefixes.len() == count {
            return length;
        }
    }
    longest
}

/// Resolve a unique id prefix, the way `git` resolves a short commit. Keeps
/// uuids out of the way without needing a separate alias registry.
pub fn resolve_id(root: &Path, prefix: &str) -> Result<String> {
    let projected = object_ids(root)?;
    let mut ids = crate::ops::object_ids(root)?;
    ids.retain(|id| projected.contains(id) || crate::ops::effective(root, id).is_ok());
    if prefix.starts_with("engr:") {
        let reference = crate::reference::EngrRef::parse_standalone(prefix)?;
        ensure!(
            reference.kind() == crate::reference::ResourceKind::Object
                && reference.section().is_none()
                && reference.snapshot_selector().is_none(),
            EXIT_NOT_FOUND,
            "{prefix:?} is not a current Object reference"
        );
        let id = crate::reference::decode_uuid(reference.id())?.to_string();
        ensure!(
            ids.contains(&id),
            EXIT_NOT_FOUND,
            "no object matches {prefix:?}"
        );
        return Ok(id);
    }
    if prefix.len() == 26 {
        if let Ok(id) = crate::reference::decode_uuid(prefix) {
            let id = id.to_string();
            if ids.contains(&id) {
                return Ok(id);
            }
        }
    }
    if ids.iter().any(|id| id == prefix) {
        return Ok(prefix.to_owned());
    }
    let matches: Vec<_> = ids.iter().filter(|id| id.starts_with(prefix)).collect();
    match matches.len() {
        1 => Ok(matches[0].clone()),
        0 => Err(Error::new(
            EXIT_NOT_FOUND,
            format!("no object matches {prefix:?}"),
        )),
        count => Err(Error::new(
            EXIT_NOT_FOUND,
            format!("{prefix:?} matches {count} objects; use more characters"),
        )),
    }
}

/// Everything a record must satisfy to belong in this store, asked on the way in
/// and on the way out.
///
/// One function rather than two lists kept in step by hand. A write path that
/// checks less than the read path is a write path that can append history
/// nothing loads — which is the same self-corrupting asymmetry whichever member
/// it is, so the whole contract moves together rather than the parts of it that
/// happened to be noticed.
///
/// `stored` is the record as the file actually holds it: the exact record text
/// alongside the value it parses to. Appending has no stored form yet, and needs
/// none — a value that came from this build's own model cannot be carrying a
/// member the model has no place for.
fn check_event_record(
    event: &Event,
    id: &str,
    stored: Option<(&str, &serde_json::Value)>,
) -> Result<()> {
    if let Some((raw, stored)) = stored {
        let canonical = serde_json::to_value(event)
            .map_err(|error| Error::new(EXIT_SCHEMA, format!("event: {error}")))?;
        // Nothing went missing on the way into the typed model. `Event` cannot
        // use `deny_unknown_fields` — its action is flattened, and serde forbids
        // the two together — so the check is against what the decode produced.
        // The dropped member is exactly the one that would have said the record
        // is not of this generation: a member carrying admission provenance,
        // silently discarded, leaves a record that replays as something nobody
        // admitted.
        ensure!(
            *stored == canonical,
            EXIT_SCHEMA,
            "an Event is stored in its exact canonical shape, and this one is not"
        );
        crate::proof::stored_within_safe_integers(stored, "Event")?;
        // The contract is not "parses to the same value". It is that the
        // persisted record *is* the RFC 8785 bytes. Comparing parsed values
        // would already have erased member order, insignificant whitespace and
        // any duplicate member name the parser collapsed, and an EventStore
        // arrives through git merge, hand edit and copy as readily as through
        // this build's own append.
        ensure!(
            raw == crate::proof::canonical_bytes(event, "Event")?,
            EXIT_SCHEMA,
            "an Event is persisted as its canonical JCS bytes, and this one is not"
        );
    }
    let value = serde_json::to_value(event)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("event: {error}")))?;
    crate::proof::stored_within_safe_integers(&value, "Event")?;
    // Revision zero is the Object before any Event: the first admitted one
    // advances it to 1. No writer emits it, and adjacency alone cannot refuse
    // it, because a `0, 1, ...` log is perfectly contiguous.
    event.validate(id)?;
    if let Some(value) = event.action.value() {
        value.content.require_canonical_order()?;
    }
    let admitted = &event.metadata.admitted;
    if admitted.by == crate::semantics::Admission::Agent {
        ensure!(
            event.action.becomes().is_none(),
            EXIT_SCHEMA,
            "an Agent Event cannot carry a destination"
        );
        ensure!(
            !matches!(
                event.action,
                Action::ObjectStateChanged { .. }
                    | Action::ObjectClassified { .. }
                    | Action::ObjectSuperseded { .. }
                    | Action::ObjectRepaired {}
                    | Action::ObjectMigrated { .. }
            ),
            EXIT_SCHEMA,
            "this action requires Human admission"
        );
        // Title work is non-authoritative navigation metadata, so an Agent may
        // create and rename without a governing Rule. Everything else it admits
        // has to name the review that let it in.
        if !matches!(
            event.action,
            Action::ObjectCreated { .. } | Action::ObjectRenamed { .. }
        ) {
            ensure!(
                admitted.review.is_some(),
                EXIT_SCHEMA,
                "an Agent semantic Object event records the passing Rule Review that admitted it"
            );
        }
    }
    Ok(())
}

/// Whether this Event would be accepted by the durable boundary, without
/// writing anything.
///
/// **There is no public append, and that is the contract.** Recomputing what an Event
/// persists is not the same as proving admission, because Event provenance is
/// deliberately minimal: it carries the review's outcome and digest and not the
/// transient inputs the decision was made from. The attempt is the one that
/// matters — every mutation carries one Agent-attested attempt, each applicable
/// Rule judges it against its own ceiling, and past any ceiling autonomous
/// Object admission stops. None of that is in the record, so no amount of
/// re-derivation at this boundary can ask it, and a public raw append would be a
/// second Agent admission API holding strictly less state than the gate.
///
/// So this is the read-only half and the only half a consumer gets: every check
/// the append performs, and no capacity to perform the append. A caller that
/// wants a record in the log goes through the gate, which is where the admission
/// inputs live. The lock is taken because the checks read the durable tail, and
/// an answer about a tail that is moving is not an answer.
pub fn check_appendable(root: &Path, event: &Event) -> Result<()> {
    with_lock(root, || check_appendable_locked(root, event, None))
}

/// The durable append, for a caller already inside [`with_lock`] — which is
/// every caller, because the only routes here are `confirm`, `admit_agent` and
/// the migration bootstrap.
pub(crate) fn append_event_locked(root: &Path, object: &str, event: &Event) -> Result<()> {
    check_appendable_locked(root, event, Some(object))?;
    let path = events_path(root, object);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| tool_error(parent.display(), error))?;
    }
    let line = crate::proof::canonical_bytes(event, "Event")?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| tool_error(path.display(), error))?;
    use std::io::Write;
    writeln!(file, "{line}").map_err(|error| tool_error(path.display(), error))
}

/// Every check the append performs, and none of the writing.
///
/// The owning Object is passed in rather than read off the Event, because the
/// Event does not carry it: the stream binds it, and the seal binds it again. A
/// caller with no Object in hand recovers it from the seal — there is at most
/// one Object the digest can have been taken for.
fn check_appendable_locked(root: &Path, event: &Event, object: Option<&str>) -> Result<()> {
    // The durable Event path is part of the workspace-generation boundary, and a
    // direct library caller reaches it without passing the gate. Asked here as
    // well as there, because "this build may write this workspace" is a property
    // of the workspace rather than of the route taken to it.
    require_current(root)?;
    let id = match object {
        Some(id) => id.to_owned(),
        None => owning_object(root, event)?,
    };
    // Before anything is written, and before the Object is saved — `confirm`
    // appends here first, so refusing at this point leaves the workspace exactly
    // as it was rather than advanced past history it could not record.
    check_event_record(event, &id, None)?;
    // Continuity against what is already there, which is the one part of the
    // read contract that is about the file rather than the record. Reading the
    // tail is the cost of not being able to append a revision the next load
    // would refuse.
    let mut tail = load_events(root, &id)?;
    if let Some(last) = tail.last() {
        ensure!(
            last.rev.checked_add(1) == Some(event.rev),
            EXIT_SCHEMA,
            "event rev {} does not immediately follow rev {}",
            event.rev,
            last.rev
        );
    }
    // And that the history this produces is one the record can actually be
    // arrived at through. A record can be well formed, contiguous and still
    // impossible: updating a Section that does not exist, or beginning a history
    // with something no Object comes from. The EventStore is append-only and is
    // never purged, so such a record is not a mistake anybody can take back — it
    // durably breaks every read that reconstructs the Object, and it breaks
    // crash recovery, which is the one thing this file is for.
    tail.push(event.clone());
    let stored = match load_object(root, &id) {
        Ok(object) => Some(object),
        Err(error) if error.code == EXIT_NOT_FOUND => None,
        Err(error) => return Err(error),
    };
    validate_recoverable_tail(&id, stored, &tail)?;
    // Last, and only for a record that is otherwise sound: a malformed Event
    // should be refused for being malformed, not for failing a proof about a
    // shape nothing could admit anyway.
    crate::gate::check_admission(root, &id, event)
}

/// Which Object an Event belongs to, recovered from its own seal.
///
/// The public read-only boundary takes an Event and nothing else, and the Event
/// deliberately does not name its Object. Rather than reintroduce a member that
/// could disagree with the stream, this asks the seal: the digest binds exactly
/// one Object id, so at most one workspace Object can answer for it.
fn owning_object(root: &Path, event: &Event) -> Result<String> {
    for id in object_ids(root)? {
        let agrees = crate::digest::EVENT
            .recheck(&event.digest, |version| event.digest_under(&id, version))
            .map(|attested| attested.agrees())
            .unwrap_or(false);
        if agrees {
            return Ok(id);
        }
    }
    Err(Error::new(
        EXIT_NOT_FOUND,
        format!(
            "event {} seals against no Object in this workspace, so there is no stream it belongs to",
            event.id
        ),
    ))
}

pub fn load_events(root: &Path, id: &str) -> Result<Vec<Event>> {
    let path = events_path(root, id);
    if !path.exists() {
        ensure!(
            !object_path(root, id).exists(),
            EXIT_SCHEMA,
            "{} exists but its append-only Event history is missing",
            object_path(root, id).display()
        );
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path).map_err(|error| tool_error(path.display(), error))?;
    decode_events(&path, id, &text)
}

/// Decode the one EventStore text a caller already captured.
pub(crate) fn decode_events(path: &Path, id: &str, text: &str) -> Result<Vec<Event>> {
    let mut events: Vec<Event> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let stored: serde_json::Value = serde_json::from_str(line).map_err(|error| {
            Error::new(
                EXIT_SCHEMA,
                format!("{}:{}: {error}", path.display(), index + 1),
            )
        })?;
        let event: Event = serde_json::from_value(stored.clone()).map_err(|error| {
            Error::new(
                EXIT_SCHEMA,
                format!("{}:{}: {error}", path.display(), index + 1),
            )
        })?;
        // Reconciliation can turn an event back into authority after a crash,
        // so corrupt recovery data must fail before it reaches the reducer — and
        // it fails against the same rules the write boundary applied, rather
        // than a second copy of them kept in step by hand.
        check_event_record(&event, id, Some((line, &stored))).map_err(|error| {
            Error::new(
                EXIT_SCHEMA,
                format!("{}:{}: {}", path.display(), index + 1, error.message),
            )
        })?;
        if let Some(previous) = events.last() {
            ensure!(
                previous.rev.checked_add(1) == Some(event.rev),
                EXIT_SCHEMA,
                "{}:{}: event rev {} does not immediately follow rev {}",
                path.display(),
                index + 1,
                event.rev,
                previous.rev
            );
        }
        events.push(event);
    }
    check_event_history(path, id, &events)?;
    Ok(events)
}

/// Every Object has a complete stream beginning at revision 1.
///
/// History is append-only evidence, not a replaceable recovery cache, and #66
/// says every Object's current-generation stream starts at 1 — there is no
/// history-prefix pruning that could make an incomplete stream legitimate. A
/// migrated Object's revision 1 is its `object.migrated.v1` bootstrap; a newly
/// created one's is `object.created.v1`.
fn check_event_history(path: &Path, id: &str, events: &[Event]) -> Result<()> {
    let Some(first) = events.first() else {
        return Ok(());
    };
    ensure!(
        first.rev == 1,
        EXIT_SCHEMA,
        "{}: non-empty Event history starts at revision 1, not {}",
        path.display(),
        first.rev
    );
    ensure!(
        matches!(
            first.action,
            Action::ObjectCreated { .. } | Action::ObjectMigrated { .. }
        ),
        EXIT_SCHEMA,
        "{}: a stream begins with the Object being created or migrated, not with {}",
        path.display(),
        first.action.event_type()
    );
    // Only ever the first. A bootstrap replaces the whole projection, so a
    // second one anywhere in a stream would silently discard everything before
    // it — the reducer refuses that too, and saying so here names the actual
    // fault instead of reporting it as a failed replay.
    for event in &events[1..] {
        ensure!(
            !matches!(
                event.action,
                Action::ObjectCreated { .. } | Action::ObjectMigrated { .. }
            ),
            EXIT_SCHEMA,
            "{}: {} establishes an Object, so it appears once, at revision 1",
            path.display(),
            event.action.event_type()
        );
    }
    let mut seen = std::collections::BTreeSet::new();
    for event in events {
        ensure!(
            seen.insert(event.id.clone()),
            EXIT_SCHEMA,
            "{}: event id {} appears more than once in the stream for {id}",
            path.display(),
            event.id
        );
    }
    Ok(())
}
