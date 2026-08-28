//! Filesystem layout, locking, and atomic writes.
//!
//! ```text
//! .engr/
//!   format.json              what version wrote this workspace
//!   lock                     one writer at a time
//!   objects/<uuid>.json      the authority
//!   events/<uuid>.jsonl      append-only admitted history
//!   candidates/<CODE>.json   awaiting a human
//!   rules/*.md               project review policy
//!   backlog/<uuid>.json      unresolved staging, admitted by nobody
//!   work/objects/<uuid>.json execution memory, owned by an object, admitted by nobody
//!   collections/<id>.json    planning metadata, admitted by nobody
//! ```
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

use crate::model::{
    project, replay_recoverable_tail, Action, Event, Merge, Object, Provenance, ReviewOutcome,
    EVENT_FORMAT,
};
use crate::{
    ensure, tool_error, Error, Result, EVENT_ENVELOPE_VERSION, EVENT_ENVELOPE_VERSION_V0,
    EXIT_NOT_FOUND, EXIT_SCHEMA, EXIT_USAGE, LEGACY_OBJECT_VERSION_V0, WORKSPACE_VERSION,
};
use fs2::FileExt;
use serde::de::DeserializeOwned;
use serde::Serialize;
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
#[serde(deny_unknown_fields)]
struct Format {
    format: String,
    version: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceFormat {
    LegacyV0,
    /// A recognized workspace at an older version of the authority.
    ///
    /// Distinct from [`Self::LegacyV0`], which has no workspace authority at
    /// all. This one is well formed and says exactly what it is; what it is is
    /// not what this build writes. Both are read-only until `engr migrate`, and
    /// they say different things to whoever is reading the error.
    OlderVersion(u32),
    Current,
}

/// Written by [`init`], because `git add -A` is the normal way people stage a
/// workspace and two of these files must never travel with it. A candidate's
/// filename *is* its challenge code, so committing a live one hands the code to
/// everyone with repository access — the gate assumes it goes to one human and
/// comes back. Telling people not to do that in a README does not stop `-A`.
const GITIGNORE: &str = "\
# The record lives in objects/ — commit that; it is where earlier wording is
# recovered from. events/ is safe to commit too: any challenge codes in it have
# already been spent, and a spent code resolves to nothing.
#
# These four are local only:
#   lock         a mutex for this machine, nothing to share
#   candidates/  each file is named after a *live* challenge code
#   migration-v3/     resumable migration plan for this machine
#   migration-v3.tmp/ plan before its atomic installation
/lock
/candidates/
/migration-v3/
/migration-v3.tmp/
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

/// A v3 migration plan is resumable local state, like the lock and live
/// candidates. Existing predecessor workspaces did not know its name, so
/// migration adds the two ignores without replacing any local ignore policy.
pub(crate) fn ensure_migration_ignored(root: &Path) -> Result<()> {
    let path = engr_dir(root).join(".gitignore");
    let mut text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(tool_error(path.display(), error)),
    };
    let missing: Vec<_> = ["/migration-v3/", "/migration-v3.tmp/"]
        .into_iter()
        .filter(|entry| !text.lines().any(|line| line.trim() == *entry))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    for entry in missing {
        text.push_str(entry);
        text.push('\n');
    }
    fs::write(&path, text).map_err(|error| tool_error(path.display(), error))
}

pub fn validate_format(root: &Path) -> Result<WorkspaceFormat> {
    let migration = engr_dir(root).join("migration-v3");
    ensure!(
        !migration.exists(),
        EXIT_SCHEMA,
        "{} marks an incomplete coordinated migration; run `engr migrate` to resume it",
        migration.display()
    );
    let declared = declared_workspace_version(root)?;
    if declared.is_none() {
        ensure!(
            detect_legacy(root)?,
            EXIT_SCHEMA,
            "{} has no format.json and is not a recognized legacy v0 workspace",
            engr_dir(root).display()
        );
        return Ok(WorkspaceFormat::LegacyV0);
    }
    let version = declared.expect("checked above");
    if version != WORKSPACE_VERSION {
        // Recognized-but-older is reported, not refused, because the workspace
        // is intact and one explicit command moves it forward. Anything else —
        // a version this build has never heard of, including a newer one — is
        // refused outright: reading it under this build's rules is precisely
        // the silent reinterpretation the version exists to prevent.
        ensure!(
            crate::MIGRATABLE_WORKSPACE_VERSIONS.contains(&version),
            EXIT_SCHEMA,
            "workspace version {} is not supported by engr {}",
            version,
            crate::IMPLEMENTATION_VERSION
        );
        return Ok(WorkspaceFormat::OlderVersion(version));
    }
    Ok(WorkspaceFormat::Current)
}

