use crate::protocol::{
    attach_integrity, candidate_hash, canonical_json, now_rfc3339, object, parse_time, read_json,
    stream_kind, strict_json, validate_event, validate_event_data, verify_integrity,
};
use crate::require;
use crate::state::{canonical_chain, reconciliation_parent, reduce_chain, CanonicalChain};
use crate::view::render_state;
use crate::{
    EngrError, Result, EVENT_SCHEMA_VERSION, EXIT_FORK, EXIT_INVARIANT, EXIT_NOT_FOUND,
    EXIT_SCHEMA, EXIT_TOOL, EXIT_VERSION, IMPLEMENTATION, IMPLEMENTATION_VERSION, PROTOCOL_VERSION,
    STATE_SCHEMA_VERSION,
};
use include_dir::{include_dir, Dir};
use rand::seq::SliceRandom;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};
use time::macros::format_description;
use uuid::Uuid;
use walkdir::WalkDir;

static PROJECT_ASSETS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../skill/assets/project");

/// Serialize mutations of a project's EventStore, State, receipts, and snapshots.
///
/// Event ids and event numbers are allocated from the current store, so writes must
/// observe one coherent view of the project.  The lock file deliberately lives in
/// `.engr/state`, matching the protocol's durable project state boundary.
fn with_project_write_lock<T>(root: &Path, operation: impl FnOnce() -> Result<T>) -> Result<T> {
    use fs2::FileExt;

    let path = engr_dir(root).join("state/.write.lock");
    let parent = path
        .parent()
        .ok_or_else(|| EngrError::new(EXIT_TOOL, "write lock has no parent directory"))?;
    fs::create_dir_all(parent)
        .map_err(|error| EngrError::new(EXIT_TOOL, format!("{}: {error}", parent.display())))?;
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|error| EngrError::new(EXIT_TOOL, format!("{}: {error}", path.display())))?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => break,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(EngrError::new(
                        EXIT_TOOL,
                        "timed out acquiring the engineering write lock",
                    ));
                }
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => {
                return Err(EngrError::new(
                    EXIT_TOOL,
                    format!("{}: {error}", path.display()),
                ));
            }
        }
    }
    let result = operation();
    FileExt::unlock(&file)
        .map_err(|error| EngrError::new(EXIT_TOOL, format!("{}: {error}", path.display())))?;
    result
}

pub fn engr_dir(root: &Path) -> PathBuf {
    root.join(".engr")
}

pub fn find_project_root(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(root) = explicit {
        let root = root.canonicalize().map_err(|_| {
            EngrError::new(
                EXIT_NOT_FOUND,
                format!("project root not found: {}", root.display()),
            )
        })?;
        require!(
            root.join(".engr/FORMAT.md").is_file(),
            EXIT_NOT_FOUND,
            ".engr/FORMAT.md not found under {}",
            root.display()
        );
        return Ok(root);
    }
    let mut current =
        std::env::current_dir().map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?;
    loop {
        if current.join(".engr/FORMAT.md").is_file() {
            return Ok(current);
        }
        if !current.pop() {
            break;
        }
    }
    Err(EngrError::new(EXIT_NOT_FOUND, "no .engr directory found"))
}

pub fn validate_format_versions(root: &Path) -> Result<()> {
    let path = engr_dir(root).join("FORMAT.md");
    let text = fs::read_to_string(&path).map_err(|error| {
        EngrError::new(
            if error.kind() == std::io::ErrorKind::NotFound {
                EXIT_NOT_FOUND
            } else {
                EXIT_TOOL
            },
            format!("FORMAT.md: {error}"),
        )
    })?;
    for (label, expected) in [
        ("Protocol", PROTOCOL_VERSION),
        ("Event schema", EVENT_SCHEMA_VERSION),
        ("State schema", STATE_SCHEMA_VERSION),
    ] {
        let values: Vec<_> = text
            .lines()
            .filter_map(|line| line.strip_suffix('\r').or(Some(line)))
            .filter_map(|line| line.strip_prefix(&format!("{label}:")))
            .map(str::trim)
            .collect();
        require!(
            values.len() == 1,
            EXIT_SCHEMA,
            "FORMAT.md must declare exactly one {label} version"
        );
        require!(
            values[0].parse::<i64>().ok() == Some(expected),
            EXIT_VERSION,
            "FORMAT.md declares unsupported {label} {}",
            values[0]
        );
    }
    Ok(())
}

fn atomic_write(path: &Path, value: &Value) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| EngrError::new(EXIT_TOOL, "cannot write a filesystem root"))?;
    fs::create_dir_all(parent)
        .map_err(|error| EngrError::new(EXIT_TOOL, format!("{}: {error}", parent.display())))?;
    let temp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file"),
        Uuid::now_v7()
    ));
    let bytes = format!(
        "{}\n",
        serde_json::to_string_pretty(value)
            .map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?
    )
    .into_bytes();
    {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .map_err(|error| EngrError::new(EXIT_TOOL, format!("{}: {error}", temp.display())))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| EngrError::new(EXIT_TOOL, format!("{}: {error}", temp.display())))?;
    }
    fs::rename(&temp, path)
        .map_err(|error| EngrError::new(EXIT_TOOL, format!("{}: {error}", path.display())))?;
    Ok(())
}

fn copy_asset_dir(dir: &Dir<'_>, destination: &Path) -> Result<()> {
    for file in dir.files() {
        let path = destination.join(file.path());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?;
        }
        fs::write(&path, file.contents())
            .map_err(|error| EngrError::new(EXIT_TOOL, format!("{}: {error}", path.display())))?;
    }
    for child in dir.dirs() {
        copy_asset_dir(child, destination)?;
    }
    Ok(())
}

pub fn init_project(root: &Path) -> Result<Value> {
    let root = root.canonicalize().map_err(|_| {
        EngrError::new(
            EXIT_NOT_FOUND,
            format!("project root not found: {}", root.display()),
        )
    })?;
    let target = engr_dir(&root);
    require!(
        !target.exists(),
        EXIT_INVARIANT,
        "refusing to overwrite existing {}",
        target.display()
    );
    copy_asset_dir(&PROJECT_ASSETS, &target)?;
    let tools = target.join("tools");
    fs::create_dir_all(&tools).map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?;
    let binary =
        std::env::current_exe().map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?;
    let executable = tools.join(if cfg!(windows) { "engr.exe" } else { "engr" });
    fs::copy(&binary, &executable)
        .map_err(|error| EngrError::new(EXIT_TOOL, format!("{}: {error}", executable.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&executable)
            .map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions)
            .map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?;
    }
    Ok(json!({"ok":true,"root":root,"tool":executable,"protocol_version":PROTOCOL_VERSION}))
}

