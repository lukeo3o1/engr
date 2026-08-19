//! Filesystem layout, locking, and atomic writes.
//!
//! ```text
//! .engr/
//!   format.json              what version wrote this workspace
//!   lock                     one writer at a time
//!   objects/<uuid>.json      the authority
//!   events/<uuid>.jsonl      append-only confirmed history
//!   candidates/<CODE>.json   awaiting a human
//!   backlog/<uuid>.json      unresolved staging, confirmed by nobody
//! ```

use crate::model::{replay_recoverable_tail, Action, Event, Object, EVENT_FORMAT};
use crate::{
    ensure, tool_error, Error, Result, EVENT_ENVELOPE_VERSION_V0, EXIT_NOT_FOUND, EXIT_SCHEMA,
    EXIT_USAGE, LEGACY_OBJECT_VERSION_V0, WORKSPACE_VERSION,
};
use fs2::FileExt;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const DIR: &str = ".engr";
pub const WORKSPACE_FORMAT: &str = "engr-workspace";

pub fn engr_dir(root: &Path) -> PathBuf {
    root.join(DIR)
}
pub fn objects_dir(root: &Path) -> PathBuf {
    engr_dir(root).join("objects")
}
pub fn events_dir(root: &Path) -> PathBuf {
    engr_dir(root).join("events")
}
pub fn candidates_dir(root: &Path) -> PathBuf {
    engr_dir(root).join("candidates")
}
pub fn object_path(root: &Path, id: &str) -> PathBuf {
    objects_dir(root).join(format!("{id}.json"))
}
pub fn events_path(root: &Path, id: &str) -> PathBuf {
    events_dir(root).join(format!("{id}.jsonl"))
}
pub fn candidate_path(root: &Path, challenge: &str) -> Result<PathBuf> {
    ensure!(
        crate::confirmation::valid_challenge(challenge),
        EXIT_USAGE,
        "candidate code {challenge:?} must be six characters from 23456789ABCDEFGHJKLMNPQRSTUVWXYZ"
    );
    Ok(candidates_dir(root).join(format!("{challenge}.json")))
}

