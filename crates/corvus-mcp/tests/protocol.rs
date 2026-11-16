//! Protocol level tests for the MCP server, driven through raw JSON-RPC.

use std::sync::Arc;

use corvus_mcp::{BackendFuture, McpServer, ToolBackend, ToolCall};
use corvus_proto::{ErrorBody, ErrorCode};
use serde_json::{json, Value};

struct FakeBackend {
    result: Result<Value, ErrorBody>,
}

impl ToolBackend for FakeBackend {
    fn call(&self, _call: ToolCall) -> BackendFuture {
        let result = self.result.clone();
        Box::pin(async move { result })
    }
}

fn server(result: Result<Value, ErrorBody>) -> Arc<McpServer> {
    Arc::new(McpServer::new(Arc::new(FakeBackend { result })))
}

async fn call(server: &McpServer, request: Value) -> Value {
    let raw = serde_json::to_string(&request).expect("encode request");
    let answer = server
        .handle_message(&raw)
        .await
        .expect("a response is due");
    serde_json::from_str(&answer).expect("decode response")
}

fn tools_call(id: i64, name: &str, arguments: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": { "name": name, "arguments": arguments },
    })
}

#[tokio::test]
async fn initialize_reports_identity_and_capabilities() {
    let server = server(Ok(json!({})));
    let answer = call(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "0.0.0" },
            },
        }),
    )
    .await;
    let result = answer.get("result").expect("result");
    assert_eq!(
        result.get("protocolVersion").and_then(Value::as_str),
        Some("2025-06-18")
    );
    assert_eq!(
        result
            .get("serverInfo")
            .and_then(|info| info.get("name"))
            .and_then(Value::as_str),
        Some("corvus")
    );
    assert!(result
        .get("capabilities")
        .and_then(|caps| caps.get("tools"))
        .is_some());
    let instructions = result
        .get("instructions")
        .and_then(Value::as_str)
        .expect("instructions");
    assert!(instructions.contains("CDP"));
    assert!(instructions.contains("browser_channel"));
    assert!(instructions.contains("page_snapshot"));
}

#[test]
fn metrics_text_exposes_the_channel_gauges() {
    let server = McpServer::new(Arc::new(FakeBackend {
        result: Ok(json!({})),
    }))
    .with_health(Arc::new(|| {
        json!({
            "browsers": [{ "browser_id": "b-1" }],
            "recording": { "sessions": 2, "steps": 5 },
            "channel": {
                "channel": "auto",
                "cdp": { "connected": true, "targets": 3 },
                "extension": { "connected": true, "browsers": ["b-1"] }
            }
        })
    }));
    let text = server.metrics_text();
    assert!(text.contains("corvus_channel{mode=\"auto\"} 1"));
    assert!(text.contains("corvus_cdp_connected 1"));
    assert!(text.contains("corvus_cdp_targets 3"));
    assert!(text.contains("corvus_extension_browsers 1"));
    assert!(text.contains("corvus_browsers_connected 1"));
    assert!(text.contains("corvus_sessions_total 2"));
    assert!(text.contains("corvus_steps_total 5"));
}

#[tokio::test]
async fn initialize_echoes_an_older_supported_revision() {
    let server = server(Ok(json!({})));
    let answer = call(
        &server,
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-03-26" } }),
    )
    .await;
    assert_eq!(
        answer
            .get("result")
            .and_then(|result| result.get("protocolVersion"))
            .and_then(Value::as_str),
        Some("2025-03-26")
    );
}

#[tokio::test]
async fn tools_list_exposes_every_fully_annotated_tool() {
    let server = server(Ok(json!({})));
    let answer = call(
        &server,
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
    )
    .await;
    let tools = answer
        .get("result")
        .and_then(|result| result.get("tools"))
        .and_then(Value::as_array)
        .expect("tools array");
    assert_eq!(tools.len(), 62);
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .collect();
    assert_eq!(
        names,
        vec![
            "browser_navigate",
            "page_snapshot",
            "browser_click",
            "browser_type",
            "page_text",
            "page_html",
            "browser_screenshot",
            "page_find",
            "browser_hover",
            "browser_press",
            "browser_scroll",
            "browser_select",
            "browser_eval",
            "browser_wait",
            "browser_history",
            "tab_list",
            "tab_new",
            "tab_close",
            "tab_switch",
            "browser_list",
            "net_capture",
            "net_log",
            "net_clear",
            "net_intercept",
            "download_list",
            "download_wait",
            "upload_file",
            "session_list",
            "session_steps",
            "session_replay",
            "session_export",
            "metrics",
            "logs_tail",
            "cdp_status",
            "cdp_targets",
            "cdp_send",
            "cdp_events",
            "browser_channel",
            "browser_console",
            "browser_cookies",
            "browser_dialog",
            "browser_emulate",
            "browser_batch",
            "tool_help",
            "page_pdf",
            "page_links",
            "page_forms",
            "page_accessibility",
            "browser_drag",
            "browser_focus",
            "browser_highlight",
            "tab_group",
            "tab_duplicate",
            "window_list",
            "window_new",
            "cookie_set",
            "cookie_clear",
            "storage_get",
            "storage_set",
            "storage_clear",
            "health",
            "version"
        ]
    );
    for tool in tools {
        let annotations = tool.get("annotations").expect("annotations");
        for hint in [
            "readOnlyHint",
            "destructiveHint",
            "idempotentHint",
            "openWorldHint",
        ] {
            assert!(
                annotations.get(hint).and_then(Value::as_bool).is_some(),
                "missing {hint}"
            );
        }
        assert_eq!(
            tool.get("inputSchema")
                .and_then(|schema| schema.get("type"))
                .and_then(Value::as_str),
            Some("object")
        );
    }
}

