//! Git interrogation.
//!
//! git is a hard dependency, not a nicety: it is where old wording is recovered
//! from and where tamper evidence ultimately lives. Every call here degrades to
//! `None` rather than failing, so a missing git turns features off instead of
//! breaking the tool — but `engr init` warns, because silently losing look-back
//! is worse than a noisy start.

use crate::model::{Object, OBJECT_FORMAT};
use crate::{ensure, Error, Result, EXIT_SCHEMA, LEGACY_OBJECT_VERSION_V0, WORKSPACE_VERSION};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

fn run(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn run_bytes(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let output = Command::new("git")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

fn literal_path(path: &str) -> String {
    format!(":(top,literal){path}")
}

pub fn is_repo(root: &Path) -> bool {
    run(root, &["rev-parse", "--git-dir"]).is_some()
}

pub fn head(root: &Path) -> Option<String> {
    run(root, &["rev-parse", "HEAD"])
}

/// Resolve any revision the user typed (`HEAD`, a tag, a short sha) to a full
/// commit id, so what lands in a section is unambiguous later.
pub fn resolve(root: &Path, revision: &str) -> Option<String> {
    run(root, &["rev-parse", &format!("{revision}^{{commit}}")])
}

pub fn exists(root: &Path, revision: &str) -> bool {
    run(root, &["cat-file", "-e", &format!("{revision}^{{commit}}")]).is_some()
}

/// Whether source files have uncommitted changes. Record files are excluded:
/// they describe assertions, rather than repository context those assertions
/// may have been formed against.
pub fn source_dirty(root: &Path) -> Option<bool> {
    let spec = outside_the_record();
    let status = run(root, &["status", "--porcelain", "--", &spec[0], &spec[1]])?;
    Some(!status.trim().is_empty())
}

/// Whether one source path has changes git has not recorded, including being
/// untracked. Narrower than [`source_dirty`] on purpose: a Backlog subject pins
/// the snapshot of the path it names, so an unrelated dirty file elsewhere says
/// nothing about whether that pin would be honest.
/// Whether the file at `path` right now differs from what `commit` holds.
///
/// The question a pinned subject actually asks. [`path_dirty`] answers a
/// different one — whether the worktree differs from `HEAD`/the index — and the
/// two only coincide when the pin *is* `HEAD`. Pin an older revision from a
/// clean worktree and the status answer is "clean" while the file plainly is not
/// what that commit reconstructs, which is the claim the subject goes on to
/// make.
///
/// `git diff` exits 1 for "differs", so the status code is the answer and a
/// non-zero exit must not be read as failure; anything else is `None`.
pub fn path_differs_at(root: &Path, commit: &str, path: &str) -> Option<bool> {
    let repository = repo_root(root)?;
    let path = literal_path(path);
    let output = Command::new("git")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .arg("-C")
        .arg(repository)
        .args(["diff", "--quiet", commit, "--", &path])
        .output()
        .ok()?;
    match output.status.code() {
        Some(0) => Some(false),
        Some(1) => Some(true),
        _ => None,
    }
}

pub fn path_dirty(root: &Path, path: &str) -> Option<bool> {
    let repository = repo_root(root)?;
    let path = literal_path(path);
    let status = run(&repository, &["status", "--porcelain", "--", &path])?;
    Some(!status.trim().is_empty())
}

/// Whether `path` exists in `commit`. A pinned snapshot that never held the
/// path is false provenance, which is exactly what pinning is meant to prevent.
pub fn path_at(root: &Path, commit: &str, path: &str) -> bool {
    run(root, &["cat-file", "-e", &format!("{commit}:{path}")]).is_some()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalWorkspaceFormat {
    format: String,
    version: u32,
}

fn historical_path(commit: &str, path: &str) -> String {
    format!("{commit}:{path}")
}

fn workspace_prefix(root: &Path) -> Result<String> {
    let repository = repo_root(root).ok_or_else(|| {
        Error::new(
            EXIT_SCHEMA,
            "could not determine repository root".to_owned(),
        )
    })?;
    let engr = crate::store::engr_dir(root);
    let relative = engr.strip_prefix(&repository).map_err(|_| {
        Error::new(
            EXIT_SCHEMA,
            "workspace .engr is outside its repository".to_owned(),
        )
    })?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

fn historical_bytes(root: &Path, commit: &str, path: &str) -> Result<Option<Vec<u8>>> {
    Ok(run_bytes(root, &["show", &historical_path(commit, path)]))
}

fn validate_historical_format(path: &str, text: &str) -> Result<u32> {
    let format: HistoricalWorkspaceFormat = serde_json::from_str(text)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{path}: {error}")))?;
    ensure!(
        format.format == crate::store::WORKSPACE_FORMAT,
        EXIT_SCHEMA,
        "{path}: not an engr workspace"
    );
    // A snapshot carries the version that was current when it was taken, so
    // pinning this to the *newest* version would make every reference recorded
    // before a migration unresolvable — the workspace moving forward would
    // retroactively break provenance that was valid when it was pinned.
    //
    // Reading an older snapshot is safe here for a reason worth stating rather
    // than assuming: what this function guards is decoding a historical
    // *Object*, and every version this build recognizes represents an Object
    // identically. The version 2 change is to how a project Rule is
    // interpreted, and no Rule is read out of a historical snapshot. If a future
    // version ever changes the Object representation itself, this must decode
    // under the snapshot's own version rather than widening the check again.
    ensure!(
        format.version == WORKSPACE_VERSION
            || crate::MIGRATABLE_WORKSPACE_VERSIONS.contains(&format.version),
        EXIT_SCHEMA,
        "{path}: workspace version {} is not supported by engr {}",
        format.version,
        crate::IMPLEMENTATION_VERSION
    );
    Ok(format.version)
}

/// A format-less snapshot predates the workspace authority. It is readable only
/// when every flat Object file carries the old per-resource markers, matching
/// the live legacy detector rather than guessing from whatever the target JSON
/// happens to deserialize as today.
fn validate_legacy_workspace_at(root: &Path, commit: &str) -> Result<()> {
    let objects = format!("{}/objects", workspace_prefix(root)?);
    let paths = run(
        root,
        &["ls-tree", "-r", "--name-only", commit, "--", &objects],
    )
    .ok_or_else(|| {
        Error::new(
            EXIT_SCHEMA,
            format!("could not inspect historical workspace at commit {commit}"),
        )
    })?;
    let prefix = format!("{objects}/");
    let mut found = false;
    for path in paths.lines() {
        let Some(name) = path.strip_prefix(&prefix) else {
            continue;
        };
        if name.contains('/') || !name.ends_with(".json") {
            continue;
        }
        found = true;
        let bytes = historical_bytes(root, commit, path)?.ok_or_else(|| {
            Error::new(
                EXIT_SCHEMA,
                format!("could not read historical object {path} at commit {commit}"),
            )
        })?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|error| Error::new(EXIT_SCHEMA, format!("{path}: {error}")))?;
        let value: serde_json::Value = serde_json::from_str(text)
            .map_err(|error| Error::new(EXIT_SCHEMA, format!("{path}: {error}")))?;
        let object_value = value.as_object().ok_or_else(|| {
            Error::new(EXIT_SCHEMA, format!("{path}: object must be a JSON object"))
        })?;
        ensure!(
            object_value.get("format").and_then(|value| value.as_str()) == Some(OBJECT_FORMAT)
                && object_value.get("version").and_then(|value| value.as_u64())
                    == Some(LEGACY_OBJECT_VERSION_V0.into()),
            EXIT_SCHEMA,
            "{path}: not a recognized legacy v0 object"
        );
        let Some(id) = name.strip_suffix(".json") else {
            continue;
        };
        let object: Object = serde_json::from_value(value)
            .map_err(|error| Error::new(EXIT_SCHEMA, format!("{path}: {error}")))?;
        object.validate()?;
        ensure!(
            object.id == id,
            EXIT_SCHEMA,
            "{path}: object id {:?} does not match its filename",
            object.id
        );
    }
    ensure!(
        found,
        EXIT_SCHEMA,
        "historical workspace at commit {commit} has no format.json and is not a recognized legacy v0 workspace"
    );
    Ok(())
}

/// Read one object exactly as a commit contains it. References use this rather
/// than pairing the worktree's wording with an unrelated HEAD. The snapshot's
/// own workspace authority decides which representation may be decoded.
pub fn object_at(root: &Path, commit: &str, id: &str) -> Result<Option<Object>> {
    let prefix = workspace_prefix(root)?;
    let format_path = format!("{prefix}/format.json");
    let version = match historical_bytes(root, commit, &format_path)? {
        Some(bytes) => {
            let text = std::str::from_utf8(&bytes)
                .map_err(|error| Error::new(EXIT_SCHEMA, format!("{format_path}: {error}")))?;
            validate_historical_format(&format_path, text)?
        }
        None => {
            validate_legacy_workspace_at(root, commit)?;
            0
        }
    };

    let path = format!("{prefix}/objects/{id}.json");
    let Some(bytes) = historical_bytes(root, commit, &path)? else {
        return Ok(None);
    };
    let text = std::str::from_utf8(&bytes)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{path}: {error}")))?;
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{path}: {error}")))?;
    let object = crate::store::decode_object_for_version(Path::new(&path), id, value, version)?;
    if version == WORKSPACE_VERSION {
        let canonical = crate::proof::canonical_bytes(&object, "historical Object")?;
        ensure!(
            bytes == canonical.as_bytes(),
            EXIT_SCHEMA,
            "{path}: workspace-v3 Object is not persisted as JCS"
        );
    }
    Ok(Some(object))
}

/// Keeping the record is not the world moving.
///
/// `confirm` asks for the object file to be committed, so counting that commit
/// makes every section stale the moment its own record is saved — the tool's
/// instructions break the tool's signal, and a signal that is always on gets
/// ignored along with the ones that matter.
///
/// Both patterns are anchored with `:(top)`. A cwd-relative `.` looks
/// equivalent and is not: with the workspace in a subdirectory it narrows the
/// whole comparison to that subdirectory, so a change to `src/` outside it
/// disappears. Falsely quiet is worse than falsely loud.
fn outside_the_record() -> [String; 2] {
    [
        ":(top)".to_owned(),
        format!(":(top,exclude,glob)**/{}/**", crate::store::DIR),
    ]
}

/// How far HEAD has moved past `from`: commits ahead, and how many files
/// changed, ignoring the record's own files. Reported as information rather
/// than a verdict — a threshold nobody has validated would be a guess, and a
/// binary "stale" on every commit would make the signal worthless.
pub fn distance(root: &Path, from: &str) -> Option<Distance> {
    if !exists(root, from) {
        return None;
    }
    let range = format!("{from}..HEAD");
    let spec = outside_the_record();
    let commits = run(
        root,
        &["rev-list", "--count", &range, "--", &spec[0], &spec[1]],
    )?
    .parse::<usize>()
    .ok()?;
    // The same pathspec on both calls, or the two halves of one sentence
    // disagree: "3 commits and 0 files have changed".
    let names = run(
        root,
        &["diff", "--name-only", &range, "--", &spec[0], &spec[1]],
    )?;
    let files: Vec<String> = names
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect();
    Some(Distance { commits, files })
}

/// The last commit that touched `path`. `show` uses it to hand the reader the
/// command that recovers what a file said before it was edited.
pub fn last_commit_for(root: &Path, path: &Path) -> Option<String> {
    let repository = repo_root(root)?;
    let relative = path.strip_prefix(&repository).unwrap_or(path);
    let literal = literal_path(&relative.to_string_lossy().replace('\\', "/"));
    let commit = run(&repository, &["log", "-1", "--format=%H", "--", &literal])?;
    (!commit.is_empty()).then_some(commit)
}

#[derive(Debug, Clone)]
pub struct Distance {
    pub commits: usize,
    pub files: Vec<String>,
}

impl Distance {
    /// Either half is enough. `rev-list` with a pathspec simplifies history and
    /// can pass over a merge whose change exists only in the merge commit,
    /// while `diff` compares the two endpoints and still names the file — and
    /// if `from` is not an ancestor of HEAD there may be no commits to count
    /// yet plenty of difference. Missing a real change is the failure that
    /// matters here.
    pub fn moved(&self) -> bool {
        self.commits > 0 || !self.files.is_empty()
    }
}

/// Whether a path has changes git has not recorded. Used to warn that current
/// projections have not yet been committed as an additional tamper anchor.
pub fn uncommitted(root: &Path, path: &Path) -> Option<bool> {
    let repository = repo_root(root)?;
    let relative = path.strip_prefix(&repository).unwrap_or(path);
    let literal = literal_path(&relative.to_string_lossy().replace('\\', "/"));
    let status = run(&repository, &["status", "--porcelain", "--", &literal])?;
    Some(!status.trim().is_empty())
}

/// The exact bytes of `path` at `commit`, as text.
///
/// Deliberately not routed through [`run`], which trims — a trailing newline is
/// content, and a basis that hashes differently depending on whether something
/// stripped its last byte is not a basis.
pub fn blob_at(root: &Path, commit: &str, path: &str) -> Option<String> {
    let output = Command::new("git")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .arg("-C")
        .arg(root)
        .args(["show", &format!("{commit}:{path}")])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

/// The repository's top level, which is what a repository-relative path is
/// relative to.
///
/// Not the same as the engr workspace root: `.engr` may sit in a subdirectory,
/// and `git show <commit>:<path>` resolves from the top level regardless. A
/// caller that reads current material from the workspace root while reading
/// pinned material through git is comparing two different files.
pub fn repo_root(root: &Path) -> Option<PathBuf> {
    run(root, &["rev-parse", "--show-toplevel"]).map(PathBuf::from)
}

/// The type of the object this id names, without peeling it.
///
/// [`exists`] asks whether a revision *reaches* a commit, which is the right
/// question for a revision and the wrong one for a stored id: an annotated tag
/// peels to a commit, so its own object id passes while the value recorded is
/// not a commit id at all. A field specified as a commit id has to be one.
pub fn object_type(root: &Path, oid: &str) -> Option<String> {
    run(root, &["cat-file", "-t", oid])
}

/// The tree entry mode for `path` at `commit`, as git records it.
///
/// The mode is the only place the distinction survives. `git show <commit>:<p>`
/// prints a symlink's *target name* as though it were file content, so content
/// alone cannot tell a regular file from a link — and a link whose target name
/// happens to equal a later regular file's contents compares equal.
pub fn tree_entry_mode(root: &Path, commit: &str, path: &str) -> Option<String> {
    let literal = literal_path(path);
    let listed = run(root, &["ls-tree", commit, "--", &literal])?;
    listed.split_whitespace().next().map(str::to_owned)
}
