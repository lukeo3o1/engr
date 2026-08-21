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
    assert!(error.message.contains("link to somewhere else"), "{error}");
}
