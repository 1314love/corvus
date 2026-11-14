//! The Corvus daemon: one runtime that owns the WebSocket transport, the
//! command router and the MCP server.

pub mod doctor;
pub mod logs;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use corvus_mcp::{BackendFuture, McpHttpServer, McpServer, Metrics, ToolBackend, ToolCall};
use corvus_proto::{ids, ErrorBody, ErrorCode};
use corvus_record::{
    now_millis, Recorder, SessionContext, SessionRow, StepRecord, StepRow, StepSink, Store,
};
use corvus_runtime::{CommandRequest, Router};

pub use corvus_runtime::RouterConfig;
use corvus_transport::{Registry, WsServer};

/// Default port of the WebSocket transport.
pub const DEFAULT_WS_PORT: u16 = 7330;
/// Default port of the MCP HTTP endpoint.
pub const DEFAULT_HTTP_PORT: u16 = 7331;
/// Default loopback address the daemon binds to.
pub const DEFAULT_BIND: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// Payloads returned to MCP clients are capped at this size.
const STEP_VALUE_CAP: usize = 65_536;
/// Exported JSONL documents are capped at this size.
const EXPORT_CAP: usize = 2_097_152;

/// Tools answered inside the daemon instead of being forwarded to a browser.
const LOCAL_TOOLS: [&str; 7] = [
    "browser_list",
    "session_list",
    "session_steps",
    "session_replay",
    "session_export",
    "metrics",
    "logs_tail",
];

/// Tools that describe or control the recording layer and must not be replayed.
const CONTROL_TOOLS: [&str; 7] = [
    "session_list",
    "session_steps",
    "session_replay",
    "session_export",
    "metrics",
    "logs_tail",
    "browser_list",
];

/// Everything needed to start a daemon.
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// Address the listeners bind to.
    pub bind: IpAddr,
    /// WebSocket port; zero lets the operating system choose.
    pub ws_port: u16,
    /// MCP HTTP port; zero lets the operating system choose.
    pub http_port: u16,
    /// Router tuning.
    pub router: RouterConfig,
    /// Recording database path; when absent the environment or user data
    /// directory is used.
    pub db_path: Option<PathBuf>,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            bind: DEFAULT_BIND,
            ws_port: DEFAULT_WS_PORT,
            http_port: DEFAULT_HTTP_PORT,
            router: RouterConfig::default(),
            db_path: None,
        }
    }
}

/// Executes MCP tool calls by routing them to the browser and answering the
/// recording tools locally.
pub struct RouterBackend {
    router: Arc<Router>,
    registry: Arc<Registry>,
    store: Arc<Store>,
    metrics: Arc<Metrics>,
    sink: StepSink,
}

impl RouterBackend {
    /// Build a backend around a router, the live browser registry and the
    /// recording store.
    pub fn new(
        router: Arc<Router>,
        registry: Arc<Registry>,
        store: Arc<Store>,
        metrics: Arc<Metrics>,
        sink: StepSink,
    ) -> Self {
        Self {
            router,
            registry,
            store,
            metrics,
            sink,
        }
    }
}

impl ToolBackend for RouterBackend {
    fn call(&self, call: ToolCall) -> BackendFuture {
        let router = Arc::clone(&self.router);
        let registry = Arc::clone(&self.registry);
        let store = Arc::clone(&self.store);
        let metrics = Arc::clone(&self.metrics);
        let sink = self.sink.clone();
        let started = Instant::now();
        Box::pin(async move {
            let ToolCall {
                name,
                arguments,
                session,
            } = call;
            let local = LOCAL_TOOLS.contains(&name.as_str());
            let result = match name.as_str() {
                "browser_list" => Ok(browser_list(&registry)),
                "session_list" => session_list(&store, &arguments),
                "session_steps" => session_steps(&store, &arguments),
                "session_replay" => replay_session(&router, &registry, &store, &arguments).await,
                "session_export" => session_export(&store, &arguments),
                "metrics" => metrics_tool(&metrics, &registry, &store),
                "logs_tail" => logs::tail(&arguments),
                _ => route(&router, name.clone(), arguments.clone(), session.as_ref()).await,
            };
            if let Err(error) = &result {
                tracing::info!(
                    tool = %name,
                    code = %error.code,
                    message = %error.message,
                    "tool call failed"
                );
            }
            if local {
                record_backend_step(&sink, session.as_ref(), &name, &arguments, &result, started);
            }
            result
        })
    }
}

