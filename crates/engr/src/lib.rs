//! engr v0 — engineering records with explicit Human or reviewed Agent authority.
//!
//! Objects hold sections. Sections are the current authority; confirmed events
//! are append-only history and audit evidence, projected immediately at confirm
//! time. Git additionally preserves committed projections.

pub mod backlog;
pub mod collection;
pub mod confirmation;
pub mod dependency;
pub mod digest;
pub mod gate;
pub mod git;
pub mod integrity;
pub mod migration;
pub mod model;
pub mod ops;
pub mod predecessor;
pub mod proof;
pub mod reference;
pub mod rules;
pub mod semantics;
pub mod store;
pub mod view;
pub mod work;

/// The compatibility generation of the redesigned workspace, held in
/// `.engr/VERSION`.
///
/// One, and deliberately unrelated to the number the predecessor
/// `.engr/format.json` carried. That sequence counted schema revisions of a
/// design this one replaces, so reusing it would make "version 1" name two
/// unrelated layouts — and a generation marker exists precisely so a build
/// refuses what it cannot read instead of reading it under its own rules.
pub const WORKSPACE_GENERATION: u32 = 1;

/// The exact bytes `.engr/VERSION` carries.
///
/// One spelling, terminated the way every other text file in this repository
/// is. A marker with two accepted encodings is a marker two implementations can
/// disagree about while each believes it agrees.
pub const WORKSPACE_VERSION_FILE: &str = "1\n";

/// The one predecessor generation this build migrates from.
///
/// Not a set and not a chain. The supported source is the officially released
/// `latest` workspace, which bootstraps
/// `{"format":"engr-workspace","version":1}`. Every other shape that ever said
/// version 1, 2 or 3 did so inside an unreleased development window, so nobody
/// holds one who did not build it themselves — and defining a route for those
/// would freeze a serializer that was never shipped into the permanent
/// contract.
pub const PREDECESSOR_WORKSPACE_VERSION: u32 = 1;

/// The release whose workspace this build migrates.
///
/// Recorded because "version 1" alone is ambiguous: `format.json` still said 1
/// through a long unreleased window whose later builds also wrote `rules/`,
/// `backlog/`, `work/` and `collections/`. The published release is the one
/// thing that says which version 1 is meant, and
/// `migration::check_released_domains` refuses a declared v1 workspace holding
/// a domain that release never had.
pub const PREDECESSOR_RELEASE_COMMIT: &str = "e7d9f99733407a8c31cec33af18a92480f4f4c6f";

/// The predecessor bootstrap value, in the spelling that release wrote.
pub const PREDECESSOR_WORKSPACE_FORMAT: &str = "engr-workspace";

/// There is no version number. One moving release tag, `latest`, and the commit
/// the binary was built from — see `build.rs` for where that comes from and what
/// `-dirty` means.
pub const IMPLEMENTATION_VERSION: &str = concat!("latest (", env!("ENGR_COMMIT"), ")");

/// The protocol this build implements, compiled in.
///
/// It is normative, and it is installed from a release archive that carries no
/// checkout — so without this the one document that says what the tool
/// guarantees is not on the machine the tool is on, and the copy people would
/// reach for describes whatever `main` says today rather than this binary.
/// Reading it against a build stamped with its own commit is what makes "where
/// this document and the implementation disagree" a question anyone can settle.
///
/// `include_str!` also makes it a build dependency: move or delete the file and
/// the compile fails, which is a harder guarantee than any check that has to be
/// remembered.
pub const PROTOCOL: &str = include_str!("../../../protocol/PROTOCOL.md");

/// What one confirmation admitted.
///
/// A human types `CONFIRM <code>` and nothing else, so the response cannot say
/// which family of question it answers. The Challenge does — `subject.type` — so
/// the dispatch reads the file rather than asking the person to remember.
#[derive(Debug)]
pub enum Confirmed {
    Object(Box<gate::Admitted>),
    Migration(migration::Report),
}

/// Admit whatever `response` answers.
///
/// Deliberately not behind `store::require_current`: a migration is confirmed
/// while the workspace is still the predecessor, which is the one confirmation
/// that could not exist if this boundary refused it.
pub fn confirm(root: &std::path::Path, response: &str) -> Result<Confirmed> {
    store::with_lock(root, || {
        let code = confirmation::authorize(
            response,
            // On the same three-way terms, and both answers that are not "it is
            // there" mean the same thing here: do not discard. A qualified
            // response is refused as a usage error either way, so failing closed
            // costs nothing and never removes a question on the strength of a
            // probe that followed a link.
            |code| {
                store::challenge_path(root, code)
                    .and_then(|path| store::resource_present(&path))
                    .unwrap_or(false)
            },
            |code| discard_challenge(root, code),
        )?;
        let challenge = store::load_challenge(root, code)?;
        match challenge.subject.kind {
            confirmation::SubjectType::Object => gate::confirm_locked(root, response)
                .map(|admitted| Confirmed::Object(Box::new(admitted))),
            confirmation::SubjectType::Migration => {
                migration::apply(root, &challenge).map(Confirmed::Migration)
            }
        }
    })
}

/// Withdraw the Challenge a qualified response declined to give assent to.
///
/// Dispatched by family for the same reason confirmation is. Disposal is not a
/// neutral file deletion: an Object Challenge is answered in a current
/// workspace and nothing but the file is its, while a migration Challenge is
/// answered while the workspace is still the predecessor and has a local plan
/// standing behind it. Routing both through the Object family's disposal meant
/// a qualified migration response hit `require_current` on a workspace that is
/// by definition not current, and the withdrawal silently did not happen — so
/// the code the human had just declined to give stayed live.
fn discard_challenge(root: &std::path::Path, code: &str) -> Result<()> {
    match store::load_challenge(root, code) {
        Ok(challenge) => match challenge.subject.kind {
            confirmation::SubjectType::Object => gate::discard_locked(root, code),
            confirmation::SubjectType::Migration => migration::discard_locked(root, code),
        },
        // A question this build cannot read is still a question somebody was
        // shown, and withdrawing it must not require reading it. Refusing here
        // would make the least usable Challenge the only one that cannot be
        // taken back — including a Challenge minted by a generator whose
        // contract has since changed, which the protocol says to prepare again
        // rather than interpret.
        Err(error) if error.code == EXIT_SCHEMA => migration::retire_prepared(root, code),
        Err(error) => Err(error),
    }
}

/// Invalid command line, or a confirmation response that did not match.
pub const EXIT_USAGE: i32 = 2;
/// Object, section, or candidate not found.
pub const EXIT_NOT_FOUND: i32 = 3;
/// Malformed or unsupported stored data.
pub const EXIT_SCHEMA: i32 = 4;
/// A rule of the model was violated.
pub const EXIT_INVARIANT: i32 = 5;
/// The object moved after the candidate was prepared.
pub const EXIT_STALE: i32 = 6;
/// Filesystem, locking, or external tooling failure.
pub const EXIT_TOOL: i32 = 8;

#[derive(Debug)]
pub struct Error {
    pub code: i32,
    pub message: String,
}

impl Error {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Fail with `code` and a formatted message unless `condition` holds.
#[macro_export]
macro_rules! ensure {
    ($condition:expr, $code:expr, $($arg:tt)*) => {
        if !$condition {
            return Err($crate::Error::new($code, format!($($arg)*)));
        }
    };
}

pub fn tool_error(context: impl std::fmt::Display, error: impl std::fmt::Display) -> Error {
    Error::new(EXIT_TOOL, format!("{context}: {error}"))
}
