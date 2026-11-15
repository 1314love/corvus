//! Errors produced by the CDP client.

use thiserror::Error;

/// Typed failure of a Chrome DevTools Protocol operation.
#[derive(Debug, Clone, Error)]
pub enum CdpError {
    /// The endpoint was not a loopback WebSocket URL.
    #[error("invalid CDP endpoint: {0}")]
    InvalidEndpoint(String),
    /// The socket failed or closed.
    #[error("CDP transport error: {0}")]
    Transport(String),
    /// The peer sent something this client cannot decode.
    #[error("CDP protocol error: {0}")]
    Protocol(String),
    /// The request did not finish within its deadline.
    #[error("CDP request timed out after {0} ms")]
    Timeout(u64),
    /// No endpoint is connected.
    #[error("the CDP endpoint is not connected")]
    NotConnected,
    /// Chrome answered with a protocol error.
    #[error("CDP error {code}: {message}")]
    Remote {
        /// Numeric protocol error code.
        code: i64,
        /// Human-readable protocol error message.
        message: String,
    },
    /// The target identifier did not resolve.
    #[error("CDP target {0} was not found")]
    TargetNotFound(String),
    /// No JavaScript dialog is currently showing.
    #[error("no dialog is showing")]
    DialogNotFound,
    /// The element exists but cannot receive the requested interaction.
    #[error("element is not interactable: {0}")]
    NotInteractable(String),
    /// A tool argument was missing or malformed.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    /// The operation has no faithful CDP equivalent.
    #[error("unsupported CDP operation: {0}")]
    Unsupported(String),
}

impl CdpError {
    /// Stable code used in tool error details.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidEndpoint(_) => "INVALID_ENDPOINT",
            Self::Transport(_) => "TRANSPORT_DOWN",
            Self::Protocol(_) => "PROTOCOL_ERROR",
            Self::Timeout(_) => "TIMEOUT",
            Self::NotConnected => "CDP_NOT_CONNECTED",
            Self::Remote { .. } => "CDP_ERROR",
            Self::TargetNotFound(_) => "NOT_FOUND",
            Self::DialogNotFound => "NOT_FOUND",
            Self::NotInteractable(_) => "NOT_INTERACTABLE",
            Self::InvalidArgument(_) => "BAD_ARGS",
            Self::Unsupported(_) => "CAPABILITY_UNAVAILABLE",
        }
    }

    /// Whether a retry could reasonably succeed.
    pub fn retryable(&self) -> bool {
        matches!(self, Self::Transport(_) | Self::NotConnected)
    }
}
