//! Project Rules: the semantic questions engr cannot answer for itself.
//!
//! Everything else in this crate enforces mechanical invariants — id grammar,
//! reference existence, valid states, size ceilings, hash integrity. Those are
//! decidable, so engr decides them. A Rule covers what is left: whether wording
//! follows *this project's* recording policy, whether an entry is really
//! unresolved engineering uncertainty, whether a plan follows the milestone
//! policy. No amount of schema answers those.
//!
//! So a Rule is not a check engr runs. It is material an agent is required to
//! have read, named precisely enough that the requirement can be verified after
//! the fact: the exact Rules, the exact project files they rest on, and the
//! exact mutation being judged, fingerprinted together. That fingerprint is the
//! whole mechanism. It proves nothing about comprehension — it cannot, and #25
//! says so plainly — but it does make silently skipping the review impossible
//! through the supported path.
//!
//! Rules are project policy data, not a new authority domain. Git is their
//! history; there is no EventStore, no candidate, no confirmation for a Rule
//! file. Changing one changes what the *next* mutation must be reviewed
//! against, and nothing already admitted.

use crate::{
    ensure, git, store, tool_error, Error, Result, EXIT_INVARIANT, EXIT_NOT_FOUND, EXIT_SCHEMA,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const DIR: &str = "rules";

pub fn dir(root: &Path) -> PathBuf {
    store::engr_dir(root).join(DIR)
}

/// The domains a Rule can apply to, and exactly those.
///
/// v1 applicability is domain-only by decision, not by omission: #25 forbids
/// action, type, state, role, field and path selectors until real use shows
/// domain-wide application causing recurring friction. A narrower selector
/// invented now would be a guess with a persisted representation.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Domain {
    Object,
    Backlog,
    Collection,
    Work,
}

impl Domain {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Object => "object",
            Self::Backlog => "backlog",
            Self::Collection => "collection",
            Self::Work => "work",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "object" => Some(Self::Object),
            "backlog" => Some(Self::Backlog),
            "collection" => Some(Self::Collection),
            "work" => Some(Self::Work),
            _ => None,
        }
    }
}

/// What a Rule asks for once its review attempts are spent.
///
/// Two values in v1, and the narrow one is the default: escalation is something
/// a Rule opts into, because a policy that pulls a person in is a claim about
/// that policy's importance and only its author can make it.
///
/// **This is a request, not an outcome.** What it costs is decided by the domain
/// and by [`Exhaustion`]: an Object stops and may escalate; a *non-destructive*
/// Backlog mutation is kept and marked, and never escalates on this field, while
/// consuming a Backlog Section needs a review that passed and simply does not
/// happen; and Collection and Work have no v1 answer at all. Even `Reject` on an
/// Object stops the *autonomous* path rather than forbidding the mutation — a
/// human may initiate the same one and override the result through the gate.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum OnExhaustion {
    Reject,
    HumanConfirmation,
}

impl OnExhaustion {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reject => "reject",
            Self::HumanConfirmation => "human_confirmation",
        }
    }

    /// Read as a string and mapped here, for the reason `applies.domains` is:
    /// an unsupported value is refused by name with the supported set spelled
    /// out, rather than by a deserializer talking about variants.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "reject" => Some(Self::Reject),
            "human_confirmation" => Some(Self::HumanConfirmation),
            _ => None,
        }
    }
}

/// The default ceiling, applied when a Rule does not name one.
///
/// The earlier reading — that an omitted limit means unlimited — was withdrawn.
/// Every v1 Rule has a finite effective ceiling, so "how many attempts does this
/// get" always has an answer without consulting anything outside the Rule.
pub const DEFAULT_MAX_ATTEMPTS: u32 = 5;

/// One Rule's review policy, **with the defaults already applied**.
///
/// This type never holds "unspecified". A Rule that omits `max_attempts` and one
/// that writes `max_attempts: 5` produce the identical value here, which is the
/// point: the effective semantics participate in review identity, and YAML
/// spelling does not. Two rules that mean the same thing must not hash
/// differently because one author was explicit.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Review {
    pub max_attempts: u32,
    pub on_exhaustion: OnExhaustion,
}

impl Default for Review {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            on_exhaustion: OnExhaustion::Reject,
        }
    }
}

impl Review {
    /// Whether this attempt is past the ceiling.
    ///
    /// Strictly greater, so a limit of 5 leaves attempts 1 through 5 reviewable
    /// and exhausts at 6. The attempt number is agent-attested process metadata
    /// and is deliberately not stored: engr does not count attempts, it only
    /// says what a given count means.
    pub fn exhausted(self, attempt: u32) -> bool {
        attempt > self.max_attempts
    }
}