/// Read only the declared authority while a coordinated migration stage may
/// exist. Missing means the legacy generation; callers that are not already
/// holding a validated migration plan must use [`validate_format`] instead.
pub(crate) fn declared_workspace_version(root: &Path) -> Result<Option<u32>> {
    let path = engr_dir(root).join("format.json");
    if !path.exists() {
        return Ok(None);
    }
    // The schema authority is a current resource too, and it is the one every
    // other decoder consults to choose its generation — so it cannot be the one
    // file exempt from the generation's own persisted representation. Checked
    // after parsing rather than before, because the version it declares is what
    // says whether the rule applies at all, and a predecessor `format.json` is
    // read under its own contract. `check_canonical_bytes` does not consult the
    // workspace version, so there is no recursion.
    let text = read_text(&path)?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))?;
    let format: Format = serde_json::from_value(value.clone())
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))?;
    if format.version >= WORKSPACE_VERSION {
        check_canonical_bytes(&path, &text, &value)?;
    }
    ensure!(
        format.format == WORKSPACE_FORMAT,
        EXIT_SCHEMA,
        "{}: not an engr workspace",
        path.display()
    );
    Ok(Some(format.version))
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
        WorkspaceFormat::OlderVersion(version) => Err(Error::new(
            EXIT_SCHEMA,
            format!(
                "workspace version {version} is read-only here; this engr writes version {WORKSPACE_VERSION}. Run `engr migrate` before mutation"
            ),
        )),
    }
}

