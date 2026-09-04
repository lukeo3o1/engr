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
//! visibility's own sake. Event provenance is deliberately minimal — `outcome`,
//! `result` and `attempts`, and none of the material the decision was made
//! against. The exact Rule artifacts, the digest that bound them and the agent's
//! explanation are decision-time material and stay out. So every mutation
//! carries one Agent-attested attempt, each applicable Rule judges it against
//! its own ceiling, and past any ceiling autonomous admission stops — and what
//! survives is the number, not the judgement it was put through. Re-deriving
//! what the record *does* carry can therefore never ask the question, and a
//! public raw append would be a second Agent admission API holding strictly less
//! state than the gate. [`check_appendable`] is the read-only half.
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

/// No persisted resource is reached by following a link.
///
/// Every path here is a workspace resource, and a workspace resource is what
/// git tracks at that path. A link breaks that in a way no digest can see: git
/// records the link — its target's *name*, as a blob — while engr reads and
/// writes the target's *bytes*, so `.engr` or any directory or file beneath it
/// can redirect the whole record outside the repository entirely, and the
/// history a reviewer reads is then not the state the tool is using. It is the
/// same provenance split the Rule loader refuses, for the same reason.
///
/// Anchored at the `.engr` component and inclusive of it, because a link *there*
/// is the one that redirects everything: a check that started below it would
/// have every path pass through the redirection before being examined. Nothing
/// above the workspace is examined — a repository reached through a link is
/// ordinary (macOS hands every temporary directory out that way), and the
/// question is never how somebody arrived at the workspace, only whether the
/// resources inside it are the ones the workspace holds.
///
/// A path that is not inside a `.engr` directory is not this boundary's
/// business and passes untouched.
pub(crate) fn contained(path: &Path) -> Result<()> {
    let mut parts = path.components();
    let mut cursor = PathBuf::new();
    // Everything above `.engr`, taken on trust and never stat'ed.
    for part in parts.by_ref() {
        cursor.push(part);
        if part.as_os_str() == DIR {
            break;
        }
    }
    if cursor.file_name().map(|name| name != DIR).unwrap_or(true) {
        return Ok(());
    }
    for part in std::iter::once(cursor.clone()).chain(parts.map(|part| {
        cursor.push(part);
        cursor.clone()
    })) {
        match fs::symlink_metadata(&part) {
            Ok(held) => ensure!(
                !held.file_type().is_symlink(),
                EXIT_SCHEMA,
                "{}: something on the way to this resource is a link to somewhere else, so what engr would read is not what this workspace holds",
                part.display()
            ),
            // Nothing below a path that is not there can be there either, and a
            // resource yet to be written is the ordinary case for a write.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(tool_error(part.display(), error)),
        }
    }
    Ok(())
}

/// Whether a resource namespace is there, on three-way terms.
///
/// **`is_dir()` gives two answers where there are three, and the missing one is
/// the dangerous one.** It follows links and reports every other kind of entry —
/// a dangling link, a regular file, a device, a directory nobody may stat — as
/// though nothing were there at all. Every enumerator in this workspace then
/// turns that into an empty resource set: a `.engr/objects` that is a regular
/// file becomes a workspace with no Objects, which is a far worse answer than a
/// refusal, and in a migration source it becomes a predecessor with nothing to
/// migrate rather than one this build must not touch.
///
/// So: only established absence is absence. A wrong shape, a link anywhere on
/// the way, and any other stat failure all fail closed.
pub(crate) fn namespace(path: &Path) -> Result<bool> {
    contained(path)?;
    let listed = match fs::symlink_metadata(path) {
        Ok(listed) => listed,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        // Not knowing is not absence.
        Err(error) => return Err(tool_error(path.display(), error)),
    };
    ensure!(
        listed.is_dir(),
        EXIT_SCHEMA,
        "{}: exists but is not a directory, so this namespace can be neither read nor called empty",
        path.display()
    );
    Ok(true)
}