fn metrics_tool(
    metrics: &Arc<Metrics>,
    registry: &Arc<Registry>,
    store: &Arc<Store>,
) -> Result<Value, ErrorBody> {
    let mut document = metrics.snapshot();
    let browsers = registry.snapshot();
    let browser_ids: Vec<String> = browsers.iter().map(|browser| browser.id.clone()).collect();
    let sessions = store.sessions_total().unwrap_or(0);
    let steps = store.steps_total().unwrap_or(0);
    if let Some(object) = document.as_object_mut() {
        object.insert(
            "protocol".to_string(),
            json!(corvus_proto::PROTOCOL_VERSION),
        );
        object.insert("browsers_connected".to_string(), json!(browsers.len()));
        object.insert("browsers".to_string(), json!(browser_ids));
        object.insert("sessions_total".to_string(), json!(sessions));
        object.insert(
            "sessions_active".to_string(),
            json!(store.sessions_active().unwrap_or(0)),
        );
        object.insert(
            "recording".to_string(),
            json!({
                "enabled": true,
                "db_path": store.path().display().to_string(),
                "sessions": sessions,
                "steps": steps,
            }),
        );
    }
    Ok(document)
}

async fn route(
    router: &Arc<Router>,
    name: String,
    arguments: Value,
    session: Option<&SessionContext>,
) -> Result<Value, ErrorBody> {
    let mut request = CommandRequest::new(name, arguments).with_trace_id(ids::next_id("trace"));
    if let Some(session) = session {
        request = request.with_session(session.clone());
    }
    router.dispatch(request).await
}

fn record_backend_step(
    sink: &StepSink,
    session: Option<&SessionContext>,
    tool: &str,
    arguments: &Value,
    result: &Result<Value, ErrorBody>,
    started: Instant,
) {
    let Some(session) = session else {
        return;
    };
    let step = StepRecord {
        session_id: session.session_id.clone(),
        ts: now_millis(),
        tool: tool.to_string(),
        args: arguments.clone(),
        result: result.as_ref().ok().cloned(),
        error_code: result
            .as_ref()
            .err()
            .map(|error| error.code.as_str().to_string()),
        duration_ms: started.elapsed().as_millis() as i64,
        browser_id: None,
        tab_id: arguments
            .get("tab_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        ok: result.is_ok(),
    };
    sink.record(step);
}

fn browser_list(registry: &Arc<Registry>) -> Value {
    let browsers: Vec<Value> = registry
        .snapshot()
        .into_iter()
        .map(|info| {
            json!({
                "browser_id": info.id,
                "profile": info.profile,
                "version": info.version,
                "capabilities": info.capabilities,
                "connected_at_ms": info.connected_at_ms,
            })
        })
        .collect();
    json!({ "browsers": browsers })
}

fn session_list(store: &Arc<Store>, arguments: &Value) -> Result<Value, ErrorBody> {
    let limit = arguments.get("limit").and_then(Value::as_u64).unwrap_or(50) as u32;
    let since_ms = arguments.get("since_ms").and_then(Value::as_i64);
    let rows = store.list_sessions(limit, since_ms).map_err(internal)?;
    let total = store.count_sessions(since_ms).map_err(internal)?;
    let sessions: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            json!({
                "session_id": row.id,
                "started_at": row.started_at,
                "ended_at": row.ended_at,
                "status": row.status,
                "transport": row.transport,
                "agent": row.agent,
                "steps": row.steps,
                "errors": row.errors,
            })
        })
        .collect();
    Ok(json!({ "sessions": sessions, "total": total }))
}