/// One project file a Rule rests on, and which version of it.
///
/// `commit` absent means the current content, whatever it is now: the Rule
/// follows the project material and a change to that material requires the
/// review to happen again. `commit` present means this Rule was written against
/// that exact historical content — which does **not** license it to keep
/// governing forever, because the current file may since have said something
/// else. See [`Basis::resolve`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Basis {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

/// A basis resolved to the bytes an agent is required to have read.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct ResolvedBasis {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// The exact content, as text. Part of the review binding, so an edit to a
    /// project file invalidates every attestation that rested on it.
    pub content: String,
}

impl Basis {
    /// Resolve to exact content, failing closed on anything ambiguous.
    ///
    /// A missing path, an unresolvable commit, a path absent at that commit, or
    /// a pinned basis whose current file has since changed are all refusals.
    /// engr does not guess a rename and does not quietly substitute another
    /// path: a Rule that cannot say what it rests on cannot be reviewed
    /// against, and admitting under an incomplete Rule set is the one outcome
    /// this mechanism exists to prevent.
    pub fn resolve(&self, root: &Path, rule: &str) -> Result<ResolvedBasis> {
        let current = self.current(root, rule)?;
        let Some(commit) = &self.commit else {
            return Ok(ResolvedBasis {
                path: self.path.clone(),
                commit: None,
                content: current,
            });
        };
        ensure!(
            crate::model::is_canonical_git_oid(commit),
            EXIT_SCHEMA,
            "rule {rule}: based_on {} pins {commit:?}, which is not a full git object id",
            self.path
        );
        ensure!(
            git::exists(root, commit),
            EXIT_NOT_FOUND,
            "rule {rule}: based_on {} pins commit {commit}, which this repository does not have",
            self.path
        );
        // The id must *be* a commit, not merely reach one. An annotated tag
        // peels, so its own object id would pass a reachability check while the
        // value stored is a tag id — and a field specified as a commit id that
        // silently holds something else is a persisted representation nobody
        // can rely on reading back.
        ensure!(
            git::object_type(root, commit).as_deref() == Some("commit"),
            EXIT_SCHEMA,
            "rule {rule}: based_on {} pins {commit}, which is not a commit; a based_on commit names the commit itself, not a tag that points at one",
            self.path
        );
        // And the entry at that commit must be an ordinary file. The mode is the
        // only place this survives: `git show <commit>:<path>` prints a
        // symlink's target *name* as though it were content, so a historical
        // link whose target name equals a later regular file's contents would
        // compare equal and the pin would look current across a change from a
        // link to a file.
        let mode = git::tree_entry_mode(root, commit, &self.path).ok_or_else(|| {
            Error::new(
                EXIT_NOT_FOUND,
                format!(
                    "rule {rule}: based_on {} is not present at commit {commit}",
                    self.path
                ),
            )
        })?;
        ensure!(
            mode == "100644" || mode == "100755",
            EXIT_SCHEMA,
            "rule {rule}: based_on {} was not a regular file at {commit} (git mode {mode}); a basis names a real file, and git records a link as its target's name rather than the file's contents",
            self.path
        );
        let pinned = git::blob_at(root, commit, &self.path).ok_or_else(|| {
            Error::new(
                EXIT_NOT_FOUND,
                format!(
                    "rule {rule}: based_on {} is not present at commit {commit}",
                    self.path
                ),
            )
        })?;
        // The pin records what the Rule was written against; it does not
        // license the Rule to go on governing new admissions after the project
        // said something else. Comparing content rather than commit ids is
        // deliberate: a repository commit that did not touch this path has
        // changed nothing this Rule depends on.
        ensure!(
            pinned == current,
            EXIT_SCHEMA,
            "rule {rule}: based_on {} was reviewed at {commit} and the current file no longer matches it; review the rule against the current material and update its based_on commit",
            self.path
        );
        Ok(ResolvedBasis {
            path: self.path.clone(),
            commit: Some(commit.clone()),
            content: current,
        })
    }

