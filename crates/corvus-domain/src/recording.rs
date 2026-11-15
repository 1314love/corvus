//! Persistence-neutral recording read models and commands.
//!
//! These types describe what the recording adapter stores. They contain no
//! database handle and no driver types, so application and presentation code
//! can depend on them directly.

use serde_json::Value;

/// Largest JSON payload kept verbatim in the database.
pub const MAX_PAYLOAD_BYTES: usize = 262_144;
/// Sessions older than this are pruned on startup.
pub const RETENTION_MS: i64 = 30 * 24 * 60 * 60 * 1_000;

/// The MCP session that produced a tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionContext {
    /// Session identifier.
    pub session_id: String,
    /// Transport that carries the session, for example "http" or "stdio".
    pub transport: String,
    /// Agent that opened the session, when the client named one.
    pub agent: Option<String>,
}

impl SessionContext {
    /// Build a context.
    pub fn new(
        session_id: impl Into<String>,
        transport: impl Into<String>,
        agent: Option<String>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            transport: transport.into(),
            agent,
        }
    }
}

/// One recorded session row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    /// Session identifier.
    pub id: String,
    /// Start time in milliseconds since the Unix epoch.
    pub started_at: i64,
    /// End time in milliseconds since the Unix epoch.
    pub ended_at: Option<i64>,
    /// Browser id that served the session.
    pub browser: Option<String>,
    /// Browser profile label.
    pub profile: Option<String>,
    /// Agent that opened the session.
    pub agent: Option<String>,
    /// Transport name, for example "http" or "stdio".
    pub transport: String,
    /// Lifecycle state.
    pub status: String,
    /// Number of recorded steps.
    pub steps: i64,
    /// Number of failed steps.
    pub errors: i64,
}

/// One recorded step row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepRow {
    /// Row id.
    pub id: i64,
    /// Session that owns the step.
    pub session_id: String,
    /// Monotonic position inside the session.
    pub seq: i64,
    /// Time the step finished, in milliseconds since the Unix epoch.
    pub ts: i64,
    /// Tool name.
    pub tool: String,
    /// Stored arguments JSON, possibly truncated.
    pub args_json: Option<String>,
    /// Stored result JSON, possibly truncated.
    pub result_json: Option<String>,
    /// Error code when the step failed.
    pub error_code: Option<String>,
    /// Duration in milliseconds.
    pub duration_ms: i64,
    /// Browser that served the call.
    pub browser_id: Option<String>,
    /// Tab named by the call.
    pub tab_id: Option<String>,
    /// Whether the call succeeded.
    pub ok: bool,
    /// Whether the arguments were truncated before storage.
    pub args_truncated: bool,
    /// Whether the result was truncated before storage.
    pub result_truncated: bool,
}

/// A step waiting to be written.
#[derive(Debug, Clone, PartialEq)]
pub struct StepRecord {
    /// Session the step belongs to.
    pub session_id: String,
    /// Time the call finished, in milliseconds since the Unix epoch.
    pub ts: i64,
    /// Tool name.
    pub tool: String,
    /// Arguments passed to the tool.
    pub args: Value,
    /// Tool result, absent for failures.
    pub result: Option<Value>,
    /// Error code for a failed call.
    pub error_code: Option<String>,
    /// Duration in milliseconds.
    pub duration_ms: i64,
    /// Browser resolved by routing.
    pub browser_id: Option<String>,
    /// Tab named by the call.
    pub tab_id: Option<String>,
    /// Whether the call succeeded.
    pub ok: bool,
}

/// Current time as milliseconds since the Unix epoch.
pub fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