fn event_files(root: &Path, stream: Option<&str>) -> Result<Vec<PathBuf>> {
    let base = engr_dir(root).join("eventstore");
    if !base.exists() {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    for entry in WalkDir::new(&base).follow_links(false) {
        let entry = entry.map_err(|error| {
            EngrError::new(EXIT_SCHEMA, format!("EventStore traversal: {error}"))
        })?;
        if entry.file_type().is_file()
            && entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                == Some("jsonl")
        {
            if stream
                .map(|stream| {
                    entry.path().file_stem().and_then(|name| name.to_str()) == Some(stream)
                })
                .unwrap_or(true)
            {
                found.push(entry.into_path());
            }
        }
    }
    found.sort();
    Ok(found)
}

fn validate_event_path(base: &Path, path: &Path, stream: Option<&str>) -> Result<String> {
    let relative = path
        .strip_prefix(base)
        .map_err(|_| EngrError::new(EXIT_SCHEMA, "EventStore path escaped root"))?;
    let parts: Vec<_> = relative.components().collect();
    require!(
        parts.len() == 4,
        EXIT_SCHEMA,
        "invalid event path: {}",
        relative.display()
    );
    let year = parts[0].as_os_str().to_string_lossy();
    let month = parts[1].as_os_str().to_string_lossy();
    let day = parts[2].as_os_str().to_string_lossy();
    let filename = parts[3].as_os_str().to_string_lossy();
    require!(
        year.len() == 4
            && year.chars().all(|item| item.is_ascii_digit())
            && month.len() == 2
            && month.chars().all(|item| item.is_ascii_digit())
            && day.len() == 2
            && day.chars().all(|item| item.is_ascii_digit()),
        EXIT_SCHEMA,
        "invalid event date partition: {}",
        relative.display()
    );
    require!(
        filename.ends_with(".jsonl"),
        EXIT_SCHEMA,
        "unexpected file in EventStore: {}",
        relative.display()
    );
    let name = filename.strip_suffix(".jsonl").unwrap();
    stream_kind(name)?;
    if let Some(stream) = stream {
        require!(
            name == stream,
            EXIT_SCHEMA,
            "stream filename mismatch: {}",
            relative.display()
        );
    }
    Ok(format!("{year}/{month}/{day}"))
}

pub fn load_events(root: &Path, stream: &str) -> Result<Vec<Value>> {
    stream_kind(stream)?;
    let base = engr_dir(root).join("eventstore");
    let mut events = Vec::new();
    for path in event_files(root, Some(stream))? {
        let partition = validate_event_path(&base, &path, Some(stream))?;
        let bytes = fs::read(&path)
            .map_err(|error| EngrError::new(EXIT_TOOL, format!("{}: {error}", path.display())))?;
        require!(
            bytes.is_empty() || bytes.ends_with(b"\n"),
            EXIT_SCHEMA,
            "partial trailing JSONL line: {}",
            path.display()
        );
        let text = String::from_utf8(bytes).map_err(|_| {
            EngrError::new(
                EXIT_SCHEMA,
                format!("invalid UTF-8 JSONL: {}", path.display()),
            )
        })?;
        for (line_number, line) in text.lines().enumerate() {
            require!(
                !line.is_empty(),
                EXIT_SCHEMA,
                "blank JSONL line: {}:{}",
                path.display(),
                line_number + 1
            );
            let event = strict_json(line, &format!("{}:{}", path.display(), line_number + 1))?;
            validate_event(&event, Some(stream), Some(&partition))?;
            events.push(event);
        }
    }
    let ids: HashSet<_> = events
        .iter()
        .filter_map(|event| event.get("event_id").and_then(Value::as_str))
        .collect();
    require!(
        ids.len() == events.len(),
        EXIT_INVARIANT,
        "duplicate event id in stream {stream}"
    );
    Ok(events)
}

pub fn list_streams(root: &Path) -> Result<Vec<String>> {
    let base = engr_dir(root).join("eventstore");
    let mut streams = HashSet::new();
    for path in event_files(root, None)? {
        validate_event_path(&base, &path, None)?;
        streams.insert(
            path.file_stem()
                .and_then(|name| name.to_str())
                .unwrap()
                .to_owned(),
        );
    }
    let mut values: Vec<_> = streams.into_iter().collect();
    values.sort();
    Ok(values)
}

fn state_path(root: &Path, stream: &str) -> Result<PathBuf> {
    Ok(engr_dir(root)
        .join("state")
        .join(if stream_kind(stream)? == "work_item" {
            "work-items"
        } else {
            "decisions"
        })
        .join(format!("{stream}.json")))
}
fn relative_posix(root: &Path, path: &Path) -> Result<String> {
    Ok(path
        .strip_prefix(root)
        .map_err(|_| EngrError::new(EXIT_TOOL, "path escaped root"))?
        .to_string_lossy()
        .replace('\\', "/"))
}
fn manifest_new() -> Value {
    json!({"format":"engineering-eventstore-manifest","protocol_version":PROTOCOL_VERSION,"event_schema_version":EVENT_SCHEMA_VERSION,"state_schema_version":STATE_SCHEMA_VERSION,"stream_heads":{}})
}
fn load_manifest(root: &Path, required: bool) -> Result<Value> {
    let path = engr_dir(root).join("state/manifest.json");
    if !path.exists() {
        require!(
            !required,
            EXIT_NOT_FOUND,
            "state manifest not found; run replay"
        );
        return Ok(manifest_new());
    }
    let value = read_json(&path, "state manifest")?;
    let map = object(&value, "state manifest")?;
    require!(
        map.get("format").and_then(Value::as_str) == Some("engineering-eventstore-manifest"),
        EXIT_SCHEMA,
        "state manifest: invalid format"
    );
    for (field, expected) in [
        ("protocol_version", PROTOCOL_VERSION),
        ("event_schema_version", EVENT_SCHEMA_VERSION),
        ("state_schema_version", STATE_SCHEMA_VERSION),
    ] {
        require!(
            map.get(field).and_then(Value::as_i64) == Some(expected),
            EXIT_VERSION,
            "state manifest: unsupported {field}"
        );
    }
    require!(
        map.get("stream_heads").is_some_and(Value::is_object),
        EXIT_SCHEMA,
        "state manifest: stream_heads must be object"
    );
    Ok(value)
}
pub fn save_state(root: &Path, state: &Value) -> Result<()> {
    let stream = state
        .get("stream")
        .and_then(Value::as_str)
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, "State.stream: expected string"))?;
    verify_integrity(state, &format!("State {stream}"))?;
    let path = state_path(root, stream)?;
    atomic_write(&path, state)?;
    let mut manifest = load_manifest(root, false)?;
    let state_map = object(state, "State")?;
    let head = state_map.get("head").unwrap();
    let integrity = state_map
        .get("integrity")
        .and_then(Value::as_object)
        .and_then(|value| value.get("value"))
        .unwrap();
    object_mut(&mut manifest,"manifest")?.get_mut("stream_heads").and_then(Value::as_object_mut).unwrap().insert(stream.to_owned(),json!({"event_id":head.get("event_id").unwrap(),"rev":head.get("rev").unwrap(),"kind":state_map.get("kind").unwrap(),"state_path":relative_posix(root,&path)?,"state_integrity":integrity}));
    atomic_write(&engr_dir(root).join("state/manifest.json"), &manifest)
}
fn object_mut<'a>(value: &'a mut Value, label: &str) -> Result<&'a mut Map<String, Value>> {
    value
        .as_object_mut()
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, format!("{label}: expected object")))
}
fn validate_creation_date(chain: &[Value]) -> Result<()> {
    let stream = chain
        .first()
        .and_then(|event| event.get("stream"))
        .and_then(Value::as_str)
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, "chain stream missing"))?;
    let date = parse_time(
        chain[0].get("time").and_then(Value::as_str).unwrap(),
        "time",
    )?
    .format(&format_description!("[year][month][day]"))
    .unwrap();
    require!(
        stream.get(3..11) == Some(date.as_str()),
        EXIT_INVARIANT,
        "stream {stream}: id date does not match genesis date"
    );
    Ok(())
}
pub fn replay_stream(root: &Path, stream: &str, persist: bool) -> Result<(Value, CanonicalChain)> {
    let events = load_events(root, stream)?;
    let canonical = canonical_chain(&events, stream)?;
    validate_creation_date(&canonical.chain)?;
    let state = reduce_chain(&canonical.chain, None)?;
    if persist {
        save_state(root, &state)?;
    }
    Ok((state, canonical))
}
pub fn load_current_state(root: &Path, stream: &str) -> Result<Value> {
    replay_stream(root, stream, false).map(|(state, _)| state)
}