pub(crate) fn decode_object(path: &Path, id: &str, value: serde_json::Value) -> Result<Object> {
    // The two spellings of one lifecycle field are checked here rather than in
    // the workspace scan, because this is where the answer matters: a file
    // claiming both cannot say which it means, and `status` is read as an alias
    // for `state`, so serde would silently take one and call it authority.
    if let Some(object) = value.as_object() {
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
    }
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

/// Decode an Object under the workspace generation that owns its bytes.
/// Compatibility fields in the in-memory model must never make a current
/// resource's missing member look like a valid legacy default.
pub(crate) fn decode_object_for_version(
    path: &Path,
    id: &str,
    value: serde_json::Value,
    version: u32,
) -> Result<Object> {
    if version >= 3 {
        crate::proof::stored_within_safe_integers(&value, &path.display().to_string())?;
        check_current_object_shape(path, &value)?;
    } else {
        check_predecessor_object_shape(path, version, &value)?;
    }
    let object = decode_object(path, id, value)?;
    if version >= 3 {
        ensure!(
            object.legacy_format.is_none() && object.legacy_version.is_none(),
            EXIT_SCHEMA,
            "{}: a current Object carries no per-resource format markers",
            path.display()
        );
        for section in &object.sections {
            ensure!(
                section
                    .refs
                    .iter()
                    .all(|reference| matches!(reference, crate::model::Ref::Selective(_))),
                EXIT_SCHEMA,
                "{}: a current Object cannot carry a legacy reference",
                path.display()
            );
        }
    }
    Ok(object)
}

/// A current aggregate seal is a digest, not merely a member that is there.
fn check_current_seal(path: &Path, value: Option<&serde_json::Value>) -> Result<()> {
    let seal = value.and_then(serde_json::Value::as_str).ok_or_else(|| {
        Error::new(
            EXIT_SCHEMA,
            format!(
                "{}: a current Object carries an aggregate seal, not a null one",
                path.display()
            ),
        )
    })?;
    ensure!(
        seal.len() == 64
            && seal
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        EXIT_SCHEMA,
        "{}: aggregate seal {seal:?} is not 64 lowercase hex characters",
        path.display()
    );
    Ok(())
}

fn exact_members(
    path: &Path,
    what: &str,
    value: &serde_json::Map<String, serde_json::Value>,
    expected: &[&str],
) -> Result<()> {
    let found: std::collections::BTreeSet<&str> = value.keys().map(String::as_str).collect();
    let expected: std::collections::BTreeSet<&str> = expected.iter().copied().collect();
    ensure!(
        found == expected,
        EXIT_SCHEMA,
        "{}: {what} members are {:?}, expected {:?}",
        path.display(),
        found,
        expected
    );
    Ok(())
}

fn check_current_object_shape(path: &Path, value: &serde_json::Value) -> Result<()> {
    let object = value.as_object().ok_or_else(|| {
        Error::new(
            EXIT_SCHEMA,
            format!("{}: object must be a JSON object", path.display()),
        )
    })?;
    exact_members(
        path,
        "Object",
        object,
        &[
            "id",
            "title",
            "type",
            "state",
            "rev",
            "next_section_id",
            "sections",
            "sha256",
        ],
    )?;
    // Present is not the same as sealed. `Object.sha256` is an `Option` only so
    // historical v1/v2 material still decodes, and `exact_members` above is
    // satisfied by the member existing — so `"sha256": null` decoded to `None`
    // and passed this boundary, producing a current Object in a shape
    // [`save_object`] refuses to write. That is a second current schema no
    // writer can produce, and the read side is where it did damage: every trust
    // diagnostic asks `sha256.as_deref().is_some_and(...)`, and `None` answers
    // false — so an Object with no Sections and its aggregate replaced by null
    // verified as healthy.
    check_current_seal(path, object.get("sha256"))?;
    let sections = object
        .get("sections")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            Error::new(
                EXIT_SCHEMA,
                format!("{}: sections must be an array", path.display()),
            )
        })?;
    let mut previous = None;
    for section in sections {
        let section = section.as_object().ok_or_else(|| {
            Error::new(
                EXIT_SCHEMA,
                format!("{}: a section must be a JSON object", path.display()),
            )
        })?;
        exact_members(
            path,
            "Section",
            section,
            &[
                "id",
                "admission",
                "role",
                "text",
                "content",
                "based_on",
                "refs",
                "relations",
                "sha256",
                "admitted_at",
            ],
        )?;
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
            previous.map_or(true, |previous| previous < id),
            EXIT_SCHEMA,
            "{}: current Sections are stored in increasing id order",
            path.display()
        );
        // Set-valued arrays have one persisted order too, and it is the shared
        // one: JCS each element, then order by those bytes. The canonical-bytes
        // check cannot see this — JCS fixes member order inside an object and
        // leaves arrays exactly as written — so without it a stored set can be
        // reordered, keep valid seals, and load: two accepted encodings for one
        // v3 value, which is the thing this generation says it does not have.
        for field in ["refs", "relations"] {
            let items = section
                .get(field)
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| {
                    Error::new(
                        EXIT_SCHEMA,
                        format!("{}: §{id} {field} must be an array", path.display()),
                    )
                })?;
            let mut canonical = items.clone();
            crate::proof::canonical_set(&mut canonical, field)?;
            ensure!(
                canonical == *items,
                EXIT_SCHEMA,
                "{}: §{id} {field} must be in canonical set order",
                path.display()
            );
        }
        previous = Some(id);
    }
    Ok(())
}