#[tokio::test]
async fn a_successful_tool_call_returns_content_and_no_error() {
    let server = server(Ok(json!({ "clicked": true })));
    let answer = call(
        &server,
        tools_call(3, "browser_click", json!({ "target": "@e12" })),
    )
    .await;
    let result = answer.get("result").expect("result");
    assert_eq!(result.get("isError").and_then(Value::as_bool), Some(false));
    assert!(
        result.get("structuredContent").is_none(),
        "the payload must not be duplicated in structuredContent"
    );
    let text = result
        .get("content")
        .and_then(Value::as_array)
        .and_then(|content| content.first())
        .and_then(|item| item.get("text"))
        .and_then(Value::as_str)
        .expect("text content");
    assert!(!text.contains('\n'), "the text block must be compact JSON");
    assert_eq!(text, "{\"clicked\":true}");
    assert_eq!(
        serde_json::from_str::<Value>(text).expect("parse text content"),
        json!({ "clicked": true })
    );
}

#[tokio::test]
async fn backend_failures_become_tool_results_with_the_error_code() {
    let error = ErrorBody::new(ErrorCode::Timeout, "no answer within 5000 ms")
        .with_details(json!({ "waited_ms": 4321 }));
    let server = server(Err(error));
    let answer = call(
        &server,
        tools_call(4, "browser_click", json!({ "target": "@e1" })),
    )
    .await;
    let result = answer
        .get("result")
        .expect("a tools/call result, not a protocol error");
    assert!(answer.get("error").is_none());
    assert_eq!(result.get("isError").and_then(Value::as_bool), Some(true));
    let text = result
        .get("content")
        .and_then(Value::as_array)
        .and_then(|content| content.first())
        .and_then(|item| item.get("text"))
        .and_then(Value::as_str)
        .expect("text content");
    assert!(text.starts_with("TIMEOUT: "), "unexpected text: {text}");
    assert!(
        text.contains("{\"waited_ms\":4321}"),
        "the details must reach the text: {text}"
    );
    assert!(result.get("structuredContent").is_none());
}

#[tokio::test]
async fn a_not_found_error_without_details_stays_clean() {
    let server = server(Err(ErrorBody::new(
        ErrorCode::NotFound,
        "no element matches @e9",
    )));
    let answer = call(
        &server,
        tools_call(41, "browser_click", json!({ "target": "@e9" })),
    )
    .await;
    let result = answer.get("result").expect("result");
    assert_eq!(result.get("isError").and_then(Value::as_bool), Some(true));
    let text = result
        .get("content")
        .and_then(Value::as_array)
        .and_then(|content| content.first())
        .and_then(|item| item.get("text"))
        .and_then(Value::as_str)
        .expect("text content");
    assert_eq!(text, "NOT_FOUND: no element matches @e9");
}

#[tokio::test]
async fn an_unknown_tool_is_a_tool_error_with_bad_args() {
    let server = server(Ok(json!({})));
    let answer = call(&server, tools_call(5, "browser_wiggle", json!({}))).await;
    let result = answer.get("result").expect("result");
    assert_eq!(result.get("isError").and_then(Value::as_bool), Some(true));
    let text = result
        .get("content")
        .and_then(Value::as_array)
        .and_then(|content| content.first())
        .and_then(|item| item.get("text"))
        .and_then(Value::as_str)
        .expect("text content");
    assert!(
        text.starts_with("BAD_ARGS: unknown tool"),
        "unexpected text: {text}"
    );
}

#[tokio::test]
async fn notifications_get_no_response() {
    let server = server(Ok(json!({})));
    let raw = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }).to_string();
    assert!(server.handle_message(&raw).await.is_none());
}

#[tokio::test]
async fn unknown_methods_answer_with_a_protocol_error() {
    let server = server(Ok(json!({})));
    let answer = call(
        &server,
        json!({ "jsonrpc": "2.0", "id": 6, "method": "tools/teleport" }),
    )
    .await;
    let error = answer.get("error").expect("error");
    assert_eq!(error.get("code").and_then(Value::as_i64), Some(-32601));
}

#[tokio::test]
async fn malformed_json_answers_with_a_parse_error() {
    let server = server(Ok(json!({})));
    let answer: Value = serde_json::from_str(
        &server
            .handle_message("{ this is not json")
            .await
            .expect("parse error response"),
    )
    .expect("decode");
    assert_eq!(
        answer
            .get("error")
            .and_then(|error| error.get("code"))
            .and_then(Value::as_i64),
        Some(-32700)
    );
}

#[tokio::test]
async fn batches_are_answered_with_a_batch() {
    let server = server(Ok(json!({ "ok": true })));
    let batch = json!([
        { "jsonrpc": "2.0", "id": 7, "method": "ping" },
        tools_call(8, "page_snapshot", json!({}))
    ]);
    let answer: Value = serde_json::from_str(
        &server
            .handle_message(&batch.to_string())
            .await
            .expect("batch response"),
    )
    .expect("decode");
    assert_eq!(answer.as_array().map(Vec::len), Some(2));
}
