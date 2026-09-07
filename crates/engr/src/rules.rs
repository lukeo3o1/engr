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

use crate::proof::{canonical_bytes, canonical_set, within_safe_integers};
use crate::{
    ensure, git, store, tool_error, Error, Result, EXIT_INVARIANT, EXIT_NOT_FOUND, EXIT_SCHEMA,
    EXIT_USAGE,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::mem::MaybeUninit;
use std::num::NonZeroU32;
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
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
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
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
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

/// One attempt number inside an active review sequence.
///
/// A sequence runs `1 -> 2 -> 3`, and one that is abandoned, lost or restarted
/// may legitimately begin again at `1`. **There is no attempt 0**: it is not
/// another independent sequence, it is a number the protocol never defines.
///
/// Made unrepresentable rather than checked. Every evaluator refusing zero
/// separately is the same rule written several times and forgotten once — and
/// the forgetting is quiet, because a missing check here returns a perfectly
/// ordinary "nothing is exhausted yet" that a caller has no reason to doubt.
/// Validating at construction means the value cannot be wrong by the time any
/// policy question is asked of it.
///
/// This carries no sequence identity and nothing is persisted. #25 is explicit
/// that v1 has no review series, retry counter or reset record, and a type that
/// only bounds one number cannot become one.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Attempt(NonZeroU32);

impl Attempt {
    /// The first attempt of a sequence.
    pub const FIRST: Self = Self(NonZeroU32::MIN);

    pub fn new(value: u32) -> Result<Self> {
        NonZeroU32::new(value).map(Self).ok_or_else(|| {
            Error::new(
                EXIT_USAGE,
                "a review attempt is counted from 1; there is no attempt 0".to_owned(),
            )
        })
    }

    pub fn get(self) -> u32 {
        self.0.get()
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
/// This type never holds "unspecified". Parsed policy uses effective values;
/// artifact provenance separately binds the exact Rule-file bytes, so even a
/// semantics-preserving spelling change invalidates an earlier review.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
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
    pub fn exhausted(self, attempt: Attempt) -> bool {
        attempt.get() > self.max_attempts
    }
}

/// Where the exact material that was reviewed lives, if anywhere.
///
/// Ruled on #25 (`5396557633`). `commit` means one thing: the exact full Git
/// commit OID whose file content **is** the material that was actually
/// reviewed. It may not be recorded because it was `HEAD`, because it was the
/// nearest committed baseline, or because it was the parent of dirty
/// working-tree content.
///
/// Material that is not in Git is said to be so, rather than attributed to a
/// commit that does not contain it:
///
/// ```text
/// reviewed bytes are what HEAD holds  -> commit = that oid, dirty omitted
/// reviewed bytes are anything else    -> commit = null, dirty = true,
///                                        content_sha256 = their identity
/// ```
///
/// **`HEAD` gets no special treatment**, which is the part worth stating twice:
/// it qualifies only when its content equals the reviewed content, and then it
/// qualifies because of that equality, not because of what it is called.
///
/// The commit looked at is the **last one that touched this path**, not `HEAD`.
///
/// Both are deterministic, and only one of them is stable. Binding `HEAD` would
/// change the recorded provenance of every reviewed file on every commit,
/// including commits that touched nothing this Rule rests on — so an unrelated
/// change elsewhere in the repository would move every review digest. The last
/// commit to touch the path moves only when the path does, which is the same
/// principle the pinned case already follows: *"a repository commit that did
/// not touch this path has changed nothing this Rule depends on."*
///
/// Searching history for any older commit that happens to hold identical bytes
/// is deliberately not done. Several could match, and a provenance value that
/// depends on which one an implementation found first is not an identity two
/// readers can agree on.
struct Provenance {
    commit: Option<String>,
    dirty: bool,
    content_sha256: Option<String>,
}

impl Provenance {
    fn of(root: &Path, path: &str, reviewed: &str) -> Self {
        // Both lookups run from the repository top level, because that is the
        // coordinate system the path is already in.
        //
        // They did not, and the two disagreed silently. `git show <commit>:<p>`
        // resolves from the top level whatever `-C` says, while `git log -- <p>`
        // applies the pathspec from the working directory — so with `.engr` in
        // `repo/sub`, a rule at `sub/.engr/rules/x.md` was looked for at
        // `sub/sub/.engr/rules/x.md` and an unpinned basis `AGENTS.md` was
        // looked for under `sub/` while its content had been read from the top.
        // Nothing matched, so committed material reported itself as dirty:
        // the wrong answer, arrived at without any error.
        let project = project_root(root);
        let committed = git::last_commit_for(&project, Path::new(path))
            .filter(|commit| git::blob_at(&project, commit, path).as_deref() == Some(reviewed));
        match committed {
            Some(commit) => Self {
                commit: Some(commit),
                dirty: false,
                content_sha256: None,
            },
            None => Self::dirty(reviewed),
        }
    }
}

impl Provenance {
    /// Material no commit holds, identified rather than attributed.
    fn dirty(reviewed: &str) -> Self {
        Self {
            commit: None,
            dirty: true,
            content_sha256: Some(crate::proof::sha256_of(reviewed)),
        }
    }
}

/// A rule file's path as Git names it, or `None` if it is outside the
/// repository.
///
/// `git show <commit>:<path>` only understands repository-relative paths, so a
/// rule found outside the working tree has no path to compare and therefore no
/// commit that could hold it.
fn rule_relative_path(root: &Path, source: &Path) -> Option<String> {
    let repository = git::repo_root(root)?;
    let source = std::fs::canonicalize(source).ok()?;
    let repository = std::fs::canonicalize(repository).ok()?;
    let relative = source.strip_prefix(&repository).ok()?;
    Some(
        relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
    )
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
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct ResolvedBasis {
    pub path: String,
    /// The exact commit whose content **is** the material that was reviewed,
    /// or `null` when no commit holds it.
    ///
    /// Ruled on #25 (`5396557633`): `commit` is an exact provenance claim, not
    /// a best-effort baseline marker. It may not be recorded merely because it
    /// was `HEAD`, the nearest committed baseline, or the parent of dirty
    /// working-tree content. If the reviewed bytes are not what that commit
    /// holds, then that commit is not the commit of the reviewed material.
    ///
    /// Always written, `null` where there is none. Not omitted when absent,
    /// unlike most optional members here. This one sits inside a hash contract,
    /// and an omitting implementation and a spelling-out one would compute
    /// different bytes for one review — which is the whole failure a shared
    /// canonical form exists to prevent.
    pub commit: Option<String>,
    /// True when the reviewed material is not in Git at all.
    ///
    /// Omitted when false, which is the shape the ruling writes out. It is the
    /// honest alternative to naming a nearby commit: a proof that says "this
    /// material was never committed" can be read correctly later, while one
    /// that names the parent of the edit claims something untrue about content
    /// nobody can now recover.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dirty: bool,
    /// The identity of the exact material reviewed, when no commit holds it.
    ///
    /// Provenance for non-committed material, not a replacement for `content`:
    /// the live review still binds the exact bytes. What this adds is a way for
    /// a later verifier to say *which* material the proof was over, once the
    /// working tree has moved on and the bytes themselves are gone.
    ///
    /// The full content is deliberately **not** copied anywhere durable to make
    /// the proof replayable. #25 says so directly, and the consequence is
    /// stated rather than hidden: when dirty material is no longer available,
    /// verification reports the material unavailable instead of pretending it
    /// can be reconstructed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_sha256: Option<String>,
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
            // Unpinned. The Rule follows the project material, so what was
            // reviewed is whatever the file says now — and the question the
            // ruling asks is whether Git actually holds those exact bytes.
            let provenance = Provenance::of(root, &self.path, &current);
            return Ok(ResolvedBasis {
                path: self.path.clone(),
                commit: provenance.commit,
                dirty: provenance.dirty,
                content_sha256: provenance.content_sha256,
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
        // The pin is honest by the check just above: `pinned == current` means
        // this commit really does hold the reviewed bytes, which is exactly
        // what the ruling requires of a recorded commit.
        Ok(ResolvedBasis {
            path: self.path.clone(),
            commit: Some(commit.clone()),
            dirty: false,
            content_sha256: None,
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
    /// Whether the file wrote a `review:` block at all.
    ///
    /// [`Review`] is what the Rule *means*, and it is deliberately the same
    /// value whether the policy was written out or left to the defaults. This
    /// is the other question, and exactly one caller asks it: the released
    /// predecessor had no Rules at all, so a Rule file in a workspace being
    /// migrated is a later domain and is refused by name rather than admitted
    /// under defaults the generation it came from never offered. Not part of
    /// identity and not hashed — nothing about what the Rule requires depends
    /// on it.
    #[serde(skip)]
    pub written_review: bool,
    /// The normative text, exactly as written.
    pub body: String,
    /// Where it was found. Not part of identity and not hashed.
    #[serde(skip)]
    pub source: PathBuf,
    /// The exact bytes this Rule was parsed from.
    ///
    /// Kept rather than re-read. `.engr/rules/` is deliberately editable
    /// outside the workspace lock, so parsing the file and then reopening it
    /// for provenance is two reads of a moving target: the binding would name
    /// one file's semantics while claiming the next file's `content_sha256` and
    /// commit. Artifact-exact Rule identity means the provenance identifies the
    /// exact artifact whose semantics were reviewed, which is only true if both
    /// come from one read. Not part of identity and not hashed — the fields
    /// derived from it are.
    #[serde(skip)]
    pub raw: String,
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
    // caller came through: `engr rules ls` refused a workspace this build does
    // not write while `rules::load_all` accepted the same file and assigned it
    // this generation's defaults, which is precisely the silent reinterpretation
    // the generation marker exists to prevent. Same shape as the raw
    // single-file loader that was made private for the same reason: one door.
    //
    // `bind` and `check` reach rules only through `applicable`, which reaches
    // them only through here, so this one check covers every semantic entry
    // point. It deliberately does not touch the historical snapshot decoder,
    // which answers a different question about a different workspace.
    store::require_current(root)?;
    load_all_for_migration(root)
}

/// Parse the complete Rule set while a recognized older workspace is inside
/// the explicit migration preflight. The caller has already selected the
/// source schema; ordinary semantic paths must use [`load_all`] instead.
pub(crate) fn load_all_for_migration(root: &Path) -> Result<Vec<Rule>> {
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

    // The profile, before the deserializer resolves the evidence away.
    check_restricted_yaml(front, &where_)?;
    let front: FrontMatter = serde_norway::from_str(front)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("{where_}: {error}")))?;

    check_rule_id(&front.id, &where_)?;

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
    // The one caller that has to know is the generation boundary, which gets
    // the answer from `written_review` rather than from `Review`.
    let written_review = front.review.is_some();
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
        written_review,
        body: body.to_owned(),
        source: path.to_path_buf(),
        raw: raw.to_owned(),
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
#[derive(Serialize, Debug)]
pub struct ReviewBinding {
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

impl ReviewBinding {
    pub(crate) fn has_rules(&self) -> bool {
        !self.rules.is_empty()
    }
}

/// One rule as it stood, with everything it rests on resolved.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
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
    /// Parsed semantics retain this exact body. The `commit`/`content_sha256`
    /// provenance below additionally binds the complete artifact bytes.
    pub body: String,
    /// The exact commit whose content is the Rule file that was reviewed, or
    /// `null` when no commit holds it.
    ///
    /// The ruling says this boundary must not be closed on one side, and the
    /// Rule's own material is the other side. A review binding that recorded
    /// exact provenance for everything a Rule *rests on* while saying nothing
    /// about the Rule *itself* would leave the normative text — the part that
    /// decides the outcome — as the one input nobody could later place.
    pub commit: Option<String>,
    /// True when the Rule file itself is not in Git.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dirty: bool,
    /// The identity of the Rule file as reviewed, when no commit holds it.
    ///
    /// Over the file's exact bytes, matching what the same member means for a
    /// basis. One member with one meaning in both places is the point: this is
    /// provenance for *the artifact that was read*, and `body` remains the
    /// semantic input the review is actually bound to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_sha256: Option<String>,
}

/// The restricted YAML profile a Rule's front matter is read under.
///
/// #66 fixes it: YAML 1.2, duplicate keys invalid, custom tags invalid, anchors
/// and aliases invalid, and only schema-required forms accepted. A typed
/// deserializer enforces almost none of that. It resolves an alias into the
/// value its anchor held and applies a tag, then hands back a perfectly valid
/// `FrontMatter` that no longer stands in any recoverable relation to the bytes
/// it came from.
///
/// That matters more here than it would elsewhere, because Rule Review identity
/// is **artifact-exact**: #66 says any byte-level change to a Rule file,
/// "including semantically equivalent YAML reformatting", changes the
/// ReviewDigest. Two files whose bytes differ must never be one policy — and an
/// anchor is exactly a way to write one policy twice. Left unrestricted, a
/// person reading the file and a build hashing it are looking at two different
/// documents.
///
/// The check runs on the raw front matter, before deserialization, because
/// afterwards the evidence is gone: the constructs have been resolved away and
/// the typed value cannot say which of them produced it.
///
/// The parser's event stream is the boundary. Presentation details such as
/// block versus flow style and ordinary versus explicit keys have already been
/// resolved there, while anchors, aliases, tags, scalar style and mapping
/// membership are still observable. That is exactly the point at which the
/// profile can be complete without trying to reimplement YAML with a textual
/// scanner.
/// The parser is libyaml, and it is the same libyaml `serde_norway` will use on
/// the same bytes a moment later — one pinned copy of one crate, checked in
/// `Cargo.lock`. A second YAML implementation would be a second opinion about
/// what the document says, and the gap between two opinions is precisely where
/// something forbidden survives being looked for.
fn check_restricted_yaml(front: &str, where_: &str) -> Result<()> {
    // SAFETY: `front` outlives the parser. The parser is initialized before any
    // use and deleted on every path that leaves this call after initialization;
    // each event is deleted exactly once, immediately after inspection, and
    // inspection never returns out of the loop. Union members are read only
    // under the tag that selects them.
    unsafe { check_restricted_yaml_events(front, where_) }
}

enum YamlContainer {
    Mapping {
        expecting_key: bool,
        keys: BTreeSet<Vec<u8>>,
    },
    Sequence,
}

/// Where in the document the next node will land, and what has been there.
///
/// The event stream says a node *begins*; it does not say whether that node is
/// a mapping key. That is positional, so it is tracked here: inside a mapping
/// the two alternate, and libyaml has already normalized explicit `? key`
/// syntax and flow style into the same alternation as ordinary block `key:`.
/// Keeping the answer in one place is what makes the duplicate-key rule and the
/// scalar-keys-only rule hold in every presentation rather than in the ones
/// somebody thought to write a scanner for.
struct YamlProfile {
    stack: Vec<YamlContainer>,
    documents: usize,
}

impl YamlProfile {
    /// Account for a node beginning at the current position.
    ///
    /// `scalar` is the key bytes when the node is a scalar, and `None` for a
    /// collection — which is only ever refused, because a rule's front matter
    /// has no use for a mapping keyed by a sequence.
    fn begin_node(&mut self, scalar: Option<&[u8]>, where_: &str, line: u64) -> Result<()> {
        let Some(YamlContainer::Mapping {
            expecting_key,
            keys,
        }) = self.stack.last_mut()
        else {
            // The document root, or a sequence entry: nothing alternates.
            return Ok(());
        };
        if *expecting_key {
            let key = scalar.ok_or_else(|| {
                Error::new(
                    EXIT_SCHEMA,
                    format!("{where_}:{line}: a rule's front matter uses only scalar mapping keys"),
                )
            })?;
            // Compared as scalar *values*, so `id` and `"id"` are the one key
            // they mean, rather than two spellings that slip past each other.
            ensure!(
                keys.insert(key.to_vec()),
                EXIT_SCHEMA,
                "{where_}:{line}: a mapping key appears twice, and a rule has one value for it"
            );
            *expecting_key = false;
        } else {
            *expecting_key = true;
        }
        Ok(())
    }
}

unsafe fn check_restricted_yaml_events(front: &str, where_: &str) -> Result<()> {
    use unsafe_libyaml_norway::{
        yaml_event_delete, yaml_event_t, yaml_parser_delete, yaml_parser_initialize,
        yaml_parser_parse, yaml_parser_set_input_string, yaml_parser_t, YAML_STREAM_END_EVENT,
    };

    let mut parser = MaybeUninit::<yaml_parser_t>::uninit();
    let parser = parser.as_mut_ptr();
    ensure!(
        !yaml_parser_initialize(parser).fail,
        EXIT_SCHEMA,
        "{where_}: could not initialize the YAML parser"
    );
    yaml_parser_set_input_string(parser, front.as_ptr(), front.len() as u64);

    let mut state = YamlProfile {
        stack: Vec::new(),
        documents: 0,
    };
    loop {
        let mut event = MaybeUninit::<yaml_event_t>::uninit();
        let event = event.as_mut_ptr();
        if yaml_parser_parse(parser, event).fail {
            yaml_parser_delete(parser);
            return Err(Error::new(
                EXIT_SCHEMA,
                format!("{where_}: invalid YAML front matter"),
            ));
        }
        let kind = (*event).type_;
        // Inspection is a call rather than a block, and that is what keeps the
        // cleanup below correct. Every refusal in this profile is written with
        // `ensure!`, which expands to `return` — inside the loop body that would
        // leave this event and the whole parser allocated on the way out. A
        // function boundary turns each of those returns into a value, so there
        // is exactly one place that leaves the loop early and it frees both.
        let checked = inspect_yaml_event(event, &mut state, where_);
        yaml_event_delete(event);
        if let Err(error) = checked {
            yaml_parser_delete(parser);
            return Err(error);
        }
        if kind == YAML_STREAM_END_EVENT {
            break;
        }
    }
    yaml_parser_delete(parser);
    Ok(())
}

/// What the profile allows one event to be.
///
/// # Safety
///
/// `event` must point at an event the parser produced and has not yet deleted.
unsafe fn inspect_yaml_event(
    event: *const unsafe_libyaml_norway::yaml_event_t,
    state: &mut YamlProfile,
    where_: &str,
) -> Result<()> {
    use unsafe_libyaml_norway::{
        YAML_ALIAS_EVENT, YAML_DOCUMENT_END_EVENT, YAML_DOCUMENT_START_EVENT,
        YAML_FOLDED_SCALAR_STYLE, YAML_LITERAL_SCALAR_STYLE, YAML_MAPPING_END_EVENT,
        YAML_MAPPING_START_EVENT, YAML_SCALAR_EVENT, YAML_SEQUENCE_END_EVENT,
        YAML_SEQUENCE_START_EVENT,
    };

    let kind = (*event).type_;
    let line = (*event).start_mark.line + 1;
    if kind == YAML_DOCUMENT_START_EVENT {
        state.documents += 1;
        ensure!(
            state.documents == 1 && (*event).data.document_start.implicit,
            EXIT_SCHEMA,
            "{where_}:{line}: a rule's front matter is one implicit document"
        );
    } else if kind == YAML_DOCUMENT_END_EVENT {
        ensure!(
            (*event).data.document_end.implicit,
            EXIT_SCHEMA,
            "{where_}:{line}: a rule's front matter is one implicit document"
        );
    } else if kind == YAML_ALIAS_EVENT {
        // An alias is the one construct with no node properties to inspect: it
        // *is* the reference.
        return Err(Error::new(
            EXIT_SCHEMA,
            format!("{where_}:{line}: a rule's front matter uses no YAML aliases"),
        ));
    } else if kind == YAML_SCALAR_EVENT {
        let scalar = (*event).data.scalar;
        check_yaml_properties(scalar.anchor, scalar.tag, where_, line)?;
        ensure!(
            scalar.style != YAML_LITERAL_SCALAR_STYLE && scalar.style != YAML_FOLDED_SCALAR_STYLE,
            EXIT_SCHEMA,
            "{where_}:{line}: a rule's front matter uses no block scalars"
        );
        // libyaml NUL-terminates every scalar, so the pointer is live even at
        // length zero; the guard is here because `from_raw_parts` is undefined
        // on a null base regardless of length, and that is not a property to
        // depend on a C library for.
        let value = if scalar.value.is_null() {
            &[][..]
        } else {
            std::slice::from_raw_parts(scalar.value, scalar.length as usize)
        };
        state.begin_node(Some(value), where_, line)?;
    } else if kind == YAML_MAPPING_START_EVENT {
        let mapping = (*event).data.mapping_start;
        check_yaml_properties(mapping.anchor, mapping.tag, where_, line)?;
        state.begin_node(None, where_, line)?;
        state.stack.push(YamlContainer::Mapping {
            expecting_key: true,
            keys: BTreeSet::new(),
        });
    } else if kind == YAML_SEQUENCE_START_EVENT {
        let sequence = (*event).data.sequence_start;
        check_yaml_properties(sequence.anchor, sequence.tag, where_, line)?;
        state.begin_node(None, where_, line)?;
        state.stack.push(YamlContainer::Sequence);
    } else if kind == YAML_MAPPING_END_EVENT {
        ensure!(
            matches!(state.stack.pop(), Some(YamlContainer::Mapping { .. })),
            EXIT_SCHEMA,
            "{where_}:{line}: invalid YAML mapping structure"
        );
    } else if kind == YAML_SEQUENCE_END_EVENT {
        ensure!(
            matches!(state.stack.pop(), Some(YamlContainer::Sequence)),
            EXIT_SCHEMA,
            "{where_}:{line}: invalid YAML sequence structure"
        );
    }
    Ok(())
}

/// Anchors and tags are node properties, and every node kind can carry both.
///
/// One function for all three kinds, because "an anchor is allowed on a
/// sequence but not a scalar" is exactly the sort of asymmetry that opens a
/// bypass, and having a single place to state the rule is how it stays absent.
fn check_yaml_properties(anchor: *const u8, tag: *const u8, where_: &str, line: u64) -> Result<()> {
    ensure!(
        anchor.is_null(),
        EXIT_SCHEMA,
        "{where_}:{line}: a rule's front matter uses no YAML anchors"
    );
    ensure!(
        tag.is_null(),
        EXIT_SCHEMA,
        "{where_}:{line}: a rule's front matter uses no YAML tags"
    );
    Ok(())
}

/// The rule-id grammar, in one place so the loader and the snapshot check
/// cannot drift.
///
/// `[a-z0-9][a-z0-9-]{0,31}`: bounded, and it must start with something. A
/// leading `-` reads as a flag wherever an id is typed, and an unbounded id is
/// a filename this build would write and some other filesystem would refuse.
pub(crate) const RULE_ID_MAX: usize = 32;

pub(crate) fn check_rule_id(id: &str, what: &str) -> Result<()> {
    let canonical = id.len() <= RULE_ID_MAX
        && id
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
        && id.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        });
    ensure!(
        canonical,
        EXIT_SCHEMA,
        "{what}: rule id {id:?} is not canonical; ids are [a-z0-9][a-z0-9-]{{0,{}}}",
        RULE_ID_MAX - 1
    );
    Ok(())
}
impl BoundRule {
    /// Hold one snapshot to the frozen Rule contract before it can be hashed.
    ///
    /// Validation rather than privacy, unlike [`crate::proof::ReviewMutation`]
    /// and [`crate::gate::ObjectSubject`]. Those are always computed, so they
    /// can be made unconstructible; a Rule snapshot arrives **as data**, read
    /// back from a candidate that stored it, so there is no construction path
    /// to route it through. What can be required is that it satisfies the same
    /// contract a resolved Rule does.
    ///
    /// Without this, `rebind_object` would take an arbitrary `Vec<BoundRule>`
    /// and hand back a scalar labelled `ReviewDigestContract 1` over a Rule
    /// that could never exist: an id outside the grammar, no applicable domain,
    /// a ceiling of zero, or provenance contradicting itself. Closing the
    /// mutation side and leaving the snapshot side open would have moved the
    /// forgery one level down rather than out.
    pub fn validate(&self) -> Result<()> {
        check_rule_id(&self.id, "rule snapshot")?;
        ensure!(
            !self.domains.is_empty(),
            EXIT_SCHEMA,
            "rule snapshot {}: a rule that applies to no domain governs nothing",
            self.id
        );
        ensure!(
            self.domains == canonical_domains(&self.domains)?,
            EXIT_SCHEMA,
            "rule snapshot {}: domains are a protocol set and arrive in canonical order",
            self.id
        );
        ensure!(
            self.review.max_attempts >= 1,
            EXIT_SCHEMA,
            "rule snapshot {}: a review gets at least one attempt; there is no ceiling of zero",
            self.id
        );
        ensure!(
            !self.body.trim().is_empty(),
            EXIT_SCHEMA,
            "rule snapshot {}: a rule with no normative text asks for nothing",
            self.id
        );
        check_material_provenance(
            self.commit.as_deref(),
            self.dirty,
            self.content_sha256.as_deref(),
            &format!("rule snapshot {}", self.id),
        )?;
        for basis in &self.based_on {
            check_material_provenance(
                basis.commit.as_deref(),
                basis.dirty,
                basis.content_sha256.as_deref(),
                &format!("rule snapshot {} basis {}", self.id, basis.path),
            )?;
        }
        ensure!(
            self.based_on == canonical_bases(&self.based_on)?,
            EXIT_SCHEMA,
            "rule snapshot {}: bases are a protocol set and arrive in canonical order",
            self.id
        );
        Ok(())
    }
}

/// The provenance members are three views of one fact, so they agree or the
/// snapshot is refused.
///
/// Ruled at #25 `5396557633`: material either has a commit that holds it, or is
/// dirty and identified by hash. A snapshot claiming both, or neither, is
/// describing a state the contract does not define.
fn check_material_provenance(
    commit: Option<&str>,
    dirty: bool,
    content_sha256: Option<&str>,
    what: &str,
) -> Result<()> {
    match (commit, dirty) {
        (Some(commit), false) => {
            ensure!(
                crate::model::is_canonical_git_oid(commit),
                EXIT_SCHEMA,
                "{what}: {commit:?} is not a full resolved Git object id"
            );
            ensure!(
                content_sha256.is_none(),
                EXIT_SCHEMA,
                "{what}: material a commit holds is located by that commit, not also by a hash"
            );
            Ok(())
        }
        (None, true) => {
            let hash = content_sha256.ok_or_else(|| {
                Error::new(
                    EXIT_SCHEMA,
                    format!("{what}: dirty material is identified by content_sha256"),
                )
            })?;
            ensure!(
                hash.len() == 64
                    && hash
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                EXIT_SCHEMA,
                "{what}: content_sha256 is 64 lowercase hex characters"
            );
            Ok(())
        }
        (Some(_), true) => Err(Error::new(
            EXIT_SCHEMA,
            format!("{what}: material cannot be both held by a commit and dirty"),
        )),
        (None, false) => Err(Error::new(
            EXIT_SCHEMA,
            format!("{what}: material with no commit is dirty, and says so"),
        )),
    }
}

fn canonical_domains(domains: &[Domain]) -> Result<Vec<Domain>> {
    let mut domains = domains.to_vec();
    canonical_set(&mut domains, "domain")?;
    Ok(domains)
}

fn canonical_bases(bases: &[ResolvedBasis]) -> Result<Vec<ResolvedBasis>> {
    let mut bases = bases.to_vec();
    canonical_set(&mut bases, "basis")?;
    Ok(bases)
}
/// The compact diagnostic Backlog records when it admits an exhausted mutation.
///
/// Deliberately two numbers. It exists to say "this went in without a passing
/// review, and here is roughly why", not to reconstruct the review: the complete
/// applicable set and its effective semantics live in the binding, and
/// duplicating per-rule ids or limits here would be a persisted review history
/// that #25 refuses.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
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
    pub fn exhausted(&self, attempt: Attempt) -> Vec<&BoundRule> {
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
    /// Attempt numbers run from 1, so a ceiling of 5 leaves 1 through 5
    /// reviewable and exhausts at 6. Zero is not a value this can be asked
    /// about: [`Attempt`] refuses it at construction, so the question never
    /// arrives here as an ordinary "nothing exhausted yet".
    pub fn exhaustion(&self, attempt: Attempt) -> Result<Exhaustion> {
        compose(self.domain, &self.rules, attempt)
    }
}

/// Compose the verdict for one attempt over an already-resolved rule set.
fn compose(domain: Domain, rules: &[BoundRule], attempt: Attempt) -> Result<Exhaustion> {
    // The smallest ceiling in the applicable set decides whether anything is
    // exhausted at all. One shared attempt passes the smallest ceiling
    // first, so "some rule is past its ceiling" and "the attempt exceeds the
    // smallest ceiling" are the same statement — which is also why this
    // number is the one Backlog records as `limit`.
    let Some(limit) = smallest_ceiling(rules) else {
        return Ok(Exhaustion::NotReached);
    };
    if attempt.get() <= limit {
        return Ok(Exhaustion::NotReached);
    }
    match domain {
        Domain::Object => {
            if rules
                .iter()
                .filter(|rule| rule.review.exhausted(attempt))
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
            attempts: attempt.get(),
            limit,
        })),
        // Refused rather than answered. #25 leaves these open on purpose,
        // and the one thing this must not do is invent behaviour for them
        // by letting another domain's composition stand in.
        Domain::Collection | Domain::Work => Err(Error::new(
            EXIT_INVARIANT,
            format!(
                "v1 does not define what an exhausted rule means for a {} mutation",
                domain.as_str()
            ),
        )),
    }
}

impl ReviewBinding {
    /// The review proof: SHA-256 over the **RFC 8785 (JCS)** bytes of this
    /// binding, carried as `ReviewDigestContract` `<version>:<digest>`.
    ///
    /// The version travels with the value because this is a long-lived proof. A
    /// bare digest tells a later reader what it is worth and not which
    /// calculation produced it, and both ways of guessing are silent: verify it
    /// under today's rules and a mismatch looks like tampering, or relabel it
    /// and claim a guarantee nobody made.
    ///
    /// The version lives in the scalar and **only** there. There is no
    /// discriminator and no version member inside the hashed payload, which the
    /// prototype this grew from had both of. Two places naming a version is two
    /// places that can disagree, and the outer one is the one a reader has
    /// before they have decided how to read the bytes — so it is the one that
    /// can actually be acted on.
    ///
    /// Deliberately not [`crate::confirmation::fingerprint`], which this used to
    /// delegate to. That primitive canonicalizes through `serde_json`, which is
    /// stable for one implementation and is not JCS — and every value it hashes
    /// is already persisted in confirmed Events and Section seals, so changing
    /// it changes hashes that exist. That is a coordinated workspace migration,
    /// not a refactor, and it is not this slice's.
    ///
    /// Which leaves engr with **two canonicalizations at once**, and that is
    /// worth saying out loud rather than discovering later: this one is JCS, the
    /// gate's is serde. They agree on everything engr's own schema produces and
    /// disagree on exotic object keys and number formats. Unifying them is the
    /// open question, recorded on the PR — not something to settle by quietly
    /// pointing one at the other.
    ///
    /// The rule and basis lists were put in canonical order before they got
    /// here; see [`canonical_set`].
    pub fn digest(&self) -> Result<crate::digest::Versioned> {
        crate::digest::REVIEW.emit(self.digest_under(crate::digest::REVIEW.current)?)
    }

    /// The bare digest this binding has **under one contract version**.
    ///
    /// Selected by version rather than by "whatever this build computes",
    /// because that is what makes a historical proof checkable at all: an
    /// attestation names the contract it was made under, and recomputing it any
    /// other way answers a different question. A version with no calculation
    /// here is refused rather than quietly served the current one — being
    /// listed as verifiable in the support table is not the same as this build
    /// knowing how to reproduce it.
    pub fn digest_under(&self, version: u32) -> Result<String> {
        match version {
            1 => {
                let canonical = canonical_bytes(self, "review binding")?;
                Ok(format!(
                    "{:x}",
                    <sha2::Sha256 as sha2::Digest>::digest(canonical.as_bytes())
                ))
            }
            other => Err(Error::new(
                EXIT_SCHEMA,
                format!(
                    "ReviewDigestContract: this build cannot compute version {other}, only the versions it implements"
                ),
            )),
        }
    }

    pub fn rules(&self) -> &[BoundRule] {
        &self.rules
    }

    /// Every rule id this review must cover, in the order the hash saw them.
    /// Sorted by id, which is **not** the order the hash saw them.
    ///
    /// The hashed order is the protocol's canonical-bytes order, which for a
    /// rule begins with its bases rather than its name. That is right for a
    /// hash and useless for a person or for comparing what an agent said it
    /// reviewed, so this answers the set question in the one order both sides
    /// can produce independently.
    pub fn rule_ids(&self) -> Vec<String> {
        canonical_rule_ids(&self.rules)
    }
}

/// The ids of a bound Rule set, in the one order two sides produce
/// independently.
///
/// One function, because the frozen Challenge and the live binding have to
/// agree on it and each used to derive it its own way: the Challenge mapped the
/// *snapshot* order straight to ids, and the snapshot order is the hash's —
/// which begins with a rule's bases rather than its name and is therefore not
/// id order at all. Two rules whose bases sort the other way round produced a
/// frozen `rules` list that was not the canonical spelling of its own set.
///
/// Sorting `String`s is the JCS element-byte order for these values: a rule id
/// is `[a-z0-9][a-z0-9-]{0,31}`, so no escape can make the quoted encoding sort
/// differently from the bare one.
pub fn canonical_rule_ids(rules: &[BoundRule]) -> Vec<String> {
    let mut ids: Vec<String> = rules.iter().map(|rule| rule.id.clone()).collect();
    ids.sort();
    ids
}

/// Hold a binding's two caller-supplied arguments to the shape its domain
/// froze, where the domain has frozen one.
///
/// The Object domain does not arrive here at all any more. Checking the outer
/// member names was never enough: an unfrozen operation name, an extra member
/// inside `operation`, a target naming nothing and an arbitrary `after` all
/// passed, and produced a scalar labelled `ReviewDigestContract 1` for a
/// descriptor #25 does not define. A shape check cannot verify that `after` is
/// *that operation's* projection, because the only thing that knows is the
/// projection itself.
///
/// So Object bindings are built from the typed frozen projection instead —
/// [`bind_object`] and [`rebind_object`] — and this refuses the untyped route
/// by name rather than trying to validate its way back to a guarantee.
///
/// Backlog describes its mutations under #8 and keeps the untyped route;
/// Collection and Work have no v1 review semantics at all, and inventing a
/// descriptor for them would be exactly the guess with a persisted
/// representation that the contract refuses.
fn check_domain_shape(
    domain: Domain,
    mutation: &serde_json::Value,
    precondition: &serde_json::Value,
) -> Result<()> {
    match domain {
        Domain::Object => Err(Error::new(
            EXIT_USAGE,
            "an object review binds the frozen projection: build it with \
             proof::object_review_mutation and bind it with rules::bind_object"
                .to_owned(),
        )),
        Domain::Backlog | Domain::Collection | Domain::Work => {
            let _ = (mutation, precondition);
            Ok(())
        }
    }
}

/// Bind an Object-domain review over the frozen projection (#25 §4).
///
/// The typed entry point, and the only one. `mutation` can only have come from
/// [`crate::proof::object_review_mutation`], which reads the candidate subject
/// for a real transition — so the operation, target and `after` are the frozen
/// table's, not a caller's description of them.
pub fn bind_object(
    root: &Path,
    mutation: &crate::proof::ReviewMutation,
    expected_rev: u64,
) -> Result<ReviewBinding> {
    let (mutation, precondition) = object_binding_inputs(mutation, expected_rev)?;
    Ok(ReviewBinding {
        domain: Domain::Object,
        mutation,
        precondition,
        rules: bound_rules(root, Domain::Object)?,
    })
}

/// Rebuild an Object-domain binding from Rule snapshots somebody else resolved.
///
/// The counterpart to [`bind_object`], as [`rebind`] is to [`bind`].
pub fn rebind_object(
    mutation: &crate::proof::ReviewMutation,
    expected_rev: u64,
    rules: Vec<BoundRule>,
) -> Result<ReviewBinding> {
    let (mutation, precondition) = object_binding_inputs(mutation, expected_rev)?;
    let rules = checked_snapshots(rules, Domain::Object)?;
    Ok(ReviewBinding {
        domain: Domain::Object,
        mutation,
        precondition,
        rules,
    })
}

fn object_binding_inputs(
    mutation: &crate::proof::ReviewMutation,
    expected_rev: u64,
) -> Result<(serde_json::Value, serde_json::Value)> {
    let mutation = serde_json::to_value(mutation)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("review mutation: {error}")))?;
    let precondition = serde_json::to_value(crate::proof::ReviewPrecondition { expected_rev })
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("review precondition: {error}")))?;
    // Belt and braces: the typed projection must still serialize to the frozen
    // shape. If it ever stops doing so, that is a defect in the projection and
    // this says so here rather than letting the bytes change quietly.
    crate::proof::check_object_review_shape(&mutation, &precondition)?;
    within_safe_integers(&mutation, "mutation")?;
    within_safe_integers(&precondition, "precondition")?;
    Ok((mutation, precondition))
}

