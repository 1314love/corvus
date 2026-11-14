//! End to end tests: a fake extension speaks the wire protocol to a running
//! daemon, and the MCP surface is driven through JSON-RPC.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use corvus_proto::{Envelope, ErrorBody, ErrorCode, Hello, HelloAck, Kind};
use corvus_record::SessionContext;
use corvusd::{Daemon, DaemonConfig, RouterConfig};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

fn temporary_database() -> PathBuf {
    let directory = tempfile::Builder::new()
        .prefix("corvus-e2e-")
        .tempdir()
        .expect("a temporary directory");
    let path = directory.path().join("corvus.db");
    std::mem::forget(directory);
    path
}

async fn start_daemon(default_timeout_ms: u64) -> Daemon {
    start_daemon_at(default_timeout_ms, temporary_database()).await
}

async fn start_daemon_at(default_timeout_ms: u64, db_path: PathBuf) -> Daemon {
    Daemon::start(DaemonConfig {
        bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
        ws_port: 0,
        http_port: 0,
        router: RouterConfig {
            default_timeout_ms,
            ..RouterConfig::default()
        },
        db_path: Some(db_path),
    })
    .await
    .expect("the daemon binds its ephemeral ports")
}

async fn handshake(addr: SocketAddr, browser: &str) -> Socket {
    let (mut socket, _) = connect_async(format!("ws://{addr}"))
        .await
        .expect("the websocket listener accepts the fake extension");
    let hello = Envelope::hello(
        "hello-1",
        browser,
        &Hello::new("default", "0.1.0").with_capability("dom"),
    );
    socket
        .send(Message::text(
            serde_json::to_string(&hello).expect("encode hello"),
        ))
        .await
        .expect("hello is sent");
    let frame = socket
        .next()
        .await
        .expect("a frame is due")
        .expect("a valid frame");
    let Message::Text(text) = frame else {
        panic!("expected a text frame, got a non-text frame");
    };
    let ack: Envelope = serde_json::from_str(text.as_str()).expect("decode hello_ack");
    assert_eq!(ack.kind, Kind::HelloAck);
    assert_eq!(ack.id, "hello-1");
    let payload: HelloAck = serde_json::from_value(ack.args).expect("decode hello_ack payload");
    assert_eq!(payload.daemon, "corvus");
    assert_eq!(payload.capabilities, vec!["dom".to_string()]);
    assert_eq!(payload.limits.max_timeout_ms, 125_000);
    assert_eq!(payload.limits.default_timeout_ms, 60_000);
    socket
}

async fn wait_for_browser(daemon: &Daemon, browser: &str) {
    for _ in 0..200 {
        if daemon.registry().get(browser).is_some() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("browser {browser} never registered");
}

async fn tools_call(daemon: &Daemon, name: &str, arguments: Value) -> Value {
    tools_call_in(daemon, None, name, arguments).await
}

async fn tools_call_in(
    daemon: &Daemon,
    session: Option<&SessionContext>,
    name: &str,
    arguments: Value,
) -> Value {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 11,
        "method": "tools/call",
        "params": { "name": name, "arguments": arguments },
    });
    let answer = daemon
        .mcp()
        .handle_message_with(&request.to_string(), session)
        .await
        .expect("tools/call always answers");
    let answer: Value = serde_json::from_str(&answer).expect("decode json-rpc answer");
    answer.get("result").cloned().expect("a tools/call result")
}