    /// The current content of the path, read from the working tree.
    ///
    /// The working tree rather than HEAD, because this is the material an agent
    /// is being told to go and read, and what it would read is what is on disk.
    /// It also means an uncommitted edit to a Rule's basis invalidates
    /// attestations that rested on the old text, which is the fail-closed
    /// direction.
    fn current(&self, root: &Path, rule: &str) -> Result<String> {
        // Resolved against the **repository** top level, which is what
        // "repository-relative" means and what `git show <commit>:<path>` uses
        // no matter where the workspace sits. `.engr` may live in a
        // subdirectory, and reading current material from there while reading
        // pinned material through git compared two different files: a rule
        // naming `AGENTS.md` in `repo/sub/.engr` bound `repo/sub/AGENTS.md`
        // now and `repo/AGENTS.md` at the pin, so it could be called stale or
        // current on the strength of a file it never named.
        let project = project_root(root);
        let path = safe_join(&project, &self.path).ok_or_else(|| {
            Error::new(
                EXIT_SCHEMA,
                format!(
                    "rule {rule}: based_on {:?} must be a repository-relative path inside the project",
                    self.path
                ),
            )
        })?;
        // Resolved, then checked against the project boundary. Rejecting `..`
        // and absolute spellings is not enough on its own: reading follows
        // symlinks, so a tracked `policy.md -> /outside/policy.md` would make a
        // rule rest on mutable material outside the repository entirely — and a
        // basis nobody in the project can see or review is not project
        // material. The comparison is on resolved paths, because that is the
        // only form in which the question has an answer.
        let resolved = std::fs::canonicalize(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Error::new(
                    EXIT_NOT_FOUND,
                    format!("rule {rule}: based_on {} does not exist", self.path),
                )
            } else {
                tool_error(path.display(), error)
            }
        })?;
        let inside = std::fs::canonicalize(&project)
            .map_err(|error| tool_error(project.display(), error))?;
        ensure!(
            resolved.starts_with(&inside),
            EXIT_SCHEMA,
            "rule {rule}: based_on {} resolves outside the project, so it is not project material anyone here can review",
            self.path
        );
        // One path, one material. A link is a path that denotes another path, and
        // the two halves of a basis cannot follow it the same way: reading from
        // disk yields the target file's contents, while `git show <commit>:<path>`
        // yields the link *blob* — the target's name as text. So a pinned basis
        // over an unchanged in-repository link compares `real contents` against
        // `real.md` and is stale forever, with nothing anyone can do to the
        // project to make it current again.
        //
        // Following it on both sides would mean re-implementing link resolution
        // over historical git trees — cycles, depth, escapes, missing targets —
        // for a case no project has asked for. Refusing keeps the invariant by
        // construction: what a basis names is what a basis is.
        let direct = self
            .path
            .split('/')
            .fold(inside.clone(), |path, part| path.join(part));
        ensure!(
            resolved == direct,
            EXIT_SCHEMA,
            "rule {rule}: based_on {} is a link rather than the file itself, and a link cannot be pinned: git records its target's name where the working tree gives the target's contents. Name the file directly",
            self.path
        );
        // A basis names a real regular file, the same rule rule *files* follow.
        // `.md` is a name, not a kind: a FIFO passes every path check above and
        // then blocks in the read until someone opens the other end, so a
        // project file nobody can commit becomes a rule that can never be
        // resolved — a hang rather than an answer.
        let kind = std::fs::metadata(&resolved)
            .map_err(|error| tool_error(resolved.display(), error))?
            .file_type();
        ensure!(
            kind.is_file(),
            EXIT_SCHEMA,
            "rule {rule}: based_on {} is not a regular file, so it is not project material git can track as this rule's basis",
            self.path
        );
        std::fs::read_to_string(&resolved).map_err(|error| tool_error(resolved.display(), error))
    }
}

/// The root a repository-relative path is relative to.
///
/// The repository top level when there is a repository, and the workspace root
/// otherwise — without git, "repository-relative" has no other meaning, and the
/// workspace is the only project boundary there is.
fn project_root(root: &Path) -> PathBuf {
    git::repo_root(root).unwrap_or_else(|| root.to_path_buf())
}

/// Repository-relative, and staying there. A basis is project material, so a
/// path that climbs out of the project or names an absolute location is refused
/// rather than resolved.
fn safe_join(root: &Path, relative: &str) -> Option<PathBuf> {
    if relative.is_empty() || relative.starts_with('/') || relative.contains('\\') {
        return None;
    }
    let mut path = root.to_path_buf();
    for part in relative.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return None;
        }
        path.push(part);
    }
    Some(path)
}

/// One Rule: what it applies to, what it rests on, and what it says.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct Rule {
    /// Stable identity. The filename is a locator only, so renaming the file
    /// without changing this is not a new Rule.
    pub id: String,
    pub domains: Vec<Domain>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub based_on: Vec<Basis>,
    /// The review policy in force, defaults resolved. See [`Review`].
    pub review: Review,
    /// The normative text, exactly as written.
    pub body: String,
    /// Where it was found. Not part of identity and not hashed.
    #[serde(skip)]
    pub source: PathBuf,
}

impl Rule {
    pub fn applies_to(&self, domain: Domain) -> bool {
        self.domains.contains(&domain)
    }
}

