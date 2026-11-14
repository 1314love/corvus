//! End to end tests: a fake extension speaks the wire protocol to a running
//! daemon, and the MCP surface is driven through JSON-RPC.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use corvus_proto::{Envelope, Hello, HelloAck, Kind};
use corvusd::{Daemon, DaemonConfig, RouterConfig};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn start_daemon(default_timeout_ms: u64) -> Daemon {
    Daemon::start(DaemonConfig {
        bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
        ws_port: 0,
        http_port: 0,
        router: RouterConfig {
            default_timeout_ms,
            ..RouterConfig::default()
        },
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
    let request = json!({
        "jsonrpc": "2.0",
        "id": 11,
        "method": "tools/call",
        "params": { "name": name, "arguments": arguments },
    });
    let answer = daemon
        .mcp()
        .handle_message(&request.to_string())
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
    let mut stream = TcpStream::connect(addr)
        .await
        .expect("connect to the http listener");
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
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
