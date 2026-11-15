//! The Corvus daemon: one runtime that owns the WebSocket transport, the
#![warn(missing_docs)]
//! command router and the MCP server.

pub mod channel;
pub mod doctor;
pub mod home;
pub mod logging;
pub mod logs;
pub mod signals;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use corvus_api::{AppState, AuthService};
pub use corvus_api::{AuthConfig, AuthMode};
use corvus_application::dto::BrowserSummary;
use corvus_application::ports::{
    BrowserDirectory, BrowserRegistry, RecordingRepository, ToolCatalog,
};
use corvus_application::{CommandRequest, Router, ToolBackend};
use corvus_cdp::{is_cdp_tool, CdpClient, CdpError};
use corvus_domain::{
    classify_target_id, route_tool, BatchPolicy, ChannelMode as DomainChannelMode, Route,
    TargetIdKind, ToolFamily, CDP_TAB_HINT, EXTENSION_TAB_HINT,
};
use corvus_mcp::{BackendFuture, McpHttpServer, McpServer, Metrics, ToolCall};
use corvus_proto::{ids, ErrorBody, ErrorCode};
use corvus_record::{
    now_millis, Recorder, SessionContext, SessionRow, StepRecord, StepRow, StepSink, Store,
};

use crate::channel::{ChannelMode, ChannelState};

pub use corvus_application::RouterConfig;
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
const LOCAL_TOOLS: [&str; 10] = [
    "browser_list",
    "browser_batch",
    "browser_channel",
    "tool_help",
    "session_list",
    "session_steps",
    "session_replay",
    "session_export",
    "metrics",
    "logs_tail",
];

/// Tools that describe or control the recording layer and must not be replayed.
const CONTROL_TOOLS: [&str; 10] = [
    "browser_channel",
    "browser_batch",
    "tool_help",
    "session_list",
    "session_steps",
    "session_replay",
    "session_export",
    "metrics",
    "logs_tail",
    "browser_list",
];

/// Authentication settings, including an optional bootstrap account.
#[derive(Debug, Clone)]
pub struct AuthSettings {
    /// JWT and mode configuration.
    pub config: AuthConfig,
    /// Bootstrap username; required when auth is enabled.
    pub username: Option<String>,
    /// Bootstrap password; required when auth is enabled.
    pub password: Option<String>,
}

impl Default for AuthSettings {
    fn default() -> Self {
        Self {
            config: AuthConfig::disabled(),
            username: None,
            password: None,
        }
    }
}

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
    /// Corvus home; when absent CORVUS_HOME or the platform default is used.
    pub home: Option<PathBuf>,
    /// Browser channel selection.
    pub channel: ChannelMode,
    /// Explicit CDP WebSocket endpoint.
    pub cdp_endpoint: Option<String>,
    /// Disable CDP entirely and stay on the extension transport.
    pub no_cdp: bool,
    /// Allow a non-loopback bind without authentication. This is unsafe and
    /// logs a warning; only use it behind a trusted authenticating proxy.
    pub allow_no_auth: bool,
    /// Optional local authentication.
    pub auth: AuthSettings,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            bind: DEFAULT_BIND,
            ws_port: DEFAULT_WS_PORT,
            http_port: DEFAULT_HTTP_PORT,
            router: RouterConfig::default(),
            db_path: None,
            home: None,
            channel: ChannelMode::Auto,
            cdp_endpoint: None,
            no_cdp: false,
            allow_no_auth: false,
            auth: AuthSettings::default(),
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
    cdp: Arc<CdpClient>,
    channel: Arc<ChannelState>,
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
        cdp: Arc<CdpClient>,
        channel: Arc<ChannelState>,
        sink: StepSink,
    ) -> Self {
        Self {
            router,
            registry,
            store,
            metrics,
            cdp,
            channel,
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
        let cdp = Arc::clone(&self.cdp);
        let channel = Arc::clone(&self.channel);
        let sink = self.sink.clone();
        let started = Instant::now();
        Box::pin(async move {
            let ToolCall {
                name,
                arguments,
                session,
            } = call;
            let local = LOCAL_TOOLS.contains(&name.as_str());
            let (result, recorded_here) = match name.as_str() {
                "browser_list" => (Ok(browser_list(&registry, &channel)), true),
                "browser_batch" => (
                    run_batch(
                        &cdp,
                        &channel,
                        &registry,
                        &router,
                        &arguments,
                        session.as_ref(),
                    )
                    .await,
                    true,
                ),
                "browser_channel" => (channel.tool(&arguments), true),
                "tool_help" => (corvus_mcp::tools::help_document(&arguments), true),
                "session_list" => (session_list(&store, &arguments), true),
                "session_steps" => (session_steps(&store, &arguments), true),
                "session_replay" => (
                    replay_session(&router, &registry, &store, &arguments).await,
                    true,
                ),
                "session_export" => (session_export(&store, &arguments), true),
                "metrics" => (metrics_tool(&metrics, &registry, &store, &channel), true),
                "logs_tail" => (logs::tail(&arguments), true),
                _ => {
                    dispatch_tool(&cdp, &channel, &router, &name, &arguments, session.as_ref())
                        .await
                }
            };
            if let Err(error) = &result {
                tracing::warn!(
                    tool = %name,
                    code = %error.code,
                    error = %error.message,
                    "tool call failed"
                );
            }
            if local || recorded_here {
                record_backend_step(&sink, session.as_ref(), &name, &arguments, &result, started);
            }
            result
        })
    }
}

