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
    assert!(rules[0].body.contains("# Architecture consistency"));

    // Renaming the file does not create a different rule. The filename is a
    // locator; the id is the identity, and anything that pinned this rule
    // pinned the id.
    std::fs::remove_file(rules::dir(&root).join("architecture.md")).expect("remove");
    write_rule(&root, "record-quality", ARCHITECTURE);
    let renamed = rules::load_all(&root).expect("rules");
    assert_eq!(renamed.len(), 1);
    assert_eq!(renamed[0].id, "architecture-consistency");
    assert_eq!(renamed[0].body, rules[0].body, "and it says the same thing");
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
fn a_domain_with_no_rule_has_an_empty_applicable_set() {
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
        "the rule layer reports the set; what an empty one means is the domain's"
    );
    assert!(rules::applicable(&root, Domain::Collection)
        .expect("collection")
        .is_empty());

    // And a workspace with no rules directory at all reports the same set.
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
/// Standard YAML, then a strict Rule schema — two layers, and both must hold.
///
/// #25 settles the syntax layer as standard YAML rather than an engr subset, so
/// spellings a conforming parser accepts are accepted here. What engr still
/// decides is whether the parsed document is a *Rule*: an unknown field, a
/// domain this version does not have, or an id outside the canonical grammar is
/// refused, because reading past any of them would review against a rule only
/// partly understood.
#[test]
fn front_matter_is_standard_yaml_judged_by_a_strict_rule_schema() {
    let (_dir, root) = workspace();
    std::fs::write(root.join("AGENTS.md"), "the contract\n").expect("basis");

    // Flow style, quoted scalars and inline maps are ordinary YAML. The old
    // hand parser rejected every one of them, which narrowed the format past
    // what the canonical contract says.
    for (why, text) in [
        (
            "flow sequences",
            "---\nid: flow-style\napplies:\n  domains: [object, backlog]\nbased_on: [{path: AGENTS.md}]\n---\n\n# Flow\n\nThe rule.\n",
        ),
        (
            "quoted scalars",
            "---\nid: \"quoted-id\"\napplies:\n  domains:\n    - \"object\"\n---\n\n# Quoted\n\nThe rule.\n",
        ),
        (
            "an inline mapping with a commit",
            &format!(
                "---\nid: inline\napplies:\n  domains: [object]\nbased_on:\n  - {{path: AGENTS.md, commit: {}}}\n---\n\n# Inline\n\nThe rule.\n",
                "a".repeat(40)
            ),
        ),
    ] {
        write_rule(&root, "yaml", text);
        let rule = rules::load_all(&root)
            .unwrap_or_else(|error| panic!("{why} is ordinary yaml: {error}"))
            .remove(0);
        assert!(rule.domains.contains(&Domain::Object), "{why}");
    }

    // Valid YAML is still not automatically a valid Rule.
    for (why, text) in [
        (
            "a field a newer version might mean something by",
            "---\nid: x\napplies:\n  domains: [object]\nseverity: blocking\n---\n\n# body\n",
        ),
        (
            "a selector v1 deliberately does not have",
            "---\nid: x\napplies:\n  domains: [object]\n  actions: [revise]\n---\n\n# body\n",
        ),
        (
            "an id outside the canonical grammar",
            "---\nid: Architecture Consistency\napplies:\n  domains: [object]\n---\n\n# body\n",
        ),
        (
            "an id with an underscore, which the grammar does not have",
            "---\nid: architecture_consistency\napplies:\n  domains: [object]\n---\n\n# body\n",
        ),
    ] {
        write_rule(&root, "yaml", text);
        let error = rules::load_all(&root).expect_err(why);
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{why}: {error}");
    }
}

/// The normative body is stored exactly as written.
///
/// The text a review surface shows has to be the text a review identifies, and
/// trimming would make those two different. Emptiness is decided by refusing,
/// never by rewriting.
#[test]
fn the_normative_body_is_never_rewritten() {
    let (_dir, root) = workspace();
    let text = "---\nid: spacing\napplies:\n  domains: [object]\n---\n\n\n   # Indented heading\n\n   Body text.   \n\n";
    write_rule(&root, "spacing", text);
    let rule = rules::load_all(&root).expect("rules").remove(0);
    assert_eq!(
        rule.body, "\n\n   # Indented heading\n\n   Body text.   \n\n",
        "every byte of the rule is the rule"
    );

    write_rule(
        &root,
        "spacing",
        "---\nid: spacing\napplies:\n  domains: [object]\n---\n\n   \n\n",
    );
    let error = rules::load_all(&root).expect_err("a body of whitespace is no body");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
}

