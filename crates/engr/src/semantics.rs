//! The Phase 3 semantic vocabulary: what an Object *is*, what state it is in,
//! what role a Section plays, what it relates to, and how much literal material
//! it may carry.
//!
//! Every vocabulary here is closed. That is the point of it: a machine-readable
//! value only helps retrieval while every reader agrees what the values are, and
//! an arbitrary-string taxonomy would put engr back where prose already was.
//! Growth is a protocol decision, not a workspace one.

use crate::{ensure, Error, Result, EXIT_INVARIANT, EXIT_SCHEMA};
use serde::{Deserialize, Serialize};

/// What kind of engineering aggregate an Object is.
///
/// Optional by design. An untyped Object is a first-class long-term form, not a
/// waiting room for classification — most confirmed knowledge is just knowledge.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum ObjectType {
    Design,
    Decision,
    Risk,
}

impl ObjectType {
    pub fn as_str(self) -> &'static str {
        match self {
            ObjectType::Design => "design",
            ObjectType::Decision => "decision",
            ObjectType::Risk => "risk",
        }
    }

    /// The states this type admits, in the order they read.
    pub fn states(self) -> &'static [State] {
        match self {
            ObjectType::Design => &[
                State::Draft,
                State::Proposed,
                State::Accepted,
                State::Rejected,
                State::Superseded,
            ],
            ObjectType::Decision => &[
                State::Proposed,
                State::Accepted,
                State::Rejected,
                State::Superseded,
            ],
            ObjectType::Risk => &[
                State::Identified,
                State::Accepted,
                State::Mitigated,
                State::Invalidated,
            ],
        }
    }
}

/// The one persisted lifecycle field.
///
/// One field, not two: the previous model stored `status` beside the semantic
/// standing, and two fields that can disagree are two truths. Which values are
/// legal depends on the Object's type — the vocabularies are deliberately not
/// symmetric, because a risk being `accepted` and a decision being `accepted`
/// are different enough that one shared enum would flatten the distinction.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Open,
    Closed,
    Draft,
    Proposed,
    Accepted,
    Rejected,
    Superseded,
    Identified,
    Mitigated,
    Invalidated,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Open => "open",
            State::Closed => "closed",
            State::Draft => "draft",
            State::Proposed => "proposed",
            State::Accepted => "accepted",
            State::Rejected => "rejected",
            State::Superseded => "superseded",
            State::Identified => "identified",
            State::Mitigated => "mitigated",
            State::Invalidated => "invalidated",
        }
    }
}

/// The states an untyped Object admits.
pub const UNTYPED_STATES: &[State] = &[State::Open, State::Closed];

/// Which states a classification admits.
pub fn states_for(object_type: Option<ObjectType>) -> &'static [State] {
    match object_type {
        None => UNTYPED_STATES,
        Some(object_type) => object_type.states(),
    }
}

fn type_name(object_type: Option<ObjectType>) -> &'static str {
    object_type.map_or("an untyped object", |value| match value {
        ObjectType::Design => "a design",
        ObjectType::Decision => "a decision",
        ObjectType::Risk => "a risk",
    })
}

/// Say what is legal rather than only that something was not, because the
/// vocabularies differ per type and a reader cannot guess which one they hit.
pub fn legal_states(object_type: Option<ObjectType>) -> String {
    states_for(object_type)
        .iter()
        .map(|state| state.as_str())
        .collect::<Vec<_>>()
        .join(" | ")
}

/// The states of this classification that are in the default attention set.
pub fn attention_states(object_type: Option<ObjectType>) -> String {
    states_for(object_type)
        .iter()
        .filter(|state| needs_attention(object_type, **state))
        .map(|state| state.as_str())
        .collect::<Vec<_>>()
        .join(" | ")
}

pub fn state_is_valid(object_type: Option<ObjectType>, state: State) -> bool {
    states_for(object_type).contains(&state)
}

pub fn validate_state(code: i32, object_type: Option<ObjectType>, state: State) -> Result<()> {
    ensure!(
        state_is_valid(object_type, state),
        code,
        "{} cannot be {}; it is one of {}",
        type_name(object_type),
        state.as_str(),
        legal_states(object_type)
    );
    Ok(())
}