fn event_storage_path(root: &Path, event: &Value) -> Result<PathBuf> {
    let time = parse_time(event.get("time").and_then(Value::as_str).unwrap(), "time")?;
    let partition = time
        .format(&format_description!("[year]/[month]/[day]"))
        .unwrap();
    Ok(engr_dir(root)
        .join("eventstore")
        .join(partition)
        .join(format!(
            "{}.jsonl",
            event.get("stream").and_then(Value::as_str).unwrap()
        )))
}
fn next_event_no(root: &Path, time: &str) -> Result<String> {
    let parsed = parse_time(time, "time")?;
    let date = parsed
        .format(&format_description!("[year][month][day]"))
        .unwrap();
    let directory = engr_dir(root).join("eventstore").join(
        parsed
            .format(&format_description!("[year]/[month]/[day]"))
            .unwrap(),
    );
    let mut count = 0usize;
    if directory.exists() {
        for path in
            fs::read_dir(directory).map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?
        {
            let path = path
                .map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?
                .path();
            if path.extension().and_then(|item| item.to_str()) == Some("jsonl") {
                let bytes =
                    fs::read(path).map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?;
                count += bytes.iter().filter(|byte| **byte == b'\n').count();
            }
        }
    }
    Ok(format!("E-{date}-{:04}", count + 1))
}
fn append_line(root: &Path, event: &Value) -> Result<PathBuf> {
    let path = event_storage_path(root, event)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?;
    }
    if path.exists() {
        let bytes =
            fs::read(&path).map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?;
        require!(
            bytes.is_empty() || bytes.ends_with(b"\n"),
            EXIT_SCHEMA,
            "cannot append after partial JSONL line: {}",
            path.display()
        );
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?;
    file.write_all(format!("{}\n", canonical_json(event)).as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?;
    Ok(path)
}
fn build_event(
    root: &Path,
    stream: &str,
    event_type: &str,
    record: &str,
    data: &Value,
    parent: Value,
    rev: i64,
    provenance: Value,
) -> Result<Value> {
    let time = now_rfc3339();
    let event = json!({"format":"engineering-event","protocol_version":PROTOCOL_VERSION,"event_schema_version":EVENT_SCHEMA_VERSION,"event_id":Uuid::now_v7().to_string(),"event_no":next_event_no(root,&time)?,"time":time,"stream":stream,"rev":rev,"parent":parent,"event":event_type,"provenance":provenance,"record":{"text":record},"data":data});
    validate_event(&event, None, None)?;
    Ok(event)
}
fn current_parent(events: &[Value], stream: &str) -> Result<Value> {
    if events.is_empty() {
        return Ok(Value::Null);
    }
    let canonical = canonical_chain(events, stream)?;
    Ok(canonical
        .chain
        .last()
        .unwrap()
        .get("event_id")
        .unwrap()
        .clone())
}
fn append_parent(events: &[Value], stream: &str, event_type: &str, data: &Value) -> Result<Value> {
    if event_type == "stream.fork_reconciled" && !events.is_empty() {
        return Ok(Value::String(reconciliation_parent(events, stream, data)?));
    }
    current_parent(events, stream)
}
pub fn append_event(
    root: &Path,
    stream: &str,
    event_type: &str,
    record: &str,
    data: Value,
    provenance: Value,
    expected_parent: Value,
) -> Result<(Value, Value, PathBuf)> {
    with_project_write_lock(root, || {
        append_event_locked(
            root,
            stream,
            event_type,
            record,
            data,
            provenance,
            expected_parent,
        )
    })
}

fn append_event_locked(
    root: &Path,
    stream: &str,
    event_type: &str,
    record: &str,
    data: Value,
    provenance: Value,
    expected_parent: Value,
) -> Result<(Value, Value, PathBuf)> {
    stream_kind(stream)?;
    let events = load_events(root, stream)?;
    let parent = append_parent(&events, stream, event_type, &data)?;
    require!(
        parent == expected_parent,
        EXIT_FORK,
        "expected parent {expected_parent:?}, current parent is {parent:?}"
    );
    let rev = if events.is_empty() {
        require!(
            event_type
                == if stream_kind(stream)? == "work_item" {
                    "work_item.created"
                } else {
                    "decision.created"
                },
            EXIT_INVARIANT,
            "new stream has an invalid root event"
        );
        1
    } else {
        events
            .iter()
            .find(|event| event.get("event_id") == Some(&parent))
            .and_then(|event| event.get("rev"))
            .and_then(Value::as_i64)
            .unwrap()
            + 1
    };
    let event = build_event(
        root, stream, event_type, record, &data, parent, rev, provenance,
    )?;
    let mut proposed = events;
    proposed.push(event.clone());
    let canonical = canonical_chain(&proposed, stream)?;
    validate_creation_date(&canonical.chain)?;
    let state = reduce_chain(&canonical.chain, None)?;
    let path = append_line(root, &event)?;
    save_state(root, &state)?;
    Ok((event, state, path))
}

pub fn version_object() -> Value {
    json!({"implementation":IMPLEMENTATION,"implementation_version":IMPLEMENTATION_VERSION,"protocol_version":PROTOCOL_VERSION,"event_schema_version":EVENT_SCHEMA_VERSION,"state_schema_version":STATE_SCHEMA_VERSION})
}
pub fn doctor(root: &Path) -> Result<Value> {
    validate_format_versions(root)?;
    let streams = list_streams(root)?;
    Ok(
        json!({"protocol_version":PROTOCOL_VERSION,"project":root,"selected_implementation":"engr","streams":streams,"implementation":version_object()}),
    )
}

fn snapshot_files_by_stream(
    root: &Path,
    streams: &HashSet<String>,
) -> Result<HashMap<String, Vec<PathBuf>>> {
    let base = engr_dir(root).join("snapshots");
    if !base.exists() {
        return Ok(HashMap::new());
    }
    let metadata = fs::symlink_metadata(&base)
        .map_err(|error| EngrError::new(EXIT_TOOL, format!("{}: {error}", base.display())))?;
    require!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        EXIT_SCHEMA,
        "snapshots root must be a real directory"
    );
    let mut result: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for entry in WalkDir::new(&base).min_depth(1) {
        let entry = entry.map_err(|error| EngrError::new(EXIT_SCHEMA, error.to_string()))?;
        let path = entry.path();
        let relative = path
            .strip_prefix(&base)
            .map_err(|_| EngrError::new(EXIT_SCHEMA, "snapshot path escaped root"))?;
        let parts: Vec<_> = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().to_string())
            .collect();
        require!(
            !entry.file_type().is_symlink(),
            EXIT_SCHEMA,
            "snapshot path must not be a symlink: {}",
            relative.display()
        );
        let folder = parts.first().map(String::as_str).unwrap_or("");
        require!(
            matches!(folder, "work-items" | "decisions"),
            EXIT_SCHEMA,
            "invalid snapshot path: {}",
            relative.display()
        );
        if entry.file_type().is_dir() {
            require!(
                parts.len() <= 2,
                EXIT_SCHEMA,
                "nested snapshot directory is not allowed: {}",
                relative.display()
            );
            if parts.len() == 2 {
                let stream = &parts[1];
                let kind = stream_kind(stream)?;
                let expected = if kind == "work_item" {
                    "work-items"
                } else {
                    "decisions"
                };
                require!(
                    folder == expected,
                    EXIT_SCHEMA,
                    "snapshot directory kind mismatch: {}",
                    relative.display()
                );
                require!(
                    streams.contains(stream),
                    EXIT_INVARIANT,
                    "orphan snapshot directory for missing stream: {}",
                    relative.display()
                );
            }
            continue;
        }
        require!(
            entry.file_type().is_file() && parts.len() == 3,
            EXIT_SCHEMA,
            "invalid snapshot file depth: {}",
            relative.display()
        );
        let stream = &parts[1];
        let kind = stream_kind(stream)?;
        let expected = if kind == "work_item" {
            "work-items"
        } else {
            "decisions"
        };
        require!(
            folder == expected,
            EXIT_SCHEMA,
            "snapshot file kind mismatch: {}",
            relative.display()
        );
        require!(
            streams.contains(stream),
            EXIT_INVARIANT,
            "orphan snapshot for missing stream: {}",
            relative.display()
        );
        result
            .entry(stream.to_owned())
            .or_default()
            .push(path.to_owned());
    }
    for paths in result.values_mut() {
        paths.sort();
    }
    Ok(result)
}

fn verify_snapshot(path: &Path, stream: &str, chain: &[Value]) -> Result<()> {
    let label = format!("snapshot {}", path.display());
    let snapshot = read_json(path, &label)?;
    let map = object(&snapshot, &label)?;
    let required = [
        "format",
        "protocol_version",
        "event_schema_version",
        "state_schema_version",
        "filename",
        "stream",
        "through",
        "created_at",
        "state",
        "integrity",
    ];
    require!(
        map.len() == required.len() && required.iter().all(|key| map.contains_key(*key)),
        EXIT_SCHEMA,
        "{label}: unexpected or missing fields"
    );
    require!(
        map.get("format").and_then(Value::as_str) == Some("engineering-eventstore-snapshot"),
        EXIT_SCHEMA,
        "{label}: invalid format"
    );
    for (field, expected) in [
        ("protocol_version", PROTOCOL_VERSION),
        ("event_schema_version", EVENT_SCHEMA_VERSION),
        ("state_schema_version", STATE_SCHEMA_VERSION),
    ] {
        require!(
            map.get(field).and_then(Value::as_i64) == Some(expected),
            EXIT_VERSION,
            "{label}: incompatible {field}"
        );
    }
    require!(
        map.get("filename").and_then(Value::as_str)
            == path.file_name().and_then(|name| name.to_str()),
        EXIT_SCHEMA,
        "{label}: filename does not match immutable identity"
    );
    require!(
        map.get("stream").and_then(Value::as_str) == Some(stream),
        EXIT_SCHEMA,
        "{label}: stream mismatch"
    );
    parse_time(
        map.get("created_at")
            .and_then(Value::as_str)
            .ok_or_else(|| EngrError::new(EXIT_SCHEMA, format!("{label}: invalid created_at")))?,
        &format!("{label}.created_at"),
    )?;
    verify_integrity(&snapshot, &label)?;
    let state = map
        .get("state")
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, format!("{label}: missing State")))?;
    verify_integrity(state, &format!("{label} State"))?;
    require!(
        state.get("stream") == Some(&Value::String(stream.to_owned()))
            && state.get("head") == map.get("through"),
        EXIT_INVARIANT,
        "{label}: State/head mismatch"
    );
    let head_id = map
        .get("through")
        .and_then(Value::as_object)
        .and_then(|head| head.get("event_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, format!("{label}: invalid through head")))?;
    let expected_suffix = format!(".{stream}.snap.{}.json", head_id.replace('-', ""));
    require!(
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(&expected_suffix)),
        EXIT_SCHEMA,
        "{label}: invalid filename"
    );
    let position = chain
        .iter()
        .position(|event| event.get("event_id").and_then(Value::as_str) == Some(head_id))
        .ok_or_else(|| EngrError::new(EXIT_INVARIANT, format!("{label}: non-canonical head")))?;
    let expected = reduce_chain(&chain[..=position], None)?;
    require!(
        state == &expected,
        EXIT_INVARIANT,
        "{label}: does not match full replay"
    );
    Ok(())
}

