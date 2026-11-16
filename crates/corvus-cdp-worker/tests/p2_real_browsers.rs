//! Real browser proof for the P2 worker actions.
//!
//! This test is ignored by default because it launches local headless
//! browsers. Run it with:
//!
//! ```
//! CORVUS_P2_SESSIONS=10 cargo test -p corvus-cdp-worker --test p2_real_browsers --offline -- --ignored --nocapture
//! ```
//!
//! Each session uses a fresh user-data directory and an ephemeral debugging
//! port (never 9222, 7330 or 7331). The report is written to the system
//! temporary directory unless CORVUS_P2_REPORT names another path.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use corvus_cdp_worker::{serve_worker, WorkerOptions};
use corvus_worker::{read_frame, read_handshake, write_frame, WorkerError, WorkerMessage};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

fn default_chrome() -> String {
    std::env::temp_dir()
        .join("corvus-chrome")
        .join("chrome-win64")
        .join("chrome.exe")
        .display()
        .to_string()
}
const DEFAULT_EDGE: &str = "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe";
const PAGE_HTML: &str = "<!doctype html><html><head><title>P2 Real Proof</title></head>\
<body><h1>P2 real proof</h1><button id=\"go\" aria-label=\"Click me\">Click</button>\
<input id=\"name\" aria-label=\"Name\"></body></html>";

struct Browser {
    child: Child,
    profile: PathBuf,
    endpoint: String,
    port: u16,
}

impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.profile);
    }
}

async fn serve_page() -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind page");
    let port = listener.local_addr().expect("page addr").port();
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buffer = [0u8; 4096];
                let _ = stream.read(&mut buffer).await;
                let body = PAGE_HTML.as_bytes();
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes()).await;
                let _ = stream.write_all(body).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    (format!("http://127.0.0.1:{port}/"), task)
}

