//! The doctor report: version, ports, recording state and the browsers a
//! daemon holds.

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use corvus_record::Store;
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
    /// Recording database path.
    pub db_path: PathBuf,
    /// Recording schema version, when a database exists.
    pub db_schema_version: Option<i64>,
    /// Recorded sessions, when the database can be read.
    pub sessions: Option<i64>,
    /// Recorded steps, when the database can be read.
    pub steps: Option<i64>,
    /// Daemon uptime in milliseconds, when a daemon answered.
    pub uptime_ms: Option<u64>,
    /// Tool calls counted by the daemon, when it answered.
    pub tool_calls_total: Option<u64>,
    /// Failed tool calls counted by the daemon, when it answered.
    pub tool_errors_total: Option<u64>,
}

impl DoctorReport {
    /// Whether a daemon answered the health probe.
    pub fn daemon_running(&self) -> bool {
        self.health.is_some()
    }
}

/// Probe the configured ports and return a report.
pub async fn inspect(bind: IpAddr, ws_port: u16, http_port: u16) -> DoctorReport {
    inspect_with_db(bind, ws_port, http_port, None).await
}

/// Probe the configured ports and report the recording state of one database.
pub async fn inspect_with_db(
    bind: IpAddr,
    ws_port: u16,
    http_port: u16,
    db_path: Option<PathBuf>,
) -> DoctorReport {
    inspect_with_paths(bind, ws_port, http_port, db_path, None).await
}

/// Probe the configured ports and report the recording state resolved from a
/// Corvus home and an optional database override.
pub async fn inspect_with_home(
    bind: IpAddr,
    ws_port: u16,
    http_port: u16,
    db_path: Option<PathBuf>,
    home: Option<PathBuf>,
) -> DoctorReport {
    inspect_with_paths(bind, ws_port, http_port, db_path, home).await
}

async fn inspect_with_paths(
    bind: IpAddr,
    ws_port: u16,
    http_port: u16,
    db_path: Option<PathBuf>,
    home: Option<PathBuf>,
) -> DoctorReport {
    let ws_addr = SocketAddr::new(bind, ws_port);
    let http_addr = SocketAddr::new(bind, http_port);
    let ws_reachable =
        tokio::time::timeout(Duration::from_millis(700), TcpStream::connect(ws_addr))
            .await
            .map(|result| result.is_ok())
            .unwrap_or(false);
    let health = probe_health(http_addr).await;
    let db_path = crate::home::resolve_database(db_path, home)
        .map(|(_, path)| path)
        .unwrap_or_else(|_| corvus_record::resolve_path(None));
    let mut report = DoctorReport {
        version: env!("CARGO_PKG_VERSION"),
        protocol: corvus_proto::PROTOCOL_VERSION,
        ws_addr,
        http_addr,
        ws_reachable,
        health,
        db_path,
        db_schema_version: None,
        sessions: None,
        steps: None,
        uptime_ms: None,
        tool_calls_total: None,
        tool_errors_total: None,
    };
    if let Some(health) = report.health.as_ref() {
        report.uptime_ms = health.get("uptime_ms").and_then(Value::as_u64);
        report.tool_calls_total = health.get("tool_calls_total").and_then(Value::as_u64);
        report.tool_errors_total = health.get("tool_errors_total").and_then(Value::as_u64);
    }
    if let Some(recording) = report
        .health
        .as_ref()
        .and_then(|health| health.get("recording"))
    {
        report.db_schema_version = recording.get("schema_version").and_then(Value::as_i64);
        report.sessions = recording.get("sessions").and_then(Value::as_i64);
        report.steps = recording.get("steps").and_then(Value::as_i64);
        if let Some(path) = recording.get("db_path").and_then(Value::as_str) {
            report.db_path = PathBuf::from(path);
        }
        return report;
    }
    if let Ok(store) = Store::open_read_only(&report.db_path) {
        report.db_schema_version = Some(store.schema_version());
        report.sessions = store.sessions_total().ok();
        report.steps = store.steps_total().ok();
    }
    report
}

/// Render a report for a terminal.
pub fn render(report: &DoctorReport) -> String {
    let mut out = String::new();
    out.push_str("corvus doctor\n");
    out.push_str(&format!("version: {}\n", report.version));
    out.push_str(&format!("protocol: {}\n", report.protocol));
    out.push_str(&format!(
        "uptime: {}\n",
        report
            .uptime_ms
            .map(|value| format!("{value} ms"))
            .unwrap_or_else(|| "unknown".to_string())
    ));
    out.push_str(&format!(
        "tool calls: {} ({} errors)\n",
        report
            .tool_calls_total
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        report
            .tool_errors_total
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    ));
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
    out.push_str(&format!("db: {}\n", report.db_path.display()));
    out.push_str(&format!(
        "schema: {}\n",
        report
            .db_schema_version
            .map(|value| value.to_string())
            .unwrap_or_else(|| "not initialized".to_string())
    ));
    out.push_str(&format!(
        "sessions: {}\n",
        report
            .sessions
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    ));
    out.push_str(&format!(
        "steps: {}\n",
        report
            .steps
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string())
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