pub(crate) fn check_predecessor_object_shape(
    path: &Path,
    source_version: u32,
    value: &serde_json::Value,
) -> Result<()> {
    let object = value.as_object().ok_or_else(|| {
        Error::new(
            EXIT_SCHEMA,
            format!("{}: object must be a JSON object", path.display()),
        )
    })?;
    ensure!(
        !object.contains_key("sha256"),
        EXIT_SCHEMA,
        "{}: predecessor Object cannot already carry a v3 aggregate seal",
        path.display()
    );
    if source_version == 0 {
        ensure!(
            object.get("format").and_then(serde_json::Value::as_str)
                == Some(crate::model::OBJECT_FORMAT)
                && object.get("version").and_then(serde_json::Value::as_u64)
                    == Some(crate::LEGACY_OBJECT_VERSION_V0.into())
                && object.contains_key("status")
                && !object.contains_key("state"),
            EXIT_SCHEMA,
            "{}: not a recognized legacy v0 Object",
            path.display()
        );
    } else {
        ensure!(
            !object.contains_key("format")
                && !object.contains_key("version")
                && object.contains_key("state")
                && !object.contains_key("status"),
            EXIT_SCHEMA,
            "{}: workspace-v{} Object does not have its generation's canonical envelope",
            path.display(),
            source_version
        );
    }
    let sections = object
        .get("sections")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            Error::new(
                EXIT_SCHEMA,
                format!("{}: sections must be an array", path.display()),
            )
        })?;
    for section in sections {
        let section = section.as_object().ok_or_else(|| {
            Error::new(
                EXIT_SCHEMA,
                format!("{}: a section must be a JSON object", path.display()),
            )
        })?;
        ensure!(
            section.contains_key("confirmed_at")
                && !section.contains_key("admitted_at")
                && !section.contains_key("admission"),
            EXIT_SCHEMA,
            "{}: predecessor Sections use confirmed_at and carry no admission member",
            path.display()
        );
    }
    Ok(())
}

pub fn migrate(root: &Path) -> Result<()> {
    with_lock(root, || crate::migration::run(root))
}

pub(crate) fn write_workspace_format(root: &Path, version: u32) -> Result<()> {
    write_json(
        &engr_dir(root).join("format.json"),
        &Format {
            format: WORKSPACE_FORMAT.to_owned(),
            version,
        },
    )
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

/// [`read_json`] for a resource of the **current** generation, whose persisted
/// bytes are one exact representation rather than a family of equivalent ones.
///
/// Three properties, and they are one check. The bytes have to be the RFC 8785
/// serialization of the value they parse to — which fixes member order, number
/// formatting and insignificant whitespace, and also settles duplicate member
/// names for free: a repeated key collapses during parsing, so the value
/// re-serializes with one occurrence and no longer matches the bytes that had
/// two. That matters beyond tidiness, because a duplicate is where two
/// conforming JSON stacks are allowed to disagree about what the file says.
///
/// On the read path rather than only in the writer, because a current resource
/// arrives through a git merge, a hand edit, a copy or another implementation
/// as readily as through this build's own `write_json`. A writer that emits one
/// representation and a reader that accepts many is not one representation.
///
/// Set-valued arrays are the part this cannot answer: JCS fixes member order
/// inside an object and leaves array order alone. Each resource requires its own
/// sets to be in the shared canonical order where it validates its shape.
pub(crate) fn read_current_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text = read_text(path)?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))?;
    check_canonical_bytes(path, &text, &value)?;
    serde_json::from_value(value)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))
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
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, text.as_bytes())
        .map_err(|error| tool_error(temporary.display(), error))?;
    fs::rename(&temporary, path).map_err(|error| tool_error(path.display(), error))?;
    Ok(())
}

pub fn load_object(root: &Path, id: &str) -> Result<Object> {
    let path = object_path(root, id);
    let text = read_text(&path)?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))?;
    let version = match validate_format(root)? {
        WorkspaceFormat::LegacyV0 => 0,
        WorkspaceFormat::OlderVersion(version) => version,
        WorkspaceFormat::Current => WORKSPACE_VERSION,
    };
    // The canonical-bytes rule belongs to the current generation and only to it.
    // A predecessor Object is read under the contract it was written under, and
    // that contract did not require one persisted spelling — refusing it here
    // would make a valid old workspace unreadable instead of migratable.
    if version >= WORKSPACE_VERSION {
        check_canonical_bytes(&path, &text, &value)?;
    }
    decode_object_for_version(&path, id, value, version)
}