/// Where the last size refusal is written down.
///
/// Inside `candidates/` deliberately, and not beside `format.json`. That
/// directory is already ignored by every `.gitignore` `init` has ever written,
/// so this file cannot travel out of the machine that made it — whereas a new
/// path at the top of `.engr` would need a line existing workspaces do not have,
/// and quietly rewriting somebody's `.gitignore` is a worse answer than picking
/// a directory that already says "local only". [`crate::gate::pending_codes`]
/// reads this directory by filename and keeps only valid challenge codes, so a
/// name that is not one is invisible to it.
pub fn refusal_path(root: &Path) -> PathBuf {
    candidates_dir(root).join("refused-oversize.json")
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

#[derive(serde::Serialize, serde::Deserialize)]
struct Format {
    format: String,
    version: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceFormat {
    LegacyV0,
    Current,
}

/// Written by [`init`], because `git add -A` is the normal way people stage a
/// workspace and two of these files must never travel with it. A candidate's
/// filename *is* its challenge code, so committing a live one hands the code to
/// everyone with repository access — the gate assumes it goes to one human and
/// comes back. Telling people not to do that in a README does not stop `-A`.
const GITIGNORE: &str = "\
# The record lives in objects/ — commit that; it is where earlier wording is
# recovered from. events/ is safe to commit too: the challenge codes in it have
# already been spent, and a spent code resolves to nothing.
#
# These two are local only:
#   lock         a mutex for this machine, nothing to share
#   candidates/  each file is named after a *live* challenge code
/lock
/candidates/
";

pub fn init(root: &Path) -> Result<PathBuf> {
    let dir = engr_dir(root);
    ensure!(
        !dir.exists(),
        EXIT_SCHEMA,
        "{} already exists",
        dir.display()
    );
    for path in [
        objects_dir(root),
        events_dir(root),
        candidates_dir(root),
        crate::backlog::dir(root),
    ] {
        fs::create_dir_all(&path).map_err(|error| tool_error(path.display(), error))?;
    }
    write_json(
        &dir.join("format.json"),
        &Format {
            format: WORKSPACE_FORMAT.to_owned(),
            version: WORKSPACE_VERSION,
        },
    )?;
    let ignore = dir.join(".gitignore");
    fs::write(&ignore, GITIGNORE).map_err(|error| tool_error(ignore.display(), error))?;
    Ok(dir)
}

pub fn validate_format(root: &Path) -> Result<WorkspaceFormat> {
    let path = engr_dir(root).join("format.json");
    if !path.exists() {
        ensure!(
            detect_legacy(root)?,
            EXIT_SCHEMA,
            "{} has no format.json and is not a recognized legacy v0 workspace",
            engr_dir(root).display()
        );
        return Ok(WorkspaceFormat::LegacyV0);
    }
    let format: Format = read_json(&path)?;
    ensure!(
        format.format == WORKSPACE_FORMAT,
        EXIT_SCHEMA,
        "{}: not an engr workspace",
        path.display()
    );
    ensure!(
        format.version == WORKSPACE_VERSION,
        EXIT_SCHEMA,
        "workspace version {} is not supported by engr {}",
        format.version,
        crate::IMPLEMENTATION_VERSION
    );
    if contains_legacy_objects(root)? {
        return Ok(WorkspaceFormat::LegacyV0);
    }
    Ok(WorkspaceFormat::Current)
}

fn detect_legacy(root: &Path) -> Result<bool> {
    let ids = object_ids(root)?;
    if ids.is_empty() {
        return Ok(false);
    }
    for id in ids {
        let value: serde_json::Value = read_json(&object_path(root, &id))?;
        if value.get("format").and_then(|value| value.as_str()) != Some("engr-object")
            || value.get("version").and_then(|value| value.as_u64())
                != Some(LEGACY_OBJECT_VERSION_V0.into())
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn contains_legacy_objects(root: &Path) -> Result<bool> {
    let mut legacy = false;
    for id in object_ids(root)? {
        let path = object_path(root, &id);
        let value: serde_json::Value = read_json(&path)?;
        let object = value.as_object().ok_or_else(|| {
            Error::new(
                EXIT_SCHEMA,
                format!("{}: object must be a JSON object", path.display()),
            )
        })?;
        ensure!(
            !(object.contains_key("status") && object.contains_key("state")),
            EXIT_SCHEMA,
            "{}: contains both legacy status and canonical state",
            path.display()
        );
        ensure!(
            object.contains_key("status") || object.contains_key("state"),
            EXIT_SCHEMA,
            "{}: object has neither status nor state",
            path.display()
        );
        legacy |= object.contains_key("status");
    }
    Ok(legacy)
}

pub fn require_current(root: &Path) -> Result<()> {
    ensure!(
        validate_format(root)? == WorkspaceFormat::Current,
        EXIT_SCHEMA,
        "legacy v0 workspace is read-only; run `engr migrate` before mutation"
    );
    Ok(())
}

struct MigrationEntry {
    id: String,
    path: PathBuf,
    object: Object,
    migrated: Option<serde_json::Value>,
}

fn decode_object(path: &Path, id: &str, value: serde_json::Value) -> Result<Object> {
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

/// Validate every retained representation before moving any one Object. A
/// malformed legacy workspace must remain exactly legacy rather than becoming a
/// mixture of old and new files because a later entry was invalid.
fn preflight_migration(root: &Path) -> Result<Vec<MigrationEntry>> {
    let mut entries = Vec::new();
    for id in object_ids(root)? {
        let path = object_path(root, &id);
        let value: serde_json::Value = read_json(&path)?;
        let object = value.as_object().ok_or_else(|| {
            Error::new(
                EXIT_SCHEMA,
                format!("{}: object must be a JSON object", path.display()),
            )
        })?;
        ensure!(
            !(object.contains_key("status") && object.contains_key("state")),
            EXIT_SCHEMA,
            "{}: contains both legacy status and canonical state",
            path.display()
        );
        ensure!(
            object.contains_key("status") || object.contains_key("state"),
            EXIT_SCHEMA,
            "{}: legacy object has neither status nor state",
            path.display()
        );

        // Deserialize the stored form, not just its lifecycle key. This catches
        // missing required fields and illegal legacy status values before any
        // neighboring file is rewritten.
        let stored = decode_object(&path, &id, value.clone())?;
        let needs_state_conversion = object.contains_key("status");
        let (object, migrated) = if needs_state_conversion {
            let mut planned = value;
            let planned_object = planned.as_object_mut().ok_or_else(|| {
                Error::new(
                    EXIT_SCHEMA,
                    format!("{}: object must be a JSON object", path.display()),
                )
            })?;
            let status = planned_object.remove("status").ok_or_else(|| {
                Error::new(
                    EXIT_SCHEMA,
                    format!("{}: legacy object has no status", path.display()),
                )
            })?;
            planned_object.insert("state".to_owned(), status);
            (decode_object(&path, &id, planned.clone())?, Some(planned))
        } else {
            (stored, None)
        };
        entries.push(MigrationEntry {
            id,
            path,
            object,
            migrated,
        });
    }
    let objects = entries
        .iter()
        .map(|entry| (entry.id.clone(), entry.object.clone()))
        .collect();
    validate_retained_events(root, &objects)?;
    Ok(entries)
}

pub fn migrate(root: &Path) -> Result<()> {
    ensure!(
        validate_format(root)? == WorkspaceFormat::LegacyV0,
        EXIT_SCHEMA,
        "workspace does not require migration"
    );
    let entries = preflight_migration(root)?;
    for entry in entries {
        if let Some(value) = entry.migrated {
            write_json(&entry.path, &value)?;
        }
    }
    let format_path = engr_dir(root).join("format.json");
    if !format_path.exists() {
        write_json(
            &format_path,
            &Format {
                format: WORKSPACE_FORMAT.to_owned(),
                version: WORKSPACE_VERSION,
            },
        )?;
    }
    ensure!(
        validate_format(root)? == WorkspaceFormat::Current,
        EXIT_SCHEMA,
        "workspace migration did not produce the current format"
    );
    let mut objects = BTreeMap::new();
    for id in object_ids(root)? {
        objects.insert(id.clone(), load_object(root, &id)?);
    }
    validate_retained_events(root, &objects)?;
    Ok(())
}

/// Hold the workspace write lock for the duration of `body`.
pub fn with_lock<T>(root: &Path, body: impl FnOnce() -> Result<T>) -> Result<T> {
    let path = engr_dir(root).join("lock");
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
    let text = fs::read_to_string(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Error::new(EXIT_NOT_FOUND, format!("{}: not found", path.display()))
        } else {
            tool_error(path.display(), error)
        }
    })?;
    serde_json::from_str(&text)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))
}

/// Write via a temporary file and rename, so a reader never sees half a file.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| tool_error(parent.display(), error))?;
    }
    let mut text = serde_json::to_string_pretty(value)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))?;
    text.push('\n');
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, text.as_bytes())
        .map_err(|error| tool_error(temporary.display(), error))?;
    fs::rename(&temporary, path).map_err(|error| tool_error(path.display(), error))?;
    Ok(())
}

