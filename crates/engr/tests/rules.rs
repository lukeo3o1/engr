//! What a project Rule is, and what engr refuses to guess about one.
//!
//! Rules carry the judgements engr cannot make: whether wording follows this
//! project's policy. So the mechanism is not "engr checks the rule" — it is
//! "engr names, exactly, what had to be read". Every test here is about that
//! word *exactly*: an id that survives a rename, a set that cannot be
//! ambiguous, a basis that cannot be approximated, and a front matter that
//! refuses what it does not understand rather than reading past it.

use engr::rules::{self, Domain};
use engr::store;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn workspace() -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().to_path_buf();
    store::init(&root).expect("init");
    std::fs::create_dir_all(rules::dir(&root)).expect("rules dir");
    (dir, root)
}

fn write_rule(root: &Path, name: &str, text: &str) -> PathBuf {
    let path = rules::dir(root).join(format!("{name}.md"));
    std::fs::write(&path, text).expect("write rule");
    path
}

fn git(root: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .expect("git");
    assert!(status.success(), "git {args:?}");
}

fn commit_all(root: &Path, message: &str) -> String {
    git(root, &["add", "-A"]);
    git(
        root,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            message,
        ],
    );
    engr::git::head(root).expect("HEAD")
}

const ARCHITECTURE: &str = "\
---
id: architecture-consistency
applies:
  domains:
    - object
    - backlog
based_on:
  - path: AGENTS.md
---

# Architecture consistency

Do not record wording that silently contradicts the architecture contract.
";

#[test]
fn a_rule_is_identified_by_its_id_and_located_by_its_filename() {
    let (_dir, root) = workspace();
    std::fs::write(root.join("AGENTS.md"), "the contract\n").expect("basis");
    write_rule(&root, "architecture", ARCHITECTURE);

    let rules = rules::load_all(&root).expect("rules");
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].id, "architecture-consistency");
    assert_eq!(rules[0].domains, vec![Domain::Backlog, Domain::Object]);
    assert_eq!(rules[0].based_on.len(), 1);
    assert_eq!(rules[0].based_on[0].path, "AGENTS.md");
    assert!(rules[0].based_on[0].commit.is_none());
    assert!(rules[0].body.starts_with("# Architecture consistency"));

    // Renaming the file does not create a different rule. The filename is a
    // locator; the id is the identity, and anything that pinned this rule
    // pinned the id.
    std::fs::remove_file(rules::dir(&root).join("architecture.md")).expect("remove");
    write_rule(&root, "record-quality", ARCHITECTURE);
    let renamed = rules::load_all(&root).expect("rules");
    assert_eq!(renamed.len(), 1);
    assert_eq!(renamed[0].id, "architecture-consistency");
    assert_eq!(renamed[0].raw, rules[0].raw, "and it says the same thing");
}

#[test]
fn two_files_cannot_share_one_rule_id() {
    let (_dir, root) = workspace();
    std::fs::write(root.join("AGENTS.md"), "the contract\n").expect("basis");
    write_rule(&root, "one", ARCHITECTURE);
    write_rule(&root, "two", ARCHITECTURE);

    let error = rules::load_all(&root).expect_err("one identity, two files");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("already used"), "{error}");
}

