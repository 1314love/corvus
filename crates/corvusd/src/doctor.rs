//! The doctor report: version, ports and the browsers a daemon holds.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// What the doctor found.
#[derive(Debug, Clone)]
pub struct DoctorReport {
    /// Daemon version.
    pub version: &'static str,
    /// Wire protocol version.
    pub protocol: u16,
    /// WebSocket address that was probed.
    pub ws_addr: SocketAddr,
    /// MCP HTTP address that was probed.
    pub http_addr: SocketAddr,
    /// Whether the WebSocket port accepted a connection.
    pub ws_reachable: bool,
    /// Health document of a running daemon, when one answered.
    pub health: Option<Value>,
}

impl DoctorReport {
    /// Whether a daemon answered the health probe.
    pub fn daemon_running(&self) -> bool {
        self.health.is_some()
    }
}

/// Probe the configured ports and return a report.
pub async fn inspect(bind: IpAddr, ws_port: u16, http_port: u16) -> DoctorReport {
    let ws_addr = SocketAddr::new(bind, ws_port);
    let http_addr = SocketAddr::new(bind, http_port);
    let ws_reachable =
        tokio::time::timeout(Duration::from_millis(700), TcpStream::connect(ws_addr))
            .await
            .map(|result| result.is_ok())
            .unwrap_or(false);
    let health = probe_health(http_addr).await;
    DoctorReport {
        version: env!("CARGO_PKG_VERSION"),
        protocol: corvus_proto::PROTOCOL_VERSION,
        ws_addr,
        http_addr,
        ws_reachable,
        health,
    }
}

/// Render a report for a terminal.
pub fn render(report: &DoctorReport) -> String {
    let mut out = String::new();
    out.push_str("corvus doctor\n");
    out.push_str(&format!("version: {}\n", report.version));
    out.push_str(&format!("protocol: {}\n", report.protocol));
    out.push_str(&format!(
        "websocket: {} ({})\n",
        report.ws_addr,
        if report.ws_reachable {
            "reachable"
        } else {
            "not reachable"
        }
    ));
    out.push_str(&format!(
        "mcp http: http://{}/mcp ({})\n",
        report.http_addr,
        if report.daemon_running() {
            "reachable"
        } else {
            "not reachable"
        }
    ));
    match &report.health {
        None => out.push_str("daemon: not running\nbrowsers: unknown\n"),
        Some(health) => {
            let browsers = health
                .get("browsers")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            out.push_str(&format!("daemon: running ({} connected)\n", browsers.len()));
            if browsers.is_empty() {
                out.push_str("browsers: none connected\n");
            } else {
                out.push_str("browsers:\n");
            }
            for browser in browsers {
                let id = browser
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let profile = browser
                    .get("profile")
                    .and_then(Value::as_str)
                    .unwrap_or("-");
                let version = browser
                    .get("version")
                    .and_then(Value::as_str)
                    .unwrap_or("-");
                let capabilities = browser
                    .get("capabilities")
                    .and_then(Value::as_array)
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(",")
                    })
                    .unwrap_or_default();
                out.push_str(&format!(
                    "  - {id} profile={profile} extension={version} capabilities=[{capabilities}]\n"
                ));
            }
        }
    }
    out
}

async fn probe_health(addr: SocketAddr) -> Option<Value> {
    let mut stream = tokio::time::timeout(Duration::from_millis(700), TcpStream::connect(addr))
        .await
        .ok()?
        .ok()?;
    let request = format!("GET /health HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.ok()?;
    let mut raw = Vec::new();
    tokio::time::timeout(Duration::from_millis(1_500), stream.read_to_end(&mut raw))
        .await
        .ok()?
        .ok()?;
    let text = String::from_utf8_lossy(&raw);
    let (_, body) = text.split_once("\r\n\r\n")?;
    serde_json::from_str(body.trim()).ok()
}
