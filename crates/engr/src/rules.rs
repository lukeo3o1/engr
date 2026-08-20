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
        let path = safe_join(root, &self.path).ok_or_else(|| {
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
        let inside =
            std::fs::canonicalize(root).map_err(|error| tool_error(root.display(), error))?;
        ensure!(
            resolved.starts_with(&inside),
            EXIT_SCHEMA,
            "rule {rule}: based_on {} resolves outside the project, so it is not project material anyone here can review",
            self.path
        );
        std::fs::read_to_string(&resolved).map_err(|error| tool_error(resolved.display(), error))
    }
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
    let dir = dir(root);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|error| tool_error(dir.display(), error))? {
        let entry = entry.map_err(|error| tool_error(dir.display(), error))?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("md") {
            files.push(path);
        }
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

pub fn load(path: &Path) -> Result<Rule> {
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