fn verify_cross_references(
    states: &HashMap<String, Value>,
    source_streams: &HashSet<String>,
) -> Result<()> {
    for stream in source_streams {
        let state = states.get(stream).ok_or_else(|| {
            EngrError::new(
                EXIT_INVARIANT,
                format!("cross-reference source {stream} is missing"),
            )
        })?;
        let kind = state
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| EngrError::new(EXIT_SCHEMA, format!("State {stream}: invalid kind")))?;
        if kind == "decision" {
            if state.get("status").and_then(Value::as_str) == Some("superseded") {
                let mut current = stream.as_str();
                let mut seen = HashSet::new();
                while states
                    .get(current)
                    .and_then(|item| item.get("status"))
                    .and_then(Value::as_str)
                    == Some("superseded")
                {
                    require!(
                        seen.insert(current.to_owned()),
                        EXIT_INVARIANT,
                        "{stream}: decision supersession cycle"
                    );
                    let target = states
                        .get(current)
                        .and_then(|item| item.get("superseded_by"))
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            EngrError::new(
                                EXIT_INVARIANT,
                                format!("{current}: superseding decision is missing"),
                            )
                        })?;
                    let target_state = states.get(target).ok_or_else(|| {
                        EngrError::new(
                            EXIT_INVARIANT,
                            format!("{current}: superseding decision {target} not found"),
                        )
                    })?;
                    require!(
                        target_state.get("kind").and_then(Value::as_str) == Some("decision"),
                        EXIT_INVARIANT,
                        "{current}: superseding stream {target} is not a decision"
                    );
                    require!(
                        matches!(
                            target_state.get("status").and_then(Value::as_str),
                            Some("accepted" | "superseded" | "revoked")
                        ),
                        EXIT_INVARIANT,
                        "{current}: superseding decision {target} has an invalid status"
                    );
                    current = target;
                }
                require!(
                    matches!(
                        states
                            .get(current)
                            .and_then(|item| item.get("status"))
                            .and_then(Value::as_str),
                        Some("accepted" | "revoked")
                    ),
                    EXIT_INVARIANT,
                    "{stream}: supersession chain has no valid terminal decision"
                );
            }
            continue;
        }
        for link in state
            .get("decisions")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                EngrError::new(EXIT_SCHEMA, format!("State {stream}: invalid decisions"))
            })?
        {
            if link.get("status").and_then(Value::as_str) != Some("linked") {
                continue;
            }
            let target = link
                .get("decision_id")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    EngrError::new(
                        EXIT_SCHEMA,
                        format!("State {stream}: invalid decision link"),
                    )
                })?;
            require!(
                states.get(target).is_some_and(
                    |item| item.get("kind").and_then(Value::as_str) == Some("decision")
                ),
                EXIT_INVARIANT,
                "{stream}: linked decision {target} not found"
            );
            require!(
                states
                    .get(target)
                    .and_then(|item| item.get("status"))
                    .and_then(Value::as_str)
                    == Some("accepted"),
                EXIT_INVARIANT,
                "{stream}: linked decision {target} is not currently accepted"
            );
        }
        for relation in state
            .get("related_work_items")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                EngrError::new(
                    EXIT_SCHEMA,
                    format!("State {stream}: invalid related work items"),
                )
            })?
        {
            if relation.get("status").and_then(Value::as_str) != Some("active") {
                continue;
            }
            let target = relation
                .get("work_item_id")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    EngrError::new(
                        EXIT_SCHEMA,
                        format!("State {stream}: invalid work item relation"),
                    )
                })?;
            require!(
                target != stream,
                EXIT_INVARIANT,
                "{stream}: a Work Item cannot relate to itself"
            );
            require!(
                states.get(target).is_some_and(
                    |item| item.get("kind").and_then(Value::as_str) == Some("work_item")
                ),
                EXIT_INVARIANT,
                "{stream}: related Work Item {target} not found"
            );
        }
        for finding in state
            .get("findings")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                EngrError::new(EXIT_SCHEMA, format!("State {stream}: invalid findings"))
            })?
        {
            if finding.get("status").and_then(Value::as_str) != Some("promoted") {
                continue;
            }
            let target = finding
                .get("work_item_id")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    EngrError::new(
                        EXIT_SCHEMA,
                        format!("State {stream}: invalid finding promotion"),
                    )
                })?;
            require!(
                target != stream,
                EXIT_INVARIANT,
                "{stream}: a finding cannot be promoted to its own Work Item"
            );
            require!(
                states.get(target).is_some_and(
                    |item| item.get("kind").and_then(Value::as_str) == Some("work_item")
                ),
                EXIT_INVARIANT,
                "{stream}: promoted Work Item {target} not found"
            );
        }
    }
    Ok(())
}

fn accepted_receipts(root: &Path) -> Result<HashMap<String, Value>> {
    let (pending, accepted, rejected) = confirmation_dirs(root);
    let mut challenges = HashSet::new();
    let mut receipts = HashMap::new();
    for (directory, expected_status) in [
        (pending, "pending"),
        (accepted, "accepted"),
        (rejected, "rejected"),
    ] {
        if !directory.exists() {
            continue;
        }
        for entry in fs::read_dir(&directory).map_err(|error| {
            EngrError::new(EXIT_TOOL, format!("{}: {error}", directory.display()))
        })? {
            let path = entry
                .map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?
                .path();
            require!(
                path.is_file()
                    && path.extension().and_then(|extension| extension.to_str()) == Some("json"),
                EXIT_SCHEMA,
                "invalid confirmation receipt path: {}",
                path.display()
            );
            let receipt = read_json(&path, "confirmation receipt")?;
            let challenge = receipt
                .get("challenge")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    EngrError::new(EXIT_SCHEMA, "confirmation receipt: invalid challenge")
                })?;
            require!(
                challenge.len() == 6
                    && challenge
                        .chars()
                        .all(|item| "23456789ABCDEFGHJKLMNPQRSTUVWXYZ".contains(item)),
                EXIT_SCHEMA,
                "confirmation receipt: invalid challenge"
            );
            require!(
                challenges.insert(challenge.to_owned()),
                EXIT_INVARIANT,
                "confirmation challenge reused: {challenge}"
            );
            require!(
                receipt.get("status").and_then(Value::as_str) == Some(expected_status),
                EXIT_INVARIANT,
                "confirmation receipt {} has wrong status",
                path.display()
            );
            if expected_status != "accepted" {
                continue;
            }
            let event_id = receipt
                .get("event_id")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    EngrError::new(EXIT_SCHEMA, "accepted confirmation: invalid event id")
                })?
                .to_owned();
            require!(
                crate::protocol::valid_event_id(&event_id),
                EXIT_SCHEMA,
                "accepted confirmation: invalid event id"
            );
            require!(
                path.file_stem().and_then(|item| item.to_str()) == Some(event_id.as_str()),
                EXIT_SCHEMA,
                "accepted confirmation: filename mismatch"
            );
            require!(
                receipts.insert(event_id.clone(), receipt).is_none(),
                EXIT_INVARIANT,
                "duplicate accepted confirmation for {event_id}"
            );
        }
    }
    Ok(receipts)
}