/// Whether `(type, state)` belongs in the default attention set.
///
/// Derived, never stored. A persisted attention flag would be a second truth
/// that drifts the moment somebody edits `state`, and the whole reason there is
/// one `state` field is to have no such pair. `open` and `closed` survive only
/// as the untyped vocabulary and as the name of this classification — not as a
/// field.
pub fn needs_attention(object_type: Option<ObjectType>, state: State) -> bool {
    match object_type {
        None => state == State::Open,
        Some(ObjectType::Design) => matches!(state, State::Draft | State::Proposed),
        Some(ObjectType::Decision) => state == State::Proposed,
        Some(ObjectType::Risk) => state == State::Identified,
    }
}

/// Which path admitted a Section's current semantics, and therefore how much
/// authority those semantics carry.
///
/// This is the field that makes mixed authority readable rather than inferred.
/// Durable engineering knowledge now arrives through two doors, and a reader who
/// cannot tell which one a Section came through cannot tell whether a human ever
/// assented to it — so the answer is persisted on the Section itself rather than
/// reconstructed from history, which is evidence and may be purged.
///
/// There is deliberately no `Object.admission`. An Object is an aggregate, and
/// an aggregate of one Human Section and one Agent Section has no single honest
/// answer; asking the question of the Section is the only place it has one.
///
/// The ordering is one-way. A Human-Gated semantic mutation of a surviving Agent
/// Section yields [`Admission::Human`], because those exact words were put
/// through the gate where a human is asked. Nothing demotes Human to Agent:
/// that would be engr deciding an admission it recorded had expired.
///
/// What this field records is **which door**, and only that. `human` says the
/// wording went through the Human Gate; it is not evidence that a human was
/// present, and nothing here can be — see the threat model in `PROTOCOL.md`,
/// which is explicit that nothing stops an agent confirming its own proposal.
/// Every rule below rests on the door, never on the presence.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum Admission {
    /// Admitted through Agent Rule Review. Durable engineering knowledge, and
    /// explicitly **not** Human-authoritative.
    Agent,
    /// Admitted through the Human Gate. Human-authoritative.
    ///
    /// The default, and only while the Agent path has no envelope to be
    /// admitted through. Until the coordinated Phase-3 contract is activated the
    /// Human Gate is the only door there is, so this is what every stored
    /// Section came through — a fact about the current protocol rather than an
    /// assumption about a missing field. See [`crate::model::Section::admission`].
    #[default]
    Human,
}

impl Admission {
    pub fn as_str(self) -> &'static str {
        match self {
            Admission::Agent => "agent",
            Admission::Human => "human",
        }
    }
}

/// What semantic role a confirmed Section plays.
///
/// Optional, and independent of the Object's type: an untyped Object may hold a
/// Section that states a decision without the Object itself being a decision
/// important enough to have its own identity and lifecycle.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Decision,
    Risk,
    Supersession,
    /// A verifiable condition, and only that. Deliberately carries no
    /// passed/failed/pending of its own: whether a criterion currently holds is
    /// evidence, it changes without anyone confirming anything, and putting it
    /// here would make the record assert something no human read.
    AcceptanceCriterion,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Decision => "decision",
            Role::Risk => "risk",
            Role::Supersession => "supersession",
            Role::AcceptanceCriterion => "acceptance_criterion",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[serde(rename_all = "snake_case")]
pub enum RelationType {
    SupersededBy,
    ImplementedBy,
}

impl RelationType {
    pub fn as_str(self) -> &'static str {
        match self {
            RelationType::SupersededBy => "superseded_by",
            RelationType::ImplementedBy => "implemented_by",
        }
    }
}

/// What a relation points at. Structurally the shared embedded target form;
/// `kind` is the structural discriminator and stays distinct from the semantic
/// `type` on both the Object and the relation.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Target {
    Engr {
        #[serde(rename = "ref")]
        reference: String,
    },
    File {
        path: String,
        commit: String,
    },
    Symbol {
        path: String,
        symbol: String,
        commit: String,
    },
}

impl Target {
    pub fn engr(reference: impl Into<String>) -> Self {
        Self::Engr {
            reference: reference.into(),
        }
    }

    /// How the target reads on a screen. Not persisted state.
    pub fn render(&self, short: impl Fn(&str) -> String) -> String {
        match self {
            Target::Engr { reference } => format!("engr:{reference}"),
            Target::File { path, commit } => format!("file   {path} @{}", short(commit)),
            Target::Symbol {
                path,
                symbol,
                commit,
            } => format!("symbol {path} :: {symbol} @{}", short(commit)),
        }
    }
}

/// An admitted typed semantic edge.
///
/// Distinct from `refs[]`, which selects semantic dependencies and drifts only
/// when those selected values move. A relation says what this assertion is
/// related to, and each type defines its own target rules; there is no shared
/// drift behaviour to inherit and none is applied.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[serde(deny_unknown_fields)]
pub struct Relation {
    #[serde(rename = "type")]
    pub relation: RelationType,
    pub target: Target,
}