fn content_text(result: &Value) -> String {
    result
        .get("content")
        .and_then(Value::as_array)
        .and_then(|content| content.first())
        .and_then(|item| item.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn is_error(result: &Value) -> bool {
    result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

async fn http_request(addr: SocketAddr, method: &str, path: &str, body: &str) -> (String, String) {
    http_request_with(addr, method, path, body, &[]).await
}

async fn http_request_with(
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: &str,
    extra_headers: &[(&str, &str)],
) -> (String, String) {
    let mut stream = TcpStream::connect(addr)
        .await
        .expect("connect to the http listener");
    let extra: String = extra_headers
        .iter()
        .map(|(name, value)| format!("{name}: {value}\r\n"))
        .collect();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write the request");
    let mut raw = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), stream.read_to_end(&mut raw))
        .await
        .expect("the response arrives")
        .expect("read the response");
    let text = String::from_utf8_lossy(&raw).into_owned();
    match text.split_once("\r\n\r\n") {
        Some((headers, body)) => (headers.to_string(), body.to_string()),
        None => (text, String::new()),
    }
}

#[tokio::test]
async fn a_click_runs_end_to_end_from_mcp_to_the_extension() {
    let daemon = start_daemon(5_000).await;
    let socket = handshake(daemon.ws_addr(), "edge:default").await;
    let (mut sink, mut stream) = socket.split();
    let responder = tokio::spawn(async move {
        while let Some(frame) = stream.next().await {
            let Ok(Message::Text(text)) = frame else {
                continue;
            };
            let envelope: Envelope = serde_json::from_str(text.as_str()).expect("decode command");
            if envelope.kind != Kind::Command {
                continue;
            }
            assert_eq!(envelope.tool.as_deref(), Some("browser_click"));
            let target = envelope.args.get("target").cloned().unwrap_or(Value::Null);
            let response =
                Envelope::response_ok(envelope.id, json!({ "clicked": true, "target": target }));
            sink.send(Message::text(
                serde_json::to_string(&response).expect("encode response"),
            ))
            .await
            .expect("the response reaches the daemon");
        }
    });
    wait_for_browser(&daemon, "edge:default").await;

    let result = tools_call(&daemon, "browser_click", json!({ "target": "@e1" })).await;
    assert!(
        !is_error(&result),
        "unexpected tool error: {}",
        content_text(&result)
    );
    let text = content_text(&result);
    assert!(text.contains("clicked"), "unexpected text: {text}");
    assert!(
        text.contains("@e1"),
        "the argument did not reach the extension: {text}"
    );

    responder.abort();
    daemon.shutdown().await;
}

#[tokio::test]
async fn a_silent_extension_times_out_and_gets_a_cancel_event() {
    let daemon = start_daemon(300).await;
    let socket = handshake(daemon.ws_addr(), "edge:default").await;
    let (_sink, mut stream) = socket.split();
    let (cancel_sender, mut cancel_receiver) = tokio::sync::mpsc::channel::<Value>(4);
    let responder = tokio::spawn(async move {
        while let Some(frame) = stream.next().await {
            let Ok(Message::Text(text)) = frame else {
                continue;
            };
            let envelope: Envelope = serde_json::from_str(text.as_str()).expect("decode envelope");
            if envelope.kind == Kind::Event && envelope.tool.as_deref() == Some("cancel") {
                let _ = cancel_sender.send(envelope.args).await;
            }
        }
    });
    wait_for_browser(&daemon, "edge:default").await;

    let result = tools_call(&daemon, "browser_click", json!({ "target": "@e2" })).await;
    assert!(
        is_error(&result),
        "a silent extension must not report success"
    );
    let text = content_text(&result);
    assert!(text.starts_with("TIMEOUT: "), "unexpected text: {text}");

    let cancel = tokio::time::timeout(Duration::from_secs(2), cancel_receiver.recv())
        .await
        .expect("the cancel event arrives")
        .expect("the cancel payload exists");
    assert!(
        cancel
            .get("command_id")
            .and_then(Value::as_str)
            .is_some_and(|id| !id.is_empty()),
        "the cancel event names the command: {cancel}"
    );

    responder.abort();
    daemon.shutdown().await;
}

#[tokio::test]
async fn error_details_reach_the_mcp_text_and_clean_errors_stay_clean() {
    let daemon = start_daemon(5_000).await;
    let socket = handshake(daemon.ws_addr(), "edge:default").await;
    let (mut sink, mut stream) = socket.split();
    let responder = tokio::spawn(async move {
        while let Some(frame) = stream.next().await {
            let Ok(Message::Text(text)) = frame else {
                continue;
            };
            let envelope: Envelope = serde_json::from_str(text.as_str()).expect("decode command");
            if envelope.kind != Kind::Command {
                continue;
            }
            let error = match envelope.tool.as_deref() {
                Some("browser_wait") => {
                    ErrorBody::new(ErrorCode::Timeout, "wait timed out after 100 ms")
                        .with_details(json!({ "waited_ms": 4321 }))
                }
                Some("browser_click") => {
                    ErrorBody::new(ErrorCode::NotFound, "no element matches @e9")
                }
                other => panic!("unexpected tool {other:?}"),
            };
            let response = Envelope::response_err(envelope.id, error);
            sink.send(Message::text(
                serde_json::to_string(&response).expect("encode response"),
            ))
            .await
            .expect("the response reaches the daemon");
        }
    });
    wait_for_browser(&daemon, "edge:default").await;

    let wait = tools_call(
        &daemon,
        "browser_wait",
        json!({ "text": "never", "timeout_ms": 100 }),
    )
    .await;
    assert!(is_error(&wait), "the extension reported TIMEOUT");
    let wait_text = content_text(&wait);
    assert!(
        wait_text.starts_with("TIMEOUT: "),
        "unexpected text: {wait_text}"
    );
    assert!(
        wait_text.contains("{\"waited_ms\":4321}"),
        "the details must reach the MCP text: {wait_text}"
    );

    let click = tools_call(&daemon, "browser_click", json!({ "target": "@e9" })).await;
    assert!(is_error(&click), "the extension reported NOT_FOUND");
    assert_eq!(
        content_text(&click),
        "NOT_FOUND: no element matches @e9",
        "an error without details must not gain a JSON suffix"
    );

    responder.abort();
    daemon.shutdown().await;
}

#[tokio::test]
async fn an_unknown_tool_is_a_tool_error() {
    let daemon = start_daemon(1_000).await;
    let result = tools_call(&daemon, "browser_wiggle", json!({})).await;
    assert!(is_error(&result));
    let text = content_text(&result);
    assert!(
        text.starts_with("BAD_ARGS: unknown tool"),
        "unexpected text: {text}"
    );
    daemon.shutdown().await;
}

#[tokio::test]
async fn a_call_without_a_browser_reports_transport_down() {
    let daemon = start_daemon(1_000).await;
    let result = tools_call(&daemon, "browser_click", json!({ "target": "@e3" })).await;
    assert!(is_error(&result));
    let text = content_text(&result);
    assert!(
        text.starts_with("TRANSPORT_DOWN: "),
        "unexpected text: {text}"
    );

    let list = tools_call(&daemon, "browser_list", json!({})).await;
    assert!(!is_error(&list), "browser_list works without a browser");
    assert_eq!(
        payload(&list)
            .get("browsers")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );

    daemon.shutdown().await;
}

#[tokio::test]
async fn the_http_listener_answers_initialize_and_health() {
    let daemon = start_daemon(1_000).await;
    let initialize = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "0.0.0" } },
    });
    let (headers, body) =
        http_request(daemon.http_addr(), "POST", "/mcp", &initialize.to_string()).await;
    assert!(
        headers.starts_with("HTTP/1.1 200"),
        "unexpected status line: {headers}"
    );
    assert!(
        headers.to_lowercase().contains("mcp-session-id"),
        "initialize must return a session header: {headers}"
    );
    let answer: Value = serde_json::from_str(&body).expect("decode the initialize answer");
    assert_eq!(
        answer
            .get("result")
            .and_then(|result| result.get("protocolVersion"))
            .and_then(Value::as_str),
        Some("2025-06-18")
    );

    let (headers, body) = http_request(daemon.http_addr(), "GET", "/health", "").await;
    assert!(
        headers.starts_with("HTTP/1.1 200"),
        "unexpected status line: {headers}"
    );
    let health: Value = serde_json::from_str(&body).expect("decode the health document");
    assert!(
        health.get("browsers").and_then(Value::as_array).is_some(),
        "health: {health}"
    );
    assert_eq!(
        health.get("ws_addr").and_then(Value::as_str),
        Some(daemon.ws_addr().to_string().as_str()),
        "health: {health}"
    );
    assert_eq!(
        health.get("http_addr").and_then(Value::as_str),
        Some(daemon.http_addr().to_string().as_str()),
        "health: {health}"
    );

    daemon.shutdown().await;
}
#[tokio::test]
async fn a_snapshot_after_navigate_reflects_the_new_document() {
    let daemon = start_daemon(5_000).await;
    let socket = handshake(daemon.ws_addr(), "edge:default").await;
    let (mut sink, mut stream) = socket.split();
    let extension_document =
        std::sync::Arc::new(tokio::sync::Mutex::new(String::from("about:blank")));
    let responder = tokio::spawn(async move {
        while let Some(frame) = stream.next().await {
            let Ok(Message::Text(text)) = frame else {
                continue;
            };
            let envelope: Envelope = serde_json::from_str(text.as_str()).expect("decode command");
            if envelope.kind != Kind::Command {
                continue;
            }
            let payload = match envelope.tool.as_deref() {
                Some("browser_navigate") => {
                    let url = envelope
                        .args
                        .get("url")
                        .and_then(Value::as_str)
                        .expect("navigate carries a url")
                        .to_string();
                    // Answer only after the new document has replaced the old one.
                    tokio::time::sleep(Duration::from_millis(80)).await;
                    *extension_document.lock().await = url.clone();
                    json!({ "loaded": true, "url": url })
                }
                Some("page_snapshot") => {
                    let url = extension_document.lock().await.clone();
                    json!({
                        "url": url,
                        "mode": "interactive",
                        "nodes": [ { "ref": "@e1", "role": "heading", "name": "new document" } ],
                    })
                }
                other => panic!("unexpected tool {other:?}"),
            };
            let response = Envelope::response_ok(envelope.id, payload);
            sink.send(Message::text(
                serde_json::to_string(&response).expect("encode response"),
            ))
            .await
            .expect("the response reaches the daemon");
        }
    });
    wait_for_browser(&daemon, "edge:default").await;

    let navigate = tools_call(
        &daemon,
        "browser_navigate",
        json!({ "url": "http://127.0.0.1:8808/" }),
    )
    .await;
    assert!(
        !is_error(&navigate),
        "unexpected tool error: {}",
        content_text(&navigate)
    );
    let navigate_text = content_text(&navigate);
    let navigate_payload: Value =
        serde_json::from_str(&navigate_text).expect("the navigate text is a JSON object");
    assert_eq!(
        navigate_payload.get("loaded").and_then(Value::as_bool),
        Some(true),
        "navigate must report that the new document loaded: {navigate_text}"
    );
    assert_eq!(
        navigate_payload.get("url").and_then(Value::as_str),
        Some("http://127.0.0.1:8808/"),
        "navigate must report the final url: {navigate_text}"
    );

    let snapshot = tools_call(&daemon, "page_snapshot", json!({})).await;
    assert!(
        !is_error(&snapshot),
        "unexpected tool error: {}",
        content_text(&snapshot)
    );
    let snapshot_text = content_text(&snapshot);
    assert!(
        snapshot_text.contains("http://127.0.0.1:8808/"),
        "the snapshot after navigate must come from the new document: {snapshot_text}"
    );
    assert!(
        snapshot_text.contains("new document"),
        "the snapshot after navigate must contain the new document nodes: {snapshot_text}"
    );

    responder.abort();
    daemon.shutdown().await;
}

#[tokio::test]
async fn a_navigate_that_runs_out_of_load_budget_still_succeeds() {
    let daemon = start_daemon(5_000).await;
    let socket = handshake(daemon.ws_addr(), "edge:default").await;
    let (mut sink, mut stream) = socket.split();
    let responder = tokio::spawn(async move {
        while let Some(frame) = stream.next().await {
            let Ok(Message::Text(text)) = frame else {
                continue;
            };
            let envelope: Envelope = serde_json::from_str(text.as_str()).expect("decode command");
            if envelope.kind != Kind::Command {
                continue;
            }
            assert_eq!(envelope.tool.as_deref(), Some("browser_navigate"));
            let response = Envelope::response_ok(
                envelope.id,
                json!({ "loaded": false, "url": "http://127.0.0.1:8808/slow" }),
            );
            sink.send(Message::text(
                serde_json::to_string(&response).expect("encode response"),
            ))
            .await
            .expect("the response reaches the daemon");
        }
    });
    wait_for_browser(&daemon, "edge:default").await;

    let result = tools_call(
        &daemon,
        "browser_navigate",
        json!({ "url": "http://127.0.0.1:8808/slow" }),
    )
    .await;
    assert!(
        !is_error(&result),
        "a busy page must not turn the navigate call into an error: {}",
        content_text(&result)
    );
    let text = content_text(&result);
    let payload: Value = serde_json::from_str(&text).expect("the navigate text is a JSON object");
    assert_eq!(
        payload.get("loaded").and_then(Value::as_bool),
        Some(false),
        "the result must carry the load state: {text}"
    );
    assert_eq!(
        payload.get("url").and_then(Value::as_str),
        Some("http://127.0.0.1:8808/slow"),
        "the result must carry the current url: {text}"
    );

    responder.abort();
    daemon.shutdown().await;
}

#[derive(Debug, Clone)]
struct SeenCommand {
    tool: String,
    timeout_ms: Option<u64>,
    args: Value,
}

fn recorded(seen: &std::sync::Arc<std::sync::Mutex<Vec<SeenCommand>>>) -> Vec<SeenCommand> {
    seen.lock()
        .expect("the recorder lock is not poisoned")
        .clone()
}

fn expect_timeout(commands: &[SeenCommand], tool: &str, timeout_ms: u64) {
    let command = commands
        .iter()
        .find(|command| command.tool == tool)
        .unwrap_or_else(|| panic!("{tool} was never forwarded to the extension"));
    assert_eq!(
        command.timeout_ms,
        Some(timeout_ms),
        "unexpected command timeout for {tool}"
    );
}

fn payload(result: &Value) -> Value {
    serde_json::from_str(&content_text(result)).expect("the tool text is a JSON object")
}

async fn answer_all_tools(
    daemon: &Daemon,
) -> (
    tokio::task::JoinHandle<()>,
    std::sync::Arc<std::sync::Mutex<Vec<SeenCommand>>>,
) {
    answer_named(daemon, "edge:default").await
}