/// Every Rule in the workspace, by id.
///
/// Fails closed on a duplicate id: two files claiming one identity means the
/// applicable Rule set is not determinable, and a review binding over an
/// indeterminate set proves nothing. Sorted by id so the set is independent of
/// filesystem enumeration order — the hash depends on this.
pub fn load_all(root: &Path) -> Result<Vec<Rule>> {
    // The workspace version decides how these bytes are read, so it is checked
    // before they are read — here, at the one place rules enter the process,
    // rather than in the command that happens to have asked.
    //
    // Leaving it to the CLI made persisted meaning depend on which public door a
    // caller came through: `engr rules ls` refused a version 1 workspace while
    // `rules::load_all` accepted the same file and assigned it the version 2
    // defaults, which is precisely the silent reinterpretation the version
    // exists to prevent. Same shape as the raw single-file loader that was made
    // private for the same reason: one door.
    //
    // `bind` and `check` reach rules only through `applicable`, which reaches
    // them only through here, so this one check covers every semantic entry
    // point. It deliberately does not touch the historical snapshot decoder,
    // which answers a different question about a different workspace.
    store::require_current(root)?;
    let dir = dir(root);
    // `exists()` follows links, so a dangling `rules` symlink answered "no" and
    // took the empty-set path — reporting that a workspace has no policy when
    // what it actually has is policy pointing somewhere unreadable. Absence and
    // a broken redirection are different facts, and only the first is an empty
    // set. Asked without following, so the answer is about `rules` itself.
    let listed = match std::fs::symlink_metadata(&dir) {
        Ok(listed) => listed,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(tool_error(dir.display(), error)),
    };
    ensure!(
        !listed.file_type().is_symlink(),
        EXIT_SCHEMA,
        "{}: the rules directory is a link to somewhere else, so the policy engr would enforce is not the policy this workspace tracks",
        dir.display()
    );
    ensure!(
        listed.is_dir(),
        EXIT_SCHEMA,
        "{}: the rules directory is not a directory",
        dir.display()
    );
    // The directory itself must be where it says it is. A redirected `rules`
    // would make every rule in the workspace come from somewhere the workspace
    // does not track, which is the same failure as a redirected rule file and
    // costs one check to refuse.
    // Anchored at the **workspace root**, not at `.engr`. Canonicalizing `.engr`
    // first made a link *there* cancel out of the comparison — both sides
    // followed it, so `repo/.engr -> /outside/workspace` compared equal and the
    // whole policy came from a tree git tracks as a link rather than as rule
    // bytes. The rule is that no link may appear anywhere on the way to a rule
    // file, so the anchor has to start above every component being checked.
    let anchored = std::fs::canonicalize(root)
        .map_err(|error| tool_error(root.display(), error))?
        .join(store::DIR)
        .join(DIR);
    let resolved = std::fs::canonicalize(&dir).map_err(|error| tool_error(dir.display(), error))?;
    ensure!(
        resolved == anchored,
        EXIT_SCHEMA,
        "{}: something on the way to the rules is a link to somewhere else, so the policy engr would enforce is not the policy this workspace tracks",
        dir.display()
    );
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|error| tool_error(dir.display(), error))? {
        let entry = entry.map_err(|error| tool_error(dir.display(), error))?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("md") {
            continue;
        }
        // A rule file is read for its bytes, and `read_to_string` follows links
        // — so a locator pointing outside would let engr enforce policy the
        // repository does not track, and one pointing inside would still differ from
        // git, which records a link as its target's *name* rather than the
        // target's contents. Rules are git-tracked project data and git is
        // their history; a rule whose bytes git does not hold is not one.
        //
        // Refused rather than followed, and refused before reading: the
        // enumeration decided this path is a rule, so this path is what has to
        // be a rule.
        let kind = entry
            .file_type()
            .map_err(|error| tool_error(path.display(), error))?;
        ensure!(
            !kind.is_symlink(),
            EXIT_SCHEMA,
            "{}: a rule file is a link rather than the rule itself; git records a link's target name where the loader would read the target's contents, so the two would not agree on what the rule says",
            path.display()
        );
        // And a regular file, not merely something that is not a link. `.md` is a
        // name, not a kind: a FIFO called `policy.md` makes `read_to_string`
        // block until someone opens the other end, so an entry nobody can
        // commit becomes a workspace that will not load rules at all. Devices
        // and sockets have no place in a model where a rule's bytes are what
        // git tracks for that path.
        //
        // Checked before reading, for the same reason as the link check: the
        // refusal has to happen while it can still be a refusal.
        ensure!(
            kind.is_file(),
            EXIT_SCHEMA,
            "{}: a rule file must be a regular file; this is not one, so it is not something git can track as project policy",
            path.display()
        );
        files.push(path);
    }
    files.sort();
    let mut rules: Vec<Rule> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for path in files {
        let rule = load(&path)?;
        ensure!(
            seen.insert(rule.id.clone()),
            EXIT_SCHEMA,
            "{}: rule id {:?} is already used by another rule; ids identify rules, so two files cannot share one",
            path.display(),
            rule.id
        );
        rules.push(rule);
    }
    rules.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(rules)
}