impl Relation {
    pub fn superseded_by(reference: impl Into<String>) -> Self {
        Self {
            relation: RelationType::SupersededBy,
            target: Target::engr(reference),
        }
    }

    /// The replacement Object this relation names, if it is a supersession.
    pub fn replacement(&self) -> Result<Option<String>> {
        if self.relation != RelationType::SupersededBy {
            return Ok(None);
        }
        let Target::Engr { reference } = &self.target else {
            return Err(Error::new(
                EXIT_SCHEMA,
                "superseded_by must target an engr Object".to_owned(),
            ));
        };
        let canonical = crate::reference::canonical_embedded(
            reference,
            &[crate::reference::ResourceKind::Object],
            "superseded_by",
        )?;
        Ok(Some(
            crate::reference::decode_uuid(canonical.id())?.to_string(),
        ))
    }

    /// Shape only. Whether the target exists, whether the path was in that
    /// commit, and whether the replacement graph stays acyclic are questions for
    /// the gate — they need the workspace and the repository, and this runs
    /// wherever a stored payload is loaded.
    pub fn validate(&self) -> Result<()> {
        match (self.relation, &self.target) {
            (RelationType::SupersededBy, Target::Engr { reference }) => {
                let canonical = crate::reference::canonical_embedded(
                    reference,
                    &[crate::reference::ResourceKind::Object],
                    "superseded_by",
                )?;
                ensure!(
                    canonical.section().is_none(),
                    EXIT_SCHEMA,
                    "superseded_by replaces a whole Object, so it cannot name a section"
                );
                Ok(())
            }
            (RelationType::SupersededBy, _) => Err(Error::new(
                EXIT_SCHEMA,
                "superseded_by must target an engr Object".to_owned(),
            )),
            (RelationType::ImplementedBy, Target::File { path, commit }) => {
                validate_repo_path("an implemented_by target", path)?;
                validate_pinned_commit("an implemented_by target", commit)
            }
            (
                RelationType::ImplementedBy,
                Target::Symbol {
                    path,
                    symbol,
                    commit,
                },
            ) => {
                validate_repo_path("an implemented_by target", path)?;
                validate_pinned_commit("an implemented_by target", commit)?;
                ensure!(
                    !symbol.trim().is_empty() && !symbol.contains('\n'),
                    EXIT_SCHEMA,
                    "an implemented_by symbol target needs a single-line symbol name"
                );
                Ok(())
            }
            (RelationType::ImplementedBy, Target::Engr { .. }) => Err(Error::new(
                EXIT_SCHEMA,
                "implemented_by names a repository artifact, not an engr resource".to_owned(),
            )),
        }
    }

    pub fn render(&self, short: impl Fn(&str) -> String) -> String {
        format!(
            "{} -> {}",
            self.relation.as_str(),
            self.target.render(short)
        )
    }
}

/// A repository path as it is written in a target: relative, forward slashes,
/// normalized. Shared with Backlog subjects, because a path that engr cannot
/// hand to `git` is the same mistake in both places.
pub fn validate_repo_path(what: &str, path: &str) -> Result<()> {
    ensure!(!path.trim().is_empty(), EXIT_SCHEMA, "{what} needs a path");
    ensure!(
        !path.contains('\\') && !path.starts_with('/') && !path.contains(':'),
        EXIT_SCHEMA,
        "{what} path {path:?} must be repository-relative with forward slashes"
    );
    ensure!(
        !path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".."),
        EXIT_SCHEMA,
        "{what} path {path:?} must be a normalized repository path"
    );
    Ok(())
}

pub fn validate_pinned_commit(what: &str, commit: &str) -> Result<()> {
    ensure!(
        crate::model::is_canonical_git_oid(commit),
        EXIT_SCHEMA,
        "{what} must pin a full resolved Git object id"
    );
    Ok(())
}

/// A bounded literal excerpt inside a Section: the code, config or data the
/// assertion needs in order to be precise.
///
/// It is a value of the Section, not a resource. No id, no state, no refs, no
/// relations, no confirmation of its own — changing one is an ordinary revision
/// of the Section that holds it, which is what keeps the Section the single unit
/// of authority, hashing and reference.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Supplement {
    /// `code.<tag>` or `data.<tag>`.
    #[serde(rename = "type")]
    pub content_type: String,
    pub body: String,
}