fn session_steps(store: &Arc<Store>, arguments: &Value) -> Result<Value, ErrorBody> {
    let session_id = arguments
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| bad_args("session_steps requires a session_id argument"))?;
    let from_seq = arguments
        .get("from_seq")
        .and_then(Value::as_u64)
        .map(|value| i64::try_from(value).unwrap_or(i64::MAX))
        .unwrap_or(0);
    let limit = arguments
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(100)
        .clamp(1, 500) as u32;
    if !store.session_exists(session_id).map_err(internal)? {
        return Err(ErrorBody::new(
            ErrorCode::NotFound,
            format!("session {session_id} was not found"),
        )
        .with_details(json!({ "session_id": session_id })));
    }
    let mut rows = store
        .get_session_steps(session_id, from_seq, limit.saturating_add(1))
        .map_err(internal)?;
    let next_seq = if rows.len() > limit as usize {
        rows.get(limit as usize).map(|row| row.seq)
    } else {
        None
    };
    rows.truncate(limit as usize);
    let total_steps = store.count_steps(session_id).map_err(internal)?;
    let steps: Vec<Value> = rows.iter().map(step_value).collect();
    let mut document = json!({
        "session_id": session_id,
        "steps": steps,
        "total_steps": total_steps,
    });
    if let Some(next_seq) = next_seq {
        document["next_seq"] = json!(next_seq);
    }
    Ok(document)
}

fn step_value(row: &StepRow) -> Value {
    let (args, args_truncated, args_bytes) =
        decode_stored(row.args_json.as_deref(), row.args_truncated);
    let (result, result_truncated, result_bytes) =
        decode_stored(row.result_json.as_deref(), row.result_truncated);
    let mut value = json!({
        "seq": row.seq,
        "ts": row.ts,
        "tool": row.tool,
        "browser_id": row.browser_id,
        "tab_id": row.tab_id,
        "ok": row.ok,
        "error_code": row.error_code,
        "duration_ms": row.duration_ms,
        "args": args,
        "args_truncated": args_truncated,
        "result": result,
        "result_truncated": result_truncated,
    });
    if let Some(bytes) = args_bytes {
        value["args_bytes"] = json!(bytes);
    }
    if let Some(bytes) = result_bytes {
        value["result_bytes"] = json!(bytes);
    }
    value
}

fn decode_stored(text: Option<&str>, stored_truncated: bool) -> (Value, bool, Option<usize>) {
    match text {
        None => (Value::Null, stored_truncated, None),
        Some(text) if text.len() > STEP_VALUE_CAP => (Value::Null, true, Some(text.len())),
        Some(text) => match serde_json::from_str(text) {
            Ok(value) => (value, stored_truncated, None),
            Err(_) => (Value::String(text.to_string()), stored_truncated, None),
        },
    }
}

fn session_export(store: &Arc<Store>, arguments: &Value) -> Result<Value, ErrorBody> {
    let session_id = arguments
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| bad_args("session_export requires a session_id argument"))?;
    let Some((session, steps)) = store.export_rows(session_id).map_err(internal)? else {
        return Err(ErrorBody::new(
            ErrorCode::NotFound,
            format!("session {session_id} was not found"),
        )
        .with_details(json!({ "session_id": session_id })));
    };
    let mut lines = Vec::with_capacity(steps.len() + 1);
    lines.push(export_session_line(&session).to_string());
    for step in steps {
        lines.push(export_step_line(&step).to_string());
    }
    let full = lines.join("\n");
    let bytes = full.len();
    let truncated = bytes > EXPORT_CAP;
    let content = if truncated {
        truncate_utf8(&full, EXPORT_CAP)
    } else {
        full
    };
    Ok(json!({
        "session_id": session_id,
        "format": "jsonl",
        "content": content,
        "bytes": bytes,
        "truncated": truncated,
    }))
}

fn export_session_line(session: &SessionRow) -> Value {
    let mut line = Map::new();
    line.insert("type".to_string(), json!("session"));
    line.insert("id".to_string(), json!(session.id));
    line.insert("started_at".to_string(), json!(session.started_at));
    line.insert("ended_at".to_string(), json!(session.ended_at));
    line.insert("browser".to_string(), json!(session.browser));
    line.insert("profile".to_string(), json!(session.profile));
    line.insert("agent".to_string(), json!(session.agent));
    line.insert("transport".to_string(), json!(session.transport));
    line.insert("status".to_string(), json!(session.status));
    line.insert("steps".to_string(), json!(session.steps));
    line.insert("errors".to_string(), json!(session.errors));
    Value::Object(line)
}