/// Browser directory adapter over the transport registry.
struct RegistryDirectory {
    registry: Arc<Registry>,
}

#[async_trait::async_trait]
impl BrowserDirectory for RegistryDirectory {
    async fn list(&self) -> Vec<BrowserSummary> {
        self.registry
            .list()
            .into_iter()
            .map(|info| BrowserSummary {
                id: info.id,
                profile: info.profile,
                browser_family: info.browser_family,
                version: info.version,
                capabilities: info.capabilities,
                cdp_available: info.cdp.as_ref().is_some_and(|cdp| cdp.available),
            })
            .collect()
    }

    async fn get(&self, id: &str) -> Option<BrowserSummary> {
        self.registry.get(id).map(|connection| {
            let info = connection.info();
            BrowserSummary {
                id: info.id.clone(),
                profile: info.profile.clone(),
                browser_family: info.browser_family.clone(),
                version: info.version.clone(),
                capabilities: info.capabilities.clone(),
                cdp_available: info.cdp.as_ref().is_some_and(|cdp| cdp.available),
            }
        })
    }
}

/// Tool catalogue adapter over the MCP schema module.
struct McpCatalog;

impl ToolCatalog for McpCatalog {
    fn definitions_for(&self, category: Option<&str>, detail: &str) -> Vec<Value> {
        corvus_mcp::tools::definitions_for(category, detail)
    }

    fn validate(&self, name: &str, arguments: &Value) -> Result<Value, corvus_proto::ErrorBody> {
        corvus_mcp::tools::validate(name, arguments)
    }
}

/// Recording adapter over the SQLite store.
struct StoreRecordings {
    store: Arc<Store>,
}

