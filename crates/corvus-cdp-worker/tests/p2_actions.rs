//! End-to-end P2 worker tests against a local mock CDP endpoint.
//!
//! The mock speaks the small subset of the browser-level protocol that the
//! worker uses. It keeps one long-lived WebSocket, attaches flattened
//! sessions, and answers the DOM, page, input and capture calls.

use std::collections::HashMap;
use std::time::Duration;

use corvus_cdp_worker::{serve_worker, WorkerOptions};
use corvus_worker::{read_frame, read_handshake, write_frame, WorkerError, WorkerMessage};
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::io::DuplexStream;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

const PNG_1X1: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Wl2n1cAAAAASUVORK5CYII=";

type Ws = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

struct MockState {
    socket: Ws,
    current_url: String,
    next_target: u32,
    sessions: HashMap<String, String>,
    announced: bool,
}

async fn serve_mock_cdp() -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
    let port = listener.local_addr().expect("mock addr").port();
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept mock");
        let socket = tokio_tungstenite::accept_async(stream)
            .await
            .expect("websocket mock");
        let mut state = MockState {
            socket,
            current_url: "https://example.test/".into(),
            next_target: 1,
            sessions: HashMap::new(),
            announced: false,
        };
        while let Some(frame) = state.socket.next().await {
            let Ok(frame) = frame else { break };
            let Message::Text(text) = frame else { continue };
            let Ok(message) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            if state.handle(message).await.is_err() {
                break;
            }
        }
    });
    (format!("ws://127.0.0.1:{port}/devtools/browser/mock"), task)
}

impl MockState {
    async fn send_json(&mut self, value: Value) -> Result<(), ()> {
        self.socket
            .send(Message::Text(value.to_string().into()))
            .await
            .map_err(|_| ())
    }

    async fn respond(
        &mut self,
        id: Option<u64>,
        session: Option<String>,
        result: Value,
    ) -> Result<(), ()> {
        let Some(id) = id else { return Ok(()) };
        let mut response = json!({ "id": id, "result": result });
        if let Some(session) = session {
            response["sessionId"] = json!(session);
        }
        self.send_json(response).await
    }

    async fn target_created(&mut self, target_id: &str, url: &str) -> Result<(), ()> {
        self.send_json(json!({
            "method": "Target.targetCreated",
            "params": {
                "targetInfo": {
                    "targetId": target_id,
                    "type": "page",
                    "title": "Mock page",
                    "url": url,
                    "attached": false
                }
            }
        }))
        .await
    }