fn verify_receipt_for_human_event(event: &Value, receipt: &Value) -> Result<()> {
    let label = "accepted confirmation";
    let map = object(receipt, label)?;
    let required = [
        "format",
        "protocol_version",
        "challenge",
        "created_at",
        "stream",
        "expected_parent",
        "event",
        "record",
        "data",
        "candidate_sha256",
        "status",
        "closed_at",
        "event_id",
    ];
    require!(
        map.len() == required.len() && required.iter().all(|key| map.contains_key(*key)),
        EXIT_SCHEMA,
        "{label}: unexpected or missing fields"
    );
    require!(
        map.get("format").and_then(Value::as_str) == Some("engineering-confirmation-candidate"),
        EXIT_SCHEMA,
        "{label}: invalid format"
    );
    require!(
        map.get("protocol_version").and_then(Value::as_i64) == Some(PROTOCOL_VERSION),
        EXIT_VERSION,
        "{label}: incompatible protocol"
    );
    require!(
        map.get("status").and_then(Value::as_str) == Some("accepted"),
        EXIT_INVARIANT,
        "{label}: wrong status"
    );
    for field in ["created_at", "closed_at"] {
        parse_time(
            map.get(field)
                .and_then(Value::as_str)
                .ok_or_else(|| EngrError::new(EXIT_SCHEMA, format!("{label}: invalid {field}")))?,
            &format!("{label}.{field}"),
        )?;
    }
    let event_map = object(event, "human event")?;
    for (receipt_field, event_field) in [
        ("event_id", "event_id"),
        ("stream", "stream"),
        ("expected_parent", "parent"),
        ("event", "event"),
        ("record", "record"),
        ("data", "data"),
    ] {
        require!(
            map.get(receipt_field) == event_map.get(event_field),
            EXIT_INVARIANT,
            "{label}: {receipt_field} mismatch"
        );
    }
    let stream = event_map.get("stream").and_then(Value::as_str).unwrap();
    let event_type = event_map.get("event").and_then(Value::as_str).unwrap();
    let digest = candidate_hash(
        stream,
        event_type,
        event_map.get("record").unwrap(),
        event_map.get("data").unwrap(),
        event_map.get("parent").unwrap(),
    );
    require!(
        map.get("candidate_sha256").and_then(Value::as_str) == Some(digest.as_str()),
        EXIT_INVARIANT,
        "{label}: candidate hash mismatch"
    );
    let confirmation = event_map
        .get("provenance")
        .and_then(Value::as_object)
        .and_then(|provenance| provenance.get("confirmation"))
        .and_then(Value::as_object)
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, "human event: invalid confirmation"))?;
    require!(
        confirmation.get("challenge") == map.get("challenge"),
        EXIT_INVARIANT,
        "{label}: challenge mismatch"
    );
    if let Some(hash) = confirmation.get("candidate_sha256") {
        require!(
            hash.as_str() == Some(digest.as_str()),
            EXIT_INVARIANT,
            "{label}: provenance hash mismatch"
        );
    }
    Ok(())
}

pub fn verify_project(root: &Path, selected: Option<&str>) -> Result<Value> {
    with_project_write_lock(root, || {
        recover_confirmation_transactions(root)?;
        verify_project_locked(root, selected)
    })
}

fn verify_project_locked(root: &Path, selected: Option<&str>) -> Result<Value> {
    validate_format_versions(root)?;
    let all = list_streams(root)?;
    require!(
        !all.is_empty(),
        EXIT_NOT_FOUND,
        "EventStore contains no streams"
    );
    let streams = if let Some(stream) = selected {
        require!(
            all.iter().any(|item| item == stream),
            EXIT_NOT_FOUND,
            "stream not found: {stream}"
        );
        vec![stream.to_owned()]
    } else {
        all.clone()
    };
    let all_set: HashSet<String> = all.iter().cloned().collect();
    let snapshots = snapshot_files_by_stream(root, &all_set)?;
    let mut event_ids = HashSet::new();
    for stream in &all {
        for event in load_events(root, stream)? {
            let id = event
                .get("event_id")
                .and_then(Value::as_str)
                .ok_or_else(|| EngrError::new(EXIT_SCHEMA, "event has no event_id"))?;
            require!(
                event_ids.insert(id.to_owned()),
                EXIT_INVARIANT,
                "duplicate event id across EventStore: {id}"
            );
        }
    }
    let manifest = load_manifest(root, true)?;
    let receipts = accepted_receipts(root)?;
    let mut reports = Vec::new();
    let mut states = HashMap::new();
    for stream in &all {
        let (state, canonical) = replay_stream(root, stream, false)?;
        states.insert(stream.clone(), state.clone());
        if streams.contains(stream) {
            let persisted = read_json(&state_path(root, stream)?, &format!("State {stream}"))?;
            verify_integrity(&persisted, &format!("State {stream}"))?;
            require!(
                persisted == state,
                EXIT_INVARIANT,
                "State {stream}: differs from full replay"
            );
            let entry = manifest
                .get("stream_heads")
                .and_then(Value::as_object)
                .and_then(|entries| entries.get(stream))
                .ok_or_else(|| {
                    EngrError::new(EXIT_INVARIANT, format!("manifest: missing stream {stream}"))
                })?;
            let expected = json!({"event_id":state["head"]["event_id"],"rev":state["head"]["rev"],"kind":state["kind"],"state_path":relative_posix(root,&state_path(root,stream)?)?,"state_integrity":state["integrity"]["value"]});
            require!(
                entry == &expected,
                EXIT_INVARIANT,
                "manifest: stale or invalid entry for {stream}"
            );
            for snapshot in snapshots.get(stream).into_iter().flatten() {
                verify_snapshot(snapshot, stream, &canonical.chain)?;
            }
            for event in load_events(root, stream)? {
                if event
                    .get("provenance")
                    .and_then(Value::as_object)
                    .and_then(|provenance| provenance.get("initiator"))
                    .and_then(Value::as_str)
                    == Some("human")
                {
                    let event_id = event.get("event_id").and_then(Value::as_str).unwrap();
                    let receipt = receipts.get(event_id).ok_or_else(|| {
                        EngrError::new(
                            EXIT_INVARIANT,
                            format!("human event {event_id} has no accepted confirmation receipt"),
                        )
                    })?;
                    verify_receipt_for_human_event(&event, receipt)?;
                }
            }
            reports.push(json!({"stream":stream,"head":state["head"],"events":load_events(root,stream)?.len(),"canonical_events":canonical.chain.len(),"rejected_events":canonical.rejected.len(),"reconciliations":canonical.resolutions.len()}));
        }
    }
    if selected.is_none() {
        require!(
            manifest
                .get("stream_heads")
                .and_then(Value::as_object)
                .is_some_and(|entries| entries.len() == all.len()
                    && all.iter().all(|stream| entries.contains_key(stream))),
            EXIT_INVARIANT,
            "manifest contains missing or extra streams"
        );
        require!(
            receipts.keys().all(|event_id| event_ids.contains(event_id)),
            EXIT_INVARIANT,
            "accepted confirmation references missing event"
        );
    }
    let selected_streams: HashSet<String> = streams.into_iter().collect();
    verify_cross_references(&states, &selected_streams)?;
    Ok(json!({"protocol_version":PROTOCOL_VERSION,"verified_streams":reports,"warnings":[]}))
}

pub fn create_snapshot(root: &Path, stream: &str, name: Option<&str>) -> Result<(PathBuf, Value)> {
    with_project_write_lock(root, || create_snapshot_locked(root, stream, name))
}

fn create_snapshot_locked(
    root: &Path,
    stream: &str,
    name: Option<&str>,
) -> Result<(PathBuf, Value)> {
    let events = load_events(root, stream)?;
    let canonical = canonical_chain(&events, stream)?;
    let state = reduce_chain(&canonical.chain, None)?;
    let label = name.map(str::to_owned).unwrap_or_else(|| {
        if state.get("kind").and_then(Value::as_str) == Some("work_item") {
            state["title"]["text"].as_str().unwrap().to_owned()
        } else {
            state["topic"]["text"].as_str().unwrap().to_owned()
        }
    });
    let slug = slug(&label, 40);
    let head = state["head"]["event_id"].as_str().unwrap().replace('-', "");
    let folder = if stream_kind(stream)? == "work_item" {
        "work-items"
    } else {
        "decisions"
    };
    let path = engr_dir(root)
        .join("snapshots")
        .join(folder)
        .join(stream)
        .join(format!("{slug}.{stream}.snap.{head}.json"));
    let mut snapshot = json!({"format":"engineering-eventstore-snapshot","protocol_version":PROTOCOL_VERSION,"event_schema_version":EVENT_SCHEMA_VERSION,"state_schema_version":STATE_SCHEMA_VERSION,"filename":path.file_name().and_then(|item|item.to_str()).unwrap(),"stream":stream,"through":state["head"],"created_at":now_rfc3339(),"state":state});
    attach_integrity(&mut snapshot)?;
    if path.exists() {
        let current = read_json(&path, "snapshot")?;
        require!(
            current.get("through") == snapshot.get("through")
                && current.get("state") == snapshot.get("state"),
            EXIT_INVARIANT,
            "immutable snapshot path already contains different data: {}",
            path.display()
        );
        return Ok((path, current));
    }
    atomic_write(&path, &snapshot)?;
    Ok((path, snapshot))
}
fn slug(text: &str, budget: usize) -> String {
    let mut value = String::new();
    let mut hyphen = false;
    for character in text.to_ascii_lowercase().chars() {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            value.push(character);
            hyphen = false;
        } else if !hyphen && !value.is_empty() {
            value.push('-');
            hyphen = true;
        }
    }
    let mut value = value.trim_matches('-').to_owned();
    if value.is_empty() {
        value = "snapshot".into();
    }
    if value.len() > budget {
        value.truncate(budget);
        value = value.trim_matches('-').to_owned();
        if value.is_empty() {
            value = "snapshot".into();
        }
    }
    value
}

