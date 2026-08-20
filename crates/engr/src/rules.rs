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
use serde::Serialize;
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
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct Basis {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
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
        std::fs::read_to_string(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Error::new(
                    EXIT_NOT_FOUND,
                    format!("rule {rule}: based_on {} does not exist", self.path),
                )
            } else {
                tool_error(path.display(), error)
            }
        })
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
    /// The whole file, byte for byte. This is what the review binding covers:
    /// "the exact canonical Rule definition" is the definition as written, not
    /// a re-serialization of a parse of it, which would let two spellings of
    /// the same policy produce two hashes — or worse, one.
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
/// An empty result means no review is required. That is a real answer, not a
/// gap: a project that has written no Rule for a domain has not asked for one.
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

/// Split the front matter from the body and read the front matter strictly.
///
/// Strictly, and deliberately. The front matter is a machine contract, and #25
/// requires that unknown or newer semantics affecting a Rule's meaning fail
/// closed rather than being ignored — an older implementation that skipped a
/// key it did not recognise would admit data under an incomplete reading of the
/// Rule. So every key, every nesting level and every value is either one this
/// version understands or a refusal naming the line.
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

    let mut id: Option<String> = None;
    let mut domains: Vec<Domain> = Vec::new();
    let mut based_on: Vec<Basis> = Vec::new();
    let mut saw_applies = false;
    let mut saw_domains = false;
    let mut section = Section::None;

    for (index, line) in front.lines().enumerate() {
        let number = index + 2;
        if line.trim().is_empty() {
            continue;
        }
        ensure!(
            !line.starts_with('\t'),
            EXIT_SCHEMA,
            "{where_}:{number}: indent with spaces, not tabs"
        );
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim_end();
        let content = trimmed.trim_start();
        match (indent, section) {
            (0, _) => {
                let (key, value) = split_key(content, &where_, number)?;
                match key {
                    "id" => {
                        ensure!(
                            id.is_none(),
                            EXIT_SCHEMA,
                            "{where_}:{number}: id is given twice"
                        );
                        let value = require_value(value, "id", &where_, number)?;
                        ensure!(
                            !value.is_empty()
                                && value.chars().all(|character| {
                                    character.is_ascii_lowercase()
                                        || character.is_ascii_digit()
                                        || character == '-'
                                }),
                            EXIT_SCHEMA,
                            "{where_}:{number}: rule id {value:?} must be lowercase letters, digits and hyphens"
                        );
                        id = Some(value.to_owned());
                        section = Section::None;
                    }
                    "applies" => {
                        ensure!(
                            value.is_empty(),
                            EXIT_SCHEMA,
                            "{where_}:{number}: applies takes a nested domains list, not a value"
                        );
                        saw_applies = true;
                        section = Section::Applies;
                    }
                    "based_on" => {
                        ensure!(
                            value.is_empty(),
                            EXIT_SCHEMA,
                            "{where_}:{number}: based_on takes a nested list, not a value"
                        );
                        section = Section::BasedOn;
                    }
                    other => {
                        return Err(Error::new(
                            EXIT_SCHEMA,
                            format!(
                                "{where_}:{number}: {other:?} is not a rule field this version understands; it is refused rather than ignored, because ignoring it would review against an incomplete rule"
                            ),
                        ))
                    }
                }
            }
            (2, Section::Applies) => {
                let (key, value) = split_key(content, &where_, number)?;
                ensure!(
                    key == "domains" && value.is_empty(),
                    EXIT_SCHEMA,
                    "{where_}:{number}: applies takes exactly one key, domains, with a nested list"
                );
                saw_domains = true;
                section = Section::Domains;
            }
            (4, Section::Domains) => {
                let value = require_item(content, &where_, number)?;
                let domain = Domain::parse(value).ok_or_else(|| Error::new(
                    EXIT_SCHEMA,
                    format!("{where_}:{number}: {value:?} is not a domain; v1 has object, backlog, collection and work"),
                ))?;
                ensure!(
                    !domains.contains(&domain),
                    EXIT_SCHEMA,
                    "{where_}:{number}: domain {value:?} is listed twice"
                );
                domains.push(domain);
            }
            (2, Section::BasedOn) => {
                let value = require_item(content, &where_, number)?;
                let (key, value) = split_key(value, &where_, number)?;
                ensure!(
                    key == "path",
                    EXIT_SCHEMA,
                    "{where_}:{number}: a based_on entry starts with path"
                );
                let value = require_value(value, "path", &where_, number)?;
                based_on.push(Basis {
                    path: value.to_owned(),
                    commit: None,
                });
            }
            (4, Section::BasedOn) => {
                let (key, value) = split_key(content, &where_, number)?;
                ensure!(
                    key == "commit",
                    EXIT_SCHEMA,
                    "{where_}:{number}: a based_on entry takes path and an optional commit"
                );
                let value = require_value(value, "commit", &where_, number)?;
                let entry = based_on.last_mut().ok_or_else(|| {
                    Error::new(
                        EXIT_SCHEMA,
                        format!("{where_}:{number}: commit belongs to a based_on path"),
                    )
                })?;
                ensure!(
                    entry.commit.is_none(),
                    EXIT_SCHEMA,
                    "{where_}:{number}: this based_on entry already pins a commit"
                );
                entry.commit = Some(value.to_owned());
            }
            _ => {
                return Err(Error::new(
                    EXIT_SCHEMA,
                    format!("{where_}:{number}: this line is indented {indent} spaces, which is not where the rule format puts anything"),
                ))
            }
        }
    }

    let id = id.ok_or_else(|| {
        Error::new(
            EXIT_SCHEMA,
            format!("{where_}: a rule needs an id, which is its stable identity"),
        )
    })?;
    ensure!(
        saw_applies && saw_domains,
        EXIT_SCHEMA,
        "{where_}: rule {id} does not say what it applies to"
    );
    ensure!(
        !domains.is_empty(),
        EXIT_SCHEMA,
        "{where_}: rule {id} lists no domains, so nothing would ever be reviewed against it"
    );
    // Sorted by name, not by the order the enum happens to be declared in. The
    // review hash covers this list, so its order is part of a machine contract
    // and must not move when someone rearranges a Rust enum.
    domains.sort_by_key(|domain| domain.as_str());
    let body = body.trim();
    ensure!(
        !body.is_empty(),
        EXIT_SCHEMA,
        "{where_}: rule {id} has no body, and the body is the rule"
    );
    let mut paths: BTreeSet<&str> = BTreeSet::new();
    for basis in &based_on {
        ensure!(
            paths.insert(basis.path.as_str()),
            EXIT_SCHEMA,
            "{where_}: rule {id} lists based_on {:?} twice",
            basis.path
        );
    }
    Ok(Rule {
        id,
        domains,
        based_on,
        body: body.to_owned(),
        source: path.to_path_buf(),
        raw: raw.to_owned(),
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    None,
    Applies,
    Domains,
    BasedOn,
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

fn split_key<'a>(content: &'a str, where_: &str, number: usize) -> Result<(&'a str, &'a str)> {
    let (key, value) = content.split_once(':').ok_or_else(|| {
        Error::new(
            EXIT_SCHEMA,
            format!("{where_}:{number}: expected `key: value`"),
        )
    })?;
    Ok((key.trim(), value.trim()))
}

fn require_value<'a>(value: &'a str, key: &str, where_: &str, number: usize) -> Result<&'a str> {
    let value = value.trim().trim_matches('"');
    ensure!(
        !value.is_empty(),
        EXIT_SCHEMA,
        "{where_}:{number}: {key} needs a value"
    );
    Ok(value)
}

fn require_item<'a>(content: &'a str, where_: &str, number: usize) -> Result<&'a str> {
    content.strip_prefix("- ").map(str::trim).ok_or_else(|| {
        Error::new(
            EXIT_SCHEMA,
            format!("{where_}:{number}: expected a `- ` list item"),
        )
    })
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
    /// The rule file exactly as written, front matter and body together.
    ///
    /// Not a re-serialization of a parse: hashing the parse would let two
    /// spellings of one policy produce one hash, and a rule edited in a way
    /// this version does not model would go unnoticed by the very value whose
    /// job is to notice.
    pub definition: String,
    pub based_on: Vec<ResolvedBasis>,
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
            definition: rule.raw,
            based_on,
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
    if ids.is_empty() {
        return Ok(());
    }
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
