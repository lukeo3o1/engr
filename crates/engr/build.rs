//! Stamps the commit this binary was built from.
//!
//! There is one version, `latest`, so the commit is the only thing that says
//! which build you are holding. `-dirty` means the code compiled in is not the
//! code at that commit — the difference between a build that can be traced back
//! to a source tree and one that cannot.

use std::path::Path;
use std::process::Command;

/// What the dirty check covers, relative to this package: everything that ends
/// up in the binary. An edited README does not change the program, and calling
/// such a build dirty would leave the marker meaning nothing. These double as
/// the rebuild triggers, so the stamp cannot outlive the code it describes.
const SOURCES: [&str; 5] = [
    "src",
    "build.rs",
    "Cargo.toml",
    "../../Cargo.toml",
    "../../Cargo.lock",
];

fn main() {
    for path in SOURCES {
        println!("cargo:rerun-if-changed={path}");
    }
    let commit = commit().unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=ENGR_COMMIT={commit}");
}

/// `None` whenever git cannot answer — no repository, no git on PATH, a source
/// archive rather than a checkout. Guessing would be worse than admitting it.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|text| text.trim().to_owned())
}

fn commit() -> Option<String> {
    watch_head();
    let commit = git(&["rev-parse", "--short=8", "HEAD"])?;
    if commit.is_empty() {
        return None;
    }
    let mut status = vec!["status", "--porcelain", "--"];
    status.extend(SOURCES);
    let dirty = git(&status).is_some_and(|changes| !changes.is_empty());
    Some(if dirty {
        format!("{commit}-dirty")
    } else {
        commit
    })
}

/// Re-run when HEAD moves, so a build made straight after `git commit` is not
/// stamped with the commit before it. Committing on the current branch does not
/// touch `HEAD` itself — it moves the ref `HEAD` points at, which may also be
/// packed and have no file to watch — so the reflog is watched as the one thing
/// every movement appends to. `--git-path` is what locates all three inside a
/// worktree, and paths that do not exist are skipped: registering a missing file
/// makes cargo consider the script dirty on every single build.
fn watch_head() {
    let mut names = vec!["HEAD".to_owned(), "logs/HEAD".to_owned()];
    if let Some(head_ref) = git(&["symbolic-ref", "--quiet", "HEAD"]) {
        names.push(head_ref);
    }
    for name in names {
        if let Some(path) = git(&["rev-parse", "--git-path", &name]) {
            if Path::new(&path).exists() {
                println!("cargo:rerun-if-changed={path}");
            }
        }
    }
}
