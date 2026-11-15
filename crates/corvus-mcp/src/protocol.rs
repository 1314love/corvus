//! JSON-RPC 2.0 handling for the Model Context Protocol.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{json, Value};
use tokio::sync::Notify;

use corvus_proto::{ErrorBody, ErrorCode};
use corvus_record::{now_millis, SessionContext, StepRecord, Store};

use crate::backend::{ToolBackend, ToolCall};
use crate::metrics::{ChannelMetrics, Metrics};
use crate::tools;

/// MCP revision this server implements.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// Revisions the server can speak when a client asks for an older one.
pub const SUPPORTED_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
/// JSON-RPC code returned when a client cancels its own request.
const REQUEST_CANCELLED: i64 = -32800;
/// Separates the MCP session from the request id inside a cancellation key.
const CANCEL_SEPARATOR: char = '\u{1f}';

#[derive(Default)]
struct RequestCancelState {
    cancelled: AtomicBool,
    notify: Notify,
}

/// Cheap handle a caller can wait on and another task can trip.
#[derive(Clone, Default)]
struct RequestCancel {
    state: Arc<RequestCancelState>,
}

impl RequestCancel {
    fn cancel(&self) {
        if !self.state.cancelled.swap(true, Ordering::SeqCst) {
            self.state.notify.notify_waiters();
        }
    }

    async fn cancelled(&self) {
        if self.state.cancelled.load(Ordering::SeqCst) {
            return;
        }
        let notified = self.state.notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if self.state.cancelled.load(Ordering::SeqCst) {
            return;
        }
        notified.await;
    }
}

/// Removes the registry entry when the call finishes for any reason.
struct CancelRegistration<'a> {
    server: &'a McpServer,
    key: String,
}

impl Drop for CancelRegistration<'_> {
    fn drop(&mut self) {
        lock_cancels(&self.server.cancels).remove(&self.key);
    }
}

enum CallOutcome {
    Answer(Value),
    Cancelled,
}

/// Builder for the banner a caller sees at the HTTP root.
pub struct McpServer {
    backend: Arc<dyn ToolBackend>,
    name: String,
    version: String,
    instructions: String,
    health: Arc<dyn Fn() -> Value + Send + Sync>,
    store: Option<Arc<Store>>,
    metrics: Arc<Metrics>,
    cancels: Mutex<HashMap<String, RequestCancel>>,
}