pub(crate) fn save_object(root: &Path, object: &Object) -> Result<()> {
    require_current(root)?;
    object.validate()?;
    ensure!(
        object.legacy_format.is_none() && object.legacy_version.is_none(),
        EXIT_SCHEMA,
        "a current Object carries no per-resource format markers"
    );
    ensure!(
        object.sha256.is_some(),
        EXIT_SCHEMA,
        "object {} has no aggregate integrity seal",
        object.id
    );
    for section in &object.sections {
        ensure!(
            section
                .refs
                .iter()
                .all(|reference| matches!(reference, crate::model::Ref::Selective(_))),
            EXIT_SCHEMA,
            "§{}: a current Object cannot carry a legacy reference",
            section.id
        );
    }
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

/// A record may carry only the shapes its own generation defined.
///
/// One rule, asked on the way in *and* on the way out, because those two are the
/// same question and answering it in only one place is how they drift. Asked
/// only at read, a write path appends history its own next read refuses. Asked
/// only at write, a file that arrived some other way is replayed under rules it
/// was never written against.
///
/// Event generation 1 excludes the merge that names the Section surviving it;
/// generation 2 defines and writes that shape. Accepting it under generation 1
/// would silently redefine retained history and replay a different Object from
/// the one that was admitted.
/// Members this generation's writers have legitimately omitted, at one time or
/// another, without the record meaning anything different.
///
/// `based_on` is the one with history behind it: an absent basis was spelled
/// `null` before it became an absent field, and every event emitted then still
/// says so. The rest are optional semantic members this build omits when they
/// carry nothing, so a record that spells one out explicitly is saying the same
/// thing the longer way.
///
/// A list of what may be *absent from the model's own output*, never a list of
/// what is forbidden. Forgetting to extend this one refuses a spelling; a list
/// of forbidden keys, forgotten, accepts a field from a generation this build
/// does not implement — and only one of those two mistakes is safe.
const OMISSIBLE_EVENT_MEMBERS: &[&str] = &["becomes", "role", "content", "based_on", "relations"];

/// Nothing in the stored bytes went missing on the way into the typed model.
///
/// `Event` cannot use `deny_unknown_fields`: its payload is flattened, and serde
/// forbids the two together. So the check is done against what the decode
/// produced — every member the record carries must be one the model has a place
/// for, and must survive with its value intact.
///
/// It has to happen *before* anything reads the decoded value, because the
/// dropped member is exactly the one that would have said the record is not of
/// this generation. A record carrying admission provenance is not a record of
/// the generation that had only one door; dropping that member leaves the stored
/// `payload_sha256` verifying, since it was never inside the payload, and replay
/// then reports `human` for bytes that said `agent`. Reconciliation can make
/// that reading authoritative after a crash.
///
/// What it must **not** do is treat the current serializer's single spelling as
/// the whole historical read contract. History is read under its own contract
/// and is never rewritten, so a member this build omits today but wrote
/// yesterday is a valid record, not an unknown field.
fn check_nothing_was_dropped(stored: &serde_json::Value, event: &Event) -> Result<()> {
    let decoded = serde_json::to_value(event)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("event: {error}")))?;
    let (stored, decoded) = match (stored.as_object(), decoded.as_object()) {
        (Some(stored), Some(decoded)) => (stored, decoded),
        _ => return Err(Error::new(EXIT_SCHEMA, "event must be a JSON object")),
    };
    for (member, value) in stored {
        match decoded.get(member) {
            Some(kept) => ensure!(
                kept == value,
                EXIT_SCHEMA,
                "{member} did not survive being read, so this record does not mean what it says"
            ),
            None => ensure!(
                OMISSIBLE_EVENT_MEMBERS.contains(&member.as_str()),
                EXIT_SCHEMA,
                "{member} is not a member this generation defines, so this record is not one of its records"
            ),
        }
    }
    Ok(())
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
/// alongside the value it parses to. Appending has no stored form yet, and
/// needs none — a value that came from this build's own model cannot be
/// carrying a member the model has no place for.
fn check_event_record(
    event: &Event,
    id: &str,
    stored: Option<(&str, &serde_json::Value)>,
) -> Result<()> {
    ensure!(
        event.format == EVENT_FORMAT,
        EXIT_SCHEMA,
        "not an engr event"
    );
    if let Some((raw, stored)) = stored {
        check_nothing_was_dropped(stored, event)?;
        if event.version == EVENT_ENVELOPE_VERSION {
            let canonical = serde_json::to_value(event)
                .map_err(|error| Error::new(EXIT_SCHEMA, format!("event: {error}")))?;
            ensure!(
                *stored == canonical,
                EXIT_SCHEMA,
                "Event v2 is not in its exact canonical shape"
            );
            crate::proof::stored_within_safe_integers(stored, "Event v2")?;
            // The contract is not "parses to the same value". It is that the
            // persisted record *is* the RFC 8785 bytes. Comparing parsed values
            // would already have erased member order, insignificant whitespace
            // and any duplicate member name the parser collapsed, and an
            // EventStore arrives through git merge, hand edit and copy as
            // readily as through this build's own append.
            ensure!(
                raw == crate::proof::canonical_bytes(event, "Event v2")?,
                EXIT_SCHEMA,
                "Event v2 is not persisted as its canonical JCS bytes"
            );
        }
    }
    check_event_generation(event)?;
    if event.version == EVENT_ENVELOPE_VERSION {
        let value = serde_json::to_value(event)
            .map_err(|error| Error::new(EXIT_SCHEMA, format!("event: {error}")))?;
        crate::proof::stored_within_safe_integers(&value, "Event v2")?;
        // Revision zero is the Object before any Event: the first admitted one
        // advances it to 1. No writer emits it, and adjacency alone cannot
        // refuse it, because a `0, 1, ...` log is perfectly contiguous.
        ensure!(event.rev >= 1, EXIT_SCHEMA, "Event v2 revisions start at 1");
        ensure!(
            time::OffsetDateTime::parse(
                &event.time,
                &time::format_description::well_known::Rfc3339
            )
            .is_ok(),
            EXIT_SCHEMA,
            "Event v2 time is not RFC3339"
        );
    }
    ensure!(
        event.payload.object == id,
        EXIT_SCHEMA,
        "event belongs to object {:?}, not {:?}",
        event.payload.object,
        id
    );
    event.payload.validate().map_err(|error| {
        Error::new(
            EXIT_SCHEMA,
            format!("invalid event payload: {}", error.message),
        )
    })?;
    let payload_sha256 = event.payload.sha256().map_err(|error| {
        Error::new(
            EXIT_SCHEMA,
            format!("invalid event payload: {}", error.message),
        )
    })?;
    // The retained generation identifies its mutation by hashing it. The
    // mixed-authority generation names the semantic transition instead, so this
    // is asked of the shape that has it rather than of every record.
    if let Some(confirmation) = event.confirmation() {
        ensure!(
            confirmation.payload_sha256 == payload_sha256,
            EXIT_SCHEMA,
            "confirmation does not match the event payload"
        );
    }
    event.provenance.validate()?;
    if let Provenance::Tagged { admission } = &event.provenance {
        if admission.kind == crate::semantics::Admission::Agent
            && !matches!(
                event.payload.action,
                Action::ObjectCreated | Action::ObjectRenamed
            )
        {
            ensure!(
                admission.rule_review.is_some(),
                EXIT_SCHEMA,
                "an Agent semantic Object event records the passing Rule Review that admitted it"
            );
        }
    }
    Ok(())
}

