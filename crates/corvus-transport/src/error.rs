//! Errors raised by the transport layer itself.

use std::fmt;

use corvus_proto::{ErrorBody, ErrorCode};

/// A message could not be handed to a browser connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    /// The connection is closed or the browser disconnected.
    Disconnected { browser: String },
}

impl SendError {
    /// Build the error returned when a connection is gone.
    pub fn disconnected(browser: impl Into<String>) -> Self {
        SendError::Disconnected {
            browser: browser.into(),
        }
    }

    /// The protocol taxonomy entry this transport failure maps to.
    pub fn error_body(&self) -> ErrorBody {
        match self {
            SendError::Disconnected { browser } => ErrorBody::new(
                ErrorCode::TransportDown,
                format!("browser {browser} is not connected"),
            ),
        }
    }
}

impl fmt::Display for SendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SendError::Disconnected { browser } => {
                write!(formatter, "browser {browser} is not connected")
            }
        }
    }
}

impl std::error::Error for SendError {}