impl Supplement {
    pub fn new(content_type: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            content_type: content_type.into(),
            body: body.into(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        validate_content_type(&self.content_type)?;
        // Not `trim().is_empty()`: whitespace can be the whole point of a code
        // or data excerpt, and an entry that is only whitespace is still an
        // entry somebody meant to write. Empty is the case that means nothing.
        //
        // Which leaves a whitespace-only body admissible and invisible on a
        // terminal — a presentation problem, answered where presentation is
        // decided rather than by reinterpreting what #14 calls literal content.
        ensure!(
            !self.body.is_empty(),
            EXIT_SCHEMA,
            "content entry {}: a body cannot be empty",
            self.content_type
        );
        Ok(())
    }
}

/// `^(code|data)\.[a-z0-9][a-z0-9-]{0,15}$`
///
/// The prefix is protocol-level and closed: `code.*` is executable-shaped
/// literal material, `data.*` is structured literal material, and there is
/// deliberately no `text` — natural-language assertion already has exactly one
/// home, and a second container for prose is how a Section becomes a blob.
///
/// The tag is not a registry. An unknown but well-formed tag is valid and must
/// survive a round trip untouched; engr does not normalize `yml` to `yaml`,
/// because an alias table is a maintenance surface with no authority behind it.
pub fn validate_content_type(value: &str) -> Result<()> {
    let (prefix, tag) = value.split_once('.').ok_or_else(|| {
        Error::new(
            EXIT_SCHEMA,
            format!("content type {value:?} must be code.<tag> or data.<tag>"),
        )
    })?;
    ensure!(
        prefix == "code" || prefix == "data",
        EXIT_SCHEMA,
        "content type {value:?} must begin with code. or data."
    );
    ensure!(
        (1..=16).contains(&tag.len()),
        EXIT_SCHEMA,
        "content type {value:?}: the tag must be 1 to 16 characters"
    );
    let mut bytes = tag.bytes();
    let first = bytes.next().unwrap_or(b'-');
    ensure!(
        first.is_ascii_lowercase() || first.is_ascii_digit(),
        EXIT_SCHEMA,
        "content type {value:?}: the tag must start with a lowercase letter or digit"
    );
    ensure!(
        bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'),
        EXIT_SCHEMA,
        "content type {value:?}: the tag may only use lowercase letters, digits and -"
    );
    Ok(())
}

/// Sizes are counted in Unicode scalar values, not bytes, because the limit is
/// about how much a human is being asked to read.
pub const TEXT_NORMAL: usize = 1200;
pub const TEXT_HARD: usize = 5000;
pub const ENTRIES_NORMAL: usize = 4;
pub const ENTRIES_HARD: usize = 8;
pub const BODY_NORMAL: usize = 2000;
pub const BODY_HARD: usize = 8000;
pub const BODIES_NORMAL: usize = 4000;
pub const BODIES_HARD: usize = 12000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exceeded {
    pub what: &'static str,
    pub found: usize,
    pub limit: usize,
    pub hard: bool,
}

impl std::fmt::Display for Exceeded {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} is {} against a {} limit of {}",
            self.what,
            self.found,
            if self.hard { "hard" } else { "normal" },
            self.limit
        )
    }
}

fn measure(what: &'static str, found: usize, normal: usize, hard: usize, out: &mut Vec<Exceeded>) {
    if found > hard {
        out.push(Exceeded {
            what,
            found,
            limit: hard,
            hard: true,
        });
    } else if found > normal {
        out.push(Exceeded {
            what,
            found,
            limit: normal,
            hard: false,
        });
    }
}

/// Every threshold this wording and its supplementary entries break.
///
/// Every revision is measured again against its own proposed value. A Section
/// that was once admitted oversize carries no exemption, because the exception
/// was admission-time metadata and was deliberately never persisted.
pub fn exceeded(text: &str, content: &[Supplement]) -> Vec<Exceeded> {
    let mut found = Vec::new();
    measure(
        "section text",
        text.chars().count(),
        TEXT_NORMAL,
        TEXT_HARD,
        &mut found,
    );
    measure(
        "content entry count",
        content.len(),
        ENTRIES_NORMAL,
        ENTRIES_HARD,
        &mut found,
    );
    let longest = content
        .iter()
        .map(|entry| entry.body.chars().count())
        .max()
        .unwrap_or(0);
    measure(
        "longest content body",
        longest,
        BODY_NORMAL,
        BODY_HARD,
        &mut found,
    );
    let total: usize = content.iter().map(|entry| entry.body.chars().count()).sum();
    measure(
        "total content body",
        total,
        BODIES_NORMAL,
        BODIES_HARD,
        &mut found,
    );
    found
}

