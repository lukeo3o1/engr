//! engr v0 — engineering records whose every word a human confirmed.
//!
//! Objects hold sections. Sections are the current authority; confirmed events
//! are append-only history and audit evidence, projected immediately at confirm
//! time. Git additionally preserves committed projections.

pub mod backlog;
pub mod collection;
pub mod confirmation;
pub mod digest;
pub mod gate;
pub mod git;
pub mod integrity;
pub mod model;
pub mod ops;
pub mod proof;
pub mod reference;
pub mod rules;
pub mod semantics;
pub mod store;
pub mod view;
pub mod work;

/// Schema version of `.engr/format.json`, the workspace-level authority.
///
/// Version 2 is the first to define `review.max_attempts` and
/// `review.on_exhaustion` on a project Rule, including their effective
/// defaults. That is a *semantic* change over unchanged bytes: a rule file with
/// no `review:` block means one thing here and another to a version 1 build, and
/// an explicit block is an unknown field there. The workspace version is what
/// stops the two from silently disagreeing — a build that does not know a
/// version refuses the workspace rather than reading it under its own rules.
///
/// This is the version this build **creates, writes and migrates to**. It is
/// deliberately not [`PHASE_3_WORKSPACE_VERSION`]: see that constant.
pub const WORKSPACE_VERSION: u32 = 2;
/// The workspace version the coordinated Phase-3 transition targets.
///
/// Named here, and named as *not yet durable*. Version 3 is one coordinated
/// contract — mixed Section authority, the admission timestamp, Section and
/// Object integrity, selective Ref digests, the Event and candidate
/// representation changes — and it is being implemented in slices. A version
/// has exactly one canonical interpretation for current resources, so a build
/// partway through must not create, write or migrate to it: a workspace moved
/// into a shape that is still changing could not be brought the rest of the way
/// afterwards, because migration is one way and confirmed history is never
/// rewritten.
///
/// So the intermediate slices implement and test the model, and the durable
/// surface stays at [`WORKSPACE_VERSION`] exactly as it was. Nothing here
/// writes a version 3 resource, nothing reads one, and no workspace anywhere
/// can be dragged into a half-built generation. The final slice of the
/// transition is what makes this the current version.
pub const PHASE_3_WORKSPACE_VERSION: u32 = 3;
/// Older workspace versions this build recognizes and can migrate forward.
///
/// Recognized is not the same as current: a workspace at one of these is read
/// only until `engr migrate` is run explicitly. It is also what makes a
/// *historical* snapshot readable — see [`git::object_at`] — because a commit
/// predating the migration carries the version that was current when it was
/// made, and refusing it would make every reference pinned before the migration
/// unresolvable.
pub const MIGRATABLE_WORKSPACE_VERSIONS: &[u32] = &[1];
/// Version carried by the supported Phase 0 Object envelope.
pub const LEGACY_OBJECT_VERSION_V0: u32 = 1;
/// Version carried by confirmed Event envelopes this build writes and reads.
pub const EVENT_ENVELOPE_VERSION_V0: u32 = 1;
/// The Event envelope generation the coordinated Phase-3 transition targets.
///
/// Version 2 is the mixed-authority generation: a merge names the Section that
/// survives it rather than listing everything it consumed, and admission
/// provenance becomes one tagged structure. Both are different statements about
/// what a record means, so it is a generation rather than an addition.
///
/// Not emitted, for the same reason [`PHASE_3_WORKSPACE_VERSION`] is not
/// written. Its envelope is not finished — the tagged provenance that completes
/// it lands with the rest of the mixed-authority Event work — and a record
/// claiming a generation whose contract it does not meet is a record a later
/// build could only accept by silently redefining that generation. History is
/// never rewritten, so there would be no way back.
pub const PHASE_3_EVENT_ENVELOPE_VERSION: u32 = 2;
/// The candidate envelope this build mints and admits.
///
/// Version 1 stored its binding and its revision-presentation metadata outside
/// any fingerprint. A live candidate is local, uncommitted and short-lived, so
/// the upgrade refuses the old envelope outright rather than reading missing
/// integrity data as if it were protected — the whole point is that what a
/// human was shown is the thing that gets admitted.
pub const CANDIDATE_ENVELOPE_VERSION: u32 = 2;
/// Version carried by candidate envelopes minted before candidate integrity.
pub const CANDIDATE_ENVELOPE_VERSION_V0: u32 = 1;
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
