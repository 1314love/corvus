//! JSON-RPC 2.0 handling for the Model Context Protocol.

use std::sync::Arc;

use serde_json::{json, Value};

use corvus_proto::{ErrorBody, ErrorCode};
use corvus_record::{SessionContext, Store};

use crate::backend::{ToolBackend, ToolCall};
use crate::metrics::Metrics;
use crate::tools;

/// MCP revision this server implements.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// Revisions the server can speak when a client asks for an older one.
pub const SUPPORTED_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;

/// Builder for the banner a caller sees at the HTTP root.
pub struct McpServer {
    backend: Arc<dyn ToolBackend>,
    name: String,
    version: String,
    instructions: String,
    health: Arc<dyn Fn() -> Value + Send + Sync>,
    store: Option<Arc<Store>>,
    metrics: Arc<Metrics>,
}

impl McpServer {
    /// Build a server around a tool backend.
    pub fn new(backend: Arc<dyn ToolBackend>) -> Self {
        Self {
            backend,
            name: "corvus".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            instructions: "Corvus drives a real browser through the Corvus extension. Call page_snapshot before browser_click or browser_type and pass a reference from the snapshot as target. Every failure names an error code; retry only when the answer says the failure is retryable.".to_string(),
            health: Arc::new(|| json!({ "status": "ok" })),
            store: None,
            metrics: Arc::new(Metrics::new()),
        }
    }

    /// Share a counter set with the caller, for example the daemon metrics tool.
    pub fn with_metrics(mut self, metrics: Arc<Metrics>) -> Self {
        self.metrics = metrics;
        self
    }

    /// The process-wide tool counters.
    pub fn metrics(&self) -> &Arc<Metrics> {
        &self.metrics
    }