/// What to do about it, rather than only that it is too big.
///
/// The first refusal exists to make an agent choose a destination, not to make
/// it shorten prose until the number goes down. Splitting one assertion into two
/// halves of the same assertion is the failure mode this text is trying to
/// prevent.
pub const OVERSIZE_ADVICE: &str = "\
An independent engineering point belongs in another Section. Unresolved \
reasoning or an open question belongs in `engr backlog`. Actual implementation \
belongs behind an implemented_by relation rather than copied in. A large log or \
result belongs outside the record, with only the smallest relevant excerpt kept \
here. If this really is one bounded assertion, prepare it again with --oversize \
and the candidate will say so where the human can see it.";

/// The same destinations, and no flag.
///
/// The last sentence of [`OVERSIZE_ADVICE`] must not appear here. An agent that
/// reads one refusal as "add the flag" learns to add it to both, which is
/// exactly what a ceiling with no override cannot afford — so the two refusals
/// have to end differently, not merely begin differently.
pub const HARD_LIMIT_ADVICE: &str = "\
An independent engineering point belongs in another Section. Unresolved \
reasoning or an open question belongs in `engr backlog`. Actual implementation \
belongs behind an implemented_by relation rather than copied in. A large log or \
result belongs outside the record, with only the smallest relevant excerpt kept \
here. There is no flag for this one: the hard ceiling always refuses, and \
--oversize will refuse it again.";

/// Refuse the proposal, or let an explicit retry through.
///
/// The hard ceiling is not a threshold with an override — it always refuses, and
/// says so differently, because an agent that reads one refusal as "add the
/// flag" would otherwise learn to add the flag to both.
pub fn check_size(text: &str, content: &[Supplement], oversize: bool) -> Result<Vec<Exceeded>> {
    let found = exceeded(text, content);
    let breaches = |hard: bool| {
        found
            .iter()
            .filter(|item| item.hard == hard)
            .map(|item| item.to_string())
            .collect::<Vec<_>>()
            .join("; ")
    };
    if found.iter().any(|item| item.hard) {
        return Err(Error::new(
            EXIT_INVARIANT,
            format!(
                "this Section is past what the record will hold at all: {}. {}",
                breaches(true),
                HARD_LIMIT_ADVICE
            ),
        ));
    }
    if !found.is_empty() && !oversize {
        return Err(Error::new(
            EXIT_INVARIANT,
            format!(
                "this Section is larger than one assertion normally needs: {}. {}",
                breaches(false),
                OVERSIZE_ADVICE
            ),
        ));
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_typed_state_has_exactly_one_attention_class() {
        // The tables are the protocol, so pin them here rather than only through
        // whichever CLI surface happens to read them.
        for (object_type, attention) in [
            (None, vec![State::Open]),
            (
                Some(ObjectType::Design),
                vec![State::Draft, State::Proposed],
            ),
            (Some(ObjectType::Decision), vec![State::Proposed]),
            (Some(ObjectType::Risk), vec![State::Identified]),
        ] {
            for state in states_for(object_type) {
                assert_eq!(
                    needs_attention(object_type, *state),
                    attention.contains(state),
                    "{object_type:?} {state:?}"
                );
            }
        }
        assert!(!state_is_valid(None, State::Accepted));
        assert!(!state_is_valid(Some(ObjectType::Risk), State::Superseded));
        assert!(!state_is_valid(Some(ObjectType::Decision), State::Draft));
        assert!(state_is_valid(Some(ObjectType::Design), State::Draft));
    }

    #[test]
    fn the_content_type_grammar_is_the_whole_registry() {
        for valid in [
            "code.rs",
            "data.json",
            "code.c",
            "data.x",
            "code.0",
            "code.a-b-c",
            // exactly sixteen, the widest tag the grammar allows
            "data.aaaaaaaaaaaaaaaa",
        ] {
            validate_content_type(valid).unwrap_or_else(|error| panic!("{valid}: {error}"));
        }
        for invalid in [
            "text.md",
            "code",
            "code.",
            "code.-rs",
            "code.RS",
            "code.r_s",
            // seventeen
            "data.aaaaaaaaaaaaaaaaa",
            "code.rs.extra",
            ".rs",
            "prose.txt",
        ] {
            assert!(
                validate_content_type(invalid).is_err(),
                "{invalid} must not be a content type"
            );
        }
    }
}