/// A basis cannot leave the project by following a link.
///
/// Rejecting `..` and absolute spellings is not enough, because reading follows
/// symlinks: a tracked `policy.md -> /outside/policy.md` would make a rule rest
/// on mutable material nobody in the project can see or review.
#[test]
#[cfg(unix)]
fn a_basis_cannot_escape_the_project_through_a_link() {
    let (_dir, root) = workspace();
    let outside = TempDir::new().expect("outside");
    let target = outside.path().join("policy.md");
    std::fs::write(&target, "material nobody here can review\n").expect("outside file");
    std::os::unix::fs::symlink(&target, root.join("policy.md")).expect("symlink");

    write_rule(
        &root,
        "escape",
        "---\nid: escape\napplies:\n  domains: [object]\nbased_on:\n  - path: policy.md\n---\n\n# Escape\n\nThe rule.\n",
    );
    let rule = rules::load_all(&root)
        .expect("the rule itself is fine")
        .remove(0);
    let error = rule.based_on[0]
        .resolve(&root, &rule.id)
        .expect_err("a basis outside the project is not project material");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("outside the project"), "{error}");

    // And a link that stays inside the project is refused too, for a different
    // reason: a basis has to name one material, and a link names a path that
    // names a file. `git show <commit>:policy.md` yields the link blob — the
    // target's *name* — where a working-tree read yields the target's
    // *contents*, so a pinned basis over an unchanged link is stale forever.
    std::fs::write(root.join("real.md"), "material anyone here can read\n").expect("inside file");
    std::fs::remove_file(root.join("policy.md")).expect("remove");
    std::os::unix::fs::symlink(root.join("real.md"), root.join("policy.md")).expect("symlink");
    let error = rule.based_on[0]
        .resolve(&root, &rule.id)
        .expect_err("a link is not the file itself");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(
        error.message.contains("is a link rather than the file"),
        "{error}"
    );

    // Naming the file directly is what works, which is the whole ask.
    write_rule(
        &root,
        "escape",
        "---\nid: escape\napplies:\n  domains: [object]\nbased_on:\n  - path: real.md\n---\n\n# Escape\n\nThe rule.\n",
    );
    let rule = rules::load_all(&root).expect("rules").remove(0);
    let resolved = rule.based_on[0]
        .resolve(&root, &rule.id)
        .expect("a real file");
    assert_eq!(resolved.content, "material anyone here can read\n");
}

/// One stored path names one file, wherever the workspace sits.
///
/// #25 defines `based_on.path` as repository-relative, and engr allows `.engr`
/// to live below the repository top level. Current material was read from the
/// workspace root while pinned material came through `git show <commit>:<path>`,
/// which always resolves from the top level — so in `repo/sub/.engr` a rule
/// naming `AGENTS.md` bound `repo/sub/AGENTS.md` now and `repo/AGENTS.md` at the
/// pin. It could then be called stale, or current, on the strength of a file it
/// never named.
///
/// The fixture puts the same filename at both levels with different contents,
/// so the test proves which bytes are bound rather than only that something
/// loaded.
#[test]
fn a_basis_path_names_the_same_file_from_both_directions() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path();
    git(repo, &["init", "-q"]);
    std::fs::create_dir_all(repo.join("sub")).expect("sub");
    std::fs::write(repo.join("AGENTS.md"), "the repository contract\n").expect("root basis");
    std::fs::write(repo.join("sub/AGENTS.md"), "a different file entirely\n").expect("sub file");

    let root = repo.join("sub");
    store::init(&root).expect("init");
    std::fs::create_dir_all(rules::dir(&root)).expect("rules dir");
    let pinned_at = commit_all(repo, "both files");

    for (why, commit) in [
        ("floating", String::new()),
        ("pinned", format!("\n    commit: {pinned_at}")),
    ] {
        write_rule(
            &root,
            "basis",
            &format!(
                "---\nid: basis\napplies:\n  domains: [object]\nbased_on:\n  - path: AGENTS.md{commit}\n---\n\n# Basis\n\nThe rule.\n"
            ),
        );
        let rule = rules::load_all(&root).expect("rules").remove(0);
        let resolved = rule.based_on[0].resolve(&root, &rule.id).expect(why);
        assert_eq!(
            resolved.content, "the repository contract\n",
            "{why}: a repository-relative path is relative to the repository"
        );
    }

    // And the two agree, which is the property the whole pinned/current
    // comparison rests on: a pin is only meaningful if both sides name one file.
    write_rule(
        &root,
        "basis",
        &format!(
            "---\nid: basis\napplies:\n  domains: [object]\nbased_on:\n  - path: AGENTS.md\n    commit: {pinned_at}\n---\n\n# Basis\n\nThe rule.\n"
        ),
    );
    let rule = rules::load_all(&root).expect("rules").remove(0);
    rule.based_on[0]
        .resolve(&root, &rule.id)
        .expect("unchanged material is not stale");

    // Editing the file the rule actually names stales it; editing the
    // same-named file beside the workspace does not.
    std::fs::write(repo.join("sub/AGENTS.md"), "still a different file\n").expect("edit sub");
    rule.based_on[0]
        .resolve(&root, &rule.id)
        .expect("a file this rule does not name cannot stale it");
    std::fs::write(repo.join("AGENTS.md"), "a revised contract\n").expect("edit root");
    let error = rule.based_on[0]
        .resolve(&root, &rule.id)
        .expect_err("the named material moved");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
}