/// Whether one persisted resource file is there, on the same three-way terms.
///
/// `exists()` follows links, so a redirected resource whose target is gone
/// reported as a plain absence — and absence is a legitimate answer for most of
/// these paths, so the redirection disappeared into an ordinary "not found"
/// instead of being refused as what it is.
pub(crate) fn resource_present(path: &Path) -> Result<bool> {
    contained(path)?;
    let listed = match fs::symlink_metadata(path) {
        Ok(listed) => listed,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(tool_error(path.display(), error)),
    };
    ensure!(
        listed.is_file(),
        EXIT_SCHEMA,
        "{}: exists but is not a regular file, so it is neither a resource nor absent",
        path.display()
    );
    Ok(true)
}

/// What a directory says about being a workspace root.
///
/// The same three answers [`namespace`] gives, named for the walk that consumes
/// them: only established absence may ascend, because ascending past the
/// workspace somebody is standing in lands their next command on an ancestor's
/// record.
enum Here {
    Workspace,
    Absent,
}

fn workspace_here(root: &Path) -> Result<Here> {
    match namespace(&engr_dir(root))? {
        true => Ok(Here::Workspace),
        false => Ok(Here::Absent),
    }
}

/// Walk up from `start` looking for a workspace, so the tool works from any
/// subdirectory the way git does.
pub fn find_root(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return match workspace_here(path)? {
            Here::Workspace => Ok(path.to_path_buf()),
            Here::Absent => Err(Error::new(
                EXIT_NOT_FOUND,
                format!("no {DIR} workspace at {}", path.display()),
            )),
        };
    }
    let current =
        std::env::current_dir().map_err(|error| tool_error("current directory", error))?;
    let mut cursor = current.as_path();
    loop {
        if let Here::Workspace = workspace_here(cursor)? {
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
    // On the same three-way terms as every other probe, because this one decides
    // whether a workspace gets *created*: `exists()` reads a dangling `.engr`
    // link as nothing there, and `create_dir_all` then materializes the whole
    // layout behind the link, outside the repository, under a name git records
    // as a link. Only established absence may proceed.
    ensure!(
        !namespace(&dir)?,
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
        create_dir_durably(&path)?;
    }
    // **The generation marker is written last, and that is the whole ordering.**
    // Its presence is what makes a workspace current — `require_current` asks
    // nothing else — so writing it before the layout is complete means a failure
    // or a crash in the remaining window leaves an *active* workspace that is
    // missing part of itself, and nothing afterwards repairs or refuses it.
    //
    // The ignore line is the one that matters here rather than a tidiness: a
    // live Challenge's filename **is** its code, and `/local/` is what keeps
    // `git add -A` from handing that code to everyone with repository access.
    // A workspace that activated without it is one where the Human Gate's own
    // secret is Git-trackable, which is not a state a later command can detect
    // as wrong. Migration activates the same way, for the same reason.
    let ignore = dir.join(".gitignore");
    write_text(&ignore, GITIGNORE)?;
    write_text(&version_path(root), crate::WORKSPACE_VERSION_FILE)?;
    Ok(dir)
}

/// Whether this workspace carries the current generation marker, on the same
/// three-way terms as every other resource.
///
/// **This is the generation boundary, not a resource diagnostic.** Its answer
/// decides which storage contract may be interpreted and which migration path
/// may write, so `exists()` is the wrong question twice over: it follows links,
/// and it reports a dangling one as absence. A dangling `.engr/VERSION` beside a
/// live predecessor bootstrap would classify the workspace as the released
/// predecessor and hand it to migration, rather than refusing a generation
/// authority this build cannot establish. Several callers ask it, which is why
/// it is one function rather than a repeated probe.
pub(crate) fn generation_present(root: &Path) -> Result<bool> {
    resource_present(&version_path(root))
}

pub fn validate_format(root: &Path) -> Result<WorkspaceFormat> {
    // `VERSION` first, and the order is the whole of what makes this correct.
    // It is written last, after every destination byte is published, so its
    // presence *is* the statement that the transaction completed. A stage left
    // beside it is therefore residue from a crash between spending the
    // Challenge and sweeping up — not an unfinished migration, and not a reason
    // to refuse reads of a workspace that is already current.
    //
    // Asking about the stage first said the opposite, and left a crash window
    // in which every read refused while the only command the refusal named
    // could no longer do anything about it.
    if generation_present(root)? {
        read_generation(root)?;
        return Ok(WorkspaceFormat::Current);
    }
    let migration = crate::migration::stage_dir(root);
    ensure!(
        !namespace(&migration)?,
        EXIT_SCHEMA,
        "{} marks an incomplete coordinated migration; run `engr migrate` to resume it",
        migration.display()
    );
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
    // `None` is "not the predecessor", and only established absence may say so.
    // A bootstrap that is there in some other shape is a `.engr` this build
    // cannot classify, which is a refusal rather than a workspace with no
    // predecessor in it.
    if !resource_present(&path)? {
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

/// The released predecessor's writer lock.
///
/// A different file from this generation's, which is the whole problem it
/// exists to solve: the released build takes `.engr/lock` and this one takes
/// `.engr/local/lock`, so two processes each holding "the" workspace lock do not
/// contend at all. Ordinary current work has nothing to say to that — a released
/// build refuses a generation-1 workspace on sight — but migration runs *on* a
/// predecessor workspace, with a released build perfectly entitled to be writing
/// to it.
pub fn predecessor_lock_path(root: &Path) -> PathBuf {
    engr_dir(root).join("lock")
}

/// Hold the workspace write lock for the duration of `body`.
pub fn with_lock<T>(root: &Path, body: impl FnOnce() -> Result<T>) -> Result<T> {
    with_lock_at(&lock_path(root), body)
}

/// Hold the *predecessor's* writer lock as well, for work that touches a
/// workspace a released build could still be writing to.
///
/// **Order: this generation's lock first, then the predecessor's.** Nothing can
/// deadlock against it, because the released build takes exactly one lock and
/// never waits for ours; every path in this build that takes both takes them in
/// this order, which is why this is a separate function rather than a second
/// call site.
pub fn with_predecessor_lock<T>(root: &Path, body: impl FnOnce() -> Result<T>) -> Result<T> {
    with_lock_at(&predecessor_lock_path(root), body)
}

fn with_lock_at<T>(path: &Path, body: impl FnOnce() -> Result<T>) -> Result<T> {
    if let Some(parent) = path.parent() {
        create_dir_durably(parent)?;
    }
    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
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
    contained(path)?;
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

/// Rename, and make the name itself durable before saying so.
///
/// `rename` is atomic for a reader and says nothing to a power failure: the
/// directory entry it creates lives in the containing directory's own metadata,
/// and only that directory's `fsync` puts it on the device. Every phase boundary
/// in this workspace is a name — a published resource, the staged destination,
/// the generation marker — so a boundary that is not durable is not a boundary.
///
/// Both directories are flushed for a cross-directory rename, because the entry
/// disappears from one and appears in the other and neither half is the whole
/// fact.
///
/// Windows has no equivalent to opening a directory and flushing it, and no
/// stable std API for the same guarantee; there this is the rename alone, which
/// is what every other tool on that platform relies on. Saying so is better than
/// a portability claim the code cannot keep.
pub(crate) fn rename_durably(from: &Path, to: &Path) -> Result<()> {
    fs::rename(from, to).map_err(|error| tool_error(to.display(), error))?;
    for directory in [from.parent(), to.parent()].into_iter().flatten() {
        sync_directory(directory)?;
    }
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<()> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| tool_error(path.display(), error))
}

/// Windows has no equivalent, and that is a fact about the platform rather than
/// about this code: a directory there cannot be opened for write, and
/// `FlushFileBuffers` requires write access, so there is no supported call that
/// puts a directory entry on the device. The protocol requires a platform that
/// *can* make a name durable to do so before reporting success, and one that
/// cannot to say so rather than imply a guarantee it does not keep — this is
/// where it is said, next to the no-op that is the whole of what it can do.
#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<()> {
    Ok(())
}

/// Create a directory and every missing parent, making each new *name* durable
/// as it is established.
///
/// `create_dir_all` leaves the same gap [`rename_durably`] closes, one level up.
/// A directory entry lives in its **parent's** metadata, so a directory that was
/// created and never flushed into that parent has no durable name — and syncing
/// the new directory itself says nothing about it, because the entry that
/// reaches it is somewhere else.
///
/// `.engr/eventstore/objects` on a fresh workspace is the case that matters.
/// `init` creates it; publication later flushes `.engr` when `.gitignore` and
/// `VERSION` land, which establishes `eventstore` — but nothing establishes
/// `objects` inside it. The first admission then syncs the Event file and the
/// directory holding it, while the Object is published under `.engr/objects`
/// whose own entry *was* covered. A power failure after the caller was told the
/// admission succeeded could therefore keep the Object and lose the directory
/// entry its history was written through: the projection ahead of history, which
/// is the one direction the recovery model cannot repair. Migration has the same
/// shape, because the released predecessor has no `eventstore/` at all and that
/// hierarchy is created on the way to activation.
///
/// So a durable publication primitive needs a durable layout primitive. Every
/// workspace directory is created through here, shallowest first, each new name
/// flushed into the parent that now holds it.
pub(crate) fn create_dir_durably(path: &Path) -> Result<()> {
    contained(path)?;
    // The missing suffix, deepest first.
    //
    // `metadata` rather than `symlink_metadata`: `contained` has already refused
    // every link at or below `.engr`, and above it a link is ordinary — macOS
    // hands out every temporary directory that way, and refusing to build a
    // workspace inside one would be a check on how somebody arrived rather than
    // on what they hold.
    let mut missing = Vec::new();
    let mut cursor = path;
    loop {
        match fs::metadata(cursor) {
            Ok(listed) => {
                ensure!(
                    listed.is_dir(),
                    EXIT_SCHEMA,
                    "{}: exists but is not a directory, so the layout beneath it cannot be created",
                    cursor.display()
                );
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(cursor);
                match cursor.parent() {
                    Some(parent) if !parent.as_os_str().is_empty() => cursor = parent,
                    _ => break,
                }
            }
            // Not knowing is not absence — here least of all, where the answer
            // decides whether a name gets created.
            Err(error) => return Err(tool_error(cursor.display(), error)),
        }
    }
    for directory in missing.iter().rev() {
        match fs::create_dir(directory) {
            Ok(()) => {}
            // Another writer established the same name first. The entry is there
            // either way; its shape is still ours to insist on.
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let listed = fs::metadata(directory)
                    .map_err(|error| tool_error(directory.display(), error))?;
                ensure!(
                    listed.is_dir(),
                    EXIT_SCHEMA,
                    "{}: exists but is not a directory, so the layout beneath it cannot be created",
                    directory.display()
                );
            }
            Err(error) => return Err(tool_error(directory.display(), error)),
        }
        if let Some(parent) = directory
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            sync_directory(parent)?;
        }
    }
    Ok(())
}