fn confirmation_dirs(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let base = engr_dir(root).join("artifacts/confirmations");
    (
        base.join("pending"),
        base.join("accepted"),
        base.join("rejected"),
    )
}
fn confirmation_transaction_dir(root: &Path) -> PathBuf {
    engr_dir(root).join("artifacts/confirmations/transactions")
}
fn archive_destination(root: &Path, status: &str, item: &str) -> Result<PathBuf> {
    let (_, accepted, rejected) = confirmation_dirs(root);
    match status {
        "accepted" => Ok(accepted.join(format!("{item}.json"))),
        "rejected" => Ok(rejected.join(format!("{item}.json"))),
        _ => Err(EngrError::new(
            EXIT_TOOL,
            format!("invalid receipt archive status: {status}"),
        )),
    }
}
fn transaction_parts(path: &Path, suffix: &str) -> Result<(String, String, String)> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix(suffix))
        .ok_or_else(|| EngrError::new(EXIT_TOOL, "invalid confirmation transaction name"))?;
    let mut parts = name.splitn(3, '.');
    let status = parts
        .next()
        .filter(|item| !item.is_empty())
        .ok_or_else(|| EngrError::new(EXIT_TOOL, "invalid confirmation transaction status"))?;
    let item = parts
        .next()
        .filter(|item| !item.is_empty())
        .ok_or_else(|| EngrError::new(EXIT_TOOL, "invalid confirmation transaction item"))?;
    let token = parts
        .next()
        .filter(|item| !item.is_empty())
        .ok_or_else(|| EngrError::new(EXIT_TOOL, "invalid confirmation transaction token"))?;
    Ok((status.to_owned(), item.to_owned(), token.to_owned()))
}
fn recover_confirmation_transactions(root: &Path) -> Result<()> {
    let directory = confirmation_transaction_dir(root);
    if !directory.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(&directory)
        .map_err(|error| EngrError::new(EXIT_TOOL, format!("{}: {error}", directory.display())))?
    {
        let prepared = entry
            .map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?
            .path();
        if !prepared
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".prepared.json"))
        {
            continue;
        }
        let (status, item, token) = transaction_parts(&prepared, ".prepared.json")?;
        let destination = archive_destination(root, &status, &item)?;
        let claimed = directory.join(format!("{status}.{item}.{token}.claimed.json"));
        if claimed.exists() {
            if !destination.exists() {
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?;
                }
                fs::rename(&prepared, &destination).map_err(|error| {
                    EngrError::new(EXIT_TOOL, format!("{}: {error}", prepared.display()))
                })?;
            } else {
                fs::remove_file(&prepared).map_err(|error| {
                    EngrError::new(EXIT_TOOL, format!("{}: {error}", prepared.display()))
                })?;
            }
            fs::remove_file(&claimed).map_err(|error| {
                EngrError::new(EXIT_TOOL, format!("{}: {error}", claimed.display()))
            })?;
        } else {
            // The claim never committed; the pending source is still authoritative.
            fs::remove_file(&prepared).map_err(|error| {
                EngrError::new(EXIT_TOOL, format!("{}: {error}", prepared.display()))
            })?;
        }
    }
    for entry in fs::read_dir(&directory)
        .map_err(|error| EngrError::new(EXIT_TOOL, format!("{}: {error}", directory.display())))?
    {
        let claimed = entry
            .map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?
            .path();
        if !claimed
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".claimed.json"))
        {
            continue;
        }
        let (status, item, _) = transaction_parts(&claimed, ".claimed.json")?;
        let destination = archive_destination(root, &status, &item)?;
        if destination.exists() {
            fs::remove_file(&claimed).map_err(|error| {
                EngrError::new(EXIT_TOOL, format!("{}: {error}", claimed.display()))
            })?;
            continue;
        }
        let receipt = read_json(&claimed, "claimed confirmation receipt")?;
        let challenge = receipt
            .get("challenge")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                EngrError::new(EXIT_SCHEMA, "claimed confirmation: invalid challenge")
            })?;
        let (pending, _, _) = confirmation_dirs(root);
        fs::create_dir_all(&pending)
            .map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?;
        let source = pending.join(format!("{challenge}.json"));
        require!(
            !source.exists(),
            EXIT_TOOL,
            "confirmation transaction has both claim and pending source: {}",
            claimed.display()
        );
        fs::rename(&claimed, &source).map_err(|error| {
            EngrError::new(EXIT_TOOL, format!("{}: {error}", claimed.display()))
        })?;
    }
    Ok(())
}
fn random_challenge() -> String {
    let alphabet: Vec<_> = "23456789ABCDEFGHJKLMNPQRSTUVWXYZ".chars().collect();
    let mut rng = rand::thread_rng();
    (0..6)
        .map(|_| *alphabet.choose(&mut rng).unwrap())
        .collect()
}
fn receipt_stream(receipt: &Value) -> Result<&str> {
    receipt
        .get("stream")
        .and_then(Value::as_str)
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, "pending confirmation: invalid stream"))
}
pub fn read_record(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|error| {
        EngrError::new(
            if error.kind() == std::io::ErrorKind::NotFound {
                EXIT_NOT_FOUND
            } else {
                EXIT_TOOL
            },
            format!("record file {}: {error}", path.display()),
        )
    })
}
pub fn read_data(path: Option<&Path>) -> Result<Value> {
    match path {
        None => Ok(json!({})),
        Some(path) => {
            let value = read_json(path, "data file")?;
            require!(
                value.is_object(),
                EXIT_SCHEMA,
                "data file: expected JSON object"
            );
            Ok(value)
        }
    }
}
fn validate_receipt(receipt: &Value, challenge: Option<&str>) -> Result<()> {
    let map = object(receipt, "pending confirmation")?;
    for key in [
        "format",
        "protocol_version",
        "challenge",
        "created_at",
        "stream",
        "expected_parent",
        "event",
        "record",
        "data",
        "candidate_sha256",
        "status",
    ] {
        require!(
            map.contains_key(key),
            EXIT_SCHEMA,
            "pending confirmation: missing field {key}"
        );
    }
    require!(
        map.get("format").and_then(Value::as_str) == Some("engineering-confirmation-candidate"),
        EXIT_SCHEMA,
        "pending confirmation: invalid format"
    );
    require!(
        map.get("protocol_version").and_then(Value::as_i64) == Some(PROTOCOL_VERSION),
        EXIT_VERSION,
        "pending confirmation: unsupported protocol"
    );
    require!(
        map.get("status").and_then(Value::as_str) == Some("pending"),
        EXIT_INVARIANT,
        "pending confirmation: wrong status"
    );
    let actual = map
        .get("challenge")
        .and_then(Value::as_str)
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, "pending confirmation: invalid challenge"))?;
    require!(
        actual.len() == 6
            && actual
                .chars()
                .all(|item| "23456789ABCDEFGHJKLMNPQRSTUVWXYZ".contains(item)),
        EXIT_SCHEMA,
        "pending confirmation: invalid challenge"
    );
    if let Some(challenge) = challenge {
        require!(
            actual == challenge,
            EXIT_SCHEMA,
            "pending confirmation: challenge mismatch"
        );
    }
    parse_time(
        map.get("created_at")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                EngrError::new(EXIT_SCHEMA, "pending confirmation: invalid created_at")
            })?,
        "pending confirmation.created_at",
    )?;
    let stream = receipt_stream(receipt)?;
    stream_kind(stream)?;
    let event = map
        .get("event")
        .and_then(Value::as_str)
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, "pending confirmation: invalid event"))?;
    validate_event_data(event, map.get("data").unwrap())?;
    let record = map
        .get("record")
        .and_then(Value::as_object)
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, "pending confirmation: invalid record"))?;
    require!(
        record
            .get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.is_empty()),
        EXIT_SCHEMA,
        "pending confirmation: invalid record text"
    );
    let digest = map
        .get("candidate_sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            EngrError::new(EXIT_SCHEMA, "pending confirmation: invalid candidate hash")
        })?;
    let expected = candidate_hash(
        stream,
        event,
        map.get("record").unwrap(),
        map.get("data").unwrap(),
        map.get("expected_parent").unwrap(),
    );
    require!(
        digest == expected,
        EXIT_SCHEMA,
        "pending confirmation candidate hash mismatch"
    );
    Ok(())
}
fn archive_receipt(
    root: &Path,
    source: &Path,
    status: &str,
    reason: Option<&str>,
    event_id: Option<&str>,
) -> Result<Value> {
    let mut receipt = read_json(source, "confirmation receipt")?;
    let challenge = receipt
        .get("challenge")
        .and_then(Value::as_str)
        .ok_or_else(|| EngrError::new(EXIT_SCHEMA, "confirmation receipt: invalid challenge"))?
        .to_owned();
    let map = object_mut(&mut receipt, "confirmation receipt")?;
    map.insert("status".into(), Value::String(status.into()));
    map.insert("closed_at".into(), Value::String(now_rfc3339()));
    if let Some(reason) = reason {
        map.insert("reason".into(), Value::String(reason.into()));
    }
    if let Some(event_id) = event_id {
        map.insert("event_id".into(), Value::String(event_id.into()));
    }
    require!(
        matches!(status, "accepted" | "rejected"),
        EXIT_TOOL,
        "invalid receipt archive status: {status}"
    );
    let item = if status == "accepted" {
        event_id.ok_or_else(|| EngrError::new(EXIT_TOOL, "accepted receipt needs event id"))?
    } else {
        challenge.as_str()
    };
    let destination = archive_destination(root, status, item)?;
    require!(
        !destination.exists(),
        EXIT_INVARIANT,
        "refusing to overwrite confirmation receipt: {}",
        destination.display()
    );
    let transactions = confirmation_transaction_dir(root);
    fs::create_dir_all(&transactions)
        .map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?;
    let token = Uuid::now_v7();
    let prepared = transactions.join(format!("{status}.{item}.{token}.prepared.json"));
    let claimed = transactions.join(format!("{status}.{item}.{token}.claimed.json"));
    atomic_write(&prepared, &receipt)?;
    fs::rename(source, &claimed)
        .map_err(|error| EngrError::new(EXIT_TOOL, format!("{}: {error}", source.display())))?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?;
    }
    fs::rename(&prepared, &destination)
        .map_err(|error| EngrError::new(EXIT_TOOL, format!("{}: {error}", prepared.display())))?;
    fs::remove_file(&claimed)
        .map_err(|error| EngrError::new(EXIT_TOOL, format!("{}: {error}", claimed.display())))?;
    Ok(receipt)
}
pub fn prepare_candidate(
    root: &Path,
    stream: &str,
    event_type: &str,
    record_text: &str,
    data: Value,
) -> Result<Value> {
    with_project_write_lock(root, || {
        prepare_candidate_locked(root, stream, event_type, record_text, data)
    })
}