/// A pinned basis over a link would be stale forever, so the link is refused.
///
/// This is the same identity invariant as the nested-workspace case, one level
/// deeper: `git show <commit>:<path>` on a symlink returns the link blob — the
/// target's name as text — while a working-tree read returns the target's
/// contents. The two halves of one persisted path would compare `real
/// contents` against `real.md`, and no change anyone could make to the project
/// would ever bring them together.
///
/// Refused rather than followed. Following would mean re-implementing link
/// resolution over historical git trees — cycles, depth, escapes, missing
/// targets — for a case no project has asked for, and every one of those edges
/// is a way for a basis to mean something other than what it names.
#[test]
#[cfg(unix)]
fn a_pinned_basis_cannot_be_taken_through_a_link() {
    let (_dir, root) = workspace();
    git(&root, &["init", "-q"]);
    std::fs::write(root.join("real.md"), "the contract\n").expect("file");
    std::os::unix::fs::symlink("real.md", root.join("policy.md")).expect("symlink");
    let pinned_at = commit_all(&root, "a file and a link to it");

    // git really does store the two differently — this is the whole finding.
    assert_eq!(
        engr::git::blob_at(&root, &pinned_at, "policy.md").expect("blob"),
        "real.md",
        "the pinned side sees the link's target name"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("policy.md")).expect("disk"),
        "the contract\n",
        "and the current side sees the target's contents"
    );

    for (why, path) in [("floating", "policy.md"), ("pinned", "policy.md")] {
        let commit = if why == "pinned" {
            format!("\n    commit: {pinned_at}")
        } else {
            String::new()
        };
        write_rule(
            &root,
            "linked",
            &format!(
                "---\nid: linked\napplies:\n  domains: [object]\nbased_on:\n  - path: {path}{commit}\n---\n\n# Linked\n\nThe rule.\n"
            ),
        );
        let rule = rules::load_all(&root).expect("rules").remove(0);
        let error = rule.based_on[0]
            .resolve(&root, &rule.id)
            .expect_err(why)
            .message;
        assert!(
            error.contains("is a link rather than the file"),
            "{why}: {error}"
        );
    }

    // The file itself pins and stays current, which is what the link was
    // standing in for.
    write_rule(
        &root,
        "linked",
        &format!(
            "---\nid: linked\napplies:\n  domains: [object]\nbased_on:\n  - path: real.md\n    commit: {pinned_at}\n---\n\n# Linked\n\nThe rule.\n"
        ),
    );
    let rule = rules::load_all(&root).expect("rules").remove(0);
    let resolved = rule.based_on[0]
        .resolve(&root, &rule.id)
        .expect("the file itself");
    assert_eq!(resolved.content, "the contract\n");
}