fn export_step_line(step: &StepRow) -> Value {
    let mut line = Map::new();
    line.insert("type".to_string(), json!("step"));
    line.insert("id".to_string(), json!(step.id));
    line.insert("session_id".to_string(), json!(step.session_id));
    line.insert("seq".to_string(), json!(step.seq));
    line.insert("ts".to_string(), json!(step.ts));
    line.insert("tool".to_string(), json!(step.tool));
    line.insert("args_json".to_string(), json!(step.args_json));
    line.insert("result_json".to_string(), json!(step.result_json));
    line.insert("error_code".to_string(), json!(step.error_code));
    line.insert("duration_ms".to_string(), json!(step.duration_ms));
    line.insert("browser_id".to_string(), json!(step.browser_id));
    line.insert("tab_id".to_string(), json!(step.tab_id));
    line.insert("ok".to_string(), json!(step.ok));
    line.insert("args_truncated".to_string(), json!(step.args_truncated));
    line.insert("result_truncated".to_string(), json!(step.result_truncated));
    Value::Object(line)
}

async fn replay_session(
    router: &Arc<Router>,
    registry: &Arc<Registry>,
    store: &Arc<Store>,
    arguments: &Value,
) -> Result<Value, ErrorBody> {
    let session_id = arguments
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| bad_args("session_replay requires a session_id argument"))?;
    let mode = arguments
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("dry_run");
    let live = mode == "live";
    let continue_on_error = arguments
        .get("continue_on_error")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let from_seq = arguments
        .get("from_seq")
        .and_then(Value::as_u64)
        .map(|value| i64::try_from(value).unwrap_or(i64::MAX))
        .unwrap_or(0);
    let to_seq = arguments
        .get("to_seq")
        .and_then(Value::as_u64)
        .map(|value| i64::try_from(value).unwrap_or(i64::MAX))
        .unwrap_or(i64::MAX);
    let requested_browser = arguments
        .get("browser_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    if !store.session_exists(session_id).map_err(internal)? {
        return Err(ErrorBody::new(
            ErrorCode::NotFound,
            format!("session {session_id} was not found"),
        )
        .with_details(json!({ "session_id": session_id })));
    }
    let rows = store
        .get_session_steps(session_id, from_seq, u32::MAX)
        .map_err(internal)?;
    let mut planned: Vec<(i64, String, Value)> = Vec::new();
    let mut skipped: Vec<Value> = Vec::new();
    for row in rows.into_iter().filter(|row| row.seq <= to_seq) {
        if CONTROL_TOOLS.contains(&row.tool.as_str()) {
            skipped.push(json!({
                "seq": row.seq,
                "tool": row.tool,
                "reason": "control_tool",
            }));
            continue;
        }
        if live && matches!(row.tool.as_str(), "tab_close" | "tab_switch") {
            skipped.push(json!({
                "seq": row.seq,
                "tool": row.tool,
                "reason": "tab_tool",
            }));
            continue;
        }
        let Some(args) = replay_arguments(row.args_json.as_deref(), row.args_truncated) else {
            skipped.push(json!({
                "seq": row.seq,
                "tool": row.tool,
                "reason": "invalid_args",
            }));
            continue;
        };
        planned.push((row.seq, row.tool, args));
    }
    if planned.is_empty() {
        return Err(bad_args("the session has no replayable steps")
            .with_details(json!({ "reason": "no_replayable_steps", "session_id": session_id })));
    }
    let total = planned.len() + skipped.len();
    let planned_document: Vec<Value> = planned
        .iter()
        .map(|(seq, tool, args)| json!({ "seq": seq, "tool": tool, "args": args }))
        .collect();
    if !live {
        return Ok(json!({
            "session_id": session_id,
            "mode": mode,
            "total": total,
            "planned": planned_document,
            "executed": [],
            "skipped": skipped,
        }));
    }
    if registry.list().is_empty() {
        return Err(ErrorBody::new(
            ErrorCode::TransportDown,
            "no browser is connected",
        ));
    }
    let replay_id = format!("s-{}", uuid::Uuid::new_v4());
    store
        .start_session(&replay_id, "internal", Some("corvus-replay"), None)
        .map_err(internal)?;
    let context = SessionContext::new(
        replay_id.clone(),
        "internal",
        Some("corvus-replay".to_string()),
    );
    let mut executed = Vec::new();
    let mut stopped_at = None;
    let mut stop_code = None;
    for (seq, tool, args) in &planned {
        let mut request = CommandRequest::new(tool.clone(), args.clone())
            .with_trace_id(ids::next_id("trace"))
            .with_session(context.clone());
        if let Some(browser) = &requested_browser {
            request = request.with_browser(browser.clone());
        }
        let started = Instant::now();
        let result = router.dispatch(request).await;
        let ok = result.is_ok();
        let error_code = result
            .as_ref()
            .err()
            .map(|error| error.code.as_str().to_string());
        executed.push(json!({
            "seq": seq,
            "tool": tool,
            "ok": ok,
            "error_code": error_code,
            "duration_ms": started.elapsed().as_millis() as i64,
        }));
        if !ok && !continue_on_error {
            stopped_at = Some(*seq);
            stop_code = error_code;
            break;
        }
    }
    store
        .finish_session(&replay_id, "closed")
        .map_err(internal)?;
    let mut document = json!({
        "session_id": session_id,
        "mode": mode,
        "total": total,
        "planned": planned_document,
        "executed": executed,
        "skipped": skipped,
        "replay_session_id": replay_id,
    });
    if let Some(seq) = stopped_at {
        document["stopped_at"] = json!(seq);
    }
    if let Some(code) = stop_code {
        document["error_code"] = json!(code);
    }
    Ok(document)
}