fn prepare_candidate_locked(
    root: &Path,
    stream: &str,
    event_type: &str,
    record_text: &str,
    data: Value,
) -> Result<Value> {
    recover_confirmation_transactions(root)?;
    stream_kind(stream)?;
    validate_event_data(event_type, &data)?;
    let events = load_events(root, stream)?;
    let parent = append_parent(&events, stream, event_type, &data)?;
    if events.is_empty() {
        require!(
            event_type
                == if stream_kind(stream)? == "work_item" {
                    "work_item.created"
                } else {
                    "decision.created"
                },
            EXIT_INVARIANT,
            "new stream must begin with the correct root event"
        );
    }
    let (pending, accepted, rejected) = confirmation_dirs(root);
    fs::create_dir_all(&pending).map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?;
    let mut used_challenges = HashSet::new();
    for directory in [&pending, &accepted, &rejected] {
        if !directory.exists() {
            continue;
        }
        for entry in fs::read_dir(directory).map_err(|error| {
            EngrError::new(EXIT_TOOL, format!("{}: {error}", directory.display()))
        })? {
            let path = entry
                .map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?
                .path();
            if path.extension().and_then(|item| item.to_str()) == Some("json") {
                let receipt = read_json(&path, "confirmation receipt")?;
                let challenge = receipt
                    .get("challenge")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        EngrError::new(EXIT_SCHEMA, "confirmation receipt: invalid challenge")
                    })?;
                used_challenges.insert(challenge.to_owned());
            }
        }
    }
    let mut challenge = random_challenge();
    while used_challenges.contains(&challenge) {
        challenge = random_challenge();
    }
    let record = json!({ "text": record_text });
    let digest = candidate_hash(stream, event_type, &record, &data, &parent);
    let receipt = json!({"format":"engineering-confirmation-candidate","protocol_version":PROTOCOL_VERSION,"challenge":challenge,"created_at":now_rfc3339(),"stream":stream,"expected_parent":parent,"event":event_type,"record":record,"data":data,"candidate_sha256":digest,"status":"pending"});
    let human = json!({"initiator":"human","basis":"human_confirmation","confirmation":{"challenge":receipt["challenge"],"candidate_sha256":receipt["candidate_sha256"]}});
    let provisional = build_event(
        root,
        stream,
        event_type,
        record_text,
        receipt.get("data").unwrap(),
        receipt.get("expected_parent").unwrap().clone(),
        if events.is_empty() {
            1
        } else {
            events
                .iter()
                .find(|event| event.get("event_id") == receipt.get("expected_parent"))
                .and_then(|event| event.get("rev"))
                .and_then(Value::as_i64)
                .unwrap()
                + 1
        },
        human,
    )?;
    let mut proposed = events;
    proposed.push(provisional);
    let chain = canonical_chain(&proposed, stream)?;
    validate_creation_date(&chain.chain)?;
    reduce_chain(&chain.chain, None)?;
    for item in
        fs::read_dir(&pending).map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?
    {
        let path = item
            .map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?
            .path();
        if path.extension().and_then(|item| item.to_str()) == Some("json") {
            let old = read_json(&path, "pending confirmation")?;
            if receipt_stream(&old)? == stream {
                archive_receipt(root, &path, "rejected", Some("superseded_candidate"), None)?;
            }
        }
    }
    atomic_write(
        &pending.join(format!("{}.json", receipt["challenge"].as_str().unwrap())),
        &receipt,
    )?;
    Ok(receipt)
}
pub fn discard_candidate(root: &Path, challenge: &str, reason: &str) -> Result<Value> {
    with_project_write_lock(root, || discard_candidate_locked(root, challenge, reason))
}

fn discard_candidate_locked(root: &Path, challenge: &str, reason: &str) -> Result<Value> {
    recover_confirmation_transactions(root)?;
    let (pending, _, _) = confirmation_dirs(root);
    let source = pending.join(format!("{challenge}.json"));
    require!(
        source.exists(),
        EXIT_NOT_FOUND,
        "pending confirmation not found: {challenge}"
    );
    let receipt = read_json(&source, "pending confirmation")?;
    validate_receipt(&receipt, Some(challenge))?;
    archive_receipt(root, &source, "rejected", Some(reason), None)
}
pub fn confirm_candidate(root: &Path, response: &str) -> Result<(Value, Value, PathBuf)> {
    with_project_write_lock(root, || confirm_candidate_locked(root, response))
}