#[async_trait::async_trait]
impl RecordingRepository for StoreRecordings {
    async fn list_sessions(
        &self,
        limit: u32,
        since_ms: Option<i64>,
    ) -> Result<Vec<SessionRow>, String> {
        let store = Arc::clone(&self.store);
        tokio::task::spawn_blocking(move || {
            store
                .list_sessions(limit, since_ms)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())?
    }

    async fn get_session(&self, id: &str) -> Result<Option<SessionRow>, String> {
        let store = Arc::clone(&self.store);
        let id = id.to_string();
        tokio::task::spawn_blocking(move || {
            store.get_session(&id).map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())?
    }

    async fn get_steps(
        &self,
        session_id: &str,
        from_seq: i64,
        limit: u32,
    ) -> Result<Vec<StepRow>, String> {
        let store = Arc::clone(&self.store);
        let session_id = session_id.to_string();
        tokio::task::spawn_blocking(move || {
            store
                .get_session_steps(&session_id, from_seq, limit)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())?
    }

    async fn record_step(&self, step: StepRecord) -> Result<(), String> {
        let store = Arc::clone(&self.store);
        tokio::task::spawn_blocking(move || {
            store.record_step(step).map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())?
    }
}

fn metrics_tool(
    metrics: &Arc<Metrics>,
    registry: &Arc<Registry>,
    store: &Arc<Store>,
    channel: &Arc<ChannelState>,
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
        object.insert("channel".to_string(), channel.status());
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

/// Tools that only the extension implements. Every browser-facing tool now
/// has a CDP handler, so the extension remains a fallback rather than a
/// capability boundary.
const EXTENSION_ONLY_TOOLS: [&str; 0] = [];

/// CDP target identifiers are 32 hexadecimal characters.
fn looks_like_cdp_target(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|text| classify_target_id(text) == TargetIdKind::CdpTarget)
}

/// Extension tab identifiers are numbers (or numeric strings).
fn looks_like_extension_tab(value: &Value) -> bool {
    match value {
        Value::Number(number) => number.as_u64().is_some(),
        Value::String(text) => classify_target_id(text) == TargetIdKind::ExtensionTab,
        _ => false,
    }
}

fn tab_namespace_error(value: &Value, hint: &str) -> ErrorBody {
    ErrorBody::new(ErrorCode::BadArgs, hint).with_details(json!({
        "hint": hint,
        "tab_id": value,
    }))
}

/// Tools that exist only on the CDP channel.
const CDP_ONLY_TOOLS: [&str; 8] = [
    "cdp_status",
    "cdp_targets",
    "cdp_send",
    "cdp_events",
    "browser_console",
    "browser_cookies",
    "browser_dialog",
    "browser_emulate",
];

/// Channel that would actually run a browser-facing tool. Daemon-local tools
/// are not browser-facing, so they never receive tab-id namespace hints.
fn browser_route(name: &str, mode: ChannelMode, cdp_connected: bool) -> Route {
    let family = if CDP_ONLY_TOOLS.contains(&name) {
        ToolFamily::CdpOnly
    } else if EXTENSION_ONLY_TOOLS.contains(&name) {
        ToolFamily::ExtensionOnly
    } else if is_cdp_tool(name) {
        ToolFamily::Shared
    } else {
        ToolFamily::Local
    };
    let mode = match mode {
        ChannelMode::Auto => DomainChannelMode::Auto,
        ChannelMode::Cdp => DomainChannelMode::Cdp,
        ChannelMode::Extension => DomainChannelMode::Extension,
    };
    route_tool(mode, family, cdp_connected)
}

async fn run_batch(
    cdp: &Arc<CdpClient>,
    channel: &Arc<ChannelState>,
    registry: &Arc<Registry>,
    router: &Arc<Router>,
    arguments: &Value,
    session: Option<&SessionContext>,
) -> Result<Value, ErrorBody> {
    let arguments = corvus_mcp::tools::validate("browser_batch", arguments)?;
    let steps = arguments
        .get("steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Err(error) = BatchPolicy::validate_count(steps.len()) {
        return Err(ErrorBody::new(ErrorCode::BadArgs, error.to_string()));
    }
    let stop_on_error = arguments
        .get("stop_on_error")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let mut results = Vec::with_capacity(steps.len());
    let mut stopped = false;
    for (index, step) in steps.iter().enumerate() {
        let number = index + 1;
        let tool = step
            .get("tool")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let raw_args = step.get("args").cloned().unwrap_or_else(|| json!({}));
        let outcome = match corvus_mcp::tools::validate(&tool, &raw_args) {
            Ok(_args) if tool == "browser_list" => Ok(browser_list(registry, channel)),
            Ok(args) => {
                dispatch_tool(cdp, channel, router, &tool, &args, session)
                    .await
                    .0
            }
            Err(error) => Err(error),
        };
        match outcome {
            Ok(data) => {
                results.push(json!({
                    "step": number,
                    "tool": tool,
                    "ok": true,
                    "data": data,
                }));
            }
            Err(error) => {
                results.push(json!({
                    "step": number,
                    "tool": tool,
                    "ok": false,
                    "error": error,
                }));
                if stop_on_error {
                    stopped = true;
                    break;
                }
            }
        }
    }
    let ok = results
        .iter()
        .all(|result| result.get("ok").and_then(Value::as_bool) == Some(true));
    Ok(json!({
        "ok": ok,
        "count": results.len(),
        "stopped": stopped,
        "results": results,
    }))
}

async fn dispatch_tool(
    cdp: &Arc<CdpClient>,
    channel: &Arc<ChannelState>,
    router: &Arc<Router>,
    name: &str,
    arguments: &Value,
    session: Option<&SessionContext>,
) -> (Result<Value, ErrorBody>, bool) {
    let mode = channel.mode();
    // Namespace validation belongs to the selected browser-facing route, so it
    // must run before any tool-family early return (including CDP-only tools).
    match browser_route(name, mode, cdp.is_connected()) {
        Route::Cdp => {
            if let Some(tab_id) = arguments.get("tab_id") {
                if looks_like_extension_tab(tab_id) {
                    return (Err(tab_namespace_error(tab_id, CDP_TAB_HINT)), false);
                }
            }
        }
        Route::Extension => {
            if let Some(tab_id) = arguments.get("tab_id") {
                if looks_like_cdp_target(tab_id) {
                    return (Err(tab_namespace_error(tab_id, EXTENSION_TAB_HINT)), false);
                }
            }
        }
        Route::Rejected => {}
    }
    if CDP_ONLY_TOOLS.contains(&name) {
        if mode == ChannelMode::Extension {
            return (
                Err(capability_error(
                    "CDP tools are disabled while the extension channel is selected",
                    mode,
                )),
                false,
            );
        }
        if !cdp.is_connected() {
            return (
                Err(capability_error("no CDP endpoint is connected", mode)),
                false,
            );
        }
        return match cdp.call_tool(name, arguments).await {
            Ok(mut value) => {
                if name == "cdp_status" {
                    if let Some(object) = value.as_object_mut() {
                        object.insert("descriptor".to_string(), json!(channel.descriptor()));
                    }
                }
                (Ok(value), true)
            }
            Err(error) => (Err(cdp_error_body(error)), true),
        };
    }
    if EXTENSION_ONLY_TOOLS.contains(&name) && mode == ChannelMode::Cdp {
        return (
            Err(ErrorBody::new(
                ErrorCode::CapabilityUnavailable,
                "this tool is implemented only by the extension and the cdp channel is selected",
            )
            .with_details(json!({
                "transport": "extension",
                "channel": mode.as_str(),
            }))),
            false,
        );
    }
    if is_cdp_tool(name) && mode != ChannelMode::Extension {
        if cdp.is_connected() {
            return match cdp.call_tool(name, arguments).await {
                Ok(value) => (Ok(value), true),
                Err(error) if fall_back_to_extension(mode, &error) => (
                    route(router, name.to_string(), arguments.clone(), session).await,
                    false,
                ),
                Err(error) => (Err(cdp_error_body(error)), true),
            };
        }
        if mode == ChannelMode::Cdp {
            return (
                Err(capability_error("no CDP endpoint is connected", mode)),
                false,
            );
        }
    }
    (
        route(router, name.to_string(), arguments.clone(), session).await,
        false,
    )
}

/// Auto mode may retry a tool on the extension only when the CDP handler
/// could not implement it at all. Transport and navigation failures are real
/// outcomes and must never be retried on another channel.
fn fall_back_to_extension(mode: ChannelMode, error: &CdpError) -> bool {
    mode == ChannelMode::Auto && matches!(error, CdpError::Unsupported(_))
}

fn capability_error(message: &str, mode: ChannelMode) -> ErrorBody {
    ErrorBody::new(ErrorCode::CapabilityUnavailable, message).with_details(json!({
        "transport": "cdp",
        "channel": mode.as_str(),
    }))
}

fn cdp_error_body(error: CdpError) -> ErrorBody {
    let code = match &error {
        CdpError::InvalidArgument(_) | CdpError::InvalidEndpoint(_) => ErrorCode::BadArgs,
        CdpError::Timeout(_) => ErrorCode::Timeout,
        CdpError::NotConnected | CdpError::Transport(_) => ErrorCode::BrowserGone,
        CdpError::TargetNotFound(_) | CdpError::DialogNotFound => ErrorCode::NotFound,
        CdpError::NotInteractable(_) => ErrorCode::NotInteractable,
        CdpError::Unsupported(_) => ErrorCode::CapabilityUnavailable,
        CdpError::Protocol(_) | CdpError::Remote { .. } => ErrorCode::Internal,
    };
    let mut details = json!({
        "transport": "cdp",
        "cdp_code": error.code(),
        "cdp_message": error.to_string(),
    });
    if let CdpError::Remote { code, .. } = &error {
        details["cdp_error_code"] = json!(code);
    }
    ErrorBody::new(code, error.to_string()).with_details(details)
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

fn browser_list(registry: &Arc<Registry>, channel: &Arc<ChannelState>) -> Value {
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
                "cdp": info.cdp,
            })
        })
        .collect();
    json!({
        "browsers": browsers,
        "channel": channel.status(),
    })
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

/// Pick the first valid CDP endpoint among all descriptor-bearing browsers.
///
/// Invalid descriptors are skipped rather than starving later valid browsers.
/// When several are valid, an endpoint that was not already attempted wins, so
/// one rejected endpoint cannot block the rest.
fn select_cdp_endpoint(
    browsers: &[corvus_transport::registry::BrowserInfo],
    attempted: Option<&str>,
) -> Result<String, String> {
    let mut valid = Vec::new();
    let mut first_error = None;
    for info in browsers {
        let Some(descriptor) = info.cdp.as_ref() else {
            continue;
        };
        match channel::validate_descriptor(descriptor, info.browser_family.as_deref()) {
            Ok(endpoint) => valid.push(endpoint),
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }
    if valid.is_empty() {
        return Err(first_error.unwrap_or_else(|| "no browser advertises a CDP endpoint".into()));
    }
    Ok(valid
        .iter()
        .find(|endpoint| attempted != Some(endpoint.as_str()))
        .cloned()
        .unwrap_or_else(|| valid[0].clone()))
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
    channel: Arc<ChannelState>,
    home: home::HomeLayout,
    recorder: Recorder,
    ws_addr: SocketAddr,
    http_addr: SocketAddr,
    shutdown: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
}

fn bind_auth_guard(bind: IpAddr, auth_enabled: bool, allow_no_auth: bool) -> std::io::Result<bool> {
    if bind.is_loopback() || auth_enabled {
        return Ok(false);
    }
    if allow_no_auth {
        return Ok(true);
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "authentication must be enabled when binding to a non-loopback address; pass --allow-no-auth to opt out",
    ))
}

impl Daemon {
    /// Bind the listeners, open the recording store, start the background
    /// tasks and log the addresses.
    pub async fn start(config: DaemonConfig) -> std::io::Result<Self> {
        logs::ensure_installed();
        if bind_auth_guard(
            config.bind,
            config.auth.config.enabled(),
            config.allow_no_auth,
        )? {
            tracing::warn!(
                bind = %config.bind,
                "authentication disabled on public bind"
            );
        }
        if config.auth.config.enabled() && !config.auth.config.secret_is_strong() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "CORVUS_AUTH_SECRET must be at least 32 bytes",
            ));
        }
        let metrics = Arc::new(Metrics::new());
        let (layout, db_path) = home::resolve_database(config.db_path.clone(), config.home.clone())
            .map_err(|error| std::io::Error::other(format!("corvus home: {error}")))?;
        layout
            .ensure()
            .map_err(|error| std::io::Error::other(format!("corvus home: {error}")))?;
        let store = Store::open(&db_path)
            .map_err(|error| std::io::Error::other(format!("recording store: {error}")))?;
        let store = Arc::new(store);
        let recorder = Recorder::start(Arc::clone(&store));

