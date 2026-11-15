//! Domain errors that are independent of any transport or storage adapter.

use thiserror::Error;

/// A violated domain rule.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PolicyError {
    /// A numeric or collection value fell outside a policy boundary.
    #[error("{field} must be between {min} and {max}, got {actual}")]
    OutOfRange {
        /// Name of the offending field.
        field: &'static str,
        /// Inclusive lower bound.
        min: i64,
        /// Inclusive upper bound.
        max: i64,
        /// Value supplied by the caller.
        actual: i64,
    },
    /// A collection is empty where at least one item is required.
    #[error("{field} must not be empty")]
    Empty {
        /// Name of the offending field.
        field: &'static str,
    },
    /// A batch would contain too many steps.
    #[error("browser_batch accepts at most {max} steps, got {actual}")]
    TooManySteps {
        /// Maximum number of steps.
        max: usize,
        /// Requested number of steps.
        actual: usize,
    },
    /// A tool is not permitted inside a batch.
    #[error("tool {tool} is not allowed in browser_batch")]
    ToolNotAllowed {
        /// Rejected tool name.
        tool: String,
    },
    /// Authentication is required but not configured for a non-loopback bind.
    #[error("authentication must be enabled when binding to {bind}")]
    InsecureBinding {
        /// Bind address that would be exposed without authentication.
        bind: String,
    },
}