    async fn handle(&mut self, message: Value) -> Result<(), ()> {
        let id = message.get("id").and_then(Value::as_u64);
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
        let session = message
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_string);
        match method.as_str() {
            "Browser.getVersion" => {
                self.respond(
                    id,
                    session,
                    json!({
                        "product": "MockChrome/157.0.0.0",
                        "protocolVersion": "1.3",
                        "userAgent": "Mock"
                    }),
                )
                .await?
            }
            "Target.setDiscoverTargets" => {
                self.respond(id, session, json!({})).await?;
                if !self.announced {
                    self.announced = true;
                    self.target_created("T1", "https://example.test/").await?;
                }
            }
            "Browser.setDownloadBehavior" => {
                self.respond(id, session, json!({})).await?
            }
            "Target.attachToTarget" => {
                let target = params
                    .get("targetId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let next_session = self.sessions.len() + 1;
                let session_id = self
                    .sessions
                    .entry(target)
                    .or_insert_with(|| format!("SESSION-{next_session}"))
                    .clone();
                self.respond(id, session, json!({ "sessionId": session_id }))
                    .await?
            }
            "Target.createTarget" => {
                self.next_target += 1;
                let target = format!("T{}", self.next_target);
                self.target_created(&target, "about:blank").await?;
                self.respond(id, session, json!({ "targetId": target })).await?
            }
            "Target.closeTarget" => {
                self.respond(id, session, json!({ "success": true })).await?
            }
            "Target.activateTarget" => self.respond(id, session, json!({})).await?,
            "Browser.getWindowForTarget" => {
                self.respond(
                    id,
                    session,
                    json!({
                        "windowId": 1,
                        "bounds": { "left": 0, "top": 0, "width": 1280, "height": 720 }
                    }),
                )
                .await?
            }
            "Runtime.evaluate" => {
                let expression = params
                    .get("expression")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let value = self.evaluate(expression);
                self.respond(id, session, json!({ "result": { "type": "object", "value": value } }))
                    .await?
            }
            "Runtime.callFunctionOn" => {
                let function = params
                    .get("functionDeclaration")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let value = if function.contains("elementFromPoint")
                    || function.contains("this.focus")
                {
                    json!(true)
                } else {
                    json!({
                        "tag": "button",
                        "role": "button",
                        "name": "Submit",
                        "disabled": false,
                        "readOnly": false,
                        "visible": true,
                        "type": "",
                        "rect": { "x": 10, "y": 20, "width": 20, "height": 10 }
                    })
                };
                self.respond(
                    id,
                    session,
                    json!({ "result": { "type": "object", "value": value } }),
                )
                .await?
            }
            "DOM.resolveNode" => {
                let backend = params
                    .get("backendNodeId")
                    .and_then(Value::as_i64)
                    .unwrap_or_default();
                if backend == 9999 {
                    self.respond(id, session, json!({})).await?
                } else {
                    self.respond(
                        id,
                        session,
                        json!({ "object": { "objectId": format!("obj-{backend}") } }),
                    )
                    .await?
                }
            }
            "DOM.getBoxModel" => {
                self.respond(
                    id,
                    session,
                    json!({ "model": { "content": [10.0, 20.0, 30.0, 20.0, 30.0, 40.0, 10.0, 40.0] } }),
                )
                .await?
            }
            "DOM.focus" | "DOM.scrollIntoViewIfNeeded" => {
                self.respond(id, session, json!({})).await?
            }
            "Page.navigate" => {
                self.current_url = params
                    .get("url")
                    .and_then(Value::as_str)
                    .unwrap_or("about:blank")
                    .to_string();
                self.respond(id, session, json!({ "frameId": "MAIN" })).await?
            }
            "Page.getFrameTree" => {
                self.respond(
                    id,
                    session,
                    json!({ "frameTree": { "frame": { "id": "MAIN", "url": self.current_url } } }),
                )
                .await?
            }
            "Page.captureScreenshot" => {
                self.respond(id, session, json!({ "data": PNG_1X1 })).await?
            }
            "DOMSnapshot.captureSnapshot" => {
                self.respond(
                    id,
                    session,
                    json!({ "documents": [], "strings": [] }),
                )
                .await?
            }
            "Accessibility.getFullAXTree" => {
                self.respond(
                    id,
                    session,
                    json!({
                        "nodes": [
                            {
                                "nodeId": "1",
                                "role": { "value": "button" },
                                "name": { "value": "Submit" },
                                "backendDOMNodeId": 13,
                                "properties": []
                            },
                            {
                                "nodeId": "2",
                                "role": { "value": "textbox" },
                                "name": { "value": "Name" },
                                "value": { "value": "Ada" },
                                "backendDOMNodeId": 14,
                                "properties": []
                            }
                        ]
                    }),
                )
                .await?
            }
            "Input.dispatchMouseEvent" | "Input.dispatchKeyEvent" | "Input.insertText" => {
                self.respond(id, session, json!({})).await?
            }
            _ => self.respond(id, session, json!({})).await?,
        }
        Ok(())
    }

    fn evaluate(&self, expression: &str) -> Value {
        if expression == "location.href" {
            return json!(self.current_url);
        }
        if expression.contains("window.innerWidth") {
            return json!({
                "url": self.current_url,
                "title": "Mock page",
                "width": 1280,
                "height": 720,
                "scrollX": 0,
                "scrollY": 0
            });
        }
        if expression.contains("readyState") {
            let ready = if self.current_url.contains("timeout") {
                "loading"
            } else {
                "complete"
            };
            return json!({
                "readyState": ready,
                "url": self.current_url,
                "title": "Mock page"
            });
        }
        if expression.contains("document.querySelector") {
            if expression.contains(".missing") {
                return Value::Null;
            }
            if expression.contains(".long") {
                return json!({ "url": self.current_url, "content": "x".repeat(5_000) });
            }
            return json!({ "url": self.current_url, "content": "selected content" });
        }
        if expression.contains("innerText") {
            return json!({ "url": self.current_url, "content": "Mock body text" });
        }
        if expression.contains("outerHTML") {
            return json!({ "url": self.current_url, "content": "<html><body>Mock</body></html>" });
        }
        Value::Null
    }
}

