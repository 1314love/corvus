//! Typed worker protocol and supervisor errors.

use thiserror::Error;

/// Failure of a worker frame, handshake or supervised operation.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum WorkerError {
    /// A frame exceeded its configured size limit.
    #[error("worker frame is too large")]
    FrameTooLarge,
    /// A frame or JSON payload was malformed.
    #[error("malformed worker frame: {0}")]
    Malformed(String),
    /// The child transport failed.
    #[error("worker transport error: {0}")]
    Io(String),
    /// The worker handshake was rejected or malformed.
    #[error("worker handshake failed: {0}")]
    Handshake(String),
    /// The handshake or invoke exceeded its deadline.
    #[error("worker operation timed out: {0}")]
    Timeout(String),
    /// The worker process exited while calls were pending.
    #[error("worker provider is lost")]
    ProviderLost,
    /// The worker speaks a different protocol version.
    #[error("unsupported worker protocol version {0}")]
    ProtocolVersion(u16),
    /// The action or arguments are invalid.
    #[error("worker argument is invalid: {0}")]
    BadArgs(String),
    /// The requested action is unavailable.
    #[error("worker capability is unavailable: {0}")]
    CapabilityUnavailable(String),
    /// A requested tab or element does not exist.
    #[error("worker target was not found: {0}")]
    NotFound(String),
    /// The page could not complete the requested navigation.
    #[error("worker navigation failed: {0}")]
    NavigationFailed(String),
    /// The operation failed without a more specific classification.
    #[error("worker operation failed: {0}")]
    Internal(String),
    /// A CDP operation failed.
    #[error("worker CDP error: {0}")]
    Cdp(String),
    /// A typed action failure whose message is already safe to show callers.
    ///
    /// Unlike the other variants this message is sent verbatim, so it never
    /// carries transport details, paths, or internal protocol vocabulary.
    #[error("{message}")]
    Action {
        /// Stable machine code used in results.
        code: &'static str,
        /// Safe human-readable description.
        message: String,
    },
}

impl WorkerError {
    /// Stable machine code used in result errors.
    pub fn code(&self) -> &'static str {
        match self {
            Self::FrameTooLarge => "FRAME_TOO_LARGE",
            Self::Malformed(_) | Self::Handshake(_) | Self::ProtocolVersion(_) => "PROTOCOL_ERROR",
            Self::Io(_) => "TRANSPORT_DOWN",
            Self::Timeout(_) => "TIMEOUT",
            Self::ProviderLost => "PROVIDER_LOST",
            Self::NotFound(_) => "NOT_FOUND",
            Self::NavigationFailed(_) => "NAVIGATION_FAILED",
            Self::Internal(_) => "INTERNAL",
            Self::BadArgs(_) => "BAD_ARGS",
            Self::CapabilityUnavailable(_) => "CAPABILITY_UNAVAILABLE",
            Self::Cdp(_) => "CDP_ERROR",
            Self::Action { code, .. } => code,
        }
    }

    /// Whether a retry could reasonably succeed.
    pub fn retryable(&self) -> bool {
        match self {
            Self::Io(_) | Self::ProviderLost | Self::Timeout(_) => true,
            Self::Action { code, .. } => matches!(*code, "TIMEOUT" | "PROVIDER_LOST"),
            _ => false,
        }
    }
}

impl From<std::io::Error> for WorkerError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

impl From<serde_json::Error> for WorkerError {
    fn from(value: serde_json::Error) -> Self {
        Self::Malformed(value.to_string())
    }
}