/// Write via a temporary file and rename, so a reader never sees half a file.
pub(crate) fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let text = crate::proof::canonical_bytes(value, &path.display().to_string())?;
    publish(path, &text)
}

/// Write exact text via a temporary file and rename.
///
/// [`write_json`] serializes; this one publishes bytes that were already
/// validated as the thing to write. Migration needs that distinction: the
/// artifact it checked and the artifact it publishes have to be the same value,
/// not two serializations that ought to agree.
pub(crate) fn write_text(path: &Path, text: &str) -> Result<()> {
    publish(path, text)
}

/// Publish exact bytes at `path` as one indivisible step.
///
/// Written beside the destination, flushed to the device, then renamed over it.
/// Every persisted file goes through this, so no reader — locked or not — can
/// observe a resource mid-write, and a crash leaves the complete previous
/// content rather than a prefix of the next one. #66's reader model has exactly
/// two states, complete old and complete new; a partial third one is not a
/// state any read path is defined over.
///
/// The flush is what makes the ordering durable rather than merely visible:
/// without it the rename can reach the device before the bytes it names do, and
/// a crash in that window leaves the destination pointing at whatever the
/// filesystem had. **The directory entry is flushed too**, and that is not a
/// generic nicety: `fsync` on a file says nothing about the durability of the
/// name that reaches it, so a completed rename can be lost by a power failure
/// while the bytes it published survive under no name at all.
///
/// One admission publishes two resources in two directories — the Event stream,
/// then the Object — and the whole recovery model rests on their order. History
/// ahead of the projection is the crash this design expects and reconciles;
/// the projection ahead of history is the direction nothing can recover, and
/// without a durable directory entry those two renames had no established order
/// across a power failure at all. The caller had already been told the admission
/// succeeded.
///
/// **The staging entry is part of the resource path, not a private detail.**
/// Its name is `<resource>.tmp` and therefore entirely predictable, so checking
/// only the destination left the whole boundary bypassable: a link planted at
/// the staging name is followed by an ordinary create, engr writes the outside
/// target, and the rename then moves *the link itself* into the canonical
/// resource path. Every resource that publishes — Objects and Event streams
/// included — went through that door. It is closed twice: the same no-link
/// containment the destination gets, and then an exclusive create, which is what
/// closes the window between asking and opening.
fn publish(path: &Path, text: &str) -> Result<()> {
    use std::io::Write;
    // Before the directories are created, so a redirected component is refused
    // rather than materialized behind the link.
    contained(path)?;
    if let Some(parent) = path.parent() {
        create_dir_durably(parent)?;
    }
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    contained(&temporary)?;
    // A crash between the create and the rename is the one thing that can
    // legitimately leave this behind, and it is a regular file when it happens —
    // `contained` has already refused every other kind. Removing it is what
    // keeps a crashed write from making the workspace permanently unwritable.
    match fs::symlink_metadata(&temporary) {
        Ok(_) => {
            fs::remove_file(&temporary).map_err(|error| tool_error(temporary.display(), error))?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(tool_error(temporary.display(), error)),
    }
    // Exclusive, so the answer cannot change between the check and the open: with
    // `O_EXCL` a link at this path fails the create rather than being followed.
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| tool_error(temporary.display(), error))?;
    file.write_all(text.as_bytes())
        .map_err(|error| tool_error(temporary.display(), error))?;
    file.sync_all()
        .map_err(|error| tool_error(temporary.display(), error))?;
    drop(file);
    rename_durably(&temporary, path)
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
    decode_object_text(&path, id, &read_text(&path)?)
}