async fn start_worker(
    endpoint: &str,
) -> (DuplexStream, JoinHandle<Result<(), WorkerError>>, Value) {
    let (mut gateway, worker) = tokio::io::duplex(1024 * 1024);
    let (mut reader, mut writer) = tokio::io::split(worker);
    let serve = tokio::spawn(async move {
        serve_worker(
            &mut reader,
            &mut writer,
            WorkerOptions {
                connect_timeout: Duration::from_secs(2),
                call_timeout: Duration::from_secs(5),
            },
        )
        .await
    });
    let hello = json!({
        "kind": "worker_hello",
        "protocol": 1,
        "worker_id": "w-p2-test",
        "browser_id": "b-p2-test",
        "cdp_ws_endpoint": endpoint,
        "home_dir": "C:/tmp",
        "log_level": "info"
    });
    write_frame(&mut gateway, &hello).await.expect("hello");
    let ack = tokio::time::timeout(Duration::from_secs(3), read_handshake(&mut gateway))
        .await
        .expect("ack timeout")
        .expect("ack");
    (gateway, serve, ack)
}

async fn invoke(gateway: &mut DuplexStream, id: u64, action: &str, args: Value) -> Value {
    let request = WorkerMessage::Invoke {
        id: id.to_string(),
        action: action.into(),
        args,
        deadline_ms: 5000,
    };
    write_frame(
        gateway,
        &serde_json::to_value(request).expect("invoke json"),
    )
    .await
    .expect("invoke write");
    let frame = tokio::time::timeout(Duration::from_secs(8), read_frame(gateway))
        .await
        .expect("result timeout")
        .expect("result frame");
    assert_eq!(frame["kind"], "result", "unexpected frame {frame}");
    assert_eq!(frame["id"], id.to_string());
    frame
}

fn result_value(frame: &Value) -> &Value {
    assert_eq!(frame["ok"], true, "invoke failed: {frame}");
    frame.get("value").expect("result value")
}

fn result_error(frame: &Value) -> (&str, &str) {
    assert_eq!(frame["ok"], false, "invoke unexpectedly succeeded: {frame}");
    let error = frame.get("error").expect("error body");
    let code = error
        .get("code")
        .and_then(Value::as_str)
        .expect("error code");
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .expect("error message");
    assert_safe_message(message);
    (code, message)
}

fn assert_safe_message(message: &str) {
    const FORBIDDEN: &[&str] = &[
        "provider",
        "extension",
        "plugin",
        "cdp",
        "worker",
        "websocket",
        "ws",
        "channel",
    ];
    let words: Vec<&str> = message
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    for forbidden in FORBIDDEN {
        assert!(
            !words
                .iter()
                .any(|word| word.eq_ignore_ascii_case(forbidden)),
            "error message exposes {forbidden}: {message}"
        );
    }
}

#[tokio::test]
async fn worker_ack_lists_p2_groups_and_system_info() {
    let (endpoint, mock) = serve_mock_cdp().await;
    let (mut gateway, worker, ack) = start_worker(&endpoint).await;
    assert_eq!(ack["kind"], "worker_ack");
    assert_eq!(ack["ok"], true);
    assert_eq!(
        ack["capabilities"],
        json!([
            "tabs",
            "page",
            "input",
            "capture",
            "tabs.list",
            "tabs.open",
            "tabs.close",
            "page.navigate",
            "page.read",
            "page.snapshot",
            "input.click",
            "input.type",
            "capture.viewport"
        ])
    );
    assert_eq!(ack["browser"]["version"], "MockChrome/157.0.0.0");

    let info = invoke(&mut gateway, 1, "system.info", json!({})).await;
    let value = result_value(&info);
    assert_eq!(value["browser_family"], "MockChrome");
    assert_eq!(value["browser_version"], "157.0.0.0");
    assert_eq!(
        value["capabilities"],
        json!([
            { "group": "tabs", "actions": ["tabs.list", "tabs.open", "tabs.close"] },
            { "group": "page", "actions": ["page.navigate", "page.read", "page.snapshot"] },
            { "group": "input", "actions": ["input.click", "input.type"] },
            { "group": "capture", "actions": ["capture.viewport"] }
        ])
    );
    assert_eq!(value["limits"]["max_invoke_bytes"], 1_048_576);
    assert_eq!(value["limits"]["max_result_bytes"], 8_388_608);

    worker.abort();
    mock.abort();
}