async fn answer_named(
    daemon: &Daemon,
    browser: &str,
) -> (
    tokio::task::JoinHandle<()>,
    std::sync::Arc<std::sync::Mutex<Vec<SeenCommand>>>,
) {
    let socket = handshake(daemon.ws_addr(), browser).await;
    let (mut sink, mut stream) = socket.split();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorder = std::sync::Arc::clone(&seen);
    let responder = tokio::spawn(async move {
        while let Some(frame) = stream.next().await {
            let Ok(Message::Text(text)) = frame else {
                continue;
            };
            let envelope: Envelope = serde_json::from_str(text.as_str()).expect("decode command");
            if envelope.kind != Kind::Command {
                continue;
            }
            let tool = envelope.tool.clone().unwrap_or_default();
            recorder
                .lock()
                .expect("the recorder lock is not poisoned")
                .push(SeenCommand {
                    tool: tool.clone(),
                    timeout_ms: envelope.meta.timeout_ms,
                    args: envelope.args.clone(),
                });
            let response = Envelope::response_ok(envelope.id, canned_result(&tool, &envelope.args));
            sink.send(Message::text(
                serde_json::to_string(&response).expect("encode response"),
            ))
            .await
            .expect("the response reaches the daemon");
        }
    });
    wait_for_browser(daemon, browser).await;
    (responder, seen)
}

fn canned_result(tool: &str, args: &Value) -> Value {
    match tool {
        "browser_navigate" => json!({
            "loaded": true,
            "url": args.get("url").cloned().unwrap_or_else(|| json!("about:blank")),
        }),
        "page_snapshot" => json!({
            "url": "https://example.test/",
            "mode": "interactive",
            "nodes": [],
        }),
        "browser_click" => json!({
            "clicked": true,
            "target": args.get("target").cloned().unwrap_or(Value::Null),
        }),
        "browser_type" => json!({ "typed": true }),
        "page_text" => json!({
            "url": "https://example.test/",
            "title": "Example",
            "text": "hello world",
            "truncated": false,
        }),
        "page_html" => json!({
            "url": "https://example.test/",
            "html": "<html><body>hello</body></html>",
            "truncated": false,
        }),
        "page_find" => json!({
            "url": "https://example.test/",
            "count": 1,
            "matches": [ { "ref": "@e1", "tag": "a", "role": "link", "text": "hello" } ],
        }),
        "browser_hover" => json!({
            "hovered": true,
            "url": "https://example.test/",
            "target": { "ref": "@e1", "tag": "a", "text": "hello" },
        }),
        "browser_press" => {
            let keys = match args.get("keys") {
                Some(Value::Array(items)) => items.clone(),
                Some(key) => vec![key.clone()],
                None => Vec::new(),
            };
            json!({ "pressed": true, "keys": keys, "url": "https://example.test/" })
        }
        "browser_scroll" => json!({
            "scrolled": true,
            "x": 0,
            "y": 640,
            "url": "https://example.test/",
        }),
        "browser_select" => json!({
            "selected": ["b", "c"],
            "value": "b",
            "url": "https://example.test/",
        }),
        "browser_eval" => {
            if args.get("expression").and_then(Value::as_str) == Some("page_error") {
                json!({
                    "ok": false,
                    "error": { "name": "ReferenceError", "message": "page error" },
                })
            } else {
                json!({ "ok": true, "value": 2 })
            }
        }
        "browser_wait" => json!({
            "matched": true,
            "waited_ms": 12,
            "url": "https://example.test/",
        }),
        "browser_history" => json!({
            "action": args.get("action").cloned().unwrap_or_else(|| json!("reload")),
            "url": "https://example.test/",
            "loaded": true,
        }),
        "tab_list" => json!({
            "tabs": [
                { "tab_id": "1", "index": 0, "active": true, "url": "https://example.test/", "title": "Example", "status": "complete" }
            ],
        }),
        "tab_new" => json!({
            "tab_id": "2",
            "url": args.get("url").cloned().unwrap_or_else(|| json!("about:blank")),
            "loaded": true,
            "previous_tab_id": "1",
        }),
        "tab_close" => json!({
            "closed": true,
            "tab_id": args.get("tab_id").cloned().unwrap_or(Value::Null),
            "next_tab_id": "1",
        }),
        "tab_switch" => json!({
            "tab_id": args.get("tab_id").cloned().unwrap_or(Value::Null),
            "url": "https://example.test/",
            "title": "Example",
            "active": true,
        }),
        "net_capture" => {
            let tab_id = args.get("tab_id").cloned().unwrap_or_else(|| json!("1"));
            match args.get("action").and_then(Value::as_str) {
                Some("start") => json!({
                    "capturing": true,
                    "tab_id": tab_id,
                    "started_at": 1_700_000_000_000u64,
                    "capacity": 2000,
                }),
                Some("stop") => json!({
                    "capturing": false,
                    "tab_id": tab_id,
                    "entries": 3,
                }),
                _ => json!({
                    "capturing": true,
                    "tab_id": tab_id,
                    "started_at": 1_700_000_000_000u64,
                    "entries": 3,
                }),
            }
        }
        "net_log" => json!({
            "entries": [{
                "seq": 7,
                "ts": 1_700_000_000_123u64,
                "tab_id": "1",
                "request_id": "req-1",
                "method": "GET",
                "url": "https://example.test/api/items",
                "status": 200,
                "mime": "application/json",
                "resource_type": "xhr",
                "from_cache": false,
                "duration_ms": 12,
                "error": null,
                "intercepted": null,
                "request_headers": { "accept": "application/json" },
                "response_headers": { "content-type": "application/json" },
            }],
            "next_seq": 7,
            "capturing": true,
        }),
        "net_clear" => json!({ "cleared": 4 }),
        "net_intercept" => {
            let tab_id = args.get("tab_id").cloned();
            let mut result = json!({
                "ok": true,
                "scope": if tab_id.is_some() { "tab" } else { "browser" },
                "rules": args.get("rules").cloned().unwrap_or_else(|| json!([])),
                "applied": true,
            });
            if let Some(tab_id) = tab_id {
                result["tab_id"] = tab_id;
            }
            result
        }
        "download_list" => json!({
            "downloads": [{
                "id": "dl-1",
                "filename": "corvus.txt",
                "url": "https://example.test/corvus.txt",
                "state": "complete",
                "bytes_received": 12,
                "total_bytes": 12,
                "danger": "safe",
                "exists": true,
                "started_at": 1_700_000_000_000u64,
                "tab_id": "1",
            }],
        }),
        "download_wait" => json!({
            "id": args.get("id").cloned().unwrap_or_else(|| json!("dl-1")),
            "state": "complete",
            "filename": "corvus.txt",
            "url": "https://example.test/corvus.txt",
            "bytes_received": 12,
            "total_bytes": 12,
        }),
        "upload_file" => json!({
            "uploaded": true,
            "files": args.get("files").cloned().unwrap_or_else(|| json!([])),
            "target": { "ref": "@e2", "tag": "input", "text": "" },
            "input": { "accept": "text/plain", "multiple": false },
        }),
        "browser_list" => {
            panic!("browser_list is answered locally and must not reach the extension")
        }
        other => panic!("the fake extension does not know {other}"),
    }
}

#[tokio::test]
async fn page_introspection_tools_round_trip() {
    let daemon = start_daemon(60_000).await;
    let (responder, seen) = answer_all_tools(&daemon).await;

    let text = tools_call(&daemon, "page_text", json!({ "max_chars": 1000 })).await;
    assert!(
        !is_error(&text),
        "unexpected error: {}",
        content_text(&text)
    );
    let text = payload(&text);
    assert_eq!(text.get("title").and_then(Value::as_str), Some("Example"));
    assert_eq!(
        text.get("text").and_then(Value::as_str),
        Some("hello world")
    );
    assert_eq!(text.get("truncated").and_then(Value::as_bool), Some(false));

    let html = tools_call(&daemon, "page_html", json!({ "target": "#main" })).await;
    assert!(
        !is_error(&html),
        "unexpected error: {}",
        content_text(&html)
    );
    let html = payload(&html);
    assert!(html
        .get("html")
        .and_then(Value::as_str)
        .is_some_and(|markup| markup.contains("hello")));

    let find = tools_call(
        &daemon,
        "page_find",
        json!({ "query": "hello", "role": "link", "limit": 5 }),
    )
    .await;
    assert!(
        !is_error(&find),
        "unexpected error: {}",
        content_text(&find)
    );
    let find = payload(&find);
    assert_eq!(find.get("count").and_then(Value::as_u64), Some(1));
    assert_eq!(
        find.get("matches")
            .and_then(Value::as_array)
            .and_then(|matches| matches.first())
            .and_then(|first| first.get("ref"))
            .and_then(Value::as_str),
        Some("@e1")
    );

    let commands = recorded(&seen);
    for tool in ["page_text", "page_html", "page_find"] {
        expect_timeout(&commands, tool, 60_000);
    }
    let find_command = commands
        .iter()
        .find(|command| command.tool == "page_find")
        .expect("page_find was recorded");
    assert_eq!(
        find_command.args.get("role").and_then(Value::as_str),
        Some("link"),
        "the validated arguments reach the extension unchanged"
    );

    responder.abort();
    daemon.shutdown().await;
}