/// One current Object from bytes a caller already holds.
///
/// Every check `load_object` performs, in the order it performs them, so a
/// caller holding the bytes some other way is held to the same contract. That
/// is not tidiness: migration resume reads its staged destination from a
/// different path, and when it did its own parse-and-decode it skipped the
/// canonical-bytes check — so a semantically equivalent rewrite of a staged file
/// could pass the digest checks, be published verbatim, and then be refused by
/// the ordinary read path of the workspace that had just declared itself
/// current. A second dialect of "read an Object" is how that happens.
pub(crate) fn decode_object_text(path: &Path, id: &str, text: &str) -> Result<Object> {
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))?;
    check_canonical_bytes(path, text, &value)?;
    decode_object(path, id, value)
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
        resource_present(&path)?,
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
    // Enumeration follows links too, so a redirected directory would list
    // identities from another tree and every load of one would then be refused;
    // and a namespace of the wrong shape is not an empty one. Said once, here,
    // rather than once per identity.
    if !namespace(&dir)? {
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
    if !namespace(&dir)? {
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
/// deliberately minimal: it carries `outcome`, `result` and `attempts`, and not
/// the material the decision was made against — the exact Rule artifacts, the
/// digest that bound them and the agent's explanation all stop at the Challenge.
/// So every mutation carries one Agent-attested attempt, each applicable Rule
/// judges it against its own ceiling, and past any ceiling autonomous Object
/// admission stops, but the record keeps the number rather than the judgement.
/// No amount of re-derivation at this boundary can ask the question, and a
/// public raw append would be a second Agent admission API holding strictly less
/// state than the gate.
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
///
/// **Append-only is the semantics, not the write.** A real `O_APPEND` write to
/// the canonical stream has three states, and the third one is durable damage:
/// an unlocked reader can observe the file mid-write, and a crash between the
/// record's bytes and its delimiter leaves a stream whose last line is a
/// complete JSON object with no newline after it — which the decoder used to
/// accept, so the *next* append concatenated onto that line and the two records
/// became one forever. Nothing recovers from that, because the EventStore is
/// never rewritten.
///
/// So the whole stream is republished through [`publish`] instead: the previous
/// bytes plus one terminated record, staged beside the file and renamed over it.
/// Readers see the complete old stream or the complete new one. The cost is
/// rewriting a file this operation has already read and validated in full, which
/// is what the continuity and replay checks above do anyway.
pub(crate) fn append_event_locked(root: &Path, object: &str, event: &Event) -> Result<()> {
    check_appendable_locked(root, event, Some(object))?;
    let path = events_path(root, object);
    let line = crate::proof::canonical_bytes(event, "Event")?;
    let held = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(tool_error(path.display(), error)),
    };
    // The decoder refuses an unterminated stream, and `check_appendable_locked`
    // has already decoded this one — so this cannot fire from a stream that got
    // here honestly. Asked anyway, because the one thing this function must
    // never do is turn two records into one.
    ensure!(
        held.is_empty() || held.ends_with('\n'),
        EXIT_SCHEMA,
        "{}: the last record has no delimiter after it, so appending would join two events into one",
        path.display()
    );
    publish(&path, &format!("{held}{line}\n"))
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
    // Which projection this record is validated against is the action's to
    // choose, and it is the same choice `prepare` and `confirm` make.
    //
    // **Repair does not read the stored bytes at all.** They are what it exists
    // to distrust: it is proposed against `ops::provable` and applies to it, so
    // validating its tail from the corrupt projection asks whether a repair
    // could be replayed over the damage it repairs. That is not a question with
    // a meaningful answer, and its answer was often no — a projection whose
    // `rev` was edited backwards replays events it has already applied, and the
    // one recovery path was refused for it at the boundary after being offered
    // and confirmed.
    //
    // Everything else builds on the stored projection, and must first establish
    // that it is one an admission may build on: the value its own admitted
    // history produced, not merely a correctly sealed one. Asked here as well as
    // at each entry to the gate, because "this workspace's authority was
    // admitted" is a property of the durable boundary rather than of the route
    // taken to it — and this is the step that would make an out-of-band edit
    // durable, by writing a record that stands behind it.
    let predecessor = if matches!(event.action, Action::ObjectRepaired {}) {
        Some(crate::ops::provable(root, &id)?)
    } else {
        if let Some(stored) = &stored {
            crate::ops::history_consistent_with(&tail, stored)?;
        }
        stored
    };
    validate_recoverable_tail(&id, predecessor, &tail)?;
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
    // Every Object the workspace holds, projection or history: an Event whose
    // stream exists and whose projection was lost to a crash still belongs to
    // that Object, and a listing of files alone would answer that its own stream
    // seals against nothing.
    for id in crate::ops::object_ids(root)? {
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
    // Three-way, and before anything follows a link: a dangling stream link
    // reported as a missing history is a redirection read as an absence, and
    // the two have different answers.
    if !resource_present(&path)? {
        ensure!(
            !resource_present(&object_path(root, id))?,
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
///
/// **One record per line, every line a record, and every record terminated.**
/// Blank lines used to be skipped, which gave a current stream a second
/// spelling — the writer never emits one, so a file carrying them is not the
/// canonical representation this generation claims to have exactly one of. It
/// also hid framing damage: a truncated write, a bad merge or a partial copy
/// shows up first as whitespace where a record belongs, and skipping it is
/// reading past the evidence. The final delimiter is required for the sharper
/// version of the same fault — an unterminated last line is a record whose write
/// did not finish, and accepting it is what would let the next append join two
/// events into one.
pub(crate) fn decode_events(path: &Path, id: &str, text: &str) -> Result<Vec<Event>> {
    ensure!(
        text.is_empty() || text.ends_with('\n'),
        EXIT_SCHEMA,
        "{}: the last event record has no delimiter after it, so this history is truncated",
        path.display()
    );
    let mut events: Vec<Event> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        ensure!(
            !line.trim().is_empty(),
            EXIT_SCHEMA,
            "{}:{}: an event stream holds one record per line and no blank ones",
            path.display(),
            index + 1
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The layout primitive builds what was missing, and accepts what is there.
    ///
    /// **Whether each new name reached the device is not observable from a
    /// test.** `fsync` returns nothing a caller can distinguish, and no power
    /// failure is injectable here. So this holds the rest of the contract — the
    /// hierarchy is created, and an existing one is not an error — while the
    /// claim that the flush happens at all rests on this being the only route
    /// any workspace directory is created through, which is what
    /// `every_workspace_directory_is_created_through_the_durable_layout_primitive`
    /// in the record tests holds.
    #[test]
    fn the_layout_primitive_builds_a_hierarchy_and_accepts_one_already_there() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let nested = eventstore_dir(temp.path());
        create_dir_durably(&nested).expect("the hierarchy is created");
        assert!(nested.is_dir(), "every component of it exists");
        assert!(
            engr_dir(temp.path()).join("eventstore").is_dir(),
            "including the intermediate one, which is the entry nothing established before"
        );
        create_dir_durably(&nested).expect("and asking again is convergence, not a failure");
    }

    /// A name of the wrong shape is refused rather than created around.
    #[test]
    fn the_layout_primitive_refuses_a_name_that_is_not_a_directory() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let engr = engr_dir(temp.path());
        fs::create_dir(&engr).expect("the workspace directory");
        let occupied = engr.join("eventstore");
        fs::write(&occupied, b"not a directory").expect("a file where a directory belongs");

        let error =
            create_dir_durably(&occupied).expect_err("a file is not a directory to publish into");
        assert_eq!(error.code, EXIT_SCHEMA, "{error}");
        assert!(error.message.contains("not a directory"), "{error}");

        // And the same shape one component down. Which *error* that is depends on
        // the platform — Unix answers `NotADirectory` for the lookup, Windows
        // resolves it to `NotFound` and refuses on the shape a step later — so
        // what is asserted is the property both keep: it fails rather than
        // building anything.
        create_dir_durably(&occupied.join("objects"))
            .expect_err("nothing is created beneath a name that is not a directory");
        assert!(
            fs::symlink_metadata(&occupied)
                .expect("still there")
                .is_file(),
            "and the file that was in the way is untouched"
        );
    }

    /// A redirected component is refused: what would be created is not what this
    /// workspace holds.
    #[test]
    #[cfg(unix)]
    fn the_layout_primitive_refuses_a_redirected_component() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let engr = engr_dir(temp.path());
        fs::create_dir(&engr).expect("the workspace directory");
        std::os::unix::fs::symlink(temp.path().join("nowhere"), engr.join("eventstore"))
            .expect("symlink");

        let error = create_dir_durably(&eventstore_dir(temp.path()))
            .expect_err("a dangling link is not an absent directory");
        assert_eq!(error.code, EXIT_SCHEMA, "{error}");
        assert!(error.message.contains("link to somewhere else"), "{error}");
        assert!(
            fs::symlink_metadata(temp.path().join("nowhere")).is_err(),
            "and nothing was materialized behind the link"
        );
    }
}