/// A rule is the file the workspace tracks, not a link to one.
///
/// `read_to_string` follows links, so a locator pointing outside the repository
/// let engr enforce policy the project does not track — and one pointing inside
/// still disagreed with git, which records a link as its target's *name* where
/// the loader reads the target's *contents*. #25 makes rules git-tracked project
/// data with git as their history; a rule whose bytes git does not hold is not
/// one.
///
/// Refused before reading, because the enumeration already decided that this
/// path is a rule — so this path is what has to be a rule.
#[test]
#[cfg(unix)]
fn a_rule_file_is_the_file_itself_and_not_a_link_to_one() {
    let (_dir, root) = workspace();
    std::fs::write(root.join("AGENTS.md"), "the contract\n").expect("basis");
    let outside = TempDir::new().expect("outside");
    let elsewhere = outside.path().join("policy.md");
    std::fs::write(&elsewhere, ARCHITECTURE).expect("outside rule");
    std::fs::write(root.join("real-rule.md"), ARCHITECTURE).expect("inside rule");

    for (why, target) in [
        ("outside the repository", elsewhere.clone()),
        ("inside it", root.join("real-rule.md")),
    ] {
        let locator = rules::dir(&root).join("architecture.md");
        std::os::unix::fs::symlink(&target, &locator).expect("symlink");
        let error = rules::load_all(&root).expect_err(why);
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{why}: {error}");
        assert!(
            error.message.contains("a link rather than the rule itself"),
            "{why}: {error}"
        );
        std::fs::remove_file(&locator).expect("remove");
    }

    // The same bytes, written as a rule file rather than pointed at, load.
    write_rule(&root, "architecture", ARCHITECTURE);
    let rules = rules::load_all(&root).expect("a real file");
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].id, "architecture-consistency");
}

/// The rules directory is where it says it is.
///
/// A redirected `rules` would make every rule in the workspace come from
/// somewhere the workspace does not track — the same failure as a redirected
/// rule file, one level up, and reached without touching a single rule.
#[test]
#[cfg(unix)]
fn the_rules_directory_cannot_be_redirected_elsewhere() {
    let (_dir, root) = workspace();
    std::fs::write(root.join("AGENTS.md"), "the contract\n").expect("basis");
    write_rule(&root, "architecture", ARCHITECTURE);
    rules::load_all(&root).expect("sound to begin with");

    let outside = TempDir::new().expect("outside");
    std::fs::write(outside.path().join("architecture.md"), ARCHITECTURE).expect("outside rule");
    std::fs::remove_dir_all(rules::dir(&root)).expect("remove");
    std::os::unix::fs::symlink(outside.path(), rules::dir(&root)).expect("symlink");

    let error = rules::load_all(&root).expect_err("the whole policy came from elsewhere");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(
        error.message.contains("is a link to somewhere else"),
        "{error}"
    );
}

/// `.md` is a name, not a kind.
///
/// Refusing links was not enough: the enumeration handed every other `*.md`
/// entry straight to a read. A directory fails noisily, but on Unix a FIFO
/// named `policy.md` makes the read block until someone opens the other end —
/// so a filesystem object nobody can commit turns into a workspace that will
/// not load rules at all, and the failure is a hang rather than an answer.
///
/// Checked before reading, because that is while it can still be a refusal.
#[test]
fn a_rule_entry_must_be_a_regular_file() {
    let (_dir, root) = workspace();
    std::fs::write(root.join("AGENTS.md"), "the contract\n").expect("basis");
    write_rule(&root, "architecture", ARCHITECTURE);
    rules::load_all(&root).expect("sound to begin with");

    // Portable: a directory whose name ends in `.md`.
    let shaped = rules::dir(&root).join("directory.md");
    std::fs::create_dir(&shaped).expect("directory");
    let error = rules::load_all(&root).expect_err("a directory is not a rule");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("must be a regular file"), "{error}");
    std::fs::remove_dir(&shaped).expect("remove");

    // Unix: the case that would otherwise hang rather than fail. Bounded on a
    // worker thread, so a reintroduced blocking read is a failing test with a
    // message rather than a suite that never finishes — the whole point is that
    // loading cannot block, and a test that hangs to prove it has proved
    // nothing anyone will read.
    #[cfg(unix)]
    {
        let pipe = rules::dir(&root).join("policy.md");
        assert!(std::process::Command::new("mkfifo")
            .arg(&pipe)
            .status()
            .expect("mkfifo")
            .success());

        let (sender, receiver) = std::sync::mpsc::channel();
        let probe = root.clone();
        std::thread::spawn(move || {
            let _ = sender.send(rules::load_all(&probe).map(|rules| rules.len()));
        });
        let outcome = receiver
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("loading rules must not block on a pipe");
        let error = outcome.expect_err("a pipe is not a rule");
        assert_eq!(error.code, engr::EXIT_SCHEMA);
        assert!(error.message.contains("must be a regular file"), "{error}");
        std::fs::remove_file(&pipe).expect("remove");
    }

    // And the real rule beside them still loads, so the refusal is about the
    // entry rather than about the directory it was found in.
    let rules = rules::load_all(&root).expect("rules");
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].id, "architecture-consistency");
}