/// An unreadable rule is not a rule that does not apply.
///
/// Every case here would be an *incomplete* applicable set if it were skipped,
/// and a review binding over an incomplete set attests to nothing. So the
/// loader refuses rather than reading past anything it does not understand —
/// including a field a newer version might add, which is the case an older
/// implementation would otherwise silently ignore.
#[test]
fn a_rule_this_version_cannot_fully_read_is_refused_rather_than_ignored() {
    let (_dir, root) = workspace();
    let path = rules::dir(&root).join("broken.md");

    for (why, text) in [
        ("no front matter at all", "# just a document\n"),
        (
            "front matter never closed",
            "---\nid: x\napplies:\n  domains:\n    - object\n\n# body\n",
        ),
        (
            "no id",
            "---\napplies:\n  domains:\n    - object\n---\n\n# body\n",
        ),
        ("no applies", "---\nid: x\n---\n\n# body\n"),
        (
            "an empty domain list",
            "---\nid: x\napplies:\n  domains:\n---\n\n# body\n",
        ),
        (
            "a domain this version does not have",
            "---\nid: x\napplies:\n  domains:\n    - rules\n---\n\n# body\n",
        ),
        (
            "a field a newer version might mean something by",
            "---\nid: x\napplies:\n  domains:\n    - object\nseverity: blocking\n---\n\n# body\n",
        ),
        (
            "a selector v1 deliberately does not have",
            "---\nid: x\napplies:\n  domains:\n    - object\n  actions:\n    - revise\n---\n\n# body\n",
        ),
        (
            "no body, which is the rule",
            "---\nid: x\napplies:\n  domains:\n    - object\n---\n\n",
        ),
        (
            "an id that is not an id",
            "---\nid: Architecture Consistency\napplies:\n  domains:\n    - object\n---\n\n# body\n",
        ),
        (
            "the same domain twice",
            "---\nid: x\napplies:\n  domains:\n    - object\n    - object\n---\n\n# body\n",
        ),
    ] {
        std::fs::write(&path, text).expect("write");
        let error = rules::load_all(&root).expect_err(why);
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{why}: {error}");
    }
}

#[test]
fn a_domain_with_no_rule_needs_no_review() {
    let (_dir, root) = workspace();
    std::fs::write(root.join("AGENTS.md"), "the contract\n").expect("basis");
    write_rule(&root, "architecture", ARCHITECTURE);

    assert_eq!(
        rules::applicable(&root, Domain::Object)
            .expect("object")
            .len(),
        1
    );
    assert_eq!(
        rules::applicable(&root, Domain::Backlog)
            .expect("backlog")
            .len(),
        1
    );
    assert!(
        rules::applicable(&root, Domain::Work)
            .expect("work")
            .is_empty(),
        "no rule for a domain is a real answer, not a gap"
    );
    assert!(rules::applicable(&root, Domain::Collection)
        .expect("collection")
        .is_empty());

    // And a workspace with no rules directory at all is the same answer.
    let (_bare_dir, bare) = {
        let dir = TempDir::new().expect("temp dir");
        let root = dir.path().to_path_buf();
        store::init(&root).expect("init");
        (dir, root)
    };
    assert!(rules::load_all(&bare).expect("none").is_empty());
    assert!(rules::applicable(&bare, Domain::Object)
        .expect("none")
        .is_empty());
}

/// The applicable set is ordered by rule id, not by the directory.
///
/// The review hash is computed over this set, so an order that came from
/// filesystem enumeration would make the same project state produce different
/// hashes on different machines — and an attestation that cannot be recomputed
/// is not an attestation.
#[test]
fn the_applicable_set_does_not_depend_on_the_filesystem() {
    let (_dir, root) = workspace();
    std::fs::write(root.join("AGENTS.md"), "the contract\n").expect("basis");
    for (file, id) in [
        ("zzz", "aaa-first"),
        ("aaa", "zzz-last"),
        ("mmm", "mmm-mid"),
    ] {
        write_rule(
            &root,
            file,
            &format!("---\nid: {id}\napplies:\n  domains:\n    - object\n---\n\n# {id}\n"),
        );
    }
    let ids: Vec<String> = rules::applicable(&root, Domain::Object)
        .expect("rules")
        .into_iter()
        .map(|rule| rule.id)
        .collect();
    assert_eq!(ids, vec!["aaa-first", "mmm-mid", "zzz-last"]);
}

/// A floating basis follows the project material.
///
/// There is no stale state for it, deliberately: it simply *is* the current
/// content, so changing that content changes what has to be reviewed.
#[test]
fn a_basis_without_a_commit_resolves_the_current_material() {
    let (_dir, root) = workspace();
    std::fs::write(root.join("AGENTS.md"), "first contract\n").expect("basis");
    write_rule(&root, "architecture", ARCHITECTURE);
    let rule = rules::load_all(&root).expect("rules").remove(0);

    let resolved = rule.based_on[0].resolve(&root, &rule.id).expect("resolve");
    assert_eq!(resolved.content, "first contract\n");
    assert!(resolved.commit.is_none());

    std::fs::write(root.join("AGENTS.md"), "second contract\n").expect("edit");
    let resolved = rule.based_on[0].resolve(&root, &rule.id).expect("resolve");
    assert_eq!(
        resolved.content, "second contract\n",
        "the basis is the current material, so it moved with it"
    );
}