#[tokio::test]
async fn browser_interaction_tools_round_trip() {
    let daemon = start_daemon(60_000).await;
    let (responder, seen) = answer_all_tools(&daemon).await;

    let hover = tools_call(&daemon, "browser_hover", json!({ "target": "@e1" })).await;
    assert!(
        !is_error(&hover),
        "unexpected error: {}",
        content_text(&hover)
    );
    let hover = payload(&hover);
    assert_eq!(hover.get("hovered").and_then(Value::as_bool), Some(true));
    assert_eq!(
        hover
            .get("target")
            .and_then(|target| target.get("tag"))
            .and_then(Value::as_str),
        Some("a")
    );

    let press = tools_call(
        &daemon,
        "browser_press",
        json!({ "keys": ["Control", "a"], "target": "@e1" }),
    )
    .await;
    assert!(
        !is_error(&press),
        "unexpected error: {}",
        content_text(&press)
    );
    let press = payload(&press);
    assert_eq!(press.get("pressed").and_then(Value::as_bool), Some(true));
    assert_eq!(
        press
            .get("keys")
            .and_then(Value::as_array)
            .and_then(|keys| keys.get(1))
            .and_then(Value::as_str),
        Some("a")
    );

    let scroll = tools_call(
        &daemon,
        "browser_scroll",
        json!({ "direction": "down", "pages": 0.5 }),
    )
    .await;
    assert!(
        !is_error(&scroll),
        "unexpected error: {}",
        content_text(&scroll)
    );
    let scroll = payload(&scroll);
    assert_eq!(scroll.get("scrolled").and_then(Value::as_bool), Some(true));
    assert_eq!(scroll.get("y").and_then(Value::as_i64), Some(640));

    let select = tools_call(
        &daemon,
        "browser_select",
        json!({ "target": "#choices", "values": ["b", "c"] }),
    )
    .await;
    assert!(
        !is_error(&select),
        "unexpected error: {}",
        content_text(&select)
    );
    let select = payload(&select);
    assert_eq!(
        select
            .get("selected")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(select.get("value").and_then(Value::as_str), Some("b"));

    let commands = recorded(&seen);
    for tool in [
        "browser_hover",
        "browser_press",
        "browser_scroll",
        "browser_select",
    ] {
        expect_timeout(&commands, tool, 60_000);
    }

    responder.abort();
    daemon.shutdown().await;
}

#[tokio::test]
async fn browser_eval_round_trip_and_page_error_passthrough() {
    let daemon = start_daemon(60_000).await;
    let (responder, seen) = answer_all_tools(&daemon).await;

    let value = tools_call(
        &daemon,
        "browser_eval",
        json!({ "expression": "1 + 1", "world": "main" }),
    )
    .await;
    assert!(
        !is_error(&value),
        "unexpected error: {}",
        content_text(&value)
    );
    let value = payload(&value);
    assert_eq!(value.get("ok").and_then(Value::as_bool), Some(true));
    assert_eq!(value.get("value").and_then(Value::as_i64), Some(2));

    let error = tools_call(
        &daemon,
        "browser_eval",
        json!({ "expression": "page_error" }),
    )
    .await;
    assert!(
        !is_error(&error),
        "a page exception is a successful tool result: {}",
        content_text(&error)
    );
    let error = payload(&error);
    assert_eq!(error.get("ok").and_then(Value::as_bool), Some(false));
    assert_eq!(
        error
            .get("error")
            .and_then(|error| error.get("name"))
            .and_then(Value::as_str),
        Some("ReferenceError")
    );

    let commands = recorded(&seen);
    for command in commands
        .iter()
        .filter(|command| command.tool == "browser_eval")
    {
        assert_eq!(
            command.timeout_ms,
            Some(30_000),
            "browser_eval has its own default timeout"
        );
    }

    responder.abort();
    daemon.shutdown().await;
}

#[tokio::test]
async fn browser_wait_uses_its_own_timeout_budget() {
    let daemon = start_daemon(60_000).await;
    let (responder, seen) = answer_all_tools(&daemon).await;

    let short = tools_call(
        &daemon,
        "browser_wait",
        json!({ "text": "ready", "timeout_ms": 200 }),
    )
    .await;
    assert!(
        !is_error(&short),
        "unexpected error: {}",
        content_text(&short)
    );
    assert_eq!(
        payload(&short).get("matched").and_then(Value::as_bool),
        Some(true)
    );

    let long = tools_call(
        &daemon,
        "browser_wait",
        json!({ "selector": "#done", "timeout_ms": 120000 }),
    )
    .await;
    assert!(
        !is_error(&long),
        "unexpected error: {}",
        content_text(&long)
    );

    let defaulted = tools_call(
        &daemon,
        "browser_wait",
        json!({ "url_contains": "example" }),
    )
    .await;
    assert!(
        !is_error(&defaulted),
        "unexpected error: {}",
        content_text(&defaulted)
    );

    let commands = recorded(&seen);
    let waits: Vec<Option<u64>> = commands
        .iter()
        .filter(|command| command.tool == "browser_wait")
        .map(|command| command.timeout_ms)
        .collect();
    assert_eq!(
        waits,
        vec![Some(5_200), Some(125_000), Some(15_000)],
        "browser_wait adds a grace period to its own timeout and caps it"
    );

    responder.abort();
    daemon.shutdown().await;
}

#[tokio::test]
async fn browser_history_round_trip() {
    let daemon = start_daemon(60_000).await;
    let (responder, seen) = answer_all_tools(&daemon).await;

    let history = tools_call(&daemon, "browser_history", json!({ "action": "back" })).await;
    assert!(
        !is_error(&history),
        "unexpected error: {}",
        content_text(&history)
    );
    let history = payload(&history);
    assert_eq!(history.get("action").and_then(Value::as_str), Some("back"));
    assert_eq!(history.get("loaded").and_then(Value::as_bool), Some(true));

    expect_timeout(&recorded(&seen), "browser_history", 60_000);

    responder.abort();
    daemon.shutdown().await;
}

#[tokio::test]
async fn tab_tools_round_trip() {
    let daemon = start_daemon(60_000).await;
    let (responder, seen) = answer_all_tools(&daemon).await;

    let list = tools_call(&daemon, "tab_list", json!({})).await;
    assert!(
        !is_error(&list),
        "unexpected error: {}",
        content_text(&list)
    );
    let list = payload(&list);
    assert_eq!(
        list.get("tabs")
            .and_then(Value::as_array)
            .and_then(|tabs| tabs.first())
            .and_then(|tab| tab.get("tab_id"))
            .and_then(Value::as_str),
        Some("1")
    );

    let new = tools_call(
        &daemon,
        "tab_new",
        json!({ "url": "https://example.test/new", "active": false }),
    )
    .await;
    assert!(!is_error(&new), "unexpected error: {}", content_text(&new));
    let new = payload(&new);
    assert_eq!(new.get("tab_id").and_then(Value::as_str), Some("2"));
    assert_eq!(
        new.get("previous_tab_id").and_then(Value::as_str),
        Some("1")
    );

    let close = tools_call(&daemon, "tab_close", json!({ "tab_id": "2" })).await;
    assert!(
        !is_error(&close),
        "unexpected error: {}",
        content_text(&close)
    );
    let close = payload(&close);
    assert_eq!(close.get("closed").and_then(Value::as_bool), Some(true));
    assert_eq!(close.get("next_tab_id").and_then(Value::as_str), Some("1"));

    let switch = tools_call(&daemon, "tab_switch", json!({ "tab_id": "1" })).await;
    assert!(
        !is_error(&switch),
        "unexpected error: {}",
        content_text(&switch)
    );
    let switch = payload(&switch);
    assert_eq!(switch.get("active").and_then(Value::as_bool), Some(true));

    let commands = recorded(&seen);
    for tool in ["tab_list", "tab_new", "tab_close", "tab_switch"] {
        expect_timeout(&commands, tool, 60_000);
    }

    responder.abort();
    daemon.shutdown().await;
}

#[tokio::test]
async fn default_command_timeout_is_sixty_seconds_for_regular_tools() {
    let daemon = start_daemon(60_000).await;
    let (responder, seen) = answer_all_tools(&daemon).await;

    let click = tools_call(&daemon, "browser_click", json!({ "target": "@e1" })).await;
    assert!(
        !is_error(&click),
        "unexpected error: {}",
        content_text(&click)
    );
    let type_text = tools_call(
        &daemon,
        "browser_type",
        json!({ "target": "@e1", "text": "hello" }),
    )
    .await;
    assert!(
        !is_error(&type_text),
        "unexpected error: {}",
        content_text(&type_text)
    );
    let snapshot = tools_call(&daemon, "page_snapshot", json!({})).await;
    assert!(
        !is_error(&snapshot),
        "unexpected error: {}",
        content_text(&snapshot)
    );
    let navigate = tools_call(
        &daemon,
        "browser_navigate",
        json!({ "url": "https://example.test/" }),
    )
    .await;
    assert!(
        !is_error(&navigate),
        "unexpected error: {}",
        content_text(&navigate)
    );

    let commands = recorded(&seen);
    for tool in [
        "browser_click",
        "browser_type",
        "page_snapshot",
        "browser_navigate",
    ] {
        expect_timeout(&commands, tool, 60_000);
    }

    responder.abort();
    daemon.shutdown().await;
}

#[tokio::test]
async fn browser_list_is_answered_locally_and_sorted_by_connection_time() {
    let daemon = start_daemon(5_000).await;
    let (first, first_seen) = answer_named(&daemon, "edge:first").await;
    let (second, second_seen) = answer_named(&daemon, "edge:second").await;

    let result = tools_call(&daemon, "browser_list", json!({})).await;
    assert!(
        !is_error(&result),
        "unexpected error: {}",
        content_text(&result)
    );
    let list = payload(&result);
    let browsers = list
        .get("browsers")
        .and_then(Value::as_array)
        .expect("a browsers array");
    assert_eq!(browsers.len(), 2, "both browsers are listed");
    let mut ids: Vec<String> = browsers
        .iter()
        .filter_map(|browser| browser.get("browser_id"))
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    ids.sort();
    assert_eq!(ids, vec!["edge:first", "edge:second"]);
    for browser in browsers {
        assert_eq!(
            browser.get("profile").and_then(Value::as_str),
            Some("default")
        );
        assert_eq!(
            browser.get("version").and_then(Value::as_str),
            Some("0.1.0")
        );
        assert_eq!(
            browser.get("capabilities").and_then(Value::as_array),
            Some(&vec![json!("dom")])
        );
        assert!(
            browser
                .get("connected_at_ms")
                .and_then(Value::as_u64)
                .is_some(),
            "connected_at_ms is reported"
        );
    }
    let times: Vec<u64> = browsers
        .iter()
        .filter_map(|browser| browser.get("connected_at_ms"))
        .filter_map(Value::as_u64)
        .collect();
    assert!(
        times.windows(2).all(|pair| pair[0] <= pair[1]),
        "browser_list is sorted oldest first: {times:?}"
    );
    assert!(
        recorded(&first_seen).is_empty() && recorded(&second_seen).is_empty(),
        "browser_list must not reach the extension"
    );

    first.abort();
    second.abort();
    daemon.shutdown().await;
}

#[tokio::test]
async fn browser_id_routes_commands_to_the_chosen_socket() {
    let daemon = start_daemon(5_000).await;
    let (first, first_seen) = answer_named(&daemon, "edge:first").await;
    let (second, second_seen) = answer_named(&daemon, "edge:second").await;

    let result = tools_call(
        &daemon,
        "browser_click",
        json!({ "target": "@e7", "browser_id": "edge:second" }),
    )
    .await;
    assert!(
        !is_error(&result),
        "unexpected error: {}",
        content_text(&result)
    );
    assert_eq!(
        payload(&result).get("target").and_then(Value::as_str),
        Some("@e7")
    );
    assert!(
        recorded(&first_seen).is_empty(),
        "the unselected browser must not receive the command"
    );
    let second_commands = recorded(&second_seen);
    assert_eq!(second_commands.len(), 1);
    assert_eq!(second_commands[0].tool, "browser_click");
    assert_eq!(second_commands[0].timeout_ms, Some(5_000));

    first.abort();
    second.abort();
    daemon.shutdown().await;
}

#[tokio::test]
async fn ambiguous_and_unknown_browser_ids_report_typed_details() {
    let daemon = start_daemon(5_000).await;
    let (first, _first_seen) = answer_named(&daemon, "edge:first").await;
    let (second, _second_seen) = answer_named(&daemon, "edge:second").await;

    let ambiguous = tools_call(&daemon, "browser_click", json!({ "target": "@e1" })).await;
    assert!(is_error(&ambiguous), "two browsers need an explicit choice");
    let text = content_text(&ambiguous);
    assert!(text.starts_with("BAD_ARGS: "), "unexpected text: {text}");
    let start = text.find('{').expect("the details are appended");
    let details: Value = serde_json::from_str(&text[start..]).expect("the details are JSON");
    assert_eq!(
        details.get("reason").and_then(Value::as_str),
        Some("multiple_browsers")
    );
    let mut browser_ids: Vec<&str> = details
        .get("browser_ids")
        .and_then(Value::as_array)
        .expect("browser_ids")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    browser_ids.sort_unstable();
    assert_eq!(browser_ids, vec!["edge:first", "edge:second"]);

    let unknown = tools_call(
        &daemon,
        "browser_click",
        json!({ "target": "@e1", "browser_id": "edge:ghost" }),
    )
    .await;
    assert!(is_error(&unknown));
    let text = content_text(&unknown);
    assert!(
        text.starts_with("TRANSPORT_DOWN: "),
        "unexpected text: {text}"
    );
    assert!(
        text.contains("\"browser_id\":\"edge:ghost\""),
        "the unknown identifier must be reported: {text}"
    );

    first.abort();
    second.abort();
    daemon.shutdown().await;
}

#[tokio::test]
async fn network_tools_round_trip() {
    let daemon = start_daemon(60_000).await;
    let (responder, seen) = answer_all_tools(&daemon).await;

    let capture = tools_call(
        &daemon,
        "net_capture",
        json!({ "action": "start", "tab_id": "1" }),
    )
    .await;
    assert!(
        !is_error(&capture),
        "unexpected error: {}",
        content_text(&capture)
    );
    let capture = payload(&capture);
    assert_eq!(
        capture.get("capturing").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(capture.get("capacity").and_then(Value::as_u64), Some(2000));

    let log = tools_call(
        &daemon,
        "net_log",
        json!({ "tab_id": "1", "since_seq": 0, "limit": 10, "include_headers": true }),
    )
    .await;
    assert!(!is_error(&log), "unexpected error: {}", content_text(&log));
    let log = payload(&log);
    assert_eq!(log.get("next_seq").and_then(Value::as_u64), Some(7));
    assert_eq!(
        log.get("entries")
            .and_then(Value::as_array)
            .and_then(|entries| entries.first())
            .and_then(|entry| entry.get("seq"))
            .and_then(Value::as_u64),
        Some(7)
    );

    let clear = tools_call(&daemon, "net_clear", json!({ "tab_id": "1" })).await;
    assert!(
        !is_error(&clear),
        "unexpected error: {}",
        content_text(&clear)
    );
    assert_eq!(
        payload(&clear).get("cleared").and_then(Value::as_u64),
        Some(4)
    );

    let browser_wide = tools_call(
        &daemon,
        "net_intercept",
        json!({
            "action": "set",
            "rules": [{ "id": "r1", "url_regex": "/api/", "action": "block" }],
        }),
    )
    .await;
    assert!(
        !is_error(&browser_wide),
        "unexpected error: {}",
        content_text(&browser_wide)
    );
    let browser_wide = payload(&browser_wide);
    assert_eq!(
        browser_wide.get("scope").and_then(Value::as_str),
        Some("browser")
    );
    assert_eq!(
        browser_wide.get("applied").and_then(Value::as_bool),
        Some(true)
    );

    let tab_wide = tools_call(
        &daemon,
        "net_intercept",
        json!({
            "action": "set",
            "tab_id": "1",
            "rules": [
                { "id": "r2", "url_regex": "/api/", "method": "POST", "action": "mock", "status": 201 }
            ],
        }),
    )
    .await;
    assert!(
        !is_error(&tab_wide),
        "unexpected error: {}",
        content_text(&tab_wide)
    );
    let tab_wide = payload(&tab_wide);
    assert_eq!(tab_wide.get("scope").and_then(Value::as_str), Some("tab"));
    assert_eq!(tab_wide.get("tab_id").and_then(Value::as_str), Some("1"));
    assert_eq!(
        tab_wide
            .get("rules")
            .and_then(Value::as_array)
            .and_then(|rules| rules.first())
            .and_then(|rule| rule.get("status"))
            .and_then(Value::as_u64),
        Some(201)
    );

    let commands = recorded(&seen);
    for tool in ["net_capture", "net_log", "net_clear", "net_intercept"] {
        expect_timeout(&commands, tool, 60_000);
    }
    let log_command = commands
        .iter()
        .find(|command| command.tool == "net_log")
        .expect("net_log was recorded");
    assert_eq!(log_command.args.get("include_headers"), Some(&json!(true)));

    responder.abort();
    daemon.shutdown().await;
}

#[tokio::test]
async fn download_tools_round_trip_and_wait_timeout() {
    let daemon = start_daemon(60_000).await;
    let (responder, seen) = answer_all_tools(&daemon).await;

    let list = tools_call(&daemon, "download_list", json!({ "limit": 5 })).await;
    assert!(
        !is_error(&list),
        "unexpected error: {}",
        content_text(&list)
    );
    let list = payload(&list);
    assert_eq!(
        list.get("downloads")
            .and_then(Value::as_array)
            .and_then(|downloads| downloads.first())
            .and_then(|download| download.get("id"))
            .and_then(Value::as_str),
        Some("dl-1")
    );

    let wait = tools_call(&daemon, "download_wait", json!({})).await;
    assert!(
        !is_error(&wait),
        "unexpected error: {}",
        content_text(&wait)
    );
    assert_eq!(
        payload(&wait).get("state").and_then(Value::as_str),
        Some("complete")
    );

    let explicit = tools_call(
        &daemon,
        "download_wait",
        json!({ "id": "dl-9", "timeout_ms": 120000 }),
    )
    .await;
    assert!(
        !is_error(&explicit),
        "unexpected error: {}",
        content_text(&explicit)
    );
    assert_eq!(
        payload(&explicit).get("id").and_then(Value::as_str),
        Some("dl-9")
    );

    let commands = recorded(&seen);
    expect_timeout(&commands, "download_list", 60_000);
    let waits: Vec<Option<u64>> = commands
        .iter()
        .filter(|command| command.tool == "download_wait")
        .map(|command| command.timeout_ms)
        .collect();
    assert_eq!(
        waits,
        vec![Some(35_000), Some(125_000)],
        "download_wait adds its grace period and caps the result"
    );

    responder.abort();
    daemon.shutdown().await;
}

#[tokio::test]
async fn upload_file_round_trip() {
    let daemon = start_daemon(60_000).await;
    let (responder, seen) = answer_all_tools(&daemon).await;
    let path = std::env::temp_dir()
        .join("corvus-upload.txt")
        .to_string_lossy()
        .into_owned();

    let upload = tools_call(
        &daemon,
        "upload_file",
        json!({ "target": "#file", "files": [path.clone()] }),
    )
    .await;
    assert!(
        !is_error(&upload),
        "unexpected error: {}",
        content_text(&upload)
    );
    let upload = payload(&upload);
    assert_eq!(upload.get("uploaded").and_then(Value::as_bool), Some(true));
    assert_eq!(
        upload
            .get("files")
            .and_then(Value::as_array)
            .and_then(|files| files.first())
            .and_then(Value::as_str),
        Some(path.as_str())
    );
    assert_eq!(
        upload
            .get("target")
            .and_then(|target| target.get("tag"))
            .and_then(Value::as_str),
        Some("input")
    );

    expect_timeout(&recorded(&seen), "upload_file", 60_000);

    responder.abort();
    daemon.shutdown().await;
}

async fn answer_with_failures(
    daemon: &Daemon,
    browser: &str,
) -> (
    tokio::task::JoinHandle<()>,
    std::sync::Arc<std::sync::Mutex<Vec<SeenCommand>>>,
) {
    let socket = handshake(daemon.ws_addr(), browser).await;
    let (mut sink, mut stream) = socket.split();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorder = std::sync::Arc::clone(&seen);
    let responder = tokio::spawn(async move {
        while let Some(frame) = stream.next().await {
            let Ok(Message::Text(text)) = frame else {
                continue;
            };
            let envelope: Envelope = serde_json::from_str(text.as_str()).expect("decode command");
            if envelope.kind != Kind::Command {
                continue;
            }
            let tool = envelope.tool.clone().unwrap_or_default();
            recorder
                .lock()
                .expect("the recorder lock is not poisoned")
                .push(SeenCommand {
                    tool: tool.clone(),
                    timeout_ms: envelope.meta.timeout_ms,
                    args: envelope.args.clone(),
                });
            let target = envelope
                .args
                .get("target")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let response = if target == "@bad" {
                Envelope::response_err(
                    envelope.id,
                    ErrorBody::new(ErrorCode::NotFound, "no element matches @bad"),
                )
            } else {
                Envelope::response_ok(envelope.id, canned_result(&tool, &envelope.args))
            };
            sink.send(Message::text(
                serde_json::to_string(&response).expect("encode response"),
            ))
            .await
            .expect("the response reaches the daemon");
        }
    });
    wait_for_browser(daemon, browser).await;
    (responder, seen)
}

#[tokio::test]
async fn session_recording_lists_steps_and_exports() {
    let daemon = start_daemon(5_000).await;
    let (responder, _seen) = answer_with_failures(&daemon, "edge:default").await;
    let first = daemon
        .mcp()
        .start_session("http", Some("agent-one"))
        .expect("the recording store is available");
    tokio::time::sleep(Duration::from_millis(5)).await;
    let second = daemon
        .mcp()
        .start_session("stdio", Some("agent-two"))
        .expect("the recording store is available");

    let click = tools_call_in(
        &daemon,
        Some(&first),
        "browser_click",
        json!({ "target": "@e1" }),
    )
    .await;
    assert!(
        !is_error(&click),
        "unexpected error: {}",
        content_text(&click)
    );
    let failure = tools_call_in(
        &daemon,
        Some(&first),
        "browser_click",
        json!({ "target": "@bad" }),
    )
    .await;
    assert!(is_error(&failure), "the extension reported NOT_FOUND");
    assert!(daemon.flush_recording().await, "the recorder flushes");

    let list = tools_call(&daemon, "session_list", json!({ "limit": 10 })).await;
    assert!(
        !is_error(&list),
        "unexpected error: {}",
        content_text(&list)
    );
    let list = payload(&list);
    assert_eq!(list.get("total").and_then(Value::as_i64), Some(2));
    let sessions = list
        .get("sessions")
        .and_then(Value::as_array)
        .expect("the session array");
    assert_eq!(sessions.len(), 2);
    assert_eq!(
        sessions[0].get("session_id").and_then(Value::as_str),
        Some(second.session_id.as_str()),
        "newest sessions come first"
    );
    assert_eq!(
        sessions[0].get("transport").and_then(Value::as_str),
        Some("stdio")
    );
    assert_eq!(sessions[0].get("steps").and_then(Value::as_i64), Some(0));
    assert_eq!(
        sessions[1].get("session_id").and_then(Value::as_str),
        Some(first.session_id.as_str())
    );
    assert_eq!(sessions[1].get("steps").and_then(Value::as_i64), Some(2));
    assert_eq!(sessions[1].get("errors").and_then(Value::as_i64), Some(1));
    assert_eq!(
        sessions[1].get("status").and_then(Value::as_str),
        Some("active")
    );

    let steps = tools_call(
        &daemon,
        "session_steps",
        json!({ "session_id": first.session_id, "limit": 1 }),
    )
    .await;
    assert!(
        !is_error(&steps),
        "unexpected error: {}",
        content_text(&steps)
    );
    let steps = payload(&steps);
    assert_eq!(steps.get("total_steps").and_then(Value::as_i64), Some(2));
    assert_eq!(steps.get("next_seq").and_then(Value::as_i64), Some(2));
    let step = steps
        .get("steps")
        .and_then(Value::as_array)
        .and_then(|steps| steps.first())
        .expect("one step");
    assert_eq!(
        step.get("tool").and_then(Value::as_str),
        Some("browser_click")
    );
    assert_eq!(
        step.get("args")
            .and_then(|args| args.get("target"))
            .and_then(Value::as_str),
        Some("@e1")
    );
    assert_eq!(
        step.get("args_truncated").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        step.get("result")
            .and_then(|result| result.get("clicked"))
            .and_then(Value::as_bool),
        Some(true)
    );

    let rest = tools_call(
        &daemon,
        "session_steps",
        json!({ "session_id": first.session_id, "from_seq": 2, "limit": 10 }),
    )
    .await;
    assert!(
        !is_error(&rest),
        "unexpected error: {}",
        content_text(&rest)
    );
    let rest = payload(&rest);
    assert!(rest.get("next_seq").is_none(), "no page remains: {rest}");
    let failed = rest
        .get("steps")
        .and_then(Value::as_array)
        .and_then(|steps| steps.first())
        .expect("the failed step");
    assert_eq!(failed.get("ok").and_then(Value::as_bool), Some(false));
    assert_eq!(
        failed.get("error_code").and_then(Value::as_str),
        Some("NOT_FOUND")
    );
    assert_eq!(failed.get("result"), Some(&Value::Null));

    let export = tools_call(
        &daemon,
        "session_export",
        json!({ "session_id": first.session_id }),
    )
    .await;
    assert!(
        !is_error(&export),
        "unexpected error: {}",
        content_text(&export)
    );
    let export = payload(&export);
    assert_eq!(export.get("format").and_then(Value::as_str), Some("jsonl"));
    assert_eq!(
        export.get("truncated").and_then(Value::as_bool),
        Some(false)
    );
    let content = export
        .get("content")
        .and_then(Value::as_str)
        .expect("the exported content");
    assert_eq!(
        export.get("bytes").and_then(Value::as_u64),
        Some(content.len() as u64)
    );
    let lines: Vec<Value> = content
        .lines()
        .map(|line| serde_json::from_str(line).expect("every line is JSON"))
        .collect();
    assert_eq!(lines.len(), 3, "one session line and two step lines");
    assert_eq!(
        lines[0].get("type").and_then(Value::as_str),
        Some("session")
    );
    assert_eq!(lines[1].get("type").and_then(Value::as_str), Some("step"));
    assert_eq!(
        lines[1].get("tool").and_then(Value::as_str),
        Some("browser_click")
    );
    assert_eq!(lines[2].get("type").and_then(Value::as_str), Some("step"));

    responder.abort();
    daemon.shutdown().await;
}

#[tokio::test]
async fn session_steps_parse_and_truncate_large_arguments() {
    let daemon = start_daemon(5_000).await;
    let (responder, _seen) = answer_all_tools(&daemon).await;
    let session = daemon
        .mcp()
        .start_session("http", Some("agent-large"))
        .expect("the recording store is available");
    let text = "x".repeat(70_000);
    let typed = tools_call_in(
        &daemon,
        Some(&session),
        "browser_type",
        json!({ "target": "#input", "text": text }),
    )
    .await;
    assert!(
        !is_error(&typed),
        "unexpected error: {}",
        content_text(&typed)
    );
    assert!(daemon.flush_recording().await, "the recorder flushes");
    let steps = tools_call(
        &daemon,
        "session_steps",
        json!({ "session_id": session.session_id }),
    )
    .await;
    assert!(
        !is_error(&steps),
        "unexpected error: {}",
        content_text(&steps)
    );
    let steps = payload(&steps);
    let step = steps
        .get("steps")
        .and_then(Value::as_array)
        .and_then(|steps| steps.first())
        .expect("one step");
    assert_eq!(step.get("args"), Some(&Value::Null));
    assert_eq!(
        step.get("args_truncated").and_then(Value::as_bool),
        Some(true)
    );
    assert!(
        step.get("args_bytes").and_then(Value::as_u64).unwrap_or(0) > 65_536,
        "the serialized size is reported: {step}"
    );
    assert_eq!(
        step.get("result")
            .and_then(|result| result.get("typed"))
            .and_then(Value::as_bool),
        Some(true),
        "the result stays complete"
    );
    assert_eq!(
        step.get("result_truncated").and_then(Value::as_bool),
        Some(false)
    );

    responder.abort();
    daemon.shutdown().await;
}

#[tokio::test]
async fn replay_dry_run_plans_and_skips_control_tools() {
    let daemon = start_daemon(5_000).await;
    let (responder, _seen) = answer_all_tools(&daemon).await;
    let session = daemon
        .mcp()
        .start_session("http", Some("agent-replay"))
        .expect("the recording store is available");
    let listed = tools_call_in(&daemon, Some(&session), "browser_list", json!({})).await;
    assert!(
        !is_error(&listed),
        "unexpected error: {}",
        content_text(&listed)
    );
    let clicked = tools_call_in(
        &daemon,
        Some(&session),
        "browser_click",
        json!({ "target": "@e1", "tab_id": "7" }),
    )
    .await;
    assert!(
        !is_error(&clicked),
        "unexpected error: {}",
        content_text(&clicked)
    );
    let closed = tools_call_in(
        &daemon,
        Some(&session),
        "tab_close",
        json!({ "tab_id": "7" }),
    )
    .await;
    assert!(
        !is_error(&closed),
        "unexpected error: {}",
        content_text(&closed)
    );
    assert!(daemon.flush_recording().await, "the recorder flushes");

    let replay = tools_call(
        &daemon,
        "session_replay",
        json!({ "session_id": session.session_id }),
    )
    .await;
    assert!(
        !is_error(&replay),
        "unexpected error: {}",
        content_text(&replay)
    );
    let replay = payload(&replay);
    assert_eq!(replay.get("mode").and_then(Value::as_str), Some("dry_run"));
    assert_eq!(replay.get("total").and_then(Value::as_i64), Some(3));
    assert_eq!(
        replay
            .get("executed")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );
    let planned = replay
        .get("planned")
        .and_then(Value::as_array)
        .expect("the plan");
    assert_eq!(planned.len(), 2);
    assert_eq!(
        planned[0].get("tool").and_then(Value::as_str),
        Some("browser_click")
    );
    assert!(
        planned[0]
            .get("args")
            .and_then(|args| args.get("tab_id"))
            .is_none(),
        "replayed arguments drop tab_id: {planned:?}"
    );
    assert_eq!(
        planned[1].get("tool").and_then(Value::as_str),
        Some("tab_close")
    );
    let skipped = replay
        .get("skipped")
        .and_then(Value::as_array)
        .expect("the skip list");
    assert_eq!(skipped.len(), 1);
    assert_eq!(
        skipped[0].get("tool").and_then(Value::as_str),
        Some("browser_list")
    );
    assert_eq!(
        skipped[0].get("reason").and_then(Value::as_str),
        Some("control_tool")
    );

    responder.abort();
    daemon.shutdown().await;
}

#[tokio::test]
async fn replay_live_executes_skips_tab_tools_and_stops_on_error() {
    let daemon = start_daemon(5_000).await;
    let (responder, seen) = answer_with_failures(&daemon, "edge:default").await;
    let session = daemon
        .mcp()
        .start_session("http", Some("agent-live"))
        .expect("the recording store is available");
    let first = tools_call_in(
        &daemon,
        Some(&session),
        "browser_click",
        json!({ "target": "@e1", "tab_id": "7" }),
    )
    .await;
    assert!(
        !is_error(&first),
        "unexpected error: {}",
        content_text(&first)
    );
    let closed = tools_call_in(
        &daemon,
        Some(&session),
        "tab_close",
        json!({ "tab_id": "7" }),
    )
    .await;
    assert!(
        !is_error(&closed),
        "unexpected error: {}",
        content_text(&closed)
    );
    let failure = tools_call_in(
        &daemon,
        Some(&session),
        "browser_click",
        json!({ "target": "@bad" }),
    )
    .await;
    assert!(is_error(&failure), "the extension reported NOT_FOUND");
    assert!(daemon.flush_recording().await, "the recorder flushes");

    let replay = tools_call(
        &daemon,
        "session_replay",
        json!({ "session_id": session.session_id, "mode": "live" }),
    )
    .await;
    assert!(
        !is_error(&replay),
        "tool failures during replay are data: {}",
        content_text(&replay)
    );
    let replay = payload(&replay);
    assert_eq!(replay.get("mode").and_then(Value::as_str), Some("live"));
    assert_eq!(replay.get("total").and_then(Value::as_i64), Some(3));
    assert_eq!(
        replay
            .get("planned")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(2)
    );
    let skipped = replay
        .get("skipped")
        .and_then(Value::as_array)
        .expect("the skip list");
    assert_eq!(
        skipped[0].get("reason").and_then(Value::as_str),
        Some("tab_tool")
    );
    let executed = replay
        .get("executed")
        .and_then(Value::as_array)
        .expect("the executions");
    assert_eq!(executed.len(), 2);
    assert_eq!(executed[0].get("ok").and_then(Value::as_bool), Some(true));
    assert_eq!(executed[1].get("ok").and_then(Value::as_bool), Some(false));
    assert_eq!(
        executed[1].get("error_code").and_then(Value::as_str),
        Some("NOT_FOUND")
    );
    assert_eq!(replay.get("stopped_at").and_then(Value::as_i64), Some(3));
    assert_eq!(
        replay.get("error_code").and_then(Value::as_str),
        Some("NOT_FOUND")
    );
    let replay_session_id = replay
        .get("replay_session_id")
        .and_then(Value::as_str)
        .expect("the replay session id")
        .to_string();
    assert!(
        replay_session_id.starts_with("s-"),
        "unexpected replay id: {replay_session_id}"
    );

    assert!(daemon.flush_recording().await, "the recorder flushes");
    let replayed_steps = tools_call(
        &daemon,
        "session_steps",
        json!({ "session_id": replay_session_id }),
    )
    .await;
    assert!(
        !is_error(&replayed_steps),
        "unexpected error: {}",
        content_text(&replayed_steps)
    );
    let replayed_steps = payload(&replayed_steps);
    assert_eq!(
        replayed_steps.get("total_steps").and_then(Value::as_i64),
        Some(2)
    );
    let replayed = replayed_steps
        .get("steps")
        .and_then(Value::as_array)
        .expect("the replayed steps");
    assert!(
        replayed[0]
            .get("args")
            .and_then(|args| args.get("tab_id"))
            .is_none(),
        "replayed arguments run against the active tab"
    );
    let commands = recorded(&seen);
    let replay_clicks: Vec<&SeenCommand> = commands
        .iter()
        .filter(|command| command.tool == "browser_click")
        .collect();
    assert_eq!(replay_clicks.len(), 4, "two recorded and two replayed");
    assert!(replay_clicks[2].args.get("tab_id").is_none());
    assert!(replay_clicks[3].args.get("tab_id").is_none());

    responder.abort();
    daemon.shutdown().await;
}

#[tokio::test]
async fn unknown_sessions_are_not_found_errors() {
    let daemon = start_daemon(1_000).await;
    for tool in ["session_steps", "session_replay", "session_export"] {
        let result = tools_call(&daemon, tool, json!({ "session_id": "s-missing" })).await;
        assert!(is_error(&result), "{tool} must fail for an unknown session");
        let text = content_text(&result);
        assert!(
            text.starts_with("NOT_FOUND: "),
            "unexpected text for {tool}: {text}"
        );
        assert!(
            text.contains("\"session_id\":\"s-missing\""),
            "the details name the session for {tool}: {text}"
        );
    }
    daemon.shutdown().await;
}

#[tokio::test]
async fn database_override_is_created_and_doctor_reports_it() {
    let db_path = temporary_database();
    let daemon = start_daemon_at(1_000, db_path.clone()).await;
    assert_eq!(daemon.db_path(), db_path.as_path());
    assert!(db_path.exists(), "the daemon creates the database file");
    let health = daemon.health();
    let recording = health.get("recording").expect("recording health");
    assert_eq!(
        recording.get("db_path").and_then(Value::as_str),
        Some(db_path.to_string_lossy().as_ref())
    );
    assert_eq!(
        recording.get("schema_version").and_then(Value::as_i64),
        Some(1)
    );
    assert_eq!(recording.get("sessions").and_then(Value::as_i64), Some(0));
    assert_eq!(recording.get("steps").and_then(Value::as_i64), Some(0));

    let report = corvusd::doctor::inspect_with_db(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        daemon.ws_addr().port(),
        daemon.http_addr().port(),
        Some(db_path.clone()),
    )
    .await;
    let text = corvusd::doctor::render(&report);
    assert!(
        text.contains("daemon: running"),
        "unexpected report: {text}"
    );
    assert!(
        text.contains(&format!("db: {}", db_path.display())),
        "unexpected report: {text}"
    );
    assert!(text.contains("schema: 1"), "unexpected report: {text}");
    assert!(text.contains("sessions: 0"), "unexpected report: {text}");
    assert!(text.contains("steps: 0"), "unexpected report: {text}");
    assert!(text.contains("uptime: "), "unexpected report: {text}");
    assert!(text.contains("tool calls: "), "unexpected report: {text}");

    daemon.shutdown().await;
}

#[tokio::test]
async fn http_sessions_record_tools_and_close_on_delete() {
    let daemon = start_daemon(5_000).await;
    let (responder, _seen) = answer_all_tools(&daemon).await;
    let initialize = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "http-agent", "version": "0.0.0" } },
    });
    let (headers, _body) =
        http_request(daemon.http_addr(), "POST", "/mcp", &initialize.to_string()).await;
    let session_id = header_value(&headers, "mcp-session-id").expect("a session header");

    let call = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": { "name": "browser_click", "arguments": { "target": "@e1" } },
    });
    let (headers, body) = http_request_with(
        daemon.http_addr(),
        "POST",
        "/mcp",
        &call.to_string(),
        &[("Mcp-Session-Id", session_id.as_str())],
    )
    .await;
    assert!(
        headers.starts_with("HTTP/1.1 200"),
        "unexpected status line: {headers}"
    );
    let answer: Value = serde_json::from_str(&body).expect("decode the tools/call answer");
    assert_eq!(
        answer
            .get("result")
            .and_then(|result| result.get("isError"))
            .and_then(Value::as_bool),
        Some(false),
        "the call failed: {body}"
    );
    let sessions = daemon
        .store()
        .list_sessions(10, None)
        .expect("the sessions are readable");
    let recorded = sessions
        .iter()
        .find(|session| session.id == session_id)
        .expect("the HTTP session was recorded");
    assert_eq!(recorded.transport, "http");
    assert_eq!(recorded.agent.as_deref(), Some("http-agent"));
    assert_eq!(recorded.steps, 1);
    assert_eq!(recorded.errors, 0);
    assert_eq!(recorded.status, "active");

    let (headers, _body) = http_request_with(
        daemon.http_addr(),
        "DELETE",
        "/mcp",
        "",
        &[("Mcp-Session-Id", session_id.as_str())],
    )
    .await;
    assert!(
        headers.starts_with("HTTP/1.1 200"),
        "unexpected delete status: {headers}"
    );
    let sessions = daemon
        .store()
        .list_sessions(10, None)
        .expect("the sessions are readable");
    let closed = sessions
        .iter()
        .find(|session| session.id == session_id)
        .expect("the HTTP session still exists");
    assert_eq!(closed.status, "closed");
    assert!(closed.ended_at.is_some(), "the session has an end time");

    responder.abort();
    daemon.shutdown().await;
}