pub fn load_object(root: &Path, id: &str) -> Result<Object> {
    let path = object_path(root, id);
    let value: serde_json::Value = read_json(&path)?;
    decode_object(&path, id, value)
}

pub fn save_object(root: &Path, object: &Object) -> Result<()> {
    require_current(root)?;
    object.validate()?;
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

fn event_ids(root: &Path) -> Result<Vec<String>> {
    let dir = events_dir(root);
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
                matches!(&created.payload.action, Action::ObjectCreated),
                EXIT_SCHEMA,
                "{id}: event rev 1 cannot reconstruct a missing object"
            );
            Object::new(id.to_owned(), String::new())?
        }
    };
    replay_recoverable_tail(object, events)
        .map(|_| ())
        .map_err(|error| {
            Error::new(
                EXIT_SCHEMA,
                format!("{id}: event tail cannot reconcile: {}", error.message),
            )
        })
}

/// Retained events can be replayed after a crash. Check that relationship
/// before migration commits the only representation change it makes.
fn validate_retained_events(root: &Path, objects: &BTreeMap<String, Object>) -> Result<()> {
    for id in event_ids(root)? {
        let events = load_events(root, &id)?;
        validate_recoverable_tail(&id, objects.get(&id).cloned(), &events)?;
    }
    Ok(())
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
    if prefix.starts_with("engr:") {
        let reference = crate::reference::EngrRef::parse_standalone(prefix)?;
        ensure!(
            reference.kind() == crate::reference::ResourceKind::Object
                && reference.section().is_none()
                && reference.snapshot_selector().is_none(),
            EXIT_NOT_FOUND,
            "{prefix:?} is not a current Object reference"
        );
        return Ok(crate::reference::decode_uuid(reference.id())?.to_string());
    }
    if prefix.len() == 26 {
        if let Ok(id) = crate::reference::decode_uuid(prefix) {
            let id = id.to_string();
            if object_path(root, &id).exists() {
                return Ok(id);
            }
        }
    }
    let ids = object_ids(root)?;
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

pub fn append_event(root: &Path, event: &Event) -> Result<()> {
    let path = events_path(root, &event.payload.object);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| tool_error(parent.display(), error))?;
    }
    let line = serde_json::to_string(event)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("event: {error}")))?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| tool_error(path.display(), error))?;
    use std::io::Write;
    writeln!(file, "{line}").map_err(|error| tool_error(path.display(), error))?;
    Ok(())
}