fn check_event_generation(event: &Event) -> Result<()> {
    // The generation itself, before anything about its contents. Leaving this to
    // the read side alone was the same asymmetry one level up: a record naming a
    // generation this build does not support could be written and then refused
    // by the very next read of the log it was written to.
    match event.version {
        EVENT_ENVELOPE_VERSION_V0 => {
            ensure!(
                !matches!(
                    &event.payload.action,
                    Action::SectionMerged {
                        merge: Merge::Into { .. }
                    }
                ),
                EXIT_SCHEMA,
                "event version {} does not define a merge that names the section surviving it",
                event.version
            );
            ensure!(
                matches!(event.provenance, Provenance::Confirmed { .. }),
                EXIT_SCHEMA,
                "event version {} carries retained confirmation provenance",
                event.version
            );
            ensure!(
                event
                    .payload
                    .content
                    .refs
                    .iter()
                    .all(|reference| matches!(reference, crate::model::Ref::Legacy(_))),
                EXIT_SCHEMA,
                "event version {} cannot carry selective references",
                event.version
            );
            Ok(())
        }
        EVENT_ENVELOPE_VERSION => {
            ensure!(
                !matches!(
                    &event.payload.action,
                    Action::SectionMerged {
                        merge: Merge::Absorbing { .. }
                    }
                ),
                EXIT_SCHEMA,
                "event version {} does not define the retained allocating merge",
                event.version
            );
            ensure!(
                matches!(event.provenance, Provenance::Tagged { .. }),
                EXIT_SCHEMA,
                "event version {} carries tagged admission provenance",
                event.version
            );
            ensure!(
                event
                    .payload
                    .content
                    .refs
                    .iter()
                    .all(|reference| matches!(reference, crate::model::Ref::Selective(_))),
                EXIT_SCHEMA,
                "event version {} cannot carry legacy references",
                event.version
            );
            event.payload.content.require_canonical_order()?;
            if let Provenance::Tagged { admission } = &event.provenance {
                if admission.kind == crate::semantics::Admission::Agent {
                    ensure!(
                        event.payload.becomes.is_none(),
                        EXIT_SCHEMA,
                        "an Agent Event cannot carry becomes"
                    );
                    ensure!(
                        !matches!(
                            event.payload.action,
                            Action::ObjectClosed
                                | Action::ObjectReopened
                                | Action::ObjectClassified { .. }
                                | Action::ObjectSuperseded
                        ),
                        EXIT_SCHEMA,
                        "this lifecycle action requires Human admission"
                    );
                }
            }
            Ok(())
        }
        _ => Err(Error::new(
            EXIT_SCHEMA,
            format!("unsupported event version {}", event.version),
        )),
    }
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
    with_lock(root, || check_appendable_locked(root, event))
}

