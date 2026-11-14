//! The seam between the MCP server and whatever executes a tool.

use std::future::Future;
use std::pin::Pin;

use corvus_proto::ErrorBody;
use corvus_record::SessionContext;
use serde_json::Value;

/// Boxed future returned by a backend.
pub type BackendFuture = Pin<Box<dyn Future<Output = Result<Value, ErrorBody>> + Send + 'static>>;

/// One tool call the server validated and wants executed.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    /// Tool name, already checked against the catalogue.
    pub name: String,
    /// Arguments, already checked against the JSON schema.
    pub arguments: Value,
    /// MCP session that produced the call, when the server tracks sessions.
    pub session: Option<SessionContext>,
}

/// Executes tool calls.
pub trait ToolBackend: Send + Sync + 'static {
    /// Run the call. Every failure must be a typed error body.
    fn call(&self, call: ToolCall) -> BackendFuture;
}
