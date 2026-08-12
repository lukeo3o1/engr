//! What the command line promises the outside world.

use std::process::Command;

/// The installers echo this line back as proof the binary they placed runs, and
/// `latest` never changes — so the part in parentheses is the only thing that
/// says which build it is. The shape is pinned and the contents are not, because
/// `unknown` is the honest answer when there is no git to ask.
#[test]
fn the_version_names_the_commit_it_was_built_from() {
    let output = Command::new(env!("CARGO_BIN_EXE_engr"))
        .arg("--version")
        .output()
        .expect("run engr --version");
    assert!(output.status.success(), "--version did not exit cleanly");
    let line = String::from_utf8(output.stdout).expect("utf8");
    let line = line.trim();
    let commit = line
        .strip_prefix("engr latest (")
        .and_then(|rest| rest.strip_suffix(')'))
        .unwrap_or_else(|| panic!("expected `engr latest (<commit>)`, got {line:?}"));
    assert!(!commit.is_empty(), "nothing was stamped in: {line:?}");
}