/// The durable append, for a caller already inside [`with_lock`] — which is
/// every caller, because the only routes here are `confirm` and `admit_agent`.
pub(crate) fn append_event_locked(root: &Path, event: &Event) -> Result<()> {
    check_appendable_locked(root, event)?;
    let path = events_path(root, &event.payload.object);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| tool_error(parent.display(), error))?;
    }
    let line = crate::proof::canonical_bytes(event, "Event v2")?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| tool_error(path.display(), error))?;
    use std::io::Write;
    writeln!(file, "{line}").map_err(|error| tool_error(path.display(), error))
}

/// Every check the append performs, and none of the writing.
fn check_appendable_locked(root: &Path, event: &Event) -> Result<()> {
    // The durable Event path is part of the workspace-generation boundary, and a
    // direct library caller reaches it without passing the gate. Asked here as
    // well as there, because "this build may write this workspace" is a property
    // of the workspace rather than of the route taken to it.
    require_current(root)?;
    ensure!(
        event.version == EVENT_ENVELOPE_VERSION,
        EXIT_SCHEMA,
        "current workspaces append Event generation {EVENT_ENVELOPE_VERSION}, not {}",
        event.version
    );
    // Before anything is written, and before the Object is saved — `confirm`
    // appends here first, so refusing at this point leaves the workspace exactly
    // as it was rather than advanced past history it could not record.
    check_event_record(event, &event.payload.object, None)?;
    let id = &event.payload.object;
    // Continuity against what is already there, which is the one part of the
    // read contract that is about the file rather than the record. Reading the
    // tail is the cost of not being able to append a revision the next load
    // would refuse.
    let mut tail = load_events(root, id)?;
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
    // impossible: revising a Section that does not exist, or beginning a history
    // with something no Object comes from. `.engr/events` is append-only and is
    // never purged, so such a record is not a mistake anybody can take back — it
    // durably breaks every read that reconstructs the Object, and it breaks
    // crash recovery, which is the one thing this file is for.
    //
    // The same check the store already applies to a retained tail, asked before
    // the tail exists rather than after, and inside the same lock so what is
    // validated is what gets written.
    tail.push(event.clone());
    let object = match load_object(root, id) {
        Ok(object) => Some(object),
        Err(error) if error.code == EXIT_NOT_FOUND => None,
        Err(error) => return Err(error),
    };
    validate_recoverable_tail(id, object, &tail)?;
    // Last, and only for a record that is otherwise sound: a malformed Event
    // should be refused for being malformed, not for failing a proof about a
    // shape nothing could admit anyway.
    crate::gate::check_admission(root, event)
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
    decode_events(root, &path, id, &text)
}

