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
use std::path::Path;
use std::process::Command;

fn run(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
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
pub fn path_dirty(root: &Path, path: &str) -> Option<bool> {
    let status = run(root, &["status", "--porcelain", "--", path])?;
    Some(!status.trim().is_empty())
}

/// Whether `path` exists in `commit`. A pinned snapshot that never held the
/// path is false provenance, which is exactly what pinning is meant to prevent.
pub fn path_at(root: &Path, commit: &str, path: &str) -> bool {
    run(root, &["cat-file", "-e", &format!("{commit}:{path}")]).is_some()
}

#[derive(Deserialize)]
struct HistoricalWorkspaceFormat {
    format: String,
    version: u32,
}

fn historical_path(commit: &str, path: &str) -> String {
    format!("{commit}:{path}")
}

fn validate_historical_format(path: &str, text: &str) -> Result<()> {
    let format: HistoricalWorkspaceFormat = serde_json::from_str(text)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{path}: {error}")))?;
    ensure!(
        format.format == crate::store::WORKSPACE_FORMAT,
        EXIT_SCHEMA,
        "{path}: not an engr workspace"
    );
    ensure!(
        format.version == WORKSPACE_VERSION,
        EXIT_SCHEMA,
        "{path}: workspace version {} is not supported by engr {}",
        format.version,
        crate::IMPLEMENTATION_VERSION
    );
    Ok(())
}

/// A format-less snapshot predates the workspace authority. It is readable only
/// when every flat Object file carries the old per-resource markers, matching
/// the live legacy detector rather than guessing from whatever the target JSON
/// happens to deserialize as today.
fn validate_legacy_workspace_at(root: &Path, commit: &str) -> Result<()> {
    let objects = format!("{}/objects", crate::store::DIR);
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
        let text = run(root, &["show", &historical_path(commit, path)]).ok_or_else(|| {
            Error::new(
                EXIT_SCHEMA,
                format!("could not read historical object {path} at commit {commit}"),
            )
        })?;
        let value: serde_json::Value = serde_json::from_str(&text)
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
    let format_path = format!("{}/format.json", crate::store::DIR);
    match run(root, &["show", &historical_path(commit, &format_path)]) {
        Some(text) => validate_historical_format(&format_path, &text)?,
        None => validate_legacy_workspace_at(root, commit)?,
    }

    let path = format!("{}/objects/{id}.json", crate::store::DIR);
    let Some(text) = run(root, &["show", &historical_path(commit, &path)]) else {
        return Ok(None);
    };
    let object: Object = serde_json::from_str(&text)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{path}: {error}")))?;
    object.validate()?;
    ensure!(
        object.id == id,
        EXIT_SCHEMA,
        "{path}: object id {:?} does not match its filename",
        object.id
    );
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
    let relative = path.strip_prefix(root).unwrap_or(path);
    let commit = run(
        root,
        &[
            "log",
            "-1",
            "--format=%H",
            "--",
            &relative.to_string_lossy().replace('\\', "/"),
        ],
    )?;
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
    let relative = path.strip_prefix(root).unwrap_or(path);
    let status = run(
        root,
        &[
            "status",
            "--porcelain",
            "--",
            &relative.to_string_lossy().replace('\\', "/"),
        ],
    )?;
    Some(!status.trim().is_empty())
}