        let registry = Arc::new(Registry::new());
        let cdp = Arc::new(CdpClient::new());
        if !config.no_cdp {
            if let Some(endpoint) = config.cdp_endpoint.as_deref() {
                if let Err(error) = cdp.connect(endpoint) {
                    tracing::warn!(endpoint = %endpoint, error = %error, "cdp endpoint rejected");
                }
            }
        }
        let channel = Arc::new(ChannelState::new(
            config.channel,
            Arc::clone(&cdp),
            Arc::clone(&registry),
            !config.no_cdp,
        ));
        let (inbound_sender, inbound_receiver) = mpsc::channel(256);
        let router = Router::new(registry.clone() as Arc<dyn BrowserRegistry>, config.router);
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
            Arc::clone(&cdp),
            Arc::clone(&channel),
            recorder.sink(),
        ));
        let http_addr_slot = Arc::new(std::sync::OnceLock::<SocketAddr>::new());
        let health_registry = Arc::clone(&registry);
        let health_store = Arc::clone(&store);
        let health_metrics = Arc::clone(&metrics);
        let health_channel = Arc::clone(&channel);
        let health_http_addr = Arc::clone(&http_addr_slot);
        let health_db_path = db_path.display().to_string();
        let health: Arc<dyn Fn() -> Value + Send + Sync> = Arc::new(move || {
            let mut document = json!({
                "status": "ok",
                "ws_addr": ws_addr.to_string(),
                "browsers": health_registry.list(),
                "uptime_ms": health_metrics.uptime_ms(),
                "started_at": health_metrics.started_at_ms(),
                "tool_calls_total": health_metrics.calls(),
                "tool_errors_total": health_metrics.errors(),
                "channel": health_channel.status(),
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
        let auth = if config.auth.config.enabled() {
            let username = config.auth.username.clone().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "CORVUS_AUTH_USERNAME is required when authentication is enabled",
                )
            })?;
            let password = config.auth.password.clone().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "CORVUS_AUTH_PASSWORD is required when authentication is enabled",
                )
            })?;
            if username.trim().is_empty() || password.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "authentication bootstrap credentials must not be empty",
                ));
            }
            let accounts = Arc::new(corvus_api::auth::MemoryAuthRepository::new());
            accounts
                .add_user(&username, &password, "admin")
                .await
                .map_err(|error| {
                    std::io::Error::other(format!("auth bootstrap failed: {error}"))
                })?;
            Some(Arc::new(AuthService::new(
                config.auth.config.clone(),
                accounts,
            )))
        } else {
            None
        };
        let mcp = Arc::new(
            McpServer::new(Arc::clone(&backend))
                .with_store(
                    Arc::clone(&store) as Arc<dyn corvus_application::ports::SessionRecording>
                )
                .with_metrics(Arc::clone(&metrics))
                .with_health(Arc::clone(&health)),
        );
        let api_state = AppState::new(
            Arc::new(RegistryDirectory {
                registry: Arc::clone(&registry),
            }),
            Arc::new(StoreRecordings {
                store: Arc::clone(&store),
            }),
            Arc::clone(&backend),
            Arc::new(McpCatalog),
            Arc::clone(&health),
            auth,
            env!("CARGO_PKG_VERSION").to_string(),
        );

        let http = McpHttpServer::bind_with(
            SocketAddr::new(config.bind, config.http_port),
            Arc::clone(&mcp),
            corvus_api::router(api_state),
        )
        .await?;
        let http_addr = http.local_addr();
        let _ = http_addr_slot.set(http_addr);
        let http_task = http.spawn(shutdown_receiver.clone());
        let discovery_task = if !config.no_cdp && config.cdp_endpoint.is_none() {
            let cdp = Arc::clone(&cdp);
            let channel = Arc::clone(&channel);
            let registry = Arc::clone(&registry);
            let mut shutdown = shutdown_receiver.clone();
            Some(tokio::spawn(async move {
                let mut attempted: Option<String> = None;
                let mut retry_after: Option<Instant> = None;
                loop {
                    tokio::select! {
                        _ = shutdown.changed() => break,
                        _ = tokio::time::sleep(Duration::from_millis(500)) => {
                            if channel.mode() == ChannelMode::Extension || cdp.is_connected() {
                                continue;
                            }
                            let endpoint = match select_cdp_endpoint(
                                &registry.snapshot(),
                                attempted.as_deref(),
                            ) {
                                Ok(endpoint) => {
                                    channel.note_descriptor_error(None);
                                    endpoint
                                }
                                Err(error) => {
                                    channel.note_descriptor_error(Some(error));
                                    attempted = None;
                                    continue;
                                }
                            };
                            if retry_after.is_some_and(|deadline| Instant::now() < deadline) {
                                continue;
                            }
                            match cdp.connect(&endpoint) {
                                Ok(()) => attempted = Some(endpoint),
                                Err(error) => {
                                    tracing::warn!(endpoint = %endpoint, error = %error, "cdp endpoint rejected");
                                    retry_after = Some(Instant::now() + Duration::from_secs(2));
                                }
                            }
                        }
                    }
                }
            }))
        } else {
            None
        };

        tracing::info!(
            version = env!("CARGO_PKG_VERSION"),
            protocol = corvus_proto::PROTOCOL_VERSION,
            ws_addr = %ws_addr,
            mcp_url = %format!("http://{http_addr}/mcp"),
            db_path = %db_path.display(),
            "corvusd listening"
        );

        Ok(Self {
            registry,
            router,
            mcp,
            store,
            metrics,
            channel,
            home: layout,
            recorder,
            ws_addr,
            http_addr,
            shutdown: shutdown_sender,
            tasks: {
                let mut tasks = vec![router_task, ws_task, http_task];
                tasks.extend(discovery_task);
                tasks
            },
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

    /// The resolved Corvus home.
    pub fn home(&self) -> &home::HomeLayout {
        &self.home
    }

    /// The process-wide tool counters.
    pub fn metrics(&self) -> &Arc<Metrics> {
        &self.metrics
    }

    /// The browser channel state.
    pub fn channel(&self) -> &Arc<ChannelState> {
        &self.channel
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
                tracing::debug!("background task shutdown timed out");
            }
        }
        mcp.close_all_sessions("closed");
        recorder.shutdown().await;
    }
}

