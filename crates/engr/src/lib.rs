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
/// Version 3 activates the coordinated mixed-authority contract: explicit
/// Section admission and timestamps, resource seals, selective references,
/// and Event generation 2.
pub const WORKSPACE_VERSION: u32 = 3;
/// Compatibility name used by the Phase-3 contract tests.
pub const PHASE_3_WORKSPACE_VERSION: u32 = WORKSPACE_VERSION;
/// Older workspace versions this build can migrate directly into v3.
///
/// Version 1 is in this set because it is the *released* generation: the
/// published `latest` release is commit `e7d9f99`, and that build writes
/// `.engr/format.json` version 1. So a version 1 workspace is not a curiosity
/// of development history — it is what every workspace the released tool
/// created says it is, and leaving it out made the record a person built with
/// the shipped binary unreachable from the shipped binary's own successor.
///
/// Both entries are *direct* migrations rather than a chain. There is
/// deliberately no v1 -> v2 step in this process: the migrator decodes the
/// generation the workspace declares, validates it under that generation's own
/// rules, and derives v3 from that one validated predecessor. Nothing is ever
/// written in an intermediate generation's spelling, which is what stops a
/// historical serializer from becoming part of the permanent contract.
///
/// What makes one pipeline able to serve both is that the v2 persisted shape is
/// a strict *superset* of the v1 one: everything v2 added — `type` on an
/// Object, `role`, `content` and `relations` on a Section, `becomes` on a
/// payload, and the `object_classified` and `object_superseded` actions — is
/// optional and absent from a v1 file. So each generation decodes under its own
/// enumerated schema and arrives at the same internal representation.
///
/// Superset is not "the same", and the difference is the whole safety
/// argument. Those members reached the v2 window part-way through, they carry
/// real v3 semantics, and the in-memory model defaults every one of them — so a
/// v1 file carrying `object_classified` decoded cleanly, reconstructed a
/// classified Object, and published a classification no human ever made. What
/// stops that is `store::check_predecessor_object_shape` and
/// `store::check_predecessor_event_shape` enumerating each generation's exact
/// members ahead of any decoding. The v1 -> v2 semantic step — how a Rule's
/// `review:` block is read — is separate again, in
/// `migration::check_predecessor_rules`.
pub const MIGRATABLE_WORKSPACE_VERSIONS: &[u32] = &[1, 2];
/// Older versions whose historical Object representation this build can read.
///
/// A snapshot is governed by the authority that wrote it, and is decoded under
/// that version's own persisted schema — which is also why one migration
/// pipeline can take either of them forward.
pub const HISTORICALLY_RECOGNIZED_WORKSPACE_VERSIONS: &[u32] = &[1, 2];
/// Version carried by the supported Phase 0 Object envelope.
pub const LEGACY_OBJECT_VERSION_V0: u32 = 1;
/// Version carried by confirmed Event envelopes this build writes and reads.
pub const EVENT_ENVELOPE_VERSION_V0: u32 = 1;
/// Event envelope generation this build emits.
///
/// Version 2 is the mixed-authority generation: a merge names the Section that
/// survives it rather than listing everything it consumed, and admission
/// provenance becomes one tagged structure. Both are different statements about
/// what a record means, so it is a generation rather than an addition.
///
pub const EVENT_ENVELOPE_VERSION: u32 = 2;
pub const PHASE_3_EVENT_ENVELOPE_VERSION: u32 = EVENT_ENVELOPE_VERSION;
/// The candidate envelope this build mints and admits.
///
/// Version 1 stored its binding and its revision-presentation metadata outside
/// any fingerprint. A live candidate is local, uncommitted and short-lived, so
/// the upgrade refuses the old envelope outright rather than reading missing
/// integrity data as if it were protected — the whole point is that what a
/// human was shown is the thing that gets admitted.
pub const CANDIDATE_ENVELOPE_VERSION: u32 = 3;
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