/// The Rules that govern a mutation in this domain, in a stable order.
///
/// What an **empty** result means is not this layer's to say. For most domains
/// it means no review is required; for autonomous Agent Object admission the
/// accepted #25 refinement makes the presence of a usable object Rule the
/// capability itself, so an empty set blocks that path instead. This function
/// reports the set; the domain that owns the mutation decides what it implies.
pub fn applicable(root: &Path, domain: Domain) -> Result<Vec<Rule>> {
    Ok(load_all(root)?
        .into_iter()
        .filter(|rule| rule.applies_to(domain))
        .collect())
}

/// Read one rule file that has **already** been established as a real regular
/// file in the rules directory.
///
/// Private, and that is the whole point. Public, it was a second answer to
/// "is this a valid rule?" — `load_all` refuses a symlink or a FIFO before
/// reading, while a caller handed the same path to this one followed the link
/// or blocked on the pipe. A persisted resource must not become legal because
/// of which door a caller came through, and the cheapest way to guarantee that
/// is to leave only one door.
///
/// If a single-rule loader is ever wanted publicly it takes the workspace root,
/// not a path, so it can enforce the same boundary rather than trusting the
/// caller to have done it.
fn load(path: &Path) -> Result<Rule> {
    let raw = std::fs::read_to_string(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Error::new(EXIT_NOT_FOUND, format!("{}: not found", path.display()))
        } else {
            tool_error(path.display(), error)
        }
    })?;
    parse(&raw, path)
}
/// Split standard YAML front matter from the normative body.
///
/// Two layers, kept apart because #25 keeps them apart: a conforming YAML
/// parser decides whether the front matter is *syntactically* valid, and the
/// schema below decides whether it is a valid Rule. A document can be perfectly
/// good YAML and still be refused here.
///
/// The schema is strict by `deny_unknown_fields`, which is the fail-closed
/// requirement: a field a newer version might mean something by is a refusal
/// rather than something read past, because reading past it would review
/// against a Rule this version only partly understood.
fn parse(raw: &str, path: &Path) -> Result<Rule> {
    let where_ = path.display().to_string();
    let rest = raw
        .strip_prefix("---\n")
        .or_else(|| raw.strip_prefix("---\r\n"))
        .ok_or_else(|| {
            Error::new(
                EXIT_SCHEMA,
                format!("{where_}: a rule starts with a `---` front matter line"),
            )
        })?;
    let (front, body) = split_front_matter(rest).ok_or_else(|| {
        Error::new(
            EXIT_SCHEMA,
            format!("{where_}: the front matter is never closed by a `---` line"),
        )
    })?;

    let front: FrontMatter = serde_norway::from_str(front)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{where_}: {error}")))?;

    ensure!(
        !front.id.is_empty()
            && front.id.chars().all(|character| {
                character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
            }),
        EXIT_SCHEMA,
        "{where_}: rule id {:?} is not canonical; v1 ids are [a-z0-9-]+",
        front.id
    );

    let mut domains = Vec::new();
    for name in &front.applies.domains {
        let domain = Domain::parse(name).ok_or_else(|| Error::new(
            EXIT_SCHEMA,
            format!("{where_}: {name:?} is not a domain; v1 has object, backlog, collection and work"),
        ))?;
        ensure!(
            !domains.contains(&domain),
            EXIT_SCHEMA,
            "{where_}: domain {name:?} is listed twice"
        );
        domains.push(domain);
    }
    ensure!(
        !domains.is_empty(),
        EXIT_SCHEMA,
        "{where_}: rule {} lists no domains, so nothing would ever be reviewed against it",
        front.id
    );
    // Sorted by name, not by the order the enum happens to be declared in, and
    // not by the order they were written. Both lists are semantically
    // order-insensitive and both are hashed, so their order is part of a machine
    // contract: reordering a rule's domains must not change the identity of a
    // review, and rearranging a Rust enum must not either.
    domains.sort_by_key(|domain| domain.as_str());

    let mut based_on = front.based_on;
    let mut paths: BTreeSet<&str> = BTreeSet::new();
    for basis in &based_on {
        ensure!(
            paths.insert(basis.path.as_str()),
            EXIT_SCHEMA,
            "{where_}: rule {} lists based_on {:?} twice",
            front.id,
            basis.path
        );
    }
    based_on.sort_by(|left, right| left.path.cmp(&right.path));

    // Resolved to effective values here, at the one place a Rule is read, so
    // nothing downstream ever has to know whether a default was written out.
    let review = match front.review {
        None => Review::default(),
        Some(written) => {
            let max_attempts = match written.max_attempts {
                None => DEFAULT_MAX_ATTEMPTS,
                Some(limit) => {
                    // Zero would exhaust before the first attempt, which is not
                    // a ceiling but a way of spelling "never reviewable" that
                    // the schema does not offer. Refused rather than read as an
                    // unreachable rule.
                    ensure!(
                        limit > 0,
                        EXIT_SCHEMA,
                        "{where_}: rule {} sets max_attempts to 0, and a positive limit is what makes a rule reviewable",
                        front.id
                    );
                    limit
                }
            };
            let on_exhaustion = match written.on_exhaustion.as_deref() {
                None => OnExhaustion::Reject,
                Some(name) => OnExhaustion::parse(name).ok_or_else(|| {
                    Error::new(
                        EXIT_SCHEMA,
                        format!(
                            "{where_}: {name:?} is not an exhaustion action; v1 has reject and human_confirmation"
                        ),
                    )
                })?,
            };
            Review {
                max_attempts,
                on_exhaustion,
            }
        }
    };

    // The body is stored exactly as written. Trimming it would rewrite the
    // normative material — leading and trailing whitespace can carry meaning in
    // Markdown, and more to the point, the text a review surface shows has to be
    // the text a review identifies. Emptiness is judged on the trimmed form and
    // decided by refusing, never by rewriting.
    ensure!(
        !body.trim().is_empty(),
        EXIT_SCHEMA,
        "{where_}: rule {} has no body, and the body is the rule",
        front.id
    );
    Ok(Rule {
        id: front.id,
        domains,
        based_on,
        review,
        body: body.to_owned(),
        source: path.to_path_buf(),
    })
}