fn replay_arguments(text: Option<&str>, truncated: bool) -> Option<Value> {
    if truncated {
        return None;
    }
    let text = text?;
    let mut value: Value = serde_json::from_str(text).ok()?;
    let object = value.as_object_mut()?;
    object.remove("tab_id");
    object.remove("browser_id");
    Some(value)
}

fn truncate_utf8(text: &str, cap: usize) -> String {
    if text.len() <= cap {
        return text.to_string();
    }
    let mut end = cap;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

fn bad_args(message: impl Into<String>) -> ErrorBody {
    ErrorBody::new(ErrorCode::BadArgs, message)
}

fn internal(error: impl std::fmt::Display) -> ErrorBody {
    ErrorBody::new(
        ErrorCode::Internal,
        format!("recording database error: {error}"),
    )
}

/// A running daemon.
pub struct Daemon {
    registry: Arc<Registry>,
    router: Arc<Router>,
    mcp: Arc<McpServer>,
    store: Arc<Store>,
    metrics: Arc<Metrics>,
    recorder: Recorder,
    ws_addr: SocketAddr,
    http_addr: SocketAddr,
    shutdown: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
}

impl Daemon {
    /// Bind the listeners, open the recording store, start the background
    /// tasks and log the addresses.
    pub async fn start(config: DaemonConfig) -> std::io::Result<Self> {
        logs::ensure_installed();
        let metrics = Arc::new(Metrics::new());
        let db_path = corvus_record::resolve_path(config.db_path.clone());
        let store = Store::open(&db_path)
            .map_err(|error| std::io::Error::other(format!("recording store: {error}")))?;
        let store = Arc::new(store);
        let recorder = Recorder::start(Arc::clone(&store));

        let registry = Arc::new(Registry::new());
        let (inbound_sender, inbound_receiver) = mpsc::channel(256);
        let router = Router::new(Arc::clone(&registry), config.router);
        let _ = router.attach_step_sink(recorder.sink());
        let router_task = router.spawn(inbound_receiver);

        let (shutdown_sender, shutdown_receiver) = watch::channel(false);
        let ws = WsServer::bind(SocketAddr::new(config.bind, config.ws_port)).await?;
        let ws_addr = ws.local_addr();
        let ws_task = ws.spawn(
            Arc::clone(&registry),
            inbound_sender,
            shutdown_receiver.clone(),
        );

        let backend: Arc<dyn ToolBackend> = Arc::new(RouterBackend::new(
            Arc::clone(&router),
            Arc::clone(&registry),
            Arc::clone(&store),
            Arc::clone(&metrics),
            recorder.sink(),
        ));
        let http_addr_slot = Arc::new(std::sync::OnceLock::<SocketAddr>::new());
        let health_registry = Arc::clone(&registry);
        let health_store = Arc::clone(&store);
        let health_metrics = Arc::clone(&metrics);
        let health_http_addr = Arc::clone(&http_addr_slot);
        let health_db_path = db_path.display().to_string();
        let health = Arc::new(move || {
            let mut document = json!({
                "status": "ok",
                "ws_addr": ws_addr.to_string(),
                "browsers": health_registry.list(),
                "uptime_ms": health_metrics.uptime_ms(),
                "started_at": health_metrics.started_at_ms(),
                "tool_calls_total": health_metrics.calls(),
                "tool_errors_total": health_metrics.errors(),
            });
            if let Some(addr) = health_http_addr.get() {
                document["http_addr"] = json!(addr.to_string());
            }
            document["recording"] = json!({
                "enabled": true,
                "db_path": health_db_path,
                "schema_version": health_store.schema_version(),
                "sessions": health_store.sessions_total().unwrap_or(0),
                "steps": health_store.steps_total().unwrap_or(0),
            });
            document
        });
        let mcp = Arc::new(
            McpServer::new(backend)
                .with_store(Arc::clone(&store))
                .with_metrics(Arc::clone(&metrics))
                .with_health(health),
        );

        let http = McpHttpServer::bind(
            SocketAddr::new(config.bind, config.http_port),
            Arc::clone(&mcp),
        )
        .await?;
        let http_addr = http.local_addr();
        let _ = http_addr_slot.set(http_addr);
        let http_task = http.spawn(shutdown_receiver);

        tracing::info!(
            version = env!("CARGO_PKG_VERSION"),
            protocol = corvus_proto::PROTOCOL_VERSION,
            ws = %ws_addr,
            mcp = %format!("http://{http_addr}/mcp"),
            db = %db_path.display(),
            "corvusd serving"
        );

        Ok(Self {
            registry,
            router,
            mcp,
            store,
            metrics,
            recorder,
            ws_addr,
            http_addr,
            shutdown: shutdown_sender,
            tasks: vec![router_task, ws_task, http_task],
        })
    }

    /// Start a daemon on the default configuration.
    pub async fn serve_default() -> std::io::Result<Self> {
        Self::start(DaemonConfig::default()).await
    }

    /// Address the WebSocket listener is bound to.
    pub fn ws_addr(&self) -> SocketAddr {
        self.ws_addr
    }

    /// Address the MCP HTTP listener is bound to.
    pub fn http_addr(&self) -> SocketAddr {
        self.http_addr
    }

    /// The browser registry.
    pub fn registry(&self) -> &Arc<Registry> {
        &self.registry
    }

    /// The command router.
    pub fn router(&self) -> &Arc<Router> {
        &self.router
    }

    /// The MCP server the HTTP listener answers with.
    pub fn mcp(&self) -> &Arc<McpServer> {
        &self.mcp
    }

    /// The recording store.
    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    /// The process-wide tool counters.
    pub fn metrics(&self) -> &Arc<Metrics> {
        &self.metrics
    }

    /// The recording database path.
    pub fn db_path(&self) -> &std::path::Path {
        self.store.path()
    }

    /// Wait until every queued recording step has been written.
    pub async fn flush_recording(&self) -> bool {
        self.recorder.flush().await
    }

    /// The health document the daemon reports.
    pub fn health(&self) -> Value {
        self.mcp.health()
    }

    /// Stop the background tasks and wait for them, bounded by a short grace.
    pub async fn shutdown(self) {
        let Self {
            shutdown,
            tasks,
            mcp,
            recorder,
            ..
        } = self;
        let _ = shutdown.send(true);
        for task in tasks {
            if tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .is_err()
            {
                tracing::debug!("a background task did not stop within the grace period");
            }
        }
        mcp.close_all_sessions("closed");
        recorder.shutdown().await;
    }
}
