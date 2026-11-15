//! The typed error taxonomy from the architecture document, section 8.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// Every failure the daemon can report, on the wire or through MCP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    /// The caller broke the schema of the tool or of the envelope.
    BadArgs,
    /// Selector, page or tab does not exist.
    NotFound,
    /// Element is covered, disabled or off screen.
    NotInteractable,
    /// The command deadline expired.
    Timeout,
    /// The page navigated while the command was running.
    NavigationLost,
    /// A capability the tool needs is not available on this browser.
    CapabilityUnavailable,
    /// The tab or the browser is gone.
    BrowserGone,
    /// The extension is not connected.
    TransportDown,
    /// A defect in the daemon or in the extension.
    Internal,
    /// The command was cancelled before the browser answered.
    Cancelled,
    /// A guardrails policy denied the tool call or navigation.
    PolicyDenied,
}

impl ErrorCode {
    /// Every code, in the order the documentation lists them.
    pub const ALL: [ErrorCode; 11] = [
        ErrorCode::BadArgs,
        ErrorCode::NotFound,
        ErrorCode::NotInteractable,
        ErrorCode::Timeout,
        ErrorCode::NavigationLost,
        ErrorCode::CapabilityUnavailable,
        ErrorCode::BrowserGone,
        ErrorCode::TransportDown,
        ErrorCode::Internal,
        ErrorCode::Cancelled,
        ErrorCode::PolicyDenied,
    ];

    /// The wire spelling of this code.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::BadArgs => "BAD_ARGS",
            ErrorCode::NotFound => "NOT_FOUND",
            ErrorCode::NotInteractable => "NOT_INTERACTABLE",
            ErrorCode::Timeout => "TIMEOUT",
            ErrorCode::NavigationLost => "NAVIGATION_LOST",
            ErrorCode::CapabilityUnavailable => "CAPABILITY_UNAVAILABLE",
            ErrorCode::BrowserGone => "BROWSER_GONE",
            ErrorCode::TransportDown => "TRANSPORT_DOWN",
            ErrorCode::Internal => "INTERNAL",
            ErrorCode::Cancelled => "CANCELLED",
            ErrorCode::PolicyDenied => "POLICY_DENIED",
        }
    }

    /// Whether retrying the same command can succeed without user action.
    pub fn retryable(self) -> bool {
        match self {
            ErrorCode::BadArgs | ErrorCode::CapabilityUnavailable | ErrorCode::BrowserGone => false,
            ErrorCode::Internal | ErrorCode::Cancelled | ErrorCode::PolicyDenied => false,
            ErrorCode::NotFound
            | ErrorCode::NotInteractable
            | ErrorCode::Timeout
            | ErrorCode::NavigationLost
            | ErrorCode::TransportDown => true,
        }
    }

    /// Parse a wire spelling back into a code.
    pub fn parse(name: &str) -> Option<Self> {
        ErrorCode::ALL
            .iter()
            .copied()
            .find(|code| code.as_str() == name)
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A typed failure carried by a response envelope or by an MCP tool result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    /// The taxonomy entry.
    pub code: ErrorCode,
    /// Human readable detail. Never a substitute for the code.
    pub message: String,
    /// Whether the caller may retry without changing the request.
    #[serde(default)]
    pub retryable: bool,
    /// Trace identifier to quote when reporting a defect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    /// Optional structured detail for clients that want more than a string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl ErrorBody {
    /// Build an error body whose retry flag matches the code.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: code.retryable(),
            trace_id: None,
            details: None,
        }
    }

    /// Attach a trace identifier.
    pub fn with_trace(mut self, trace_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
        self
    }

    /// Attach structured detail.
    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    /// One line form used in MCP tool results and logs.
    pub fn label(&self) -> String {
        format!("{}: {}", self.code, self.message)
    }
}

impl fmt::Display for ErrorBody {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.label())
    }
}

impl std::error::Error for ErrorBody {}