/// The Rule schema, applied to whatever the YAML parser produced.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FrontMatter {
    id: String,
    applies: Applies,
    #[serde(default)]
    based_on: Vec<Basis>,
    /// Absent is a first-class answer: it means the defaults, not "no policy".
    #[serde(default)]
    review: Option<ReviewFrontMatter>,
}

/// The review block as it may be written, where every field may be left out.
///
/// Kept separate from [`Review`] on purpose. This type is what the file is
/// allowed to say; `Review` is what the Rule means. Collapsing them would put an
/// `Option` into the value that gets hashed, and then an omitted default and a
/// written one would be two different review identities.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewFrontMatter {
    #[serde(default)]
    max_attempts: Option<u32>,
    #[serde(default)]
    on_exhaustion: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Applies {
    /// Read as strings and mapped afterwards, so an unsupported value is
    /// refused by name with the supported set spelled out, rather than by a
    /// deserializer talking about variants.
    domains: Vec<String>,
}

fn split_front_matter(rest: &str) -> Option<(&str, &str)> {
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Some((&rest[..offset], &rest[offset + line.len()..]));
        }
        offset += line.len();
    }
    None
}

/// The exact subject of a review, fingerprinted.
///
/// This is the whole mechanism. An attestation is not a claim that an agent
/// understood anything — it is a claim that it reviewed *this* mutation against
/// *these* rules resting on *this* material. So the binding names all three
/// exactly, and the hash over it is the identity of that subject. Change any of
/// them and the hash changes, and the previous attestation stops naming
/// anything that exists.
///
/// Deterministic and stateless by construction. There is no pending review
/// object, no nonce, no session: a process that restarts recomputes the same
/// value from the same inputs, and admission recomputes it from current state
/// rather than trusting anything it was handed.
#[derive(Serialize)]
pub struct ReviewBinding {
    /// A discriminator, so a hash from this version can never be mistaken for
    /// one produced under different binding rules.
    binding: &'static str,
    version: u32,
    domain: Domain,
    /// The exact semantic mutation, as the domain canonicalizes it.
    mutation: serde_json::Value,
    /// The exact state the mutation is being applied to.
    ///
    /// Without this the binding would cover only the proposed output, and
    /// another agent could change the target underneath a review while leaving
    /// the proposal untouched — the attestation would still verify, against a
    /// subject that no longer exists.
    precondition: serde_json::Value,
    rules: Vec<BoundRule>,
}