    /// Render Prometheus text using browser and recording counts from health.
    pub fn metrics_text(&self) -> String {
        let health = (self.health)();
        let browsers = health
            .get("browsers")
            .and_then(Value::as_array)
            .map_or(0, |browsers| browsers.len() as u64);
        let recording = health.get("recording");
        let sessions = recording
            .and_then(|recording| recording.get("sessions"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let steps = recording
            .and_then(|recording| recording.get("steps"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        self.metrics.prometheus(browsers, sessions, steps)
    }

    /// Attach the recording store used for session bookkeeping.
    pub fn with_store(mut self, store: Arc<Store>) -> Self {
        self.store = Some(store);
        self
    }

    /// Open a new recorded session and return its context.
    pub fn start_session(&self, transport: &str, agent: Option<&str>) -> Option<SessionContext> {
        let store = self.store.as_ref()?;
        let session_id = format!("s-{}", uuid::Uuid::new_v4());
        let context = SessionContext::new(session_id.clone(), transport, agent.map(str::to_string));
        if let Err(error) = store.start_session(&session_id, transport, agent, None) {
            tracing::warn!(error = %error, "could not start a recording session");
            return None;
        }
        Some(context)
    }

    /// Open the session for an initialize request when the body carries one.
    pub fn initialize_session(&self, raw: &str, transport: &str) -> Option<SessionContext> {
        let value: Value = serde_json::from_str(raw).ok()?;
        let is_initialize =
            |request: &Value| request.get("method").and_then(Value::as_str) == Some("initialize");
        let request = match &value {
            Value::Array(items) => items.iter().find(|item| is_initialize(item))?,
            other if is_initialize(other) => other,
            _ => return None,
        };
        let agent = client_agent(request);
        self.start_session(transport, agent.as_deref())
    }

    /// Context for a known session identifier.
    pub fn session_context(&self, session_id: &str, transport: &str) -> Option<SessionContext> {
        let store = self.store.as_ref()?;
        match store.session_exists(session_id) {
            Ok(true) => Some(SessionContext::new(session_id, transport, None)),
            Ok(false) => None,
            Err(error) => {
                tracing::warn!(error = %error, "could not look up a recording session");
                None
            }
        }
    }

    /// Close one session.
    pub fn finish_session(&self, session_id: &str, status: &str) {
        let Some(store) = self.store.as_ref() else {
            return;
        };
        if let Err(error) = store.finish_session(session_id, status) {
            tracing::warn!(error = %error, "could not close a recording session");
        }
    }

    /// Close every session that is still open.
    pub fn close_all_sessions(&self, status: &str) {
        let Some(store) = self.store.as_ref() else {
            return;
        };
        match store.list_sessions(u32::MAX, None) {
            Ok(sessions) => {
                for session in sessions
                    .into_iter()
                    .filter(|session| session.ended_at.is_none())
                {
                    if let Err(error) = store.finish_session(&session.id, status) {
                        tracing::warn!(error = %error, session = %session.id, "could not close a session");
                    }
                }
            }
            Err(error) => tracing::warn!(error = %error, "could not list sessions to close"),
        }
    }

    /// Replace the JSON document served from the health endpoint.
    pub fn with_health(mut self, health: Arc<dyn Fn() -> Value + Send + Sync>) -> Self {
        self.health = health;
        self
    }

    /// The health document, enriched with identity fields.
    pub fn health(&self) -> Value {
        let mut value = (self.health)();
        if let Some(object) = value.as_object_mut() {
            object.insert("name".to_string(), json!(self.name));
            object.insert("version".to_string(), json!(self.version));
            object.insert("protocol".to_string(), json!(PROTOCOL_VERSION));
        }
        value
    }

    /// Turn a raw JSON-RPC message into a raw answer, if one is due.
    pub async fn handle_message(&self, raw: &str) -> Option<String> {
        self.handle_message_with(raw, None).await
    }

    /// Handle one raw message inside an optional recorded session.
    pub async fn handle_message_with(
        &self,
        raw: &str,
        session: Option<&SessionContext>,
    ) -> Option<String> {
        let value: Value = match serde_json::from_str(raw) {
            Ok(value) => value,
            Err(error) => {
                return Some(encode(&error_response(
                    Value::Null,
                    PARSE_ERROR,
                    format!("parse error: {error}"),
                )))
            }
        };
        match value {
            Value::Array(items) => {
                if items.is_empty() {
                    return Some(encode(&error_response(
                        Value::Null,
                        INVALID_REQUEST,
                        "invalid request: empty batch",
                    )));
                }
                let mut responses = Vec::new();
                for item in items {
                    if let Some(response) = self.handle_value_with(item, session).await {
                        responses.push(response);
                    }
                }
                if responses.is_empty() {
                    None
                } else {
                    Some(encode(&Value::Array(responses)))
                }
            }
            other => self
                .handle_value_with(other, session)
                .await
                .map(|value| encode(&value)),
        }
    }

    /// Handle one decoded JSON-RPC value outside any recorded session.
    pub async fn handle_value(&self, value: Value) -> Option<Value> {
        self.handle_value_with(value, None).await
    }

    async fn handle_value_with(
        &self,
        value: Value,
        session: Option<&SessionContext>,
    ) -> Option<Value> {
        let Some(object) = value.as_object() else {
            return Some(error_response(
                Value::Null,
                INVALID_REQUEST,
                "invalid request: expected a JSON object",
            ));
        };
        let notification = !object.contains_key("id");
        let id = object.get("id").cloned().unwrap_or(Value::Null);
        if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return if notification {
                None
            } else {
                Some(error_response(
                    id,
                    INVALID_REQUEST,
                    "invalid request: jsonrpc must be 2.0",
                ))
            };
        }
        let method = match object.get("method").and_then(Value::as_str) {
            Some(method) => method.to_string(),
            None => {
                return if notification {
                    None
                } else {
                    Some(error_response(
                        id,
                        INVALID_REQUEST,
                        "invalid request: missing method",
                    ))
                }
            }
        };
        let params = object.get("params").cloned().unwrap_or(Value::Null);
        if method == "initialize" {
            if let Some(session) = session {
                self.record_initialize(session, &params);
            }
        }
        match method.as_str() {
            "initialize" => Some(success(id, self.initialize(&params))),
            "notifications/initialized" | "notifications/cancelled" | "notifications/progress" => {
                None
            }
            "ping" => Some(success(id, json!({}))),
            "tools/list" => Some(success(id, json!({ "tools": tools::definitions() }))),
            "tools/call" => Some(success(id, self.call_tool(&params, session).await)),
            other => {
                if notification {
                    None
                } else {
                    Some(error_response(
                        id,
                        METHOD_NOT_FOUND,
                        format!("method not found: {other}"),
                    ))
                }
            }
        }
    }

    fn initialize(&self, params: &Value) -> Value {
        let requested = params
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or(PROTOCOL_VERSION);
        let version = if SUPPORTED_VERSIONS.contains(&requested) {
            requested
        } else {
            PROTOCOL_VERSION
        };
        json!({
            "protocolVersion": version,
            "capabilities": {
                "tools": { "listChanged": false }
            },
            "serverInfo": {
                "name": self.name,
                "version": self.version,
            },
            "instructions": self.instructions,
        })
    }

    fn record_initialize(&self, session: &SessionContext, params: &Value) {
        let Some(store) = self.store.as_ref() else {
            return;
        };
        let agent = params
            .get("clientInfo")
            .and_then(|client| client.get("name"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| "unknown".to_string());
        if let Err(error) =
            store.start_session(&session.session_id, &session.transport, Some(&agent), None)
        {
            tracing::warn!(error = %error, "could not record the client agent");
        }
    }

    async fn call_tool(&self, params: &Value, session: Option<&SessionContext>) -> Value {
        let name = match params.get("name").and_then(Value::as_str) {
            Some(name) if !name.trim().is_empty() => name.to_string(),
            _ => {
                return tool_error(&ErrorBody::new(
                    ErrorCode::BadArgs,
                    "tools/call requires a non-empty tool name",
                ))
            }
        };
        let mut measurement = self.metrics.begin(&name);
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let arguments = match tools::validate(&name, &arguments) {
            Ok(arguments) => arguments,
            Err(error) => {
                measurement.finish(Some(&error));
                return tool_error(&error);
            }
        };
        let call = ToolCall {
            name,
            arguments,
            session: session.cloned(),
        };
        match self.backend.call(call).await {
            Ok(result) => {
                measurement.finish(None);
                tool_success(&result)
            }
            Err(error) => {
                measurement.finish(Some(&error));
                tool_error(&error)
            }
        }
    }
}

fn client_agent(value: &Value) -> Option<String> {
    value
        .get("params")
        .and_then(|params| params.get("clientInfo"))
        .and_then(|client| client.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn encode(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
}

fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message.into() },
    })
}

fn tool_success(result: &Value) -> Value {
    let text = serde_json::to_string_pretty(result).unwrap_or_else(|_| result.to_string());
    let mut answer = json!({
        "content": [{ "type": "text", "text": text }],
        "isError": false,
    });
    if result.is_object() {
        if let Some(object) = answer.as_object_mut() {
            object.insert("structuredContent".to_string(), result.clone());
        }
    }
    answer
}

fn tool_error(error: &ErrorBody) -> Value {
    let mut text = error.label();
    if let Some(trace_id) = &error.trace_id {
        text.push_str(&format!(" (trace {trace_id})"));
    }
    if let Some(details) = &error.details {
        text.push(' ');
        text.push_str(&encode(details));
    }
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": true,
    })
}
