//! Engr implements the Engineering Record protocol-v1 contract.  The public modules are kept
//! deliberately small: protocol validation, deterministic reduction, storage, and rendering.

pub mod protocol;
pub mod state;
pub mod store;
pub mod view;

use std::fmt::{Display, Formatter};

pub const IMPLEMENTATION: &str = "rust";
pub const IMPLEMENTATION_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const PROTOCOL_VERSION: i64 = 1;
pub const EVENT_SCHEMA_VERSION: i64 = 1;
pub const STATE_SCHEMA_VERSION: i64 = 1;

pub const EXIT_USAGE: i32 = 2;
pub const EXIT_NOT_FOUND: i32 = 3;
pub const EXIT_SCHEMA: i32 = 4;
pub const EXIT_INVARIANT: i32 = 5;
pub const EXIT_FORK: i32 = 6;
pub const EXIT_VERSION: i32 = 7;
pub const EXIT_TOOL: i32 = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngrError {
    pub code: i32,
    pub message: String,
}

impl EngrError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl Display for EngrError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(f)
    }
}

impl std::error::Error for EngrError {}

pub type Result<T> = std::result::Result<T, EngrError>;

#[macro_export]
macro_rules! require {
    ($condition:expr, $code:expr, $($message:tt)*) => {
        if !($condition) {
            return Err($crate::EngrError::new($code, format!($($message)*)));
        }
    };
}