/// A basis names a real regular file, exactly as a rule file must.
///
/// The path and symlink checks pass a FIFO happily, and then the read blocks
/// until someone opens the other end — so a project file nobody can commit
/// becomes a rule that can never be resolved, and the failure is a hang rather
/// than an answer. Bounded here for the same reason it is bounded for rule
/// files: a test that hangs to prove resolution cannot hang has proved nothing.
#[test]
#[cfg(unix)]
fn a_basis_must_be_a_regular_file() {
    let (_dir, root) = workspace();
    std::fs::write(root.join("AGENTS.md"), "the contract\n").expect("basis");
    write_rule(&root, "architecture", ARCHITECTURE);
    let rule = rules::load_all(&root).expect("rules").remove(0);
    rule.based_on[0]
        .resolve(&root, &rule.id)
        .expect("a real file");

    std::fs::remove_file(root.join("AGENTS.md")).expect("remove");
    assert!(std::process::Command::new("mkfifo")
        .arg(root.join("AGENTS.md"))
        .status()
        .expect("mkfifo")
        .success());

    let (sender, receiver) = std::sync::mpsc::channel();
    let probe = root.clone();
    std::thread::spawn(move || {
        let rule = rules::load_all(&probe).expect("rules").remove(0);
        let _ = sender.send(rule.based_on[0].resolve(&probe, &rule.id).map(|_| ()));
    });
    let outcome = receiver
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("resolving a basis must not block on a pipe");
    let error = outcome.expect_err("a pipe is not project material");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("not a regular file"), "{error}");
}

/// A pinned basis checks what git recorded, not only what git prints.
///
/// `git show <commit>:<path>` prints a symlink's *target name* as though it
/// were content. So a historical link whose target name happens to equal a
/// later regular file's contents compares equal, and the pin reads as current
/// across a change from a link to a file — the one transition the no-symlink
/// rule exists to keep visible. The tree entry mode is the only place that
/// distinction survives.
#[test]
#[cfg(unix)]
fn a_pinned_basis_checks_the_recorded_mode_and_not_only_the_bytes() {
    let (_dir, root) = workspace();
    git(&root, &["init", "-q"]);
    std::fs::write(root.join("real.md"), "the contract\n").expect("file");
    std::os::unix::fs::symlink("real.md", root.join("policy.md")).expect("symlink");
    let pinned_at = commit_all(&root, "a link and its target");

    // The trap: replace the link with a regular file whose literal contents are
    // the link's old target name. Byte-for-byte, both sides now read `real.md`.
    std::fs::remove_file(root.join("policy.md")).expect("remove");
    std::fs::write(root.join("policy.md"), "real.md").expect("same bytes");
    assert_eq!(
        engr::git::blob_at(&root, &pinned_at, "policy.md").expect("blob"),
        std::fs::read_to_string(root.join("policy.md")).expect("disk"),
        "the two sides agree on content, which is exactly the trap"
    );

    write_rule(
        &root,
        "pinned",
        &format!(
            "---\nid: pinned\napplies:\n  domains: [object]\nbased_on:\n  - path: policy.md\n    commit: {pinned_at}\n---\n\n# Pinned\n\nThe rule.\n"
        ),
    );
    let rule = rules::load_all(&root).expect("rules").remove(0);
    let error = rule.based_on[0]
        .resolve(&root, &rule.id)
        .expect_err("the historical entry was a link, whatever its bytes said");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("not a regular file at"), "{error}");
}

/// A `based_on` commit names a commit, not a tag that points at one.
#[test]
fn a_pinned_basis_refuses_an_object_that_is_not_a_commit() {
    let (_dir, root) = workspace();
    git(&root, &["init", "-q"]);
    std::fs::write(root.join("AGENTS.md"), "the contract\n").expect("basis");
    commit_all(&root, "the contract");
    git(
        &root,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            "tag",
            "-a",
            "v1",
            "-m",
            "annotated",
        ],
    );
    let tag = String::from_utf8(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["rev-parse", "v1"])
            .output()
            .expect("rev-parse")
            .stdout,
    )
    .expect("utf8")
    .trim()
    .to_owned();

    // It reaches a commit — which is why a reachability check accepted it — and
    // it is not one.
    assert!(engr::git::exists(&root, &tag));
    assert_eq!(
        engr::git::object_type(&root, &tag).as_deref(),
        Some("tag"),
        "an annotated tag is its own object"
    );

    write_rule(
        &root,
        "tagged",
        &format!(
            "---\nid: tagged\napplies:\n  domains: [object]\nbased_on:\n  - path: AGENTS.md\n    commit: {tag}\n---\n\n# Tagged\n\nThe rule.\n"
        ),
    );
    let rule = rules::load_all(&root).expect("rules").remove(0);
    let error = rule.based_on[0]
        .resolve(&root, &rule.id)
        .expect_err("a tag id is not a commit id");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(error.message.contains("is not a commit"), "{error}");
}