/// One rule as it stood, with everything it rests on resolved.
#[derive(Serialize)]
pub struct BoundRule {
    pub id: String,
    /// Sorted, because the set is semantically order-insensitive.
    pub domains: Vec<Domain>,
    /// Sorted by path, for the same reason.
    pub based_on: Vec<ResolvedBasis>,
    /// The effective review policy, defaults resolved before it got here.
    ///
    /// It belongs in the identity because it decides the outcome: the same
    /// wording reviewed under a ceiling of 5 and under a ceiling of 1 are not
    /// the same review, and one that escalates to a human on exhaustion is not
    /// the same as one that simply refuses. A binding that omitted this would
    /// verify unchanged while the thing it governs had changed meaning.
    pub review: Review,
    /// The normative text, exactly as written and never normalized.
    ///
    /// Built from parsed semantics plus this exact body rather than from the
    /// file's raw bytes. Raw bytes would make review identity depend on
    /// incidental YAML spelling: reordering `applies.domains` changes nothing
    /// about what the rule means or what a reviewer had to read, and it must
    /// not invalidate an attestation. The body is the one part where every
    /// byte is meaning, so it is carried untouched.
    pub body: String,
}

pub const BINDING: &str = "engr-rule-review";
pub const BINDING_VERSION: u32 = 1;

/// The compact diagnostic Backlog records when it admits an exhausted mutation.
///
/// Deliberately two numbers. It exists to say "this went in without a passing
/// review, and here is roughly why", not to reconstruct the review: the complete
/// applicable set and its effective semantics live in the binding, and
/// duplicating per-rule ids or limits here would be a persisted review history
/// that #25 refuses.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct RuleReview {
    /// The mutation-level attempt value supplied for the review.
    pub attempts: u32,
    /// The earliest ceiling in the applicable set — the one that made this
    /// exhausted, since a shared attempt passes the smallest ceiling first.
    pub limit: u32,
}

/// What the applicable rules say about one attempt, **in the terms its domain
/// defines**.
///
/// There is deliberately no domain-neutral answer. #25 gives Object and Backlog
/// opposite ones: Object stops and may call a human, while Backlog admits the
/// unresolved state anyway and marks it, because preserving unresolved
/// engineering intent is the whole point of that domain. A single enum meaning
/// the same thing everywhere would hand the next caller Object's behaviour for a
/// Backlog mutation, which is exactly what #25 forbids.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Exhaustion {
    /// No applicable rule is past its ceiling at this attempt. Every domain.
    NotReached,
    /// **Object.** At least one rule is past its ceiling and none asks for a
    /// human, so autonomous admission stops and the mutation does not happen.
    Refused,
    /// **Object.** At least one exhausted rule asks a human to confirm.
    ///
    /// Escalation wins over refusal among exhausted Object rules, because a rule
    /// naming a human is asking for a decision rather than for the attempt to be
    /// discarded — and a human can still decide to refuse.
    HumanConfirmation,
    /// **Backlog.** Exhausted, with the diagnostic to record — and no
    /// escalation, whatever any exhausted rule's `on_exhaustion` says.
    ///
    /// Whether the mutation may then proceed is the *mutation's* question, not
    /// this one's: a non-destructive Backlog edit soft-admits and stores this
    /// marker, while a consume does not, because destroying unresolved work
    /// needs a review that actually passed. This variant reports that the
    /// ceiling was passed and what to record; it does not authorize anything.
    Exhausted(RuleReview),
}

impl ReviewBinding {
    /// The applicable rules this attempt has exhausted, in the hashed order.
    ///
    /// Returned rather than counted because "which rule stopped this" is what a
    /// caller has to be able to say. A composition that reports only a verdict
    /// leaves a person told no and not told by what.
    pub fn exhausted(&self, attempt: u32) -> Vec<&BoundRule> {
        self.rules
            .iter()
            .filter(|rule| rule.review.exhausted(attempt))
            .collect()
    }

