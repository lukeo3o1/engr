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
//!   work/objects/<uuid>.json execution memory, owned by an object, confirmed by nobody
//!   collections/<id>.json    planning metadata, confirmed by nobody
//! ```

use crate::model::{replay_recoverable_tail, Action, Event, Merge, Object, EVENT_FORMAT};
use crate::semantics::Admission;
use crate::{
    ensure, tool_error, Error, Result, EVENT_ENVELOPE_VERSION, EVENT_ENVELOPE_VERSION_V1,
    EXIT_NOT_FOUND, EXIT_SCHEMA, EXIT_USAGE, LEGACY_OBJECT_VERSION_V0, WORKSPACE_VERSION,
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
    /// No `format.json` at all, recognized from the legacy per-resource markers
    /// every Object still carries. This one predates the workspace authority.
    LegacyV0,
    /// A recognized workspace at an older version of the authority.
    ///
    /// Well formed, and says exactly what it is; what it is is not what this
    /// build writes.
    OlderVersion(u32),
    /// The authority names the version this build writes, but a resource still
    /// uses a representation migration replaces.
    ///
    /// Its own case rather than being reported as one of the two above, because
    /// the reader's next question — what is stale about mine — has a different
    /// answer here: the workspace says it is current and one of its files
    /// disagrees. A hand edit reaches this, and so does a file restored from
    /// before a migration.
    SupersededResources,
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
        crate::work::dir(root),
        crate::collection::dir(root),
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
    if format.version != WORKSPACE_VERSION {
        // Recognized-but-older is reported, not refused, because the workspace
        // is intact and one explicit command moves it forward. Anything else —
        // a version this build has never heard of, including a newer one — is
        // refused outright: reading it under this build's rules is precisely
        // the silent reinterpretation the version exists to prevent.
        ensure!(
            crate::MIGRATABLE_WORKSPACE_VERSIONS.contains(&format.version),
            EXIT_SCHEMA,
            "workspace version {} is not supported by engr {}",
            format.version,
            crate::IMPLEMENTATION_VERSION
        );
        return Ok(WorkspaceFormat::OlderVersion(format.version));
    }
    if contains_legacy_objects(root)? {
        return Ok(WorkspaceFormat::SupersededResources);
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

/// Does any Object here still use a representation migration replaces?
///
/// Asked of every command, in every domain, because the answer decides whether
/// the workspace may be mutated at all. That reach is why it must not fail on a
/// file it cannot read: one malformed Object used to make `backlog ls`, every
/// Work command and every Collection command exit with a parse error about a
/// file none of them were going to touch — a single bad byte disabling three
/// domains that do not depend on it.
///
/// So a file that will not read is not evidence of anything here, and is not an
/// error here either. It stays an error where it matters: `decode_object`
/// refuses it the moment something actually loads that Object, `verify` reports
/// it, and `preflight_migration` still validates every retained representation
/// before moving any of them. Failing closed on Object authority and staying
/// out of the way of the other domains are the same rule, applied where each
/// belongs.
fn contains_legacy_objects(root: &Path) -> Result<bool> {
    let mut legacy = false;
    for id in object_ids(root)? {
        let path = object_path(root, &id);
        let Ok(mut value) = read_json::<serde_json::Value>(&path) else {
            continue;
        };
        // The same question migration asks, asked by the same function, so this
        // scan cannot come to know about fewer superseded representations than
        // the migration can repair. A scan that knew about only some of them
        // would report a workspace as current while holding exactly the shape
        // that `engr migrate` exists to put right — and `migrate` would then
        // decline it as needing nothing.
        //
        // A file that contradicts itself is not counted here on the strength of
        // one spelling: the conversion refuses it, and `decode_object` refuses
        // it again the moment something loads that Object.
        legacy |= to_current_object(
            &path.display().to_string(),
            Generation::at(WORKSPACE_VERSION),
            Reading::Repairing,
            &mut value,
        )
        .unwrap_or(false);
    }
    Ok(legacy)
}

pub fn require_current(root: &Path) -> Result<()> {
    // Named rather than lumped together: "legacy v0" is wrong and confusing for
    // a workspace that states a version perfectly clearly, and the reader's next
    // question — what is stale about mine — has a different answer in each case.
    match validate_format(root)? {
        WorkspaceFormat::Current => Ok(()),
        WorkspaceFormat::LegacyV0 => Err(Error::new(
            EXIT_SCHEMA,
            "legacy v0 workspace is read-only; run `engr migrate` before mutation".to_owned(),
        )),
        WorkspaceFormat::SupersededResources => Err(Error::new(
            EXIT_SCHEMA,
            format!(
                "this workspace says version {WORKSPACE_VERSION}, but a resource still uses a representation that version replaced; it is read-only until `engr migrate` states what those files mean"
            ),
        )),
        WorkspaceFormat::OlderVersion(version) => Err(Error::new(
            EXIT_SCHEMA,
            format!(
                "workspace version {version} is read-only here; this engr writes version {WORKSPACE_VERSION}. Run `engr migrate` before mutation"
            ),
        )),
    }
}

struct MigrationEntry {
    id: String,
    path: PathBuf,
    object: Object,
    migrated: Option<serde_json::Value>,
}

/// Rewrite one stored Object into the representation this build understands,
/// in place, reporting whether anything moved.
///
/// The one place a superseded representation is interpreted. Everything that
/// reads an Object — the load path, migration, and the historical reader in
/// [`crate::git`] — goes through here first, so a shape that predates a field
/// means exactly one thing no matter who reads it, and a shape that contradicts
/// itself is refused everywhere rather than wherever somebody remembered to
/// check.
///
/// The alternative was a serde `alias` and a `default`, and it is worse in a way
/// that matters: serde would answer both questions silently, at every read, with
/// no way to distinguish a file that predates a field from one that lost it. A
/// file claiming both spellings cannot say which it means, and is refused here
/// rather than resolved by whichever one serde happened to take.
///
/// Every conversion restates a guarantee the old protocol had already made. No
/// authority is invented: a Section written before [`Admission`] existed was
/// admitted through the Human Gate, because that was the only door there was.
pub fn to_current_object(
    label: &str,
    source: Generation,
    reading: Reading,
    value: &mut serde_json::Value,
) -> Result<bool> {
    let object = value.as_object_mut().ok_or_else(|| {
        Error::new(
            EXIT_SCHEMA,
            format!("{label}: object must be a JSON object"),
        )
    })?;
    let mut moved = false;
    if let Some(status) = take_superseded(object, "status", "state", label)? {
        object.insert("state".to_owned(), status);
        moved = true;
    }
    ensure!(
        object.contains_key("state"),
        EXIT_SCHEMA,
        "{label}: object has neither status nor state"
    );
    let Some(sections) = object
        .get_mut("sections")
        .and_then(|value| value.as_array_mut())
    else {
        return Ok(moved);
    };
    for (index, section) in sections.iter_mut().enumerate() {
        let label = format!("{label}: section {index}");
        let section = section
            .as_object_mut()
            .ok_or_else(|| Error::new(EXIT_SCHEMA, format!("{label} must be a JSON object")))?;
        if source.admits_mixed_authority() {
            // A resource restored from before this generation, sitting under an
            // authority that already names it. Reading it is refused — a file
            // that disagrees with its own generation is not evidence of anything
            // — but migration must be able to *repair* it, or the one explicit
            // recovery command has nothing to say about the one representation
            // this generation replaced.
            //
            // Narrow, and narrow is what makes it safe: only the exact earlier
            // shape, which carries no authority claim at all. A section offering
            // an `admission` alongside `confirmed_at` is offering something no
            // build of the earlier generation could have written, and is refused
            // here exactly as it is below.
            if reading == Reading::Repairing
                && section.contains_key("confirmed_at")
                && !section.contains_key("admission")
                && !section.contains_key("admitted_at")
            {
                let confirmed_at = section.remove("confirmed_at").expect("checked above");
                section.insert("admitted_at".to_owned(), confirmed_at);
                section.insert(
                    "admission".to_owned(),
                    serde_json::Value::String(Admission::Human.as_str().to_owned()),
                );
                moved = true;
                continue;
            }
            // Its own generation's shape, exactly. `admission` is required
            // there, so absence is a file that lost it rather than one that
            // predates it, and reading a lost field as `human` would invent the
            // answer this field exists to stop anybody inventing.
            ensure!(
                !section.contains_key("confirmed_at"),
                EXIT_SCHEMA,
                "{label}: workspace version {} spells this admitted_at",
                source.version()
            );
            ensure!(
                section.contains_key("admission") && section.contains_key("admitted_at"),
                EXIT_SCHEMA,
                "{label}: workspace version {} requires admission and admitted_at",
                source.version()
            );
            continue;
        }
        // Below the mixed-authority generation there is exactly one answer, and
        // the file does not get to supply it. `agent` is an authority no version
        // before this one had a path to produce, so a section claiming it did
        // not come from that version — it was written by something else, and
        // treating it as evidence would let a hand edit mint authority a human
        // never gave. Fail closed instead: a field the source generation never
        // defined is not data, it is a contradiction.
        for invented in ["admission", "admitted_at"] {
            ensure!(
                !section.contains_key(invented),
                EXIT_SCHEMA,
                "{label}: workspace version {} has no {invented}, so this file is not what it says it is",
                source.version()
            );
        }
        let confirmed_at = section.remove("confirmed_at").ok_or_else(|| {
            Error::new(
                EXIT_SCHEMA,
                format!(
                    "{label}: workspace version {} requires confirmed_at",
                    source.version()
                ),
            )
        })?;
        section.insert("admitted_at".to_owned(), confirmed_at);
        // Mechanical, and justified by the old protocol rather than by the file:
        // every Section admitted under that generation went through the Human
        // Gate, because that was the only door it had.
        section.insert(
            "admission".to_owned(),
            serde_json::Value::String(Admission::Human.as_str().to_owned()),
        );
        moved = true;
    }
    Ok(moved)
}

/// Whether this read may repair a superseded representation it finds, or only
/// report it.
///
/// Two different questions, and answering them with one rule was the mistake:
/// a reader must refuse a resource that disagrees with its own generation,
/// because such a file is not evidence of anything — while `engr migrate`, the
/// one explicit recovery command, must be able to put it right. Without the
/// distinction the only representation this generation replaces is precisely the
/// one migration cannot fix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reading {
    /// Refuse anything that is not exactly what its generation defines.
    Strict,
    /// Additionally convert the earlier generation's shape where it carries no
    /// claim that generation could not have made.
    Repairing,
}

/// The workspace generation a stored representation was written under.
///
/// Carried explicitly rather than inferred from the bytes, because inferring it
/// is precisely the mistake: a file is then read under whichever contract its
/// own contents suggest, and a field nobody could legitimately have written
/// becomes the evidence that it was legitimate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Generation(u32);

/// The generation of a workspace with no `format.json`, which predates the
/// authority entirely.
pub const LEGACY_GENERATION: Generation = Generation(0);

impl Generation {
    pub fn at(version: u32) -> Self {
        Self(version)
    }

    pub fn version(self) -> u32 {
        self.0
    }

    /// Whether Sections written under this generation say which door they came
    /// through. Below it, there was only one door.
    fn admits_mixed_authority(self) -> bool {
        self.0 >= MIXED_AUTHORITY_WORKSPACE_VERSION
    }
}

/// The first workspace version whose Sections carry `admission`.
const MIXED_AUTHORITY_WORKSPACE_VERSION: u32 = 3;

/// What generation this workspace's stored resources were written under.
///
/// Read from the authority alone, without looking at a single resource. Any
/// answer derived from the resources would be the resources deciding which
/// contract to be read under.
fn declared_generation(root: &Path) -> Result<Generation> {
    let path = engr_dir(root).join("format.json");
    if !path.exists() {
        return Ok(LEGACY_GENERATION);
    }
    let format: Format = read_json(&path)?;
    Ok(Generation::at(format.version))
}

/// Take the value of a field whose spelling was replaced, refusing a record that
/// carries both spellings.
fn take_superseded(
    value: &mut serde_json::Map<String, serde_json::Value>,
    superseded: &str,
    current: &str,
    label: &str,
) -> Result<Option<serde_json::Value>> {
    ensure!(
        !(value.contains_key(superseded) && value.contains_key(current)),
        EXIT_SCHEMA,
        "{label}: contains both legacy {superseded} and canonical {current}, so it cannot say which it means"
    );
    Ok(value.remove(superseded))
}

fn decode_object(
    path: &Path,
    id: &str,
    source: Generation,
    mut value: serde_json::Value,
) -> Result<Object> {
    // Reading an unmigrated workspace is allowed; writing to one is not. So the
    // conversion happens here, on the way in, and `require_current` is what
    // stops anything from being written back through it.
    to_current_object(
        &path.display().to_string(),
        source,
        Reading::Strict,
        &mut value,
    )?;
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
    let source = declared_generation(root)?;
    let mut entries = Vec::new();
    for id in object_ids(root)? {
        let path = object_path(root, &id);
        let mut planned: serde_json::Value = read_json(&path)?;
        let moved = to_current_object(
            &path.display().to_string(),
            source,
            Reading::Repairing,
            &mut planned,
        )?;
        // Deserialize the planned form, not just its converted keys. This
        // catches missing required fields and illegal legacy values before any
        // neighboring file is rewritten.
        let object = decode_object(
            &path,
            &id,
            Generation::at(WORKSPACE_VERSION),
            planned.clone(),
        )?;
        entries.push(MigrationEntry {
            id,
            path,
            object,
            migrated: moved.then_some(planned),
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
    let before = validate_format(root)?;
    ensure!(
        before != WorkspaceFormat::Current,
        EXIT_SCHEMA,
        "workspace does not require migration"
    );
    // Every retained representation is validated first, then this. The order is
    // the point: a workspace that could not migrate anyway must hear why, and
    // hear it in the same terms it always did, rather than being turned away at
    // the door by an unrelated refusal that hides a real corruption.
    let entries = preflight_migration(root)?;
    // Only a migration that would *change* the declared version is held back.
    // Repairing a workspace whose authority already names this version moves it
    // nowhere: it is already here, and refusing would leave the one explicit
    // recovery command unable to put right the one representation this version
    // replaced.
    let moves_the_version = before != WorkspaceFormat::SupersededResources;
    ensure!(
        crate::WORKSPACE_VERSION_IS_COMPLETE || !moves_the_version,
        EXIT_SCHEMA,
        "this build implements workspace version {WORKSPACE_VERSION}, which is not finished yet, so it will not move a workspace into it. Migration is one way and confirmed history is never rewritten, so a workspace migrated into a shape that is still changing could not be brought the rest of the way afterwards. Use a released build"
    );
    for entry in entries {
        if let Some(value) = entry.migrated {
            write_json(&entry.path, &value)?;
        }
    }
    let format_path = engr_dir(root).join("format.json");
    // Rewritten when it is absent *or* when it names an older version. A
    // workspace already at the current version keeps its file byte for byte:
    // migration exists to move representation, not to touch what is already
    // right.
    if !format_path.exists() || matches!(before, WorkspaceFormat::OlderVersion(_)) {
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
    decode_object(&path, id, declared_generation(root)?, value)
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
            event.version == EVENT_ENVELOPE_VERSION || event.version == EVENT_ENVELOPE_VERSION_V1,
            EXIT_SCHEMA,
            "{}:{}: unsupported event version {}",
            path.display(),
            index + 1,
            event.version
        );
        // A generation is a statement about what its payloads mean, so a record
        // may only carry the shapes its own generation defined. Without this the
        // two merge shapes would be interchangeable in either direction: a v1
        // record could name a survivor the v1 contract never had, and a v2
        // record could consume every participant and allocate a fresh id — which
        // is exactly the behaviour version 2 exists to have replaced.
        if let Action::SectionMerged { merge } = &event.payload.action {
            let expected = match merge {
                Merge::Into { .. } => EVENT_ENVELOPE_VERSION,
                Merge::Absorbing { .. } => EVENT_ENVELOPE_VERSION_V1,
            };
            ensure!(
                event.version == expected,
                EXIT_SCHEMA,
                "{}:{}: event version {} does not use this merge representation",
                path.display(),
                index + 1,
                event.version
            );
        }
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