/// Decode the one EventStore text a caller already captured.
///
/// Migration needs this form because fingerprinting one read and replaying a
/// second one can derive a plan from bytes the manifest does not name. Ordinary
/// reads take the same route after obtaining their text from disk.
pub(crate) fn decode_events(root: &Path, path: &Path, id: &str, text: &str) -> Result<Vec<Event>> {
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
    check_event_history(root, path, id, &events)?;
    Ok(events)
}

/// History is append-only evidence, not a replaceable recovery cache. Its
/// first admitted transition is therefore always revision 1, and a generation
/// that introduced mixed authority cannot be followed by the older semantic
/// generation again.
fn check_event_history(root: &Path, path: &Path, id: &str, events: &[Event]) -> Result<()> {
    ensure!(
        !events.is_empty(),
        EXIT_SCHEMA,
        "{} is an empty Event history file",
        path.display()
    );
    ensure!(
        events[0].rev == 1,
        EXIT_SCHEMA,
        "{}: non-empty Event history starts at revision {}, not 1",
        path.display(),
        events[0].rev
    );
    let mut seen_v2 = false;
    for event in events {
        if event.version == EVENT_ENVELOPE_VERSION {
            seen_v2 = true;
        } else {
            ensure!(
                !seen_v2,
                EXIT_SCHEMA,
                "{}: retained Event generation 1 cannot follow generation 2",
                path.display()
            );
        }
    }
    check_human_candidate_digests(root, id, events)
}

/// Exactly the Sections the Human rechecks below will read.
///
/// A CandidateDigest is operation-specific. `object.renamed` hashes the title
/// and lifecycle either side and names no Section at all; a Section operation
/// names its own participants. Only `section_merged` and `object.superseded`
/// project the whole Object, because `ObjectInvariant` carries every Section's
/// semantics.
///
/// Converting more than that is not a wasted cycle, it is a wider precondition:
/// every legacy Ref on a converted Section has its pinned commit reopened, so
/// converting the whole Object makes an unrelated Ref's availability decide
/// whether the Object can be read at all. Asking for the operation's own inputs
/// keeps a lost commit a fact about the operations that actually depend on it.
///
/// A Section this generation *adds* cannot carry a legacy Ref — it did not exist
/// before the boundary — and the before-state of an add names no Section, so
/// `section.added` needs nothing converted.
fn sections_the_rechecks_read(events: &[Event]) -> crate::migration::Migrated {
    let mut wanted = crate::migration::Migrated::nothing();
    for event in events {
        if event.version != EVENT_ENVELOPE_VERSION || event.human_confirmation().is_none() {
            continue;
        }
        wanted.widen(&event.payload.action);
    }
    wanted
}

/// Event-v2 keeps a Human candidate digest after its short-lived envelope is
/// deleted. The transition it seals remains reconstructable from immutable
/// history, so accepting only the digest's scalar syntax would turn durable
/// admission evidence into an unauthenticated label after confirmation.
fn check_human_candidate_digests(root: &Path, id: &str, events: &[Event]) -> Result<()> {
    let wanted = sections_the_rechecks_read(events);
    let mut projection = Object::new(id.to_owned(), String::new())?;
    let mut migrated = false;
    for event in events {
        if event.version == EVENT_ENVELOPE_VERSION && !migrated {
            projection = crate::migration::migrated_replay(root, projection, &wanted)?;
            migrated = true;
        }
        let before = projection.clone();
        project(&mut projection, event).map_err(|error| {
            Error::new(
                EXIT_SCHEMA,
                format!(
                    "{id}: Event history cannot reconstruct its transition: {}",
                    error.message
                ),
            )
        })?;
        let Some(confirmation) = event.human_confirmation() else {
            continue;
        };
        let review_digest = match &event.provenance {
            Provenance::Tagged { admission } => admission.rule_review.as_ref().and_then(|review| {
                (review.outcome == ReviewOutcome::Overridden).then(|| review.review_digest.clone())
            }),
            Provenance::Confirmed { .. } => None,
        };
        let subject =
            crate::proof::candidate_subject(&before, &projection, &event.payload, review_digest)?;
        ensure!(
            subject.digest()? == confirmation.candidate_digest,
            EXIT_SCHEMA,
            "{id}: Event rev {} carries a candidate digest no Human transition produced",
            event.rev
        );
    }
    Ok(())
}