/// Every snapshot a caller hands in, held to the frozen Rule contract **for the
/// domain being bound**, and to being a set of Rule identities.
///
/// One place, so the two rebind entry points cannot enforce different amounts.
///
/// The domain argument is the part that was missing. `#25` defines `rules` as
/// the *complete applicable* Rule set for the binding domain, and non-empty is
/// not the same as applicable: a perfectly well-formed `[backlog]` Rule could
/// be hashed into an Object binding, where `bound_rules(root, Object)` could
/// never have selected it.
///
/// Identity uniqueness is the second half, and `canonical_set` does not cover
/// it: that rejects two snapshots whose *canonical bytes* are equal, while two
/// snapshots sharing one `id` with different bodies are not equal and both
/// survived. Rule ids are workspace-unique, so a set holding one identity twice
/// in two versions is not a resolved Rule set at all.
fn checked_snapshots(rules: Vec<BoundRule>, domain: Domain) -> Result<Vec<BoundRule>> {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for rule in &rules {
        rule.validate()?;
        ensure!(
            rule.domains.contains(&domain),
            EXIT_SCHEMA,
            "rule snapshot {}: applies to {}, and this is a {} review; the set is the rules that apply",
            rule.id,
            rule.domains
                .iter()
                .map(|domain| domain.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            domain.as_str()
        );
        ensure!(
            seen.insert(rule.id.as_str()),
            EXIT_SCHEMA,
            "rule snapshot {}: one rule id, two versions; ids are unique in a workspace",
            rule.id
        );
    }
    let mut rules = rules;
    canonical_set(&mut rules, "rule")?;
    Ok(rules)
}
/// Rebuild a binding from Rule snapshots somebody else already resolved.
///
/// The counterpart to [`bind`], which reads the workspace. This one takes what
/// a candidate stored, so a reader can recompute the review identity **from the
/// candidate alone** and find out whether the name it gives itself is the name
/// its own contents produce.
///
/// That is a different question from whether the review is still current, and
/// the difference matters: this catches a candidate edited on disk, while only
/// re-resolving the workspace catches project policy that has moved since. Both
/// are asked, at different moments, and neither substitutes for the other.
pub fn rebind(
    domain: Domain,
    mutation: serde_json::Value,
    precondition: serde_json::Value,
    rules: Vec<BoundRule>,
) -> Result<ReviewBinding> {
    check_domain_shape(domain, &mutation, &precondition)?;
    within_safe_integers(&mutation, "mutation")?;
    within_safe_integers(&precondition, "precondition")?;
    let rules = checked_snapshots(rules, domain)?;
    Ok(ReviewBinding {
        domain,
        mutation,
        precondition,
        rules,
    })
}

/// The smallest ceiling in an applicable set — the one an attempt meets first.
pub fn smallest_ceiling(rules: &[BoundRule]) -> Option<u32> {
    rules.iter().map(|rule| rule.review.max_attempts).min()
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
    // Before anything is resolved or hashed: the two arguments that are
    // arbitrary caller JSON must be the shape their domain freezes, and inside
    // what canonical bytes can carry.
    check_domain_shape(domain, &mutation, &precondition)?;
    within_safe_integers(&mutation, "mutation")?;
    within_safe_integers(&precondition, "precondition")?;
    Ok(ReviewBinding {
        domain,
        mutation,
        precondition,
        rules: bound_rules(root, domain)?,
    })
}

/// The applicable rules with everything they rest on resolved, in hashed order.
///
/// Split out from [`bind`] because two callers need the rules and only one of
/// them has a mutation to describe. Composing an exhaustion verdict reads no
/// more than the rules — but it must read them the *same* way an attestation
/// would, bases resolved and all, or a rule whose material has gone missing
/// would quietly stop counting for one caller while refusing the other.
fn bound_rules(root: &Path, domain: Domain) -> Result<Vec<BoundRule>> {
    let mut bound = Vec::new();
    for rule in applicable(root, domain)? {
        let mut based_on = Vec::new();
        for basis in &rule.based_on {
            based_on.push(basis.resolve(root, &rule.id)?);
        }
        // Every set that reaches the hash goes through the one protocol rule.
        // `Rule::based_on` is kept in path order for reading; the *hashed* order
        // is this one, because the two answer different questions and only one
        // of them is a contract.
        canonical_set(&mut based_on, "basis")?;
        let mut domains = rule.domains;
        canonical_set(&mut domains, "domain")?;
        // The Rule's own provenance, from the same bytes the semantics above
        // were parsed from. Not a second `read_to_string` of the locator:
        // `.engr/rules/` is editable outside the workspace lock, so a second
        // read can return a different artifact, and the binding would then name
        // one file's normative text while claiming the other file's
        // `content_sha256` and commit. Artifact-exact identity means the
        // provenance identifies the exact artifact whose semantics were
        // reviewed. Only the *commit lookup* depends on the file being inside
        // the repository.
        //
        // It hashed `rule.body` when there was no repository path, which
        // dropped the front matter from the artifact identity — two Rule files
        // with one body and different front matter would have shared an
        // identity, and this member is supposed to say which file was read.
        let text = rule.raw;
        let provenance = match rule_relative_path(root, &rule.source) {
            Some(path) => Provenance::of(root, &path, &text),
            // Outside the repository entirely, so no commit can hold it. Not a
            // refusal: a Rule read from outside a Git working tree is still a
            // Rule, and saying "no commit has this" is the true answer rather
            // than an evasion.
            None => Provenance::dirty(&text),
        };
        bound.push(BoundRule {
            id: rule.id,
            domains,
            based_on,
            review: rule.review,
            body: rule.body,
            commit: provenance.commit,
            dirty: provenance.dirty,
            content_sha256: provenance.content_sha256,
        });
    }
    canonical_set(&mut bound, "applicable rule")?;
    Ok(bound)
}

/// What the applicable rules say about one attempt, without a mutation to
/// describe.
///
/// A domain whose mutations are applied directly — Backlog — still has to ask
/// the exhaustion question, and it has no prepared candidate to hang a binding
/// on. The verdict is composed from the same resolved rules either way, so this
/// and [`ReviewBinding::exhaustion`] cannot drift apart.
pub fn exhaustion(root: &Path, domain: Domain, attempt: Attempt) -> Result<Exhaustion> {
    compose(domain, &bound_rules(root, domain)?, attempt)
}

/// Whether any project rule governs this domain — and so whether a review was
/// required at all.
///
/// The question every caller that enforces "a reviewed mutation must carry what
/// it was reviewed against" has to ask first, because with no applicable rule
/// there is no review for a predecessor to anchor.
///
/// For an answer a mutation will act on, use [`assess`] instead: this reads
/// policy on its own, and two separate reads can disagree.
pub fn governs(root: &Path, domain: Domain) -> Result<bool> {
    Ok(!applicable(root, domain)?.is_empty())
}

/// Both questions a mutation asks of policy, from **one** read of it.
///
/// Whether a rule governs at all, and what the attempt means under the rules
/// that do — asked separately, these come from two reads, and nothing stops the
/// rules changing in between. The workspace lock does not help: `.engr/rules/`
/// is edited by people and by `git checkout`, not through engr.
///
/// The dangerous direction is specific. A first read seeing no rule and a second
/// seeing one gives `governed = false` with a verdict composed from the new set;
/// at an in-limit attempt that verdict is `NotReached`, and the mutation then
/// proceeds without the predecessor a governed mutation must carry — one apply
/// acting on two contradictory pictures of policy. One snapshot cannot disagree
/// with itself.
pub fn assess(root: &Path, domain: Domain, attempt: Attempt) -> Result<(bool, Exhaustion)> {
    let rules = bound_rules(root, domain)?;
    let governed = !rules.is_empty();
    Ok((governed, compose(domain, &rules, attempt)?))
}

/// The applicable-Rule boundary for a domain whose mutations apply directly.
///
/// Collection and Work have no prepared candidate to bind, so this is the whole
/// of their Rule obligation, and it is not optional: a `domain: collection` or
/// `domain: work` Rule that exists must be *establishable* before anything is
/// written — every basis readable, every ceiling known — and the caller's
/// attempt must be inside those ceilings. Skipping it let a Rule with missing
/// material, or one already past its limit, sit in the workspace while every
/// mutation proceeded exactly as if it were not there.
///
/// What an exhausted ceiling then *means* for these two domains is deliberately
/// unsettled, so [`compose`] refuses rather than borrowing another domain's
/// answer. That refusal is the fail-closed behaviour, not a gap in this one.
pub fn direct(root: &Path, domain: Domain, attempt: Attempt) -> Result<()> {
    match assess(root, domain, attempt)?.1 {
        Exhaustion::NotReached => Ok(()),
        other => Err(Error::new(
            EXIT_INVARIANT,
            format!(
                "a {} mutation was given the {other:?} verdict, which is not its own",
                domain.as_str()
            ),
        )),
    }
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
    // `bind` first, because it is what enforces the workspace-version boundary:
    // an older workspace must be told to migrate, not handed a complaint about
    // the attestation's spelling. Reading the digest first inverted that and
    // reported the wrong refusal for the more fundamental problem.
    let binding = bind(root, domain, mutation, precondition)?;
    // Then read the attestation through its contract, and recompute **under the
    // version it names** rather than under the current emitter. A malformed
    // scalar and a scalar naming a contract this build cannot verify are
    // different answers, and neither is "the subject moved" — reporting either
    // as a mismatch would tell an agent to re-review something that was never
    // the problem, and recomputing an old proof with today's calculation would
    // do exactly that to every historical attestation the moment a second
    // contract version exists.
    let checked =
        crate::digest::REVIEW.recheck(attested, |version| binding.digest_under(version))?;
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
        checked.agrees(),
        EXIT_INVARIANT,
        "this review was of something else: the mutation, its target, a rule, or a rule's material has changed since it was reviewed. Review the current subject and attest to {}",
        checked.expected
    );
    Ok(())
}
