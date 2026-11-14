//! JSON-RPC 2.0 handling for the Model Context Protocol.

use std::sync::Arc;

use serde_json::{json, Value};

use corvus_proto::{ids, ErrorBody, ErrorCode};

use crate::backend::{ToolBackend, ToolCall};
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

    /// A session identifier to return when the body contains an initialize request.
    pub fn session_id_for(&self, raw: &str) -> Option<String> {
        let value: Value = serde_json::from_str(raw).ok()?;
        let is_initialize =
            |request: &Value| request.get("method").and_then(Value::as_str) == Some("initialize");
        let found = match &value {
            Value::Array(items) => items.iter().any(is_initialize),
            other => is_initialize(other),
        };
        found.then(ids::connection_id)
    }

    /// Turn a raw JSON-RPC message into a raw answer, if one is due.
    pub async fn handle_message(&self, raw: &str) -> Option<String> {
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
                    if let Some(response) = self.handle_value(item).await {
                        responses.push(response);
                    }
                }
                if responses.is_empty() {
                    None
                } else {
                    Some(encode(&Value::Array(responses)))
                }
            }
            other => self.handle_value(other).await.map(|value| encode(&value)),
        }
    }

    /// Handle one decoded JSON-RPC value.
    pub async fn handle_value(&self, value: Value) -> Option<Value> {
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
        match method.as_str() {
            "initialize" => Some(success(id, self.initialize(&params))),
            "notifications/initialized" | "notifications/cancelled" | "notifications/progress" => {
                None
            }
            "ping" => Some(success(id, json!({}))),
            "tools/list" => Some(success(id, json!({ "tools": tools::definitions() }))),
            "tools/call" => Some(success(id, self.call_tool(&params).await)),
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

    async fn call_tool(&self, params: &Value) -> Value {
        let name = match params.get("name").and_then(Value::as_str) {
            Some(name) if !name.trim().is_empty() => name.to_string(),
            _ => {
                return tool_error(&ErrorBody::new(
                    ErrorCode::BadArgs,
                    "tools/call requires a non-empty tool name",
                ))
            }
        };
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let arguments = match tools::validate(&name, &arguments) {
            Ok(arguments) => arguments,
            Err(error) => return tool_error(&error),
        };
        let call = ToolCall { name, arguments };
        match self.backend.call(call).await {
            Ok(result) => tool_success(&result),
            Err(error) => tool_error(&error),
        }
    }
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
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": true,
    })
}