/// A broken `rules` link is not an empty rule set.
///
/// `Path::exists()` follows links, so a dangling `.engr/rules` answered "no" and
/// took the empty-set path — reporting that a workspace has no policy when what
/// it has is policy pointing somewhere unreadable. Absence and a broken
/// redirection are different facts, and only the first is an empty set.
#[test]
#[cfg(unix)]
fn a_dangling_rules_directory_is_not_an_absent_one() {
    let (_dir, root) = workspace();
    std::fs::remove_dir_all(rules::dir(&root)).expect("remove");
    std::os::unix::fs::symlink(root.join("nowhere"), rules::dir(&root)).expect("symlink");
    assert!(
        !rules::dir(&root).exists(),
        "following the link finds nothing"
    );

    let error = rules::load_all(&root).expect_err("a broken redirection is not absence");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(
        error.message.contains("is a link to somewhere else"),
        "{error}"
    );

    // Genuine absence still reports an empty set, which is the fact being kept
    // distinct rather than a message being changed.
    std::fs::remove_file(rules::dir(&root)).expect("remove link");
    assert!(rules::load_all(&root).expect("absent").is_empty());
}

/// Every public way in enforces the same file-identity boundary.
///
/// The boundary is now guaranteed by visibility: the raw single-file reader is
/// private, so there is no public entry point that takes a path and trusts it.
/// What a test can still pin is that the doors which *do* exist agree — a rule
/// must not become legal because a caller reached it through `applicable`
/// rather than `load_all`.
#[test]
#[cfg(unix)]
fn no_public_loader_accepts_what_another_refuses() {
    let (_dir, root) = workspace();
    std::fs::write(root.join("AGENTS.md"), "the contract\n").expect("basis");
    std::fs::write(root.join("real-rule.md"), ARCHITECTURE).expect("real rule");

    let locator = rules::dir(&root).join("architecture.md");
    std::os::unix::fs::symlink(root.join("real-rule.md"), &locator).expect("symlink");
    for (why, outcome) in [
        ("load_all", rules::load_all(&root)),
        ("applicable", rules::applicable(&root, Domain::Object)),
    ] {
        let error = outcome.expect_err(why);
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{why}: {error}");
        assert!(
            error.message.contains("a link rather than the rule itself"),
            "{why}: {error}"
        );
    }
    std::fs::remove_file(&locator).expect("remove");

    // And the same for an entry that would block rather than refuse, bounded so
    // a regression fails instead of stalling.
    assert!(std::process::Command::new("mkfifo")
        .arg(&locator)
        .status()
        .expect("mkfifo")
        .success());
    for why in ["load_all", "applicable"] {
        let (sender, receiver) = std::sync::mpsc::channel();
        let probe = root.clone();
        let via = why;
        std::thread::spawn(move || {
            let outcome = if via == "load_all" {
                rules::load_all(&probe).map(|rules| rules.len())
            } else {
                rules::applicable(&probe, Domain::Object).map(|rules| rules.len())
            };
            let _ = sender.send(outcome);
        });
        let outcome = receiver
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap_or_else(|_| panic!("{why} must not block on a pipe"));
        let error = outcome.expect_err(why);
        assert!(
            error.message.contains("must be a regular file"),
            "{why}: {error}"
        );
    }
}

