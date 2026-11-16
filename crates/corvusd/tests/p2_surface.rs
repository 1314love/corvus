//! End-to-end tests for the v2 ten-tool MCP surface.

mod support;

use std::net::SocketAddr;
use std::time::Duration;

use corvus_mcp::surface::PUBLIC_TOOLS;
use corvus_provider::ProviderMessage;
use serde_json::{json, Value};
use support::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const TINY_PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAFgwJ/lK3w5QAAAABJRU5ErkJggg==";

async fn http(
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: &str,
    headers: &[(&str, &str)],
) -> (String, String) {
    let mut stream = TcpStream::connect(addr).await.expect("http connect");
    let extra: String = headers
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
        .expect("http write");
    let mut raw = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut raw))
        .await
        .expect("http response arrives")
        .expect("http read");
    let text = String::from_utf8_lossy(&raw).into_owned();
    match text.split_once("\r\n\r\n") {
        Some((headers, body)) => (headers.to_string(), body.to_string()),
        None => (text, String::new()),
    }
}

fn session_header(headers: &str) -> String {
    for line in headers.lines() {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("mcp-session-id") {
                return value.trim().to_string();
            }
        }
    }
    panic!("no session header in {headers}");
}

async fn initialize(daemon: &corvusd::Daemon) -> (String, Value) {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "p2-test", "version": "0.0.0" },
        },
    });
    let (headers, body) = http(
        daemon.http_addr(),
        "POST",
        "/mcp",
        &request.to_string(),
        &[],
    )
    .await;
    assert!(headers.starts_with("HTTP/1.1 200"), "headers: {headers}");
    let answer: Value = serde_json::from_str(&body).expect("initialize json");
    (session_header(&headers), answer)
}

async fn tools_list(daemon: &corvusd::Daemon, session: &str) -> Vec<Value> {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {},
    });
    let (_, body) = http(
        daemon.http_addr(),
        "POST",
        "/mcp",
        &request.to_string(),
        &[("mcp-session-id", session)],
    )
    .await;
    let answer: Value = serde_json::from_str(&body).expect("tools/list json");
    answer["result"]["tools"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

async fn tool_names(daemon: &corvusd::Daemon, session: &str) -> Vec<String> {
    tools_list(daemon, session)
        .await
        .into_iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_string))
        .collect()
}

async fn wait_for_tools(daemon: &corvusd::Daemon, session: &str, expected: usize) -> Vec<String> {
    for _ in 0..60 {
        let names = tool_names(daemon, session).await;
        if names.len() == expected {
            return names;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("tool list never reached {expected} entries");
}

async fn call_tool(daemon: &corvusd::Daemon, session: &str, name: &str, arguments: Value) -> Value {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": { "name": name, "arguments": arguments },
    });
    let (_, body) = http(
        daemon.http_addr(),
        "POST",
        "/mcp",
        &request.to_string(),
        &[("mcp-session-id", session)],
    )
    .await;
    let answer: Value = serde_json::from_str(&body).expect("tools/call json");
    answer["result"].clone()
}

fn payload(result: &Value) -> Value {
    let text = result["content"][0]["text"]
        .as_str()
        .expect("one text block");
    serde_json::from_str(text).expect("json payload")
}

fn error_code(result: &Value) -> String {
    assert_eq!(result["isError"], true, "result: {result}");
    let text = result["content"][0]["text"].as_str().expect("error text");
    let error: Value = serde_json::from_str(text).expect("error json");
    error["error"]["code"].as_str().expect("code").to_string()
}

fn assert_no_internal_wording(value: &Value) {
    let text = value.to_string().to_ascii_lowercase();
    for word in [
        "provider",
        "extension",
        "plugin",
        "cdp",
        "worker",
        "websocket",
        "channel",
        " ws",
    ] {
        assert!(
            !text
                .split(|character: char| !character.is_ascii_alphanumeric())
                .any(|token| token == word.trim()),
            "the word {word} leaked into {text}"
        );
    }
}

