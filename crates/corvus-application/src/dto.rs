//! Transport-neutral data transfer objects.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A registered browser summary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrowserSummary {
    /// Browser id.
    pub id: String,
    /// Extension profile string.
    pub profile: String,
    /// Negotiated browser family, when known.
    pub browser_family: Option<String>,
    /// Extension or browser version.
    pub version: String,
    /// Capabilities advertised by the connection.
    pub capabilities: Vec<String>,
    /// Whether CDP is available for this browser.
    pub cdp_available: bool,
}

/// A user account read by the login use case.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserAccount {
    /// User id.
    pub id: String,
    /// Login name.
    pub username: String,
    /// Argon2id hash.
    pub password_hash: String,
    /// Role name.
    pub role: String,
    /// Whether the account is disabled.
    pub disabled: bool,
}

/// Public user representation returned to clients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserView {
    /// User id.
    pub id: String,
    /// Login name.
    pub username: String,
    /// Role name.
    pub role: String,
}

/// Login request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoginRequest {
    /// Login name.
    pub username: String,
    /// Plaintext password.
    pub password: String,
}

/// Refresh request carrying an opaque refresh token.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefreshRequest {
    /// Opaque refresh token.
    pub refresh_token: String,
}

/// Access and refresh token pair.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenPair {
    /// Signed JWT access token.
    pub access_token: String,
    /// Opaque rotating refresh token.
    pub refresh_token: String,
    /// Access-token lifetime in seconds.
    pub expires_in: u64,
    /// Token type, always Bearer.
    pub token_type: String,
}

/// Generic tool-call request for the REST surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallRequest {
    /// Tool arguments.
    #[serde(default)]
    pub arguments: Value,
    /// Optional MCP-style session context.
    #[serde(default)]
    pub session_id: Option<String>,
}

/// Generic tool-call response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallResponse {
    /// Whether the tool succeeded.
    pub ok: bool,
    /// Tool data when successful.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    /// Typed error when unsuccessful.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
}