/// A link anywhere on the way to a rule redirects the whole policy.
///
/// Checking `.engr/rules` and each `*.md` entry was not enough. The anchor was
/// built by canonicalizing `.engr` first, so a link *there* cancelled out of the
/// comparison — both sides followed it, and `repo/.engr -> /outside/workspace`
/// compared equal. Git would then track `.engr` as a link rather than the rule
/// bytes, which is the policy-versus-source mismatch the no-symlink rule exists
/// to prevent.
///
/// The tree behind the link is entirely well-formed. That is the point: nothing
/// about the rules themselves is wrong, and the loader must still refuse,
/// because what is wrong is how they were reached.
#[test]
#[cfg(unix)]
fn a_link_anywhere_on_the_way_to_a_rule_is_refused() {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().to_path_buf();
    std::fs::write(root.join("AGENTS.md"), "the contract\n").expect("basis");

    // A perfectly good workspace, built somewhere else.
    let outside = TempDir::new().expect("outside");
    let elsewhere = outside.path().join("engr-workspace");
    store::init(&root).expect("init");
    std::fs::create_dir_all(rules::dir(&root)).expect("rules dir");
    write_rule(&root, "architecture", ARCHITECTURE);
    assert_eq!(
        rules::load_all(&root).expect("sound to begin with").len(),
        1
    );
    std::fs::rename(store::engr_dir(&root), &elsewhere).expect("move the workspace");
    std::os::unix::fs::symlink(&elsewhere, store::engr_dir(&root)).expect("symlink");

    // Everything behind the link is intact — the rule file is an ordinary
    // regular file and `rules` is an ordinary directory.
    assert!(rules::dir(&root).join("architecture.md").is_file());
    assert!(std::fs::symlink_metadata(rules::dir(&root))
        .expect("metadata")
        .is_dir());

    for (why, outcome) in [
        ("load_all", rules::load_all(&root)),
        ("applicable", rules::applicable(&root, Domain::Object)),
    ] {
        let error = outcome.expect_err(why);
        assert_eq!(error.code, engr::EXIT_SCHEMA, "{why}: {error}");
        assert!(
            error.message.contains("is a link to somewhere else"),
            "{why}: {error}"
        );
    }

    // A link whose target stays inside the project is refused for the same
    // reason. The boundary is not what the link *escapes*; it is that git would
    // track the link rather than the rule bytes either way.
    std::fs::remove_file(store::engr_dir(&root)).expect("remove link");
    std::fs::rename(&elsewhere, root.join("real-engr")).expect("move inside");
    std::os::unix::fs::symlink("real-engr", store::engr_dir(&root)).expect("symlink");
    let error = rules::load_all(&root).expect_err("inside is not the question");
    assert_eq!(error.code, engr::EXIT_SCHEMA);
    assert!(
        error.message.contains("is a link to somewhere else"),
        "{error}"
    );

    // Put it back where it belongs and it loads again, so the refusal is about
    // the link and not about the workspace.
    std::fs::remove_file(store::engr_dir(&root)).expect("remove link");
    std::fs::rename(root.join("real-engr"), store::engr_dir(&root)).expect("restore");
    assert_eq!(rules::load_all(&root).expect("restored").len(), 1);
}

fn subject() -> (serde_json::Value, serde_json::Value) {
    (
        serde_json::json!({"action": "backlog.section_added", "text": "still unresolved"}),
        serde_json::json!({"item": "01a0", "sections": 2}),
    )
}

/// The hash is the identity of an exact review subject.
///
/// Same subject, same value — across processes, because nothing about it is
/// remembered. Different subject in *any* of its parts, different value, so an
/// attestation stops naming anything the moment what it covered moves.
#[test]
fn the_review_hash_is_the_identity_of_what_had_to_be_reviewed() {
    let (_dir, root) = workspace();
    std::fs::write(root.join("AGENTS.md"), "the contract\n").expect("basis");
    write_rule(&root, "architecture", ARCHITECTURE);
    let (mutation, precondition) = subject();

    let hash = |mutation: &serde_json::Value, precondition: &serde_json::Value| {
        rules::bind(
            &root,
            Domain::Backlog,
            mutation.clone(),
            precondition.clone(),
        )
        .expect("bind")
        .sha256()
        .expect("hash")
    };
    let original = hash(&mutation, &precondition);
    assert_eq!(
        original,
        hash(&mutation, &precondition),
        "nothing is remembered, so recomputation is the same value"
    );

    // The mutation.
    assert_ne!(
        original,
        hash(
            &serde_json::json!({"action": "backlog.section_added", "text": "something else"}),
            &precondition
        )
    );
    // The target it is being applied to, with the proposal untouched. Without
    // this in the binding, another agent could move the target under a review
    // and the attestation would still verify.
    assert_ne!(
        original,
        hash(
            &mutation,
            &serde_json::json!({"item": "01a0", "sections": 3})
        )
    );
    // The rule's own text.
    write_rule(
        &root,
        "architecture",
        &ARCHITECTURE.replace("silently contradicts", "contradicts"),
    );
    let after_rule_edit = hash(&mutation, &precondition);
    assert_ne!(original, after_rule_edit);
    write_rule(&root, "architecture", ARCHITECTURE);

    // The material the rule rests on.
    std::fs::write(root.join("AGENTS.md"), "a different contract\n").expect("edit");
    assert_ne!(original, hash(&mutation, &precondition));
    std::fs::write(root.join("AGENTS.md"), "the contract\n").expect("restore");
    assert_eq!(original, hash(&mutation, &precondition));

    // The applicable set. Another rule for this domain is another thing that
    // had to be read, so the review that did not read it no longer stands.
    write_rule(
        &root,
        "second",
        "---\nid: backlog-quality\napplies:\n  domains:\n    - backlog\n---\n\n# Backlog quality\n\nUnresolved matters only.\n",
    );
    assert_ne!(original, hash(&mutation, &precondition));

    // And a rule for a different domain changes nothing here.
    write_rule(
        &root,
        "third",
        "---\nid: work-handoff\napplies:\n  domains:\n    - work\n---\n\n# Work handoff\n\nSay what is left.\n",
    );
    let with_other_domain = hash(&mutation, &precondition);
    std::fs::remove_file(rules::dir(&root).join("third.md")).expect("remove");
    assert_eq!(
        with_other_domain,
        hash(&mutation, &precondition),
        "a rule that does not govern this domain is not part of this review"
    );
}