impl McpServer {
    /// Build a server around a tool backend.
    pub fn new(backend: Arc<dyn ToolBackend>) -> Self {
        Self {
            backend,
            name: "corvus".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            instructions: "Corvus drives a real browser over CDP when a browser-level endpoint is available, and through the Corvus extension otherwise. Call browser_channel to see which channel is active. Both channels implement page_snapshot and the reference-based tools: call page_snapshot before browser_click or browser_type and pass a reference from the snapshot as target. CDP references look like @c<backendNodeId> and extension references look like @eN. browser_batch can run up to 20 browser tool calls in order. Every failure names an error code; retry only when the answer says the failure is retryable.".to_string(),
            health: Arc::new(|| json!({ "status": "ok" })),
            store: None,
            metrics: Arc::new(Metrics::new()),
            cancels: Mutex::new(HashMap::new()),
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
        let channel = health.get("channel");
        let channel_metrics = ChannelMetrics {
            mode: channel
                .and_then(|channel| channel.get("channel"))
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            cdp_connected: channel
                .and_then(|channel| channel.get("cdp"))
                .and_then(|cdp| cdp.get("connected"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            cdp_targets: channel
                .and_then(|channel| channel.get("cdp"))
                .and_then(|cdp| cdp.get("targets"))
                .and_then(Value::as_u64)
                .unwrap_or(0),
            extension_browsers: channel
                .and_then(|channel| channel.get("extension"))
                .and_then(|extension| extension.get("browsers"))
                .and_then(Value::as_array)
                .map_or(0, |browsers| browsers.len() as u64),
        };
        self.metrics
            .prometheus_with_channel(browsers, sessions, steps, &channel_metrics)
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
                        tracing::warn!(
                            error = %error,
                            session_id = %session.id,
                            "could not close a session"
                        );
                    }
                }
            }
            Err(error) => tracing::warn!(error = %error, "could not list sessions to close"),
        }
    }

    /// Cancel every in-flight request of one MCP session and forget its ids.
    pub fn cancel_session(&self, session_id: &str) {
        let prefix = format!("{session_id}{CANCEL_SEPARATOR}");
        let tokens: Vec<RequestCancel> = {
            let mut cancels = lock_cancels(&self.cancels);
            let keys: Vec<String> = cancels
                .keys()
                .filter(|key| key.starts_with(&prefix))
                .cloned()
                .collect();
            keys.into_iter()
                .filter_map(|key| cancels.remove(&key))
                .collect()
        };
        for token in tokens {
            token.cancel();
        }
    }

    fn register_cancel(&self, key: String) -> RequestCancel {
        let token = RequestCancel::default();
        lock_cancels(&self.cancels).insert(key, token.clone());
        token
    }

    fn cancel_request(&self, session: Option<&SessionContext>, request_id: &Value) {
        if request_id.is_null() {
            return;
        }
        let key = cancel_key(session, request_id);
        if let Some(token) = lock_cancels(&self.cancels).remove(&key) {
            token.cancel();
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
            "notifications/initialized" | "notifications/progress" => None,
            "notifications/cancelled" => {
                if let Some(request_id) = params.get("requestId") {
                    self.cancel_request(session, request_id);
                }
                None
            }
            "ping" => Some(success(id, json!({}))),
            "tools/list" => match tools::list(&params) {
                Ok(tools) => Some(success(id, json!({ "tools": tools }))),
                Err(error) => Some(error_response(
                    id,
                    INVALID_REQUEST,
                    format!("invalid tools/list params: {}", error.message),
                )),
            },
            "tools/call" => match self.call_tool(&params, session, &id).await {
                CallOutcome::Answer(answer) => Some(success(id, answer)),
                CallOutcome::Cancelled => {
                    Some(error_response(id, REQUEST_CANCELLED, "request cancelled"))
                }
            },
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

    async fn call_tool(
        &self,
        params: &Value,
        session: Option<&SessionContext>,
        id: &Value,
    ) -> CallOutcome {
        let name = match params.get("name").and_then(Value::as_str) {
            Some(name) if !name.trim().is_empty() => name.to_string(),
            _ => {
                return CallOutcome::Answer(tool_error(&ErrorBody::new(
                    ErrorCode::BadArgs,
                    "tools/call requires a non-empty tool name",
                )))
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
                return CallOutcome::Answer(tool_error(&error));
            }
        };
        let call = ToolCall {
            name: name.clone(),
            arguments: arguments.clone(),
            session: session.cloned(),
        };

        let key = cancel_key(session, id);
        let cancel = if id.is_null() {
            None
        } else {
            Some(self.register_cancel(key.clone()))
        };
        let registration = cancel
            .as_ref()
            .map(|_| CancelRegistration { server: self, key });
        let started = Instant::now();
        let backend = self.backend.call(call);
        let outcome = if let Some(cancel) = &cancel {
            tokio::select! {
                biased;
                result = backend => Some(result),
                _ = cancel.cancelled() => None,
            }
        } else {
            Some(backend.await)
        };
        drop(registration);
        match outcome {
            Some(Ok(result)) => {
                measurement.finish(None);
                CallOutcome::Answer(tool_success(&result))
            }
            Some(Err(error)) => {
                measurement.finish(Some(&error));
                CallOutcome::Answer(tool_error(&error))
            }
            None => {
                let elapsed = started.elapsed().as_millis() as i64;
                self.record_cancelled(session, &name, &arguments, elapsed);
                measurement.finish(None);
                CallOutcome::Cancelled
            }
        }
    }

    fn record_cancelled(
        &self,
        session: Option<&SessionContext>,
        tool: &str,
        arguments: &Value,
        duration_ms: i64,
    ) {
        let (Some(session), Some(store)) = (session, self.store.as_ref()) else {
            return;
        };
        let step = StepRecord {
            session_id: session.session_id.clone(),
            ts: now_millis(),
            tool: tool.to_string(),
            args: arguments.clone(),
            result: None,
            error_code: Some("CANCELLED".to_string()),
            duration_ms,
            browser_id: argument_string(arguments, "browser_id"),
            tab_id: argument_string(arguments, "tab_id"),
            ok: false,
        };
        if let Err(error) = store.record_step(step) {
            tracing::warn!(error = %error, "could not record a cancelled tool call");
        }
    }
}

fn lock_cancels(
    cancels: &Mutex<HashMap<String, RequestCancel>>,
) -> std::sync::MutexGuard<'_, HashMap<String, RequestCancel>> {
    cancels
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn cancel_key(session: Option<&SessionContext>, id: &Value) -> String {
    format!(
        "{}{CANCEL_SEPARATOR}{}",
        session.map_or("", |session| session.session_id.as_str()),
        serde_json::to_string(id).unwrap_or_default()
    )
}

fn argument_string(arguments: &Value, field: &str) -> Option<String> {
    match arguments.get(field)? {
        Value::String(value) if !value.is_empty() => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
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
    // A tools/call result carries the payload exactly once. The text block is
    // compact JSON; duplicating the same object in structuredContent doubles the
    // wire size of large results such as page_snapshot without adding information.
    json!({
        "content": [{ "type": "text", "text": encode(result) }],
        "isError": false,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn representative_snapshot() -> Value {
        let text = [
            "@c10 button \"Press\" bbox=8,93,50,21",
            "@c11 textbox \"Name\" value=\"Ada Lovelace\" bbox=106,93,177,21",
            "@c12 combobox \"Choice\" value=\"Alpha\" bbox=283,94,57,19",
            "@c13 option \"Alpha\" [selected] bbox=283,114,57,19",
            "@c14 option \"Beta\" bbox=283,134,57,19",
            "@c15 checkbox \"Remember me\" [mixed] bbox=8,150,20,20",
            "@c16 link \"Documentation\" bbox=40,150,91,20",
            "@c17 button \"Submit order\" [disabled] bbox=8,180,92,28",
            "@c18 heading \"Corvus compact snapshot fixture\" bbox=8,220,280,30",
            "@c19 text \"A representative page used to measure the MCP result envelope.\" bbox=8,260,340,20",
            "@c20 img \"Corvus logo\" bbox=8,290,64,64",
            "@c21 slider \"Volume\" value=\"50\" bbox=8,360,160,20",
            "@c22 spinbutton \"Quantity\" value=\"2\" bbox=8,390,80,20",
            "@c23 menuitem \"Account\" bbox=8,420,80,20",
            "@c24 tab \"Overview\" [selected] bbox=8,450,80,24",
            "@c25 tab \"Settings\" bbox=96,450,80,24"
        ]
        .join("\n");
        json!({
            "format": "compact",
            "tab_id": "T1",
            "url": "https://example.test/checkout",
            "title": "Corvus compact snapshot fixture",
            "viewport": {
                "width": 1280,
                "height": 720,
                "scrollX": 0,
                "scrollY": 0
            },
            "mode": "interactive",
            "node_count": 16,
            "truncated": false,
            "text": text
        })
    }

    // The old envelope, kept only to measure and prove the wire-size reduction.
    fn legacy_tool_success(result: &Value) -> Value {
        let text = serde_json::to_string_pretty(result).expect("pretty snapshot");
        let mut answer = json!({
            "content": [{ "type": "text", "text": text }],
            "isError": false,
        });
        if result.is_object() {
            answer
                .as_object_mut()
                .expect("object result")
                .insert("structuredContent".to_string(), result.clone());
        }
        answer
    }

    #[test]
    fn success_carries_one_compact_json_text_block() {
        let result = representative_snapshot();
        let answer = tool_success(&result);
        assert_eq!(answer.get("isError").and_then(Value::as_bool), Some(false));
        assert!(answer.get("structuredContent").is_none());
        assert_eq!(answer.as_object().expect("object answer").len(), 2);
        let content = answer
            .get("content")
            .and_then(Value::as_array)
            .expect("content array");
        assert_eq!(content.len(), 1);
        let text = content[0]
            .get("text")
            .and_then(Value::as_str)
            .expect("text content");
        assert_eq!(content[0].get("type").and_then(Value::as_str), Some("text"));
        assert_eq!(text, serde_json::to_string(&result).expect("compact json"));
        assert!(
            !text.contains('\n'),
            "the text block must not be pretty printed"
        );
        assert_eq!(
            serde_json::from_str::<Value>(text).expect("parse text json"),
            result
        );
    }

    #[test]
    fn compact_envelope_removes_the_duplicate_page_snapshot_payload() {
        let result = representative_snapshot();
        let payload = encode(&result);
        let pretty = serde_json::to_string_pretty(&result).expect("pretty json");
        let before = encode(&success(json!(23), legacy_tool_success(&result)));
        let after = encode(&success(json!(23), tool_success(&result)));
        println!(
            "page_snapshot tools/call bytes: payload={} pretty={} before={} after={} saved={}",
            payload.len(),
            pretty.len(),
            before.len(),
            after.len(),
            before.len() - after.len()
        );
        assert!(after.len() < before.len());
        assert!(after.len() < payload.len() + pretty.len());
    }
}