    /// Compose one mutation-level attempt across every applicable rule.
    ///
    /// v1 carries **one scalar for the whole prepared mutation**, compared
    /// independently against each rule's own effective ceiling — there is no
    /// per-rule attempt map, and no counter engr keeps. The number is
    /// agent-attested process metadata, which is why it arrives as an argument
    /// here and is not a field of the binding: it must not be able to reach
    /// [`Self::sha256`], and the shape makes that structural rather than
    /// remembered.
    ///
    /// Attempt numbers are 1-based, so a ceiling of 5 leaves 1 through 5
    /// reviewable and exhausts at 6. A `0` is therefore before the first
    /// attempt and exhausts nothing; there is no admission path that can supply
    /// one yet, and refusing it here would be inventing a rule #25 has not
    /// written.
    pub fn exhaustion(&self, attempt: u32) -> Result<Exhaustion> {
        // The smallest ceiling in the applicable set decides whether anything is
        // exhausted at all. One shared attempt passes the smallest ceiling
        // first, so "some rule is past its ceiling" and "the attempt exceeds the
        // smallest ceiling" are the same statement — which is also why this
        // number is the one Backlog records as `limit`.
        let Some(limit) = self.rules.iter().map(|rule| rule.review.max_attempts).min() else {
            return Ok(Exhaustion::NotReached);
        };
        if attempt <= limit {
            return Ok(Exhaustion::NotReached);
        }
        match self.domain {
            Domain::Object => {
                if self
                    .exhausted(attempt)
                    .iter()
                    .any(|rule| rule.review.on_exhaustion == OnExhaustion::HumanConfirmation)
                {
                    Ok(Exhaustion::HumanConfirmation)
                } else {
                    Ok(Exhaustion::Refused)
                }
            }
            // `on_exhaustion` is deliberately not consulted. For Backlog it
            // never routes to the Human Gate: the domain prioritizes keeping
            // unresolved intent over blocking admission, and the marker is what
            // tells a later reader this was not a passing review.
            Domain::Backlog => Ok(Exhaustion::Exhausted(RuleReview {
                attempts: attempt,
                limit,
            })),
            // Refused rather than answered. #25 leaves these open on purpose,
            // and the one thing this must not do is invent behaviour for them
            // by letting another domain's composition stand in.
            Domain::Collection | Domain::Work => Err(Error::new(
                EXIT_INVARIANT,
                format!(
                    "v1 does not define what an exhausted rule means for a {} mutation",
                    self.domain.as_str()
                ),
            )),
        }
    }
}

impl ReviewBinding {
    /// SHA-256 over the canonical JSON form.
    ///
    /// The same primitive the confirmation gate uses, so key order comes from a
    /// `BTreeMap` rather than from declaration order, and the rule and basis
    /// lists were sorted before they got here.
    pub fn sha256(&self) -> Result<String> {
        crate::confirmation::fingerprint(self)
    }

    pub fn rules(&self) -> &[BoundRule] {
        &self.rules
    }

    /// Every rule id this review must cover, in the order the hash saw them.
    pub fn rule_ids(&self) -> Vec<String> {
        self.rules.iter().map(|rule| rule.id.clone()).collect()
    }
}

/// Freeze what a review of this mutation has to cover.
///
/// Fails closed if any applicable rule cannot be fully resolved. An unusable
/// rule is not a rule that does not apply: admitting under an incomplete set is
/// the one outcome this mechanism exists to prevent, so the mutation it governs
/// is blocked until the rule is repaired.
pub fn bind(
    root: &Path,
    domain: Domain,
    mutation: serde_json::Value,
    precondition: serde_json::Value,
) -> Result<ReviewBinding> {
    let mut bound = Vec::new();
    for rule in applicable(root, domain)? {
        let mut based_on = Vec::new();
        for basis in &rule.based_on {
            based_on.push(basis.resolve(root, &rule.id)?);
        }
        bound.push(BoundRule {
            id: rule.id,
            domains: rule.domains,
            based_on,
            review: rule.review,
            body: rule.body,
        });
    }
    Ok(ReviewBinding {
        binding: BINDING,
        version: BINDING_VERSION,
        domain,
        mutation,
        precondition,
        rules: bound,
    })
}

/// Check an attestation against the subject as it stands right now.
///
/// Recomputed, never looked up. The hash an agent submits is only meaningful if
/// the thing it names is recomputed from current state at the moment of
/// admission — otherwise the interval between review and admission is exactly
/// where the subject can change.
///
/// The rule ids are checked as well as the hash. They are redundant against a
/// correct implementation, and they are not redundant against a confused one: an
/// agent that names the wrong set has told us its review covered something else,
/// and it is better to say so than to accept a hash it may have obtained without
/// reading what the hash was over.
pub fn check(
    root: &Path,
    domain: Domain,
    mutation: serde_json::Value,
    precondition: serde_json::Value,
    attested: &str,
    reviewed: &[String],
) -> Result<()> {
    let binding = bind(root, domain, mutation, precondition)?;
    let expected = binding.sha256()?;
    let ids = binding.rule_ids();
    let mut named: Vec<String> = reviewed.to_vec();
    named.sort();
    named.dedup();
    ensure!(
        named == ids,
        EXIT_INVARIANT,
        "the review names {}, and what governs this {} mutation is {}",
        if named.is_empty() {
            "no rules".to_owned()
        } else {
            named.join(", ")
        },
        domain.as_str(),
        ids.join(", ")
    );
    ensure!(
        attested == expected,
        EXIT_INVARIANT,
        "this review was of something else: the mutation, its target, a rule, or a rule's material has changed since it was reviewed. Review the current subject and attest to {expected}"
    );
    Ok(())
}
