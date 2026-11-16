//! Typed provider channel errors and close codes.

use thiserror::Error;

/// Failure of the provider handshake or encrypted session.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProviderError {
    /// The pairing token, transcript proof or ciphertext did not authenticate.
    #[error("provider authentication failed")]
    Authentication,
    /// A frame sequence number was replayed, skipped or exhausted.
    #[error("provider frame sequence is invalid")]
    Sequence,
    /// The peer speaks a different provider protocol version.
    #[error("unsupported provider protocol version {0}")]
    Version(u16),
    /// A frame or message was malformed.
    #[error("malformed provider frame: {0}")]
    Malformed(String),
    /// The handshake did not finish before its deadline.
    #[error("provider handshake timed out")]
    HandshakeTimeout,
    /// A frame exceeded the protocol size limit.
    #[error("provider frame is too large")]
    TooLarge,
    /// The transport failed.
    #[error("provider transport error: {0}")]
    Io(String),
    /// A valid frame carried an unexpected message.
    #[error("provider message error: {0}")]
    Message(String),
    /// The requested capability is not advertised by the provider.
    #[error("provider capability is unavailable: {0}")]
    CapabilityUnavailable(String),
    /// The requested action or argument is invalid.
    #[error("provider argument is invalid: {0}")]
    BadArgs(String),
}

impl ProviderError {
    /// WebSocket close code to send before dropping the connection.
    pub fn close_code(&self) -> u16 {
        match self {
            Self::Authentication => 4401,
            Self::Sequence => 4402,
            Self::Version(_) => 4403,
            Self::Malformed(_)
            | Self::Message(_)
            | Self::BadArgs(_)
            | Self::CapabilityUnavailable(_) => 4404,
            Self::HandshakeTimeout => 4408,
            Self::TooLarge => 1013,
            Self::Io(_) => 1011,
        }
    }

    /// Stable machine code used in inner result messages.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Authentication => "AUTHENTICATION_FAILED",
            Self::Sequence => "SEQUENCE_ERROR",
            Self::Version(_) => "PROTOCOL_VERSION",
            Self::Malformed(_) | Self::Message(_) => "PROTOCOL_ERROR",
            Self::HandshakeTimeout => "TIMEOUT",
            Self::TooLarge => "FRAME_TOO_LARGE",
            Self::Io(_) => "TRANSPORT_DOWN",
            Self::CapabilityUnavailable(_) => "CAPABILITY_UNAVAILABLE",
            Self::BadArgs(_) => "BAD_ARGS",
        }
    }

    /// Whether the peer may retry after this error.
    pub fn retryable(&self) -> bool {
        matches!(self, Self::Io(_) | Self::HandshakeTimeout)
    }
}

impl From<std::io::Error> for ProviderError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

impl From<serde_json::Error> for ProviderError {
    fn from(value: serde_json::Error) -> Self {
        Self::Malformed(value.to_string())
    }
}
