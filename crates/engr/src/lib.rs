//! engr v0 — engineering records whose every word a human confirmed.
//!
//! Objects hold sections. Sections are the authority; the event log is a buffer
//! that is projected into them at confirm time and may then be purged. History
//! is delegated to git.

pub mod gate;
pub mod git;
pub mod model;
pub mod ops;
pub mod store;
pub mod view;

pub const FORMAT_VERSION: u32 = 1;
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