#[tokio::test]
async fn metrics_tool_reports_counters_and_logs_tail_reads_events() {
    let daemon = start_daemon(5_000).await;
    let (responder, _seen) = answer_with_failures(&daemon, "edge:default").await;
    let session = daemon
        .mcp()
        .start_session("stdio", Some("metrics-agent"))
        .expect("the recording store is available");
    let success = tools_call_in(
        &daemon,
        Some(&session),
        "browser_click",
        json!({ "target": "@e1" }),
    )
    .await;
    assert!(
        !is_error(&success),
        "unexpected error: {}",
        content_text(&success)
    );
    let since = corvus_record::now_millis();
    let failure = tools_call_in(
        &daemon,
        Some(&session),
        "browser_click",
        json!({ "target": "@bad" }),
    )
    .await;
    assert!(is_error(&failure), "the extension reported NOT_FOUND");

    let metrics = tools_call(&daemon, "metrics", json!({})).await;
    assert!(
        !is_error(&metrics),
        "unexpected error: {}",
        content_text(&metrics)
    );
    let metrics = payload(&metrics);
    assert_eq!(metrics.get("protocol").and_then(Value::as_u64), Some(1));
    assert_eq!(
        metrics.get("browsers_connected").and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        metrics["by_tool"]["browser_click"]["calls"].as_u64(),
        Some(2)
    );
    assert_eq!(
        metrics["by_tool"]["browser_click"]["errors"].as_u64(),
        Some(1)
    );
    assert_eq!(metrics["errors_by_code"]["NOT_FOUND"].as_u64(), Some(1));
    assert!(metrics.get("uptime_ms").and_then(Value::as_u64).is_some());
    assert!(metrics.get("inflight").and_then(Value::as_u64).is_some());
    assert!(metrics
        .get("sessions_total")
        .and_then(Value::as_i64)
        .is_some_and(|sessions| sessions >= 1));
    assert_eq!(
        metrics["recording"]["enabled"].as_bool(),
        Some(true),
        "recording health is part of metrics"
    );
    assert!(metrics["recording"]["db_path"].as_str().is_some());

    let logs = tools_call(
        &daemon,
        "logs_tail",
        json!({ "limit": 1000, "since_ms": since }),
    )
    .await;
    assert!(
        !is_error(&logs),
        "unexpected error: {}",
        content_text(&logs)
    );
    let logs = payload(&logs);
    let entries = logs
        .get("entries")
        .and_then(Value::as_array)
        .expect("the log entries");
    assert!(
        entries.iter().any(|entry| {
            entry["fields"]["tool"].as_str() == Some("browser_click")
                && entry["fields"]["code"].as_str() == Some("NOT_FOUND")
        }),
        "the failed tool call is missing from the log entries: {logs}"
    );
    assert!(logs.get("dropped").and_then(Value::as_u64).is_some());

    let warnings = tools_call(&daemon, "logs_tail", json!({ "level": "warn" })).await;
    assert!(
        !is_error(&warnings),
        "unexpected error: {}",
        content_text(&warnings)
    );
    for entry in payload(&warnings)["entries"]
        .as_array()
        .expect("the warning entries")
    {
        let level = entry
            .get("level")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(
            level == "warn" || level == "error",
            "unexpected level {level}"
        );
    }
    let limited = tools_call(&daemon, "logs_tail", json!({ "limit": 1 })).await;
    let limited = payload(&limited);
    assert!(limited["entries"].as_array().expect("entries").len() <= 1);

    let invalid = tools_call(&daemon, "logs_tail", json!({ "level": "verbose" })).await;
    assert!(is_error(&invalid), "an invalid level is a tool error");
    let text = content_text(&invalid);
    assert!(text.contains("BAD_ARGS"), "unexpected text: {text}");
    assert!(
        text.contains("\"level\":\"verbose\""),
        "unexpected text: {text}"
    );

    responder.abort();
    daemon.shutdown().await;
}