fn confirm_candidate_locked(root: &Path, response: &str) -> Result<(Value, Value, PathBuf)> {
    recover_confirmation_transactions(root)?;
    let words: Vec<_> = response.split(' ').collect();
    if words.len() != 2
        || words[0] != "CONFIRM"
        || words[1].len() != 6
        || !words[1]
            .chars()
            .all(|item| "23456789ABCDEFGHJKLMNPQRSTUVWXYZ".contains(item))
    {
        if response.starts_with("CONFIRM ") {
            let code = response.get(8..14).unwrap_or("");
            let (pending, _, _) = confirmation_dirs(root);
            let source = pending.join(format!("{code}.json"));
            if source.exists() {
                let _ = discard_candidate_locked(root, code, "qualified_or_non_exact_response");
            }
        }
        return Err(EngrError::new(
            crate::EXIT_USAGE,
            "confirmation response must exactly match CONFIRM <code>",
        ));
    }
    let challenge = words[1];
    let (pending, accepted, _) = confirmation_dirs(root);
    let source = pending.join(format!("{challenge}.json"));
    if !source.exists() {
        if accepted.exists() {
            for item in fs::read_dir(&accepted)
                .map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?
            {
                let path = item
                    .map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?
                    .path();
                let receipt = read_json(&path, "accepted confirmation")?;
                if receipt.get("challenge").and_then(Value::as_str) == Some(challenge) {
                    let stream = receipt_stream(&receipt)?;
                    let event_id =
                        receipt
                            .get("event_id")
                            .and_then(Value::as_str)
                            .ok_or_else(|| {
                                EngrError::new(
                                    EXIT_INVARIANT,
                                    "accepted confirmation: invalid event id",
                                )
                            })?;
                    let events = load_events(root, stream)?;
                    let event = events
                        .into_iter()
                        .find(|event| {
                            event.get("event_id").and_then(Value::as_str) == Some(event_id)
                        })
                        .ok_or_else(|| {
                            EngrError::new(EXIT_INVARIANT, "accepted confirmation event is missing")
                        })?;
                    let (state, _) = replay_stream(root, stream, true)?;
                    let path = event_storage_path(root, &event)?;
                    return Ok((event, state, path));
                }
            }
        }
        return Err(EngrError::new(
            crate::EXIT_USAGE,
            format!("confirmation response does not match the pending candidate: {challenge}"),
        ));
    }
    let receipt = read_json(&source, "pending confirmation")?;
    validate_receipt(&receipt, Some(challenge))?;
    let stream = receipt_stream(&receipt)?.to_owned();
    let events = load_events(root, &stream)?;
    let provenance = json!({"initiator":"human","basis":"human_confirmation","confirmation":{"challenge":challenge,"candidate_sha256":receipt["candidate_sha256"]}});
    let matching: Vec<&Value> = events
        .iter()
        .filter(|event| {
            event.get("parent") == Some(&receipt["expected_parent"])
                && event.get("event") == Some(&receipt["event"])
                && event.get("record") == Some(&receipt["record"])
                && event.get("data") == Some(&receipt["data"])
                && event.get("provenance") == Some(&provenance)
        })
        .collect();
    require!(
        matching.len() <= 1,
        EXIT_INVARIANT,
        "confirmation recovery matched multiple events"
    );
    if let Some(event) = matching.into_iter().next() {
        let event = event.clone();
        let (state, _) = replay_stream(root, &stream, true)?;
        let path = event_storage_path(root, &event)?;
        archive_receipt(
            root,
            &source,
            "accepted",
            None,
            Some(event["event_id"].as_str().unwrap()),
        )?;
        return Ok((event, state, path));
    }
    require!(
        append_parent(
            &events,
            &stream,
            receipt["event"].as_str().unwrap(),
            &receipt["data"],
        )? == receipt["expected_parent"],
        EXIT_INVARIANT,
        "the stream head changed after this candidate was prepared"
    );
    let (event, state, path) = append_event_locked(
        root,
        &stream,
        receipt["event"].as_str().unwrap(),
        receipt["record"]["text"].as_str().unwrap(),
        receipt["data"].clone(),
        provenance,
        receipt["expected_parent"].clone(),
    )?;
    archive_receipt(
        root,
        &source,
        "accepted",
        None,
        Some(event["event_id"].as_str().unwrap()),
    )?;
    Ok((event, state, path))
}

fn subset_failures(expected: &Value, actual: &Value, path: &str, failures: &mut Vec<String>) {
    match (expected, actual) {
        (Value::Object(expected), Value::Object(actual)) => {
            for (key, value) in expected {
                match actual.get(key) {
                    Some(actual) => {
                        subset_failures(value, actual, &format!("{path}.{key}"), failures)
                    }
                    None => failures.push(format!("{path}.{key}: missing")),
                }
            }
        }
        (Value::Array(expected), Value::Array(actual)) => {
            if expected.len() != actual.len() {
                failures.push(format!("{path}: length differs"));
            } else {
                for (index, (expected, actual)) in expected.iter().zip(actual).enumerate() {
                    subset_failures(expected, actual, &format!("{path}[{index}]"), failures);
                }
            }
        }
        _ => {
            if expected != actual {
                failures.push(format!("{path}: differs"));
            }
        }
    }
}
pub fn run_conformance(root: &Path) -> Result<Value> {
    let folder = engr_dir(root).join("conformance");
    run_conformance_dir(&folder)
}
pub fn run_conformance_dir(folder: &Path) -> Result<Value> {
    require!(
        folder.is_dir(),
        EXIT_NOT_FOUND,
        "conformance fixtures not found: {}",
        folder.display()
    );
    let mut results = Vec::new();
    let mut failures = Vec::new();
    for entry in
        fs::read_dir(folder).map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?
    {
        let path = entry
            .map_err(|error| EngrError::new(EXIT_TOOL, error.to_string()))?
            .path();
        if path.extension().and_then(|item| item.to_str()) != Some("json") {
            continue;
        }
        let fixture = read_json(&path, "conformance fixture")?;
        let name = fixture
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("unnamed");
        let expected = fixture
            .get("expected")
            .and_then(Value::as_object)
            .ok_or_else(|| EngrError::new(EXIT_SCHEMA, format!("{name}: missing expected")))?;
        let expected_exit = expected.get("exit").and_then(Value::as_i64).unwrap_or(0) as i32;
        let streams = fixture
            .get("streams")
            .and_then(Value::as_object)
            .ok_or_else(|| EngrError::new(EXIT_SCHEMA, format!("{name}: missing streams")))?;
        let command_stream = fixture
            .get("command")
            .and_then(Value::as_object)
            .and_then(|command| command.get("stream"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                EngrError::new(EXIT_SCHEMA, format!("{name}: missing command.stream"))
            })?;
        let outcome = (|| -> Result<Vec<(String, Value, CanonicalChain)>> {
            let mut states = Vec::new();
            for (stream, events) in streams {
                let events = events.as_array().ok_or_else(|| {
                    EngrError::new(
                        EXIT_SCHEMA,
                        format!("{name}: stream {stream} must be array"),
                    )
                })?;
                for event in events {
                    validate_event(event, Some(stream), None)?;
                }
                let canonical = canonical_chain(events, stream)?;
                validate_creation_date(&canonical.chain)?;
                states.push((
                    stream.clone(),
                    reduce_chain(&canonical.chain, None)?,
                    canonical,
                ));
            }
            Ok(states)
        })();
        match outcome {
            Ok(states) if expected_exit == 0 => {
                let (_, state, canonical) = states
                    .into_iter()
                    .find(|(stream, _, _)| stream == command_stream)
                    .ok_or_else(|| {
                        EngrError::new(
                            EXIT_SCHEMA,
                            format!("{name}: command stream {command_stream} is not present"),
                        )
                    })?;
                {
                    let mut faults = Vec::new();
                    if expected
                        .get("head")
                        .is_some_and(|head| state.get("head") != Some(head))
                    {
                        faults.push("head differs".to_owned());
                    }
                    if let Some(status) = expected.get("status").and_then(Value::as_str) {
                        if state.get("status").and_then(Value::as_str) != Some(status) {
                            faults.push("status differs".to_owned());
                        }
                    }
                    if let Some(digest) = expected.get("state_integrity").and_then(Value::as_str) {
                        if state
                            .get("integrity")
                            .and_then(Value::as_object)
                            .and_then(|value| value.get("value"))
                            .and_then(Value::as_str)
                            != Some(digest)
                        {
                            faults.push("state integrity differs".to_owned());
                        }
                    }
                    if let Some(subset) = expected.get("state_subset") {
                        subset_failures(subset, &state, "state_subset", &mut faults);
                    }
                    if let Some(expected_ids) = expected.get("rejected_event_ids") {
                        let actual = Value::Array(
                            canonical
                                .rejected
                                .iter()
                                .cloned()
                                .map(Value::String)
                                .collect(),
                        );
                        if &actual != expected_ids {
                            faults.push("rejected event ids differ".to_owned());
                        }
                    }
                    if let Some(lines) = expected.get("brief_view").and_then(Value::as_array) {
                        let actual = Value::Array(
                            render_state(&state, false, false)?
                                .trim_end()
                                .split('\n')
                                .map(|line| Value::String(line.to_owned()))
                                .collect(),
                        );
                        if actual != Value::Array(lines.clone()) {
                            faults.push("brief view differs".to_owned());
                        }
                    }
                    if let Some(lines) = expected.get("provenance_view").and_then(Value::as_array) {
                        let actual = Value::Array(
                            render_state(&state, true, false)?
                                .trim_end()
                                .split('\n')
                                .map(|line| Value::String(line.to_owned()))
                                .collect(),
                        );
                        if actual != Value::Array(lines.clone()) {
                            faults.push("provenance view differs".to_owned());
                        }
                    }
                    if !faults.is_empty() {
                        failures.push(format!("{name}: {}", faults.join("; ")));
                    }
                }
            }
            Ok(_) => failures.push(format!("{name}: expected exit {expected_exit}, got 0")),
            Err(error) if error.code == expected_exit => {}
            Err(error) => failures.push(format!(
                "{name}: expected exit {expected_exit}, got {} ({})",
                error.code, error.message
            )),
        }
        results.push(json!({"name": name, "passed": !failures.iter().any(|item: &String| item.starts_with(&format!("{name}:")))}));
    }
    require!(
        failures.is_empty(),
        EXIT_INVARIANT,
        "conformance failed: {}",
        failures.join(" | ")
    );
    Ok(json!({"ok": true, "fixtures": results}))
}