/// A pinned basis records what the rule was written against — and stops being
/// usable once the project says something else.
///
/// The comparison is on content, not on commit ids. A repository commit that
/// did not touch the file changed nothing this rule depends on, and staling the
/// rule for it would train everyone to update the pin without reading anything.
#[test]
fn a_pinned_basis_goes_stale_when_the_current_material_changes_and_not_before() {
    let (_dir, root) = workspace();
    git(&root, &["init", "-q"]);
    std::fs::write(root.join("AGENTS.md"), "the contract\n").expect("basis");
    std::fs::write(root.join("unrelated.md"), "something else\n").expect("other");
    let pinned_at = commit_all(&root, "the contract");

    write_rule(
        &root,
        "architecture",
        &format!(
            "---\nid: architecture\napplies:\n  domains:\n    - object\nbased_on:\n  - path: AGENTS.md\n    commit: {pinned_at}\n---\n\n# Architecture\n\nThe rule.\n"
        ),
    );
    let rule = rules::load_all(&root).expect("rules").remove(0);
    assert_eq!(rule.based_on[0].commit.as_deref(), Some(pinned_at.as_str()));

    let resolved = rule.based_on[0].resolve(&root, &rule.id).expect("current");
    assert_eq!(resolved.content, "the contract\n");

    // A later commit that did not touch the path leaves the rule usable.
    std::fs::write(root.join("unrelated.md"), "changed elsewhere\n").expect("edit");
    commit_all(&root, "unrelated change");
    rule.based_on[0]
        .resolve(&root, &rule.id)
        .expect("an untouched path is not stale");

    // Changing the material itself is what stales it, and the refusal says what
    // to do rather than only that something is wrong.
    std::fs::write(root.join("AGENTS.md"), "a different contract\n").expect("edit");
    let error = rule.based_on[0]
        .resolve(&root, &rule.id)
        .expect_err("the material moved out from under the rule");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(
        error.message.contains("update its based_on commit"),
        "{error}"
    );
}

/// engr does not guess what a rule meant to rest on.
#[test]
fn an_unresolvable_basis_fails_closed() {
    let (_dir, root) = workspace();
    git(&root, &["init", "-q"]);
    std::fs::write(root.join("AGENTS.md"), "the contract\n").expect("basis");
    let real = commit_all(&root, "the contract");

    let cases: [(&str, &str, i32); 4] = [
        (
            "a path that is not there",
            "docs/missing.md",
            engr::EXIT_NOT_FOUND,
        ),
        (
            "a path climbing out of the project",
            "../outside.md",
            engr::EXIT_SCHEMA,
        ),
        ("an absolute path", "/etc/hosts", engr::EXIT_SCHEMA),
        (
            "a path that is there but not at the pin",
            "later.md",
            engr::EXIT_NOT_FOUND,
        ),
    ];
    std::fs::write(root.join("later.md"), "added after the pin\n").expect("later");

    for (why, path, code) in cases {
        let commit = if path == "later.md" {
            format!("\n    commit: {real}")
        } else {
            String::new()
        };
        write_rule(
            &root,
            "basis",
            &format!(
                "---\nid: basis\napplies:\n  domains:\n    - object\nbased_on:\n  - path: {path}{commit}\n---\n\n# Basis\n\nThe rule.\n"
            ),
        );
        let rule = rules::load_all(&root).expect("rules").remove(0);
        let error = rule.based_on[0].resolve(&root, &rule.id).expect_err(why);
        assert_eq!(error.code, code, "{why}: {error}");
    }

    // A commit this repository does not have is refused rather than resolved
    // against whatever is currently on disk.
    write_rule(
        &root,
        "basis",
        &format!(
            "---\nid: basis\napplies:\n  domains:\n    - object\nbased_on:\n  - path: AGENTS.md\n    commit: {}\n---\n\n# Basis\n\nThe rule.\n",
            "0".repeat(40)
        ),
    );
    let rule = rules::load_all(&root).expect("rules").remove(0);
    let error = rule.based_on[0]
        .resolve(&root, &rule.id)
        .expect_err("an absent commit");
    assert_eq!(error.code, engr::EXIT_NOT_FOUND);
}