#[cfg(test)]
mod channel_routing_tests {
    use super::*;

    #[test]
    fn non_loopback_bind_without_auth_or_opt_out_is_refused() {
        let error = bind_auth_guard(std::net::IpAddr::from([0, 0, 0, 0]), false, false)
            .expect_err("a non-loopback bind must require auth");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("--allow-no-auth"));
    }

    #[test]
    fn allow_no_auth_opts_out_and_loopback_or_auth_never_warns() {
        assert!(
            bind_auth_guard(std::net::IpAddr::from([0, 0, 0, 0]), false, true)
                .expect("explicit opt-out is allowed")
        );
        assert!(
            !bind_auth_guard(std::net::IpAddr::from([127, 0, 0, 1]), false, false)
                .expect("loopback")
        );
        assert!(
            !bind_auth_guard(std::net::IpAddr::from([0, 0, 0, 0]), true, false)
                .expect("auth enabled")
        );
    }

    #[test]
    fn auto_mode_falls_back_only_for_unsupported_cdp_tools() {
        let unsupported = CdpError::Unsupported("not on cdp".into());
        assert!(fall_back_to_extension(ChannelMode::Auto, &unsupported));
        assert!(!fall_back_to_extension(ChannelMode::Cdp, &unsupported));
        assert!(!fall_back_to_extension(
            ChannelMode::Extension,
            &unsupported
        ));
        let timeout = CdpError::Timeout(1);
        assert!(!fall_back_to_extension(ChannelMode::Auto, &timeout));
        let gone = CdpError::NotConnected;
        assert!(!fall_back_to_extension(ChannelMode::Auto, &gone));
        let remote = CdpError::Remote {
            code: -32000,
            message: "navigation lost".into(),
        };
        assert!(!fall_back_to_extension(ChannelMode::Auto, &remote));
    }

    #[tokio::test]
    async fn forced_cdp_requires_a_connection_for_dom_tools() {
        let registry = Arc::new(Registry::new());
        let (outbound, _receiver) = mpsc::unbounded_channel();
        registry.register(Arc::new(corvus_transport::BrowserConnection::new(
            browser(
                "edge:default",
                "edge",
                "ws://127.0.0.1:9222/devtools/browser/edge",
            ),
            "connection-1",
            outbound,
        )));
        let cdp = Arc::new(CdpClient::new());
        let channel = Arc::new(ChannelState::new(
            ChannelMode::Cdp,
            Arc::clone(&cdp),
            Arc::clone(&registry),
            true,
        ));
        let router = Router::new(
            registry.clone() as Arc<dyn BrowserRegistry>,
            RouterConfig::default(),
        );
        let (result, recorded) =
            dispatch_tool(&cdp, &channel, &router, "page_snapshot", &json!({}), None).await;
        let error = result.expect_err("forced cdp needs a connected endpoint for DOM tools");
        assert_eq!(error.code, ErrorCode::CapabilityUnavailable);
        assert_eq!(error.message, "no CDP endpoint is connected");
        let details = error.details.expect("details");
        assert_eq!(details["transport"], "cdp");
        assert_eq!(details["channel"], "cdp");
        assert!(!recorded);
    }

    #[tokio::test]
    async fn browser_batch_runs_allowed_steps_and_honours_stop_on_error() {
        let registry = Arc::new(Registry::new());
        let cdp = Arc::new(CdpClient::new());
        let channel = Arc::new(ChannelState::new(
            ChannelMode::Auto,
            Arc::clone(&cdp),
            Arc::clone(&registry),
            true,
        ));
        let router = Router::new(
            registry.clone() as Arc<dyn BrowserRegistry>,
            RouterConfig::default(),
        );
        let (result, recorded) = run_batch(
            &cdp,
            &channel,
            &registry,
            &router,
            &json!({
                "steps": [
                    { "tool": "browser_list" },
                    { "tool": "browser_navigate", "args": { "url": "https://example.test/" } },
                    { "tool": "browser_list" }
                ],
                "stop_on_error": false
            }),
            None,
        )
        .await
        .map(|value| (value, true))
        .expect("a batch always returns a result envelope");
        assert!(recorded);
        assert_eq!(result["ok"], false);
        assert_eq!(result["count"], 3);
        assert_eq!(result["stopped"], false);
        let results = result["results"].as_array().expect("step results");
        assert_eq!(results.len(), 3);
        assert_eq!(results[0]["step"], 1);
        assert_eq!(results[0]["ok"], true);
        assert_eq!(results[1]["step"], 2);
        assert_eq!(results[1]["ok"], false);
        assert_eq!(results[1]["error"]["code"], "TRANSPORT_DOWN");
        assert_eq!(results[2]["step"], 3);
        assert_eq!(results[2]["ok"], true);

        let stopped = run_batch(
            &cdp,
            &channel,
            &registry,
            &router,
            &json!({
                "steps": [
                    { "tool": "browser_navigate", "args": { "url": "https://example.test/" } },
                    { "tool": "browser_list" }
                ]
            }),
            None,
        )
        .await
        .expect("a failing first step is still an envelope");
        assert_eq!(stopped["ok"], false);
        assert_eq!(stopped["count"], 1);
        assert_eq!(stopped["stopped"], true);
    }

    #[tokio::test]
    async fn browser_batch_refuses_recursion_at_the_executor() {
        let registry = Arc::new(Registry::new());
        let cdp = Arc::new(CdpClient::new());
        let channel = Arc::new(ChannelState::new(
            ChannelMode::Auto,
            Arc::clone(&cdp),
            Arc::clone(&registry),
            true,
        ));
        let router = Router::new(
            registry.clone() as Arc<dyn BrowserRegistry>,
            RouterConfig::default(),
        );
        let error = run_batch(
            &cdp,
            &channel,
            &registry,
            &router,
            &json!({ "steps": [{ "tool": "browser_batch", "args": { "steps": [] } }] }),
            None,
        )
        .await
        .expect_err("browser_batch must never recurse");
        assert_eq!(error.code, ErrorCode::BadArgs);
        assert_eq!(error.message, "browser_batch cannot call itself");
    }

    #[tokio::test]
    async fn forced_extension_still_rejects_cdp_only_tools() {
        let registry = Arc::new(Registry::new());
        let cdp = Arc::new(CdpClient::new());
        let channel = Arc::new(ChannelState::new(
            ChannelMode::Extension,
            Arc::clone(&cdp),
            Arc::clone(&registry),
            true,
        ));
        let router = Router::new(registry, RouterConfig::default());
        let (result, recorded) =
            dispatch_tool(&cdp, &channel, &router, "cdp_status", &json!({}), None).await;
        let error = result.expect_err("extension mode must reject cdp-only tools");
        assert_eq!(error.code, ErrorCode::CapabilityUnavailable);
        let details = error.details.expect("details");
        assert_eq!(details["transport"], "cdp");
        assert_eq!(details["channel"], "extension");
        assert!(!recorded);
    }

    #[tokio::test]
    async fn auto_mode_routes_extension_only_tools_to_the_extension() {
        let registry = Arc::new(Registry::new());
        let cdp = Arc::new(CdpClient::new());
        let channel = Arc::new(ChannelState::new(
            ChannelMode::Auto,
            Arc::clone(&cdp),
            Arc::clone(&registry),
            true,
        ));
        let router = Router::new(registry, RouterConfig::default());
        let (result, recorded) =
            dispatch_tool(&cdp, &channel, &router, "page_snapshot", &json!({}), None).await;
        let error = result.expect_err("no extension is connected in this test");
        assert_eq!(error.code, ErrorCode::TransportDown);
        assert!(!recorded);
    }

    #[tokio::test]
    async fn extension_only_tools_reject_cdp_target_ids() {
        let registry = Arc::new(Registry::new());
        let cdp = Arc::new(CdpClient::new());
        let channel = Arc::new(ChannelState::new(
            ChannelMode::Extension,
            Arc::clone(&cdp),
            Arc::clone(&registry),
            true,
        ));
        let router = Router::new(registry, RouterConfig::default());
        let target = "0123456789abcdef0123456789abcdef";
        let (result, recorded) = dispatch_tool(
            &cdp,
            &channel,
            &router,
            "page_snapshot",
            &json!({ "tab_id": target }),
            None,
        )
        .await;
        let error = result.expect_err("a CDP target id is not an extension tab id");
        assert_eq!(error.code, ErrorCode::BadArgs);
        assert_eq!(error.message, EXTENSION_TAB_HINT);
        let details = error.details.expect("details");
        assert_eq!(details["hint"], EXTENSION_TAB_HINT);
        assert_eq!(details["tab_id"], target);
        assert!(!recorded);
    }

    #[tokio::test]
    async fn cdp_only_tools_reject_numeric_extension_tab_ids() {
        let registry = Arc::new(Registry::new());
        let cdp = Arc::new(CdpClient::new());
        let channel = Arc::new(ChannelState::new(
            ChannelMode::Cdp,
            Arc::clone(&cdp),
            Arc::clone(&registry),
            true,
        ));
        let router = Router::new(registry, RouterConfig::default());
        let (result, recorded) = dispatch_tool(
            &cdp,
            &channel,
            &router,
            "browser_console",
            &json!({ "tab_id": 7 }),
            None,
        )
        .await;
        let error = result.expect_err("a numeric extension tab id is not a CDP target id");
        assert_eq!(error.code, ErrorCode::BadArgs);
        assert_eq!(error.message, CDP_TAB_HINT);
        let details = error.details.expect("details");
        assert_eq!(details["hint"], CDP_TAB_HINT);
        assert_eq!(details["tab_id"], 7);
        assert!(!recorded);
    }

    #[tokio::test]
    async fn cdp_tools_reject_numeric_extension_tab_ids() {
        let registry = Arc::new(Registry::new());
        let cdp = Arc::new(CdpClient::new());
        let channel = Arc::new(ChannelState::new(
            ChannelMode::Cdp,
            Arc::clone(&cdp),
            Arc::clone(&registry),
            true,
        ));
        let router = Router::new(registry, RouterConfig::default());
        let (result, recorded) = dispatch_tool(
            &cdp,
            &channel,
            &router,
            "browser_navigate",
            &json!({ "url": "https://example.test/", "tab_id": 7 }),
            None,
        )
        .await;
        let error = result.expect_err("a numeric extension tab id is not a CDP target id");
        assert_eq!(error.code, ErrorCode::BadArgs);
        assert_eq!(error.message, CDP_TAB_HINT);
        let details = error.details.expect("details");
        assert_eq!(details["hint"], CDP_TAB_HINT);
        assert_eq!(details["tab_id"], 7);
        assert!(!recorded);
    }

    fn browser(
        profile: &str,
        family: &str,
        endpoint: &str,
    ) -> corvus_transport::registry::BrowserInfo {
        corvus_transport::registry::BrowserInfo {
            id: profile.to_string(),
            profile: profile.to_string(),
            browser_family: Some(family.to_string()),
            version: "1.1.0".into(),
            extension: None,
            cdp: Some(corvus_proto::CdpDescriptor {
                available: true,
                web_socket_debugger_url: Some(endpoint.to_string()),
                browser_family: Some(family.to_string()),
                source: Some("auto".into()),
                ..Default::default()
            }),
            capabilities: vec![corvus_proto::capability::CDP.to_string()],
            connected_at_ms: 0,
            remote: "127.0.0.1:1".into(),
        }
    }

    #[test]
    fn discovery_skips_an_invalid_browser_and_uses_the_next_valid_one() {
        let mut starved = browser(
            "edge:second",
            "edge",
            "ws://127.0.0.1:9223/devtools/browser/ignored",
        );
        starved.cdp = None;
        let mut invalid = browser(
            "edge:default",
            "chrome",
            "ws://127.0.0.1:9222/devtools/browser/edge",
        );
        invalid
            .cdp
            .as_mut()
            .expect("invalid browser advertises cdp")
            .browser_family = Some("edge".into());
        let valid = browser(
            "chrome:default",
            "chromium",
            "ws://127.0.0.1:9444/devtools/browser/chrome",
        );
        let selected = select_cdp_endpoint(&[starved, invalid, valid], None).unwrap();
        assert_eq!(selected, "ws://127.0.0.1:9444/devtools/browser/chrome");
        assert!(select_cdp_endpoint(&[], None).is_err());
    }

    #[test]
    fn discovery_accepts_a_random_profile_when_the_handshake_family_matches() {
        let endpoint = "ws://127.0.0.1:9333/devtools/browser/chrome";
        let profile = "8f14e45fceea167a5a36dedd4bea2543";
        let matching = browser(profile, "chrome", endpoint);
        assert_eq!(
            select_cdp_endpoint(std::slice::from_ref(&matching), None)
                .expect("chrome family matches"),
            endpoint
        );
        let mut mismatched = matching;
        mismatched
            .cdp
            .as_mut()
            .expect("mismatched browser advertises cdp")
            .browser_family = Some("edge".into());
        let error = select_cdp_endpoint(&[mismatched], None)
            .expect_err("edge descriptor must not match chrome family");
        assert!(error.contains("does not match"), "{error}");
    }

    #[tokio::test]
    async fn shared_extension_tools_reject_cdp_target_ids() {
        let registry = Arc::new(Registry::new());
        let cdp = Arc::new(CdpClient::new());
        let channel = Arc::new(ChannelState::new(
            ChannelMode::Extension,
            Arc::clone(&cdp),
            Arc::clone(&registry),
            true,
        ));
        let router = Router::new(registry, RouterConfig::default());
        let target = "0123456789abcdef0123456789abcdef";
        let (result, recorded) = dispatch_tool(
            &cdp,
            &channel,
            &router,
            "tab_close",
            &json!({ "tab_id": target }),
            None,
        )
        .await;
        let error = result.expect_err("a CDP target id is not an extension tab id");
        assert_eq!(error.code, ErrorCode::BadArgs);
        assert_eq!(error.message, EXTENSION_TAB_HINT);
        assert!(!recorded);
    }

    #[tokio::test]
    async fn shared_cdp_tools_reject_numeric_extension_tab_ids() {
        let registry = Arc::new(Registry::new());
        let cdp = Arc::new(CdpClient::new());
        let channel = Arc::new(ChannelState::new(
            ChannelMode::Cdp,
            Arc::clone(&cdp),
            Arc::clone(&registry),
            true,
        ));
        let router = Router::new(registry, RouterConfig::default());
        let (result, recorded) = dispatch_tool(
            &cdp,
            &channel,
            &router,
            "tab_close",
            &json!({ "tab_id": 7 }),
            None,
        )
        .await;
        let error = result.expect_err("a numeric extension tab id is not a CDP target id");
        assert_eq!(error.code, ErrorCode::BadArgs);
        assert_eq!(error.message, CDP_TAB_HINT);
        assert!(!recorded);
    }

    #[test]
    fn absent_dialog_maps_to_not_found() {
        let body = cdp_error_body(CdpError::DialogNotFound);
        assert_eq!(body.code, ErrorCode::NotFound);
        assert_eq!(body.message, "no dialog is showing");
        assert_eq!(body.details.expect("details")["transport"], "cdp");
    }

    #[test]
    fn unclickable_element_maps_to_not_interactable() {
        let body = cdp_error_body(CdpError::NotInteractable("element is covered".into()));
        assert_eq!(body.code, ErrorCode::NotInteractable);
        assert_eq!(
            body.message,
            "element is not interactable: element is covered"
        );
        let details = body.details.expect("details");
        assert_eq!(details["transport"], "cdp");
        assert_eq!(details["cdp_code"], "NOT_INTERACTABLE");
    }
}