pub fn load_events(root: &Path, id: &str) -> Result<Vec<Event>> {
    let path = events_path(root, id);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path).map_err(|error| tool_error(path.display(), error))?;
    let mut events: Vec<Event> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let event: Event = serde_json::from_str(line).map_err(|error| {
            Error::new(
                EXIT_SCHEMA,
                format!("{}:{}: {error}", path.display(), index + 1),
            )
        })?;
        ensure!(
            event.format == EVENT_FORMAT,
            EXIT_SCHEMA,
            "{}:{}: not an engr event",
            path.display(),
            index + 1
        );
        // Reconciliation can turn an event back into authority after a crash,
        // so corrupt recovery data must fail before it reaches the reducer.
        ensure!(
            event.version == EVENT_ENVELOPE_VERSION_V0,
            EXIT_SCHEMA,
            "{}:{}: unsupported event version {}",
            path.display(),
            index + 1,
            event.version
        );
        ensure!(
            event.payload.object == id,
            EXIT_SCHEMA,
            "{}:{}: event belongs to object {:?}, not {:?}",
            path.display(),
            index + 1,
            event.payload.object,
            id
        );
        event.payload.validate().map_err(|error| {
            Error::new(
                EXIT_SCHEMA,
                format!(
                    "{}:{}: invalid event payload: {}",
                    path.display(),
                    index + 1,
                    error.message
                ),
            )
        })?;
        let payload_sha256 = event.payload.sha256().map_err(|error| {
            Error::new(
                EXIT_SCHEMA,
                format!(
                    "{}:{}: invalid event payload: {}",
                    path.display(),
                    index + 1,
                    error.message
                ),
            )
        })?;
        ensure!(
            event.confirmation.payload_sha256 == payload_sha256,
            EXIT_SCHEMA,
            "{}:{}: confirmation does not match the event payload",
            path.display(),
            index + 1
        );
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
    Ok(events)
}