#[tokio::test]
async fn tabs_list_open_and_close_round_trip() {
    let (endpoint, mock) = serve_mock_cdp().await;
    let (mut gateway, worker, _ack) = start_worker(&endpoint).await;

    let listed = invoke(&mut gateway, 1, "tabs.list", json!({})).await;
    let value = result_value(&listed);
    assert_eq!(value["count"], 1);
    assert_eq!(value["tabs"][0]["tab_id"], "T1");
    assert_eq!(value["tabs"][0]["window_id"], 1);

    let opened = invoke(
        &mut gateway,
        2,
        "tabs.open",
        json!({ "url": "https://example.test/new", "active": true }),
    )
    .await;
    let value = result_value(&opened);
    assert_eq!(value["tab_id"], "T2");
    assert_eq!(value["window_id"], 1);
    assert_eq!(value["url"], "https://example.test/new");
    assert_eq!(value["active"], true);

    let closed = invoke(&mut gateway, 3, "tabs.close", json!({ "tab_id": "T2" })).await;
    assert_eq!(result_value(&closed)["closed"], true);
    let listed = invoke(&mut gateway, 4, "tabs.list", json!({})).await;
    assert_eq!(result_value(&listed)["count"], 1);

    worker.abort();
    mock.abort();
}

#[tokio::test]
async fn page_navigate_read_and_snapshot_return_exact_shapes() {
    let (endpoint, mock) = serve_mock_cdp().await;
    let (mut gateway, worker, _ack) = start_worker(&endpoint).await;

    let navigated = invoke(
        &mut gateway,
        1,
        "page.navigate",
        json!({ "url": "https://example.test/page", "wait_until": "load", "timeout_ms": 4000 }),
    )
    .await;
    let value = result_value(&navigated);
    assert_eq!(value["status"], "loaded");
    assert_eq!(value["tab_id"], "T1");
    assert_eq!(value["url"], "https://example.test/page");

    let text = invoke(&mut gateway, 2, "page.read", json!({ "mode": "text" })).await;
    let value = result_value(&text);
    assert_eq!(value["mode"], "text");
    assert_eq!(value["content"], "Mock body text");
    assert_eq!(value["truncated"], false);

    let html = invoke(
        &mut gateway,
        3,
        "page.read",
        json!({ "mode": "html", "selector": "body", "max_chars": 1000 }),
    )
    .await;
    assert_eq!(result_value(&html)["content"], "selected content");

    let snapshot = invoke(
        &mut gateway,
        4,
        "page.snapshot",
        json!({ "max_nodes": 100, "include_hidden": false }),
    )
    .await;
    let value = result_value(&snapshot);
    assert_eq!(value["tab_id"], "T1");
    assert!(value["nodes"]
        .as_array()
        .is_some_and(|nodes| nodes.len() >= 2));
    assert!(value["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .any(|node| node["ref"] == "@c13" && node["role"] == "button"));

    worker.abort();
    mock.abort();
}

#[tokio::test]
async fn input_click_and_type_return_exact_shapes() {
    let (endpoint, mock) = serve_mock_cdp().await;
    let (mut gateway, worker, _ack) = start_worker(&endpoint).await;

    let clicked = invoke(&mut gateway, 1, "input.click", json!({ "ref": "@c13" })).await;
    let value = result_value(&clicked);
    assert_eq!(value["tab_id"], "T1");
    assert_eq!(value["ref"], "@c13");
    assert_eq!(value["clicked"], true);

    let typed = invoke(
        &mut gateway,
        2,
        "input.type",
        json!({ "ref": "@c14", "text": "hello", "clear": true, "submit": true }),
    )
    .await;
    let value = result_value(&typed);
    assert_eq!(value["tab_id"], "T1");
    assert_eq!(value["ref"], "@c14");
    assert_eq!(value["typed_chars"], 5);
    assert_eq!(value["submitted"], true);

    worker.abort();
    mock.abort();
}

#[tokio::test]
async fn capture_viewport_returns_png_dimensions_and_data() {
    let (endpoint, mock) = serve_mock_cdp().await;
    let (mut gateway, worker, _ack) = start_worker(&endpoint).await;

    let captured = invoke(
        &mut gateway,
        1,
        "capture.viewport",
        json!({ "format": "png" }),
    )
    .await;
    let value = result_value(&captured);
    assert_eq!(value["format"], "png");
    assert_eq!(value["mime_type"], "image/png");
    assert_eq!(value["width"], 1);
    assert_eq!(value["height"], 1);
    assert_eq!(value["data"], PNG_1X1);

    worker.abort();
    mock.abort();
}

#[tokio::test]
async fn bad_arguments_and_oversize_input_are_typed() {
    let (endpoint, mock) = serve_mock_cdp().await;
    let (mut gateway, worker, _ack) = start_worker(&endpoint).await;

    let frame = invoke(&mut gateway, 1, "tabs.list", json!({ "unexpected": true })).await;
    let (code, message) = result_error(&frame);
    assert_eq!(code, "BAD_ARGS");
    assert_eq!(message, "unknown argument");
    assert_safe_message(message);

    let long = "x".repeat(20_001);
    let frame = invoke(
        &mut gateway,
        2,
        "input.type",
        json!({ "ref": "@c14", "text": long }),
    )
    .await;
    let (code, message) = result_error(&frame);
    assert_eq!(code, "BAD_ARGS");
    assert!(message.contains("20000"));

    let frame = invoke(
        &mut gateway,
        3,
        "capture.viewport",
        json!({ "format": "webp" }),
    )
    .await;
    assert_eq!(result_error(&frame).0, "BAD_ARGS");

    let frame = invoke(
        &mut gateway,
        4,
        "capture.viewport",
        json!({ "format": "jpeg", "quality": 0 }),
    )
    .await;
    assert_eq!(result_error(&frame).0, "BAD_ARGS");

    worker.abort();
    mock.abort();
}

#[tokio::test]
async fn unknown_and_legacy_actions_are_rejected() {
    let (endpoint, mock) = serve_mock_cdp().await;
    let (mut gateway, worker, _ack) = start_worker(&endpoint).await;

    let frame = invoke(&mut gateway, 1, "tab_list", json!({})).await;
    let (code, message) = result_error(&frame);
    assert_eq!(code, "BAD_ARGS");
    assert!(message.contains("unknown action"));

    worker.abort();
    mock.abort();
}

#[tokio::test]
async fn missing_tab_and_ref_are_not_found() {
    let (endpoint, mock) = serve_mock_cdp().await;
    let (mut gateway, worker, _ack) = start_worker(&endpoint).await;

    let frame = invoke(&mut gateway, 1, "tabs.close", json!({ "tab_id": "T404" })).await;
    assert_eq!(result_error(&frame).0, "NOT_FOUND");

    let frame = invoke(
        &mut gateway,
        2,
        "page.read",
        json!({ "selector": ".missing" }),
    )
    .await;
    assert_eq!(result_error(&frame).0, "NOT_FOUND");

    let frame = invoke(&mut gateway, 3, "input.click", json!({ "ref": "@c9999" })).await;
    assert_eq!(result_error(&frame).0, "NOT_FOUND");

    worker.abort();
    mock.abort();
}

#[tokio::test]
async fn required_fields_are_rejected_for_every_action() {
    let (endpoint, mock) = serve_mock_cdp().await;
    let (mut gateway, worker, _ack) = start_worker(&endpoint).await;

    for (id, action) in [
        (1, "tabs.open"),
        (2, "tabs.close"),
        (3, "page.navigate"),
        (4, "input.click"),
        (5, "input.type"),
    ] {
        let frame = invoke(&mut gateway, id, action, json!({})).await;
        let (code, message) = result_error(&frame);
        assert_eq!(code, "BAD_ARGS", "{action} accepted missing fields");
        assert_safe_message(message);
    }

    worker.abort();
    mock.abort();
}

#[tokio::test]
async fn oversized_results_are_truncated_at_the_requested_limit() {
    let (endpoint, mock) = serve_mock_cdp().await;
    let (mut gateway, worker, _ack) = start_worker(&endpoint).await;

    let frame = invoke(
        &mut gateway,
        1,
        "page.read",
        json!({ "mode": "text", "selector": ".long", "max_chars": 1000 }),
    )
    .await;
    let value = result_value(&frame);
    assert_eq!(value["truncated"], true);
    assert_eq!(value["content"].as_str().map(str::len), Some(1000));

    worker.abort();
    mock.abort();
}

#[tokio::test]
async fn navigation_timeout_is_typed() {
    let (endpoint, mock) = serve_mock_cdp().await;
    let (mut gateway, worker, _ack) = start_worker(&endpoint).await;

    let frame = invoke(
        &mut gateway,
        1,
        "page.navigate",
        json!({ "url": "https://example.test/timeout", "timeout_ms": 1200 }),
    )
    .await;
    let (code, _message) = result_error(&frame);
    assert_eq!(code, "TIMEOUT");

    worker.abort();
    mock.abort();
}