async fn answer(
    socket: &mut Socket,
    session: &mut corvus_provider::ProviderSession,
    expected: &str,
    value: Value,
) -> Value {
    let message = recv_encrypted(socket, session).await;
    let ProviderMessage::Invoke {
        id, action, args, ..
    } = message
    else {
        panic!("expected invoke, got {message:?}");
    };
    assert_eq!(action, expected);
    send_encrypted(
        socket,
        session,
        &ProviderMessage::Result {
            id,
            ok: true,
            value: Some(value),
            error: None,
        },
    )
    .await;
    args
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn surface_starts_with_browser_status_and_follows_provider_lifecycle() {
    let test = start_daemon_with(true).await;
    let (session, initialized) = initialize(&test.daemon).await;
    assert_eq!(
        initialized["result"]["instructions"].as_str(),
        Some("Browser tools appear when a browser session is connected. Use browser_status to see available groups.")
    );
    assert_eq!(
        initialized["result"]["capabilities"]["tools"]["listChanged"].as_bool(),
        Some(false),
        "http cannot push list_changed without sse"
    );
    let names = tool_names(&test.daemon, &session).await;
    assert_eq!(names, vec!["browser_status"]);
    assert_no_internal_wording(&initialized);

    let result = call_tool(&test.daemon, &session, "tab_list", json!({})).await;
    assert_eq!(error_code(&result), "CAPABILITY_UNAVAILABLE");
    assert_no_internal_wording(&result);

    let result = call_tool(&test.daemon, &session, "browser_everything", json!({})).await;
    assert_eq!(error_code(&result), "BAD_ARGS");

    let status = payload(&call_tool(&test.daemon, &session, "browser_status", json!({})).await);
    assert_eq!(status["count"], 0);
    assert_eq!(
        status["hint"].as_str(),
        Some("No browser session is connected. Connect a supported browser and retry.")
    );
    assert_eq!(status["capabilities"].as_array().expect("groups").len(), 4);

    let mut socket = connect(&test.daemon).await;
    let mut provider = establish_with(&mut socket, &test.token, full_manifest(PROVIDER_ID)).await;
    let names = wait_for_tools(&test.daemon, &session, 10).await;
    let expected: Vec<String> = PUBLIC_TOOLS
        .iter()
        .map(|name| (*name).to_string())
        .collect();
    assert_eq!(names, expected);
    let tools = tools_list(&test.daemon, &session).await;
    println!(
        "p2 tools/list full array bytes={} tools={}",
        serde_json::to_vec(&tools).expect("serialize tools").len(),
        tools.len()
    );
    assert_no_internal_wording(&json!(tools));

    let status = payload(&call_tool(&test.daemon, &session, "browser_status", json!({})).await);
    assert_eq!(status["count"], 1);
    assert_eq!(status["browsers"][0]["family"], "edge");
    assert_eq!(status["browsers"][0]["connected"], true);
    for capability in status["capabilities"].as_array().expect("groups") {
        assert_eq!(capability["available"], true, "status: {status}");
    }

    let task = tokio::spawn({
        let addr = test.daemon.http_addr();
        let session_id = session.clone();
        async move {
            let request = json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": { "name": "tab_list", "arguments": {} },
            });
            let (_, body) = http(
                addr,
                "POST",
                "/mcp",
                &request.to_string(),
                &[("mcp-session-id", &session_id)],
            )
            .await;
            let answer: Value = serde_json::from_str(&body).expect("tab_list json");
            answer["result"].clone()
        }
    });
    let args = answer(
        &mut socket,
        &mut provider,
        "tabs.list",
        json!({ "tabs": [{ "tab_id": 9, "window_id": 2, "title": "B", "url": "https://b.test/", "active": false }], "count": 1 }),
    )
    .await;
    assert_eq!(args, json!({}));
    let tabs = payload(&task.await.expect("tab_list join"));
    assert_eq!(tabs["count"], 1);
    assert_eq!(tabs["tabs"][0]["tab_id"], "9");

    let _ = socket.close(None).await;
    drop(socket);
    wait_for_tools(&test.daemon, &session, 1).await;
    test.daemon.shutdown().await;
    let _ = test.home;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn snapshot_refs_are_opaque_and_drive_input_actions() {
    let test = start_daemon_with(true).await;
    let (session, _) = initialize(&test.daemon).await;
    let mut socket = connect(&test.daemon).await;
    let mut provider = establish_with(&mut socket, &test.token, full_manifest(PROVIDER_ID)).await;
    wait_for_tools(&test.daemon, &session, 10).await;

    let snapshot_task = tokio::spawn({
        let addr = test.daemon.http_addr();
        let session_id = session.clone();
        async move {
            let arguments = json!({ "tab_id": "2" });
            let request = json!({
                "jsonrpc": "2.0",
                "id": 5,
                "method": "tools/call",
                "params": { "name": "page_snapshot", "arguments": arguments },
            });
            let (_, body) = http(
                addr,
                "POST",
                "/mcp",
                &request.to_string(),
                &[("mcp-session-id", &session_id)],
            )
            .await;
            let answer: Value = serde_json::from_str(&body).expect("snapshot json");
            answer["result"].clone()
        }
    });
    let args = answer(
        &mut socket,
        &mut provider,
        "page.snapshot",
        json!({
            "tab_id": "2",
            "url": "https://example.test/",
            "nodes": [
                { "ref": "private-node-77", "role": "button", "name": "Go", "bbox": { "x": 1, "y": 2, "width": 3, "height": 4 } }
            ],
            "truncated": false
        }),
    )
    .await;
    assert_eq!(args["max_nodes"], 500);
    let snapshot = payload(&snapshot_task.await.expect("snapshot join"));
    let opaque = snapshot["nodes"][0]["ref"]
        .as_str()
        .expect("opaque ref")
        .to_string();
    assert!(opaque.starts_with("r-"), "ref: {opaque}");
    assert_eq!(opaque.len(), 10);
    assert!(!snapshot.to_string().contains("private-node-77"));
    assert_eq!(snapshot["browser_id"].as_str().expect("browser").len(), 10);

    let click_task = tokio::spawn({
        let addr = test.daemon.http_addr();
        let session_id = session.clone();
        let opaque = opaque.clone();
        async move {
            let request = json!({
                "jsonrpc": "2.0",
                "id": 6,
                "method": "tools/call",
                "params": { "name": "element_click", "arguments": { "ref": opaque, "tab_id": "2" } },
            });
            let (_, body) = http(
                addr,
                "POST",
                "/mcp",
                &request.to_string(),
                &[("mcp-session-id", &session_id)],
            )
            .await;
            let answer: Value = serde_json::from_str(&body).expect("click json");
            answer["result"].clone()
        }
    });
    let args = answer(
        &mut socket,
        &mut provider,
        "input.click",
        json!({ "tab_id": "2", "ref": "private-node-77", "clicked": true }),
    )
    .await;
    assert_eq!(args["ref"], "private-node-77");
    let clicked = payload(&click_task.await.expect("click join"));
    assert_eq!(clicked["clicked"], true);
    assert_eq!(clicked["ref"], opaque);

    let navigate_task = tokio::spawn({
        let addr = test.daemon.http_addr();
        let session_id = session.clone();
        async move {
            let request = json!({
                "jsonrpc": "2.0",
                "id": 7,
                "method": "tools/call",
                "params": { "name": "page_navigate", "arguments": { "url": "https://example.test/next" } },
            });
            let (_, body) = http(
                addr,
                "POST",
                "/mcp",
                &request.to_string(),
                &[("mcp-session-id", &session_id)],
            )
            .await;
            let answer: Value = serde_json::from_str(&body).expect("navigate json");
            answer["result"].clone()
        }
    });
    let _ = answer(
        &mut socket,
        &mut provider,
        "page.navigate",
        json!({ "tab_id": "2", "url": "https://example.test/next", "status": "loaded" }),
    )
    .await;
    let navigated = payload(&navigate_task.await.expect("navigate join"));
    assert_eq!(navigated["status"], "loaded");

    let result = call_tool(
        &test.daemon,
        &session,
        "element_click",
        json!({ "ref": opaque, "tab_id": "2" }),
    )
    .await;
    assert_eq!(error_code(&result), "NOT_FOUND");
    assert_no_internal_wording(&result);

    test.daemon.shutdown().await;
    let _ = test.home;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn screenshots_are_written_and_only_inlined_on_request() {
    let test = start_daemon_with(true).await;
    let (session, _) = initialize(&test.daemon).await;
    let mut socket = connect(&test.daemon).await;
    let mut provider = establish_with(&mut socket, &test.token, full_manifest(PROVIDER_ID)).await;
    wait_for_tools(&test.daemon, &session, 10).await;

    let file_task = tokio::spawn({
        let addr = test.daemon.http_addr();
        let session_id = session.clone();
        async move {
            let request = json!({
                "jsonrpc": "2.0",
                "id": 8,
                "method": "tools/call",
                "params": { "name": "page_screenshot", "arguments": {} },
            });
            let (_, body) = http(
                addr,
                "POST",
                "/mcp",
                &request.to_string(),
                &[("mcp-session-id", &session_id)],
            )
            .await;
            let answer: Value = serde_json::from_str(&body).expect("screenshot json");
            answer["result"].clone()
        }
    });
    let args = answer(
        &mut socket,
        &mut provider,
        "capture.viewport",
        json!({
            "tab_id": "2",
            "url": "https://example.test/",
            "format": "png",
            "mime_type": "image/png",
            "width": 1,
            "height": 1,
            "data": TINY_PNG,
        }),
    )
    .await;
    assert_eq!(args["format"], "png");
    assert!(
        args.get("quality").is_none(),
        "png must not forward a quality argument: {args}"
    );
    let result = file_task.await.expect("screenshot join");
    let metadata = payload(&result);
    assert_eq!(metadata["inline"], false);
    assert_eq!(metadata["bytes"].as_u64(), Some(70));
    assert_eq!(metadata["summary"], "1x1 png 70 bytes");
    assert!(metadata.get("image").is_none());
    assert!(!metadata.to_string().contains(TINY_PNG));
    let path = metadata["path"].as_str().expect("path");
    assert!(path.contains("screenshots"));
    let bytes = tokio::fs::read(path).await.expect("screenshot file");
    assert_eq!(bytes.len(), 70);

    let inline_task = tokio::spawn({
        let addr = test.daemon.http_addr();
        let session_id = session.clone();
        async move {
            let request = json!({
                "jsonrpc": "2.0",
                "id": 9,
                "method": "tools/call",
                "params": { "name": "page_screenshot", "arguments": { "inline": true, "format": "png" } },
            });
            let (_, body) = http(
                addr,
                "POST",
                "/mcp",
                &request.to_string(),
                &[("mcp-session-id", &session_id)],
            )
            .await;
            let answer: Value = serde_json::from_str(&body).expect("inline json");
            answer["result"].clone()
        }
    });
    answer(
        &mut socket,
        &mut provider,
        "capture.viewport",
        json!({
            "tab_id": "2",
            "url": "https://example.test/",
            "format": "png",
            "mime_type": "image/png",
            "width": 1,
            "height": 1,
            "data": TINY_PNG,
        }),
    )
    .await;
    let result = inline_task.await.expect("inline join");
    assert_eq!(result["content"].as_array().expect("content").len(), 2);
    assert_eq!(result["content"][1]["type"], "image");
    assert_eq!(result["content"][1]["data"], TINY_PNG);
    let metadata: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().expect("metadata"))
            .expect("metadata json");
    assert!(
        metadata["image"].get("data").is_none(),
        "metadata: {metadata}"
    );

    let jpeg_task = tokio::spawn({
        let addr = test.daemon.http_addr();
        let session_id = session.clone();
        async move {
            let request = json!({
                "jsonrpc": "2.0",
                "id": 10,
                "method": "tools/call",
                "params": {
                    "name": "page_screenshot",
                    "arguments": { "format": "jpeg", "quality": 72 }
                },
            });
            let (_, body) = http(
                addr,
                "POST",
                "/mcp",
                &request.to_string(),
                &[("mcp-session-id", &session_id)],
            )
            .await;
            let answer: Value = serde_json::from_str(&body).expect("jpeg json");
            answer["result"].clone()
        }
    });
    let args = answer(
        &mut socket,
        &mut provider,
        "capture.viewport",
        json!({
            "tab_id": "2",
            "url": "https://example.test/",
            "format": "jpeg",
            "mime_type": "image/jpeg",
            "width": 1,
            "height": 1,
            "data": TINY_PNG,
        }),
    )
    .await;
    assert_eq!(args["format"], "jpeg");
    assert_eq!(args["quality"], 72);
    let jpeg = payload(&jpeg_task.await.expect("jpeg join"));
    assert_eq!(jpeg["format"], "jpeg");
    assert_eq!(jpeg["summary"], "1x1 jpeg 70 bytes");

    test.daemon.shutdown().await;
    let _ = test.home;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn group_counts_follow_the_advertised_capabilities() {
    let test = start_daemon_with(true).await;
    let (session, _) = initialize(&test.daemon).await;
    let all = vec![
        capability("system.info", &["info"]),
        capability("tabs", &["list", "open", "close"]),
        capability("page", &["navigate", "read", "snapshot"]),
        capability("input", &["click", "type"]),
        capability("capture", &["viewport"]),
    ];
    let cases: Vec<(Vec<corvus_provider::Capability>, usize)> = vec![
        (all.clone(), 10),
        (
            all.iter()
                .filter(|cap| cap.id != "capture")
                .cloned()
                .collect(),
            9,
        ),
        (
            all.iter()
                .filter(|cap| cap.id != "input")
                .cloned()
                .collect(),
            8,
        ),
        (
            all.iter().filter(|cap| cap.id != "tabs").cloned().collect(),
            7,
        ),
        (
            all.iter()
                .filter(|cap| cap.id != "page" && cap.id != "input")
                .cloned()
                .collect(),
            5,
        ),
        (
            all.iter()
                .filter(|cap| cap.id == "system.info" || cap.id == "capture")
                .cloned()
                .collect(),
            2,
        ),
        (
            all.iter()
                .filter(|cap| cap.id == "system.info")
                .cloned()
                .collect(),
            1,
        ),
    ];

    for (capabilities, expected) in cases {
        let mut socket = connect(&test.daemon).await;
        let provider = establish_with(
            &mut socket,
            &test.token,
            manifest_for(PROVIDER_ID, capabilities),
        )
        .await;
        wait_for_tools(&test.daemon, &session, expected).await;
        if expected == 1 {
            let result = call_tool(&test.daemon, &session, "tab_list", json!({})).await;
            assert_eq!(error_code(&result), "CAPABILITY_UNAVAILABLE");
        }
        let _ = provider;
        let _ = socket.close(None).await;
        drop(socket);
        wait_for_tools(&test.daemon, &session, 1).await;
    }

    test.daemon.shutdown().await;
    let _ = test.home;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn page_navigate_targets_the_requested_tab_and_plain_versions_survive() {
    let test = start_daemon_with(true).await;
    let (session, _) = initialize(&test.daemon).await;
    let mut socket = connect(&test.daemon).await;
    let mut provider = establish_with(&mut socket, &test.token, full_manifest(PROVIDER_ID)).await;
    wait_for_tools(&test.daemon, &session, 10).await;

    let status = payload(&call_tool(&test.daemon, &session, "browser_status", json!({})).await);
    assert_eq!(
        status["browsers"][0]["version"], "157.0.0.0",
        "plain provider version must survive status: {status}"
    );
    println!(
        "plain provider version status: {}",
        status["browsers"][0]["version"]
    );

    let tab_list_task = tokio::spawn({
        let addr = test.daemon.http_addr();
        let session_id = session.clone();
        async move {
            let request = json!({
                "jsonrpc": "2.0",
                "id": 11,
                "method": "tools/call",
                "params": { "name": "tab_list", "arguments": {} },
            });
            let (_, body) = http(
                addr,
                "POST",
                "/mcp",
                &request.to_string(),
                &[("mcp-session-id", &session_id)],
            )
            .await;
            let answer: Value = serde_json::from_str(&body).expect("tab_list json");
            answer["result"].clone()
        }
    });
    let args = answer(
        &mut socket,
        &mut provider,
        "tabs.list",
        json!({
            "tabs": [
                { "tab_id": "1", "window_id": 1, "title": "Active", "url": "https://active.test/", "active": true },
                { "tab_id": "2", "window_id": 1, "title": "Second", "url": "https://second.test/", "active": false }
            ],
            "count": 2
        }),
    )
    .await;
    assert_eq!(args, json!({}));
    let tabs = payload(&tab_list_task.await.expect("tab_list join"));
    assert_eq!(tabs["count"], 2);
    assert_eq!(tabs["tabs"][0]["tab_id"], "1");
    assert_eq!(tabs["tabs"][1]["tab_id"], "2");

    let navigate_task = tokio::spawn({
        let addr = test.daemon.http_addr();
        let session_id = session.clone();
        async move {
            let request = json!({
                "jsonrpc": "2.0",
                "id": 12,
                "method": "tools/call",
                "params": {
                    "name": "page_navigate",
                    "arguments": { "url": "https://second.test/next", "tab_id": "2" }
                },
            });
            let (_, body) = http(
                addr,
                "POST",
                "/mcp",
                &request.to_string(),
                &[("mcp-session-id", &session_id)],
            )
            .await;
            let answer: Value = serde_json::from_str(&body).expect("navigate json");
            answer["result"].clone()
        }
    });
    let args = answer(
        &mut socket,
        &mut provider,
        "page.navigate",
        json!({
            "tab_id": "2",
            "url": "https://second.test/next",
            "status": "loaded",
            "title": "Second next"
        }),
    )
    .await;
    assert_eq!(args["tab_id"], "2", "provider args: {args}");
    assert_eq!(args["url"], "https://second.test/next");
    let navigated = payload(&navigate_task.await.expect("navigate join"));
    assert_eq!(navigated["tab_id"], "2");
    assert_eq!(navigated["title"], "Second next");
    println!(
        "two-tab navigate requested=2 provider_arg={} result={}",
        args["tab_id"], navigated["tab_id"]
    );
    assert_no_internal_wording(&navigated);

    test.daemon.shutdown().await;
    let _ = test.home;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_browsers_require_a_choice_and_route_to_the_named_one() {
    let test = start_daemon_with(true).await;
    let (session, _) = initialize(&test.daemon).await;
    let second_id = "provider-22222222-2222-4222-8222-222222222222";
    let mut first_socket = connect(&test.daemon).await;
    let mut first =
        establish_with(&mut first_socket, &test.token, full_manifest(PROVIDER_ID)).await;
    let mut second_socket = connect(&test.daemon).await;
    let mut second =
        establish_with(&mut second_socket, &test.token, full_manifest(second_id)).await;
    let names = wait_for_tools(&test.daemon, &session, 10).await;
    assert_eq!(names.len(), 10);

    let status = payload(&call_tool(&test.daemon, &session, "browser_status", json!({})).await);
    assert_eq!(status["count"], 2);
    let mut ids: Vec<String> = status["browsers"]
        .as_array()
        .expect("browsers")
        .iter()
        .filter_map(|browser| browser["browser_id"].as_str().map(str::to_string))
        .collect();
    ids.sort();
    assert_eq!(ids.len(), 2);

    let result = call_tool(&test.daemon, &session, "tab_list", json!({})).await;
    assert_eq!(error_code(&result), "AMBIGUOUS_BROWSER");
    assert_no_internal_wording(&result);

    let chosen = status["browsers"]
        .as_array()
        .expect("browsers")
        .iter()
        .find(|browser| browser["browser_id"] != ids[0])
        .and_then(|browser| browser["browser_id"].as_str())
        .expect("second browser id")
        .to_string();
    let first_target = ids[0] == chosen;
    let task = tokio::spawn({
        let addr = test.daemon.http_addr();
        let session_id = session.clone();
        let chosen = chosen.clone();
        async move {
            let request = json!({
                "jsonrpc": "2.0",
                "id": 10,
                "method": "tools/call",
                "params": { "name": "tab_list", "arguments": { "browser_id": chosen } },
            });
            let (_, body) = http(
                addr,
                "POST",
                "/mcp",
                &request.to_string(),
                &[("mcp-session-id", &session_id)],
            )
            .await;
            let answer: Value = serde_json::from_str(&body).expect("tab_list json");
            answer["result"].clone()
        }
    });
    let (selected_socket, selected_provider) = if first_target {
        (&mut first_socket, &mut first)
    } else {
        (&mut second_socket, &mut second)
    };
    answer(
        selected_socket,
        selected_provider,
        "tabs.list",
        json!({ "tabs": [{ "tab_id": 4, "window_id": 1, "title": "Chosen", "url": "https://chosen.test/", "active": true }], "count": 1 }),
    )
    .await;
    let tabs = payload(&task.await.expect("tab_list join"));
    assert_eq!(tabs["browser_id"], chosen);
    assert_eq!(tabs["count"], 1);

    test.daemon.shutdown().await;
    let _ = (first, second, test.home);
}