#[test]
fn an_attestation_is_checked_against_the_subject_as_it_stands_now() {
    let (_dir, root) = workspace();
    std::fs::write(root.join("AGENTS.md"), "the contract\n").expect("basis");
    write_rule(&root, "architecture", ARCHITECTURE);
    let (mutation, precondition) = subject();
    let reviewed = vec!["architecture-consistency".to_owned()];

    let binding = rules::bind(
        &root,
        Domain::Backlog,
        mutation.clone(),
        precondition.clone(),
    )
    .expect("bind");
    let attested = binding.sha256().expect("hash");
    assert_eq!(binding.rule_ids(), reviewed);

    rules::check(
        &root,
        Domain::Backlog,
        mutation.clone(),
        precondition.clone(),
        &attested,
        &reviewed,
    )
    .expect("an unchanged subject");

    // Naming the wrong set is refused even when the hash is right — an agent
    // that names something else has told us its review covered something else.
    let error = rules::check(
        &root,
        Domain::Backlog,
        mutation.clone(),
        precondition.clone(),
        &attested,
        &["something-else".to_owned()],
    )
    .expect_err("the wrong rules");
    assert_eq!(error.code, engr::EXIT_INVARIANT);

    // The material moves; the attestation stops naming anything that exists.
    std::fs::write(root.join("AGENTS.md"), "a different contract\n").expect("edit");
    let error = rules::check(
        &root,
        Domain::Backlog,
        mutation.clone(),
        precondition.clone(),
        &attested,
        &reviewed,
    )
    .expect_err("the subject moved");
    assert_eq!(error.code, engr::EXIT_INVARIANT);
    assert!(error.message.contains("was of something else"), "{error}");
    std::fs::write(root.join("AGENTS.md"), "the contract\n").expect("restore");

    // A rule that cannot be resolved blocks what it governs rather than being
    // skipped. An unusable rule is not a rule that does not apply.
    std::fs::remove_file(root.join("AGENTS.md")).expect("remove basis");
    let error = rules::check(
        &root,
        Domain::Backlog,
        mutation.clone(),
        precondition.clone(),
        &attested,
        &reviewed,
    )
    .expect_err("an unusable rule blocks");
    assert_eq!(error.code, engr::EXIT_NOT_FOUND);

    // A domain with no rules still has a binding, and this layer still checks
    // it. What an empty set *means* is the domain's call — no review required
    // for most, and the thing that blocks autonomous Object admission for one —
    // so the rule layer must not decide it by short-circuiting.
    let empty = rules::bind(
        &root,
        Domain::Collection,
        mutation.clone(),
        precondition.clone(),
    )
    .expect("bind");
    assert!(empty.rule_ids().is_empty());
    rules::check(
        &root,
        Domain::Collection,
        mutation.clone(),
        precondition.clone(),
        &empty.sha256().expect("hash"),
        &[],
    )
    .expect("an empty set is still a subject");
    rules::check(
        &root,
        Domain::Collection,
        mutation,
        precondition,
        "not the hash",
        &[],
    )
    .expect_err("and it is still checked");
}