#[tokio::test]
async fn the_http_metrics_endpoint_exposes_prometheus_series() {
    let daemon = start_daemon(5_000).await;
    let (responder, _seen) = answer_with_failures(&daemon, "edge:default").await;
    let success = tools_call(&daemon, "browser_click", json!({ "target": "@e1" })).await;
    assert!(
        !is_error(&success),
        "unexpected error: {}",
        content_text(&success)
    );
    let failure = tools_call(&daemon, "browser_click", json!({ "target": "@bad" })).await;
    assert!(is_error(&failure), "the extension reported NOT_FOUND");

    let (headers, body) = http_request(daemon.http_addr(), "GET", "/metrics", "").await;
    assert!(
        headers.starts_with("HTTP/1.1 200"),
        "unexpected status line: {headers}"
    );
    assert!(
        headers
            .to_ascii_lowercase()
            .contains("content-type: text/plain"),
        "unexpected content type: {headers}"
    );
    for expected in [
        "corvus_uptime_seconds ",
        "corvus_browsers_connected 1",
        "corvus_tool_calls_total{tool=\"browser_click\"} 2",
        "corvus_tool_errors_total{code=\"NOT_FOUND\"} 1",
        "corvus_sessions_total ",
        "corvus_steps_total ",
    ] {
        assert!(body.contains(expected), "missing {expected} in:\n{body}");
    }

    responder.abort();
    daemon.shutdown().await;
}

fn header_value(headers: &str, name: &str) -> Option<String> {
    headers.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name)
            .then(|| value.trim().to_string())
    })
}