// The returned Browser owns the child process and kills/waits for it in Drop.
#[allow(clippy::zombie_processes)]
fn launch(binary: &str, label: &str) -> Browser {
    let profile = std::env::temp_dir().join(format!(
        "corvus-p2-real-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or_default()
    ));
    fs::create_dir_all(&profile).expect("profile");
    let mut child = Command::new(binary)
        .arg("--headless=new")
        .arg("--no-sandbox")
        .arg("--disable-dev-shm-usage")
        .arg("--disable-gpu")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-extensions")
        .arg("--disable-background-networking")
        .arg("--disable-component-update")
        .arg("--disable-sync")
        .arg("--metrics-recording-only")
        .arg("--mute-audio")
        .arg("--hide-scrollbars")
        .arg("--window-size=1280,720")
        .arg("--remote-debugging-port=0")
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("about:blank")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|error| panic!("launch {label}: {error}"));

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(contents) = fs::read_to_string(profile.join("DevToolsActivePort")) {
            let mut lines = contents.lines();
            let port: u16 = lines
                .next()
                .and_then(|value| value.trim().parse().ok())
                .unwrap_or(0);
            let path = lines.next().unwrap_or_default().trim().to_string();
            if port != 0 && path.starts_with("/devtools/browser/") {
                assert!(
                    ![9222u16, 7330, 7331].contains(&port),
                    "ephemeral port collision with a shared port"
                );
                let endpoint = format!("ws://127.0.0.1:{port}{path}");
                return Browser {
                    child,
                    profile,
                    endpoint,
                    port,
                };
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{label} did not expose a debugging endpoint");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

async fn start_worker(
    endpoint: &str,
) -> (DuplexStream, JoinHandle<Result<(), WorkerError>>, Value) {
    let (mut gateway, worker) = tokio::io::duplex(4 * 1024 * 1024);
    let (mut reader, mut writer) = tokio::io::split(worker);
    let serve = tokio::spawn(async move {
        serve_worker(
            &mut reader,
            &mut writer,
            WorkerOptions {
                connect_timeout: Duration::from_secs(10),
                call_timeout: Duration::from_secs(20),
            },
        )
        .await
    });
    let hello = json!({
        "kind": "worker_hello",
        "protocol": 1,
        "worker_id": "w-p2-real",
        "browser_id": "b-p2-real",
        "cdp_ws_endpoint": endpoint,
        "home_dir": "C:/tmp",
        "log_level": "warn"
    });
    write_frame(&mut gateway, &hello).await.expect("hello");
    let ack = tokio::time::timeout(Duration::from_secs(15), read_handshake(&mut gateway))
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
        deadline_ms: 20_000,
    };
    write_frame(gateway, &serde_json::to_value(request).expect("invoke"))
        .await
        .expect("invoke write");
    let frame = tokio::time::timeout(Duration::from_secs(30), read_frame(gateway))
        .await
        .expect("result timeout")
        .expect("result frame");
    assert_eq!(frame["kind"], "result", "unexpected frame {frame}");
    assert_eq!(frame["id"], id.to_string());
    assert_eq!(frame["ok"], true, "invoke {action} failed: {frame}");
    frame.get("value").cloned().expect("value")
}

fn keys(value: &Value, expected: &[&str]) {
    for key in expected {
        assert!(value.get(*key).is_some(), "missing key {key} in {value}");
    }
}

async fn run_session(binary: &str, label: &str, page_url: &str, index: usize) -> Value {
    let browser = launch(binary, &format!("{label}-{index}"));
    let (mut gateway, worker, ack) = start_worker(&browser.endpoint).await;
    assert_eq!(ack["ok"], true, "worker refused: {ack}");
    assert!(ack["browser"]["version"].as_str().is_some());

    let info = invoke(&mut gateway, 1, "system.info", json!({})).await;
    keys(
        &info,
        &[
            "browser_family",
            "browser_version",
            "capabilities",
            "limits",
        ],
    );
    assert_eq!(info["limits"]["max_invoke_bytes"].as_u64(), Some(1_048_576));
    assert_eq!(info["limits"]["max_result_bytes"].as_u64(), Some(8_388_608));

    let listed = invoke(&mut gateway, 2, "tabs.list", json!({})).await;
    keys(&listed, &["tabs", "count"]);
    assert!(listed["count"].as_u64().unwrap_or(0) >= 1);
    let row = &listed["tabs"][0];
    keys(row, &["tab_id", "window_id", "title", "url", "active"]);

    let opened = invoke(
        &mut gateway,
        3,
        "tabs.open",
        json!({ "url": page_url, "active": true }),
    )
    .await;
    keys(&opened, &["tab_id", "window_id", "url", "active"]);
    let tab_id = opened["tab_id"].as_str().expect("tab_id").to_string();
    assert_eq!(opened["active"].as_bool(), Some(true));
    assert_eq!(opened["url"].as_str(), Some(page_url));

    let navigated = invoke(
        &mut gateway,
        4,
        "page.navigate",
        json!({ "url": page_url, "tab_id": tab_id, "wait_until": "load", "timeout_ms": 15000 }),
    )
    .await;
    keys(&navigated, &["tab_id", "url", "status"]);
    assert_eq!(navigated["status"].as_str(), Some("loaded"));
    assert_eq!(navigated["tab_id"].as_str(), Some(tab_id.as_str()));

    let read = invoke(
        &mut gateway,
        5,
        "page.read",
        json!({ "tab_id": tab_id, "mode": "text", "max_chars": 20000 }),
    )
    .await;
    keys(&read, &["tab_id", "url", "mode", "content", "truncated"]);
    assert!(read["content"]
        .as_str()
        .is_some_and(|text| text.contains("P2 real proof")));

    let snapshot = invoke(
        &mut gateway,
        6,
        "page.snapshot",
        json!({ "tab_id": tab_id, "max_nodes": 500, "include_hidden": false }),
    )
    .await;
    keys(&snapshot, &["tab_id", "url", "nodes", "truncated"]);
    let nodes = snapshot["nodes"].as_array().expect("nodes");
    let button = nodes
        .iter()
        .find(|node| node["role"].as_str() == Some("button"))
        .unwrap_or_else(|| panic!("no button in snapshot: {snapshot}"));
    let textbox = nodes
        .iter()
        .find(|node| node["role"].as_str() == Some("textbox"))
        .unwrap_or_else(|| panic!("no textbox in snapshot: {snapshot}"));
    let button_ref = button["ref"].as_str().expect("button ref").to_string();
    let textbox_ref = textbox["ref"].as_str().expect("textbox ref").to_string();
    keys(button, &["ref", "role", "name"]);

    let clicked = invoke(
        &mut gateway,
        7,
        "input.click",
        json!({ "ref": button_ref, "tab_id": tab_id }),
    )
    .await;
    keys(&clicked, &["tab_id", "ref", "clicked"]);
    assert_eq!(clicked["clicked"].as_bool(), Some(true));

    let typed = invoke(
        &mut gateway,
        8,
        "input.type",
        json!({
            "ref": textbox_ref,
            "text": "hello",
            "clear": true,
            "submit": false,
            "tab_id": tab_id
        }),
    )
    .await;
    keys(&typed, &["tab_id", "ref", "typed_chars", "submitted"]);
    assert_eq!(typed["typed_chars"].as_u64(), Some(5));
    assert_eq!(typed["submitted"].as_bool(), Some(false));

    let captured = invoke(
        &mut gateway,
        9,
        "capture.viewport",
        json!({ "tab_id": tab_id, "format": "png", "quality": 80 }),
    )
    .await;
    keys(
        &captured,
        &[
            "tab_id",
            "url",
            "format",
            "mime_type",
            "width",
            "height",
            "data",
        ],
    );
    assert_eq!(captured["format"].as_str(), Some("png"));
    assert_eq!(captured["mime_type"].as_str(), Some("image/png"));
    assert!(captured["width"].as_u64().unwrap_or(0) > 0);
    assert!(captured["height"].as_u64().unwrap_or(0) > 0);
    let data = captured["data"].as_str().expect("capture data");
    assert!(data.starts_with("iVBORw0KGgo"), "capture is not a PNG");

    let closed = invoke(&mut gateway, 10, "tabs.close", json!({ "tab_id": tab_id })).await;
    keys(&closed, &["tab_id", "closed"]);
    assert_eq!(closed["closed"].as_bool(), Some(true));

    let _ = write_frame(&mut gateway, &json!({ "kind": "shutdown" })).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), worker).await;
    drop(gateway);

    json!({
        "session": index,
        "browser": label,
        "binary": binary,
        "port": browser.port,
        "browser_family": info["browser_family"],
        "browser_version": info["browser_version"],
        "actions": [
            "system.info", "tabs.list", "tabs.open", "page.navigate", "page.read",
            "page.snapshot", "input.click", "input.type", "capture.viewport", "tabs.close"
        ],
        "snapshot_nodes": nodes.len(),
        "capture": {
            "format": captured["format"],
            "width": captured["width"],
            "height": captured["height"],
            "base64_bytes": data.len()
        }
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "launches real headless Chrome and Edge"]
async fn twenty_consecutive_real_browser_sessions() {
    let chrome = std::env::var("CORVUS_CHROME_BIN").unwrap_or_else(|_| default_chrome());
    let edge = std::env::var("CORVUS_EDGE_BIN").unwrap_or_else(|_| DEFAULT_EDGE.to_string());
    assert!(Path::new(&chrome).is_file(), "missing Chrome at {chrome}");
    assert!(Path::new(&edge).is_file(), "missing Edge at {edge}");
    let per_browser: usize = std::env::var("CORVUS_P2_SESSIONS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(10);
    assert!(per_browser >= 10, "at least ten sessions per browser");

    let (page_url, page_server) = serve_page().await;
    let mut sessions = Vec::new();
    for index in 1..=per_browser {
        sessions.push(run_session(&chrome, "chrome", &page_url, index).await);
        println!("P2 real session chrome {index}/{per_browser} passed");
    }
    for index in 1..=per_browser {
        sessions.push(run_session(&edge, "edge", &page_url, index).await);
        println!("P2 real session edge {index}/{per_browser} passed");
    }
    page_server.abort();

    assert_eq!(sessions.len(), per_browser * 2);
    let report = json!({
        "generated_at_epoch_seconds": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_secs())
            .unwrap_or_default(),
        "sessions": sessions,
        "session_count": per_browser * 2,
        "page_url": page_url
    });
    let report_path = std::env::var("CORVUS_P2_REPORT").unwrap_or_else(|_| {
        std::env::temp_dir()
            .join("corvus-p2-real-browser-proof.json")
            .to_string_lossy()
            .into_owned()
    });
    fs::write(
        &report_path,
        serde_json::to_vec_pretty(&report).expect("report"),
    )
    .unwrap_or_else(|error| panic!("write report {report_path}: {error}"));
    println!("P2_REAL_EVIDENCE={report_path}");
}
