//! Git interrogation.
//!
//! git is a hard dependency, not a nicety: it is where old wording is recovered
//! from and where tamper evidence ultimately lives. Every call here degrades to
//! `None` rather than failing, so a missing git turns features off instead of
//! breaking the tool — but `engr init` warns, because silently losing look-back
//! is worse than a noisy start.

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

/// How far HEAD has moved past `from`: commits ahead, and how many files
/// changed. Reported as information rather than a verdict — a threshold nobody
/// has validated would be a guess, and a binary "stale" on every commit would
/// make the signal worthless.
pub fn distance(root: &Path, from: &str) -> Option<Distance> {
    if !exists(root, from) {
        return None;
    }
    let commits = run(root, &["rev-list", "--count", &format!("{from}..HEAD")])?
        .parse::<usize>()
        .ok()?;
    let names = run(root, &["diff", "--name-only", &format!("{from}..HEAD")])?;
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
    pub fn moved(&self) -> bool {
        self.commits > 0
    }
}

/// Whether a path has changes git has not recorded. Used to warn that
/// look-back would be lost, and as the honest half of `verify`: once events are
/// purged, a hash stored beside the content it covers only catches careless
/// edits, so committed history is the real anchor.
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
