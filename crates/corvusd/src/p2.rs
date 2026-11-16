//! v2 capability-mode runtime.
//!
//! This module owns everything that is dynamic in the ten-tool surface: the
//! live browser graph, the opaque reference store, provider/worker selection,
//! screenshot finalisation and the bridge to the MCP server. It deliberately
//! owns no transport code and speaks only the frozen internal action names.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use base64::Engine as _;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use corvus_application::ports::ToolCatalog;
use corvus_application::{now_millis, SessionContext, StepRecord, ToolCall};
use corvus_cdp::{CdpClient, CdpStatus, P2ActionError};
use corvus_mcp::surface::ToolSurface;
use corvus_mcp::surface::{
    definition, group_visible, validate_args, GROUPS, PUBLIC_TOOLS, SURFACE_INSTRUCTIONS,
};
use corvus_mcp::{BackendFuture, ToolBackend};
use corvus_proto::{ErrorBody, ErrorCode};
use corvus_provider::BrowserInfo;
use corvus_record::StepSink;
use corvus_worker::{WorkerError, WorkerStatus, WorkerSupervisor};

use crate::guardrails::Guardrails;
use crate::provider_gateway::{ProviderGateway, ProviderGatewayError, ProviderSnapshot};

const MAX_SCREENSHOT_BYTES: usize = 8 * 1024 * 1024;
const MAX_SCREENSHOT_BASE64: usize = MAX_SCREENSHOT_BYTES.div_ceil(3) * 4;
const DEFAULT_ACTION_TIMEOUT: Duration = Duration::from_secs(30);

/// Capability groups selected for the whole graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Provider,
    Worker,
    Direct,
}

#[derive(Debug, Clone)]
struct BrowserView {
    browser_id: String,
    family: String,
    version: String,
    profile: String,
    provider_id: Option<String>,
    sources: BTreeMap<&'static str, Source>,
}

#[derive(Debug, Clone, Default)]
struct Graph {
    browsers: Vec<BrowserView>,
    groups: BTreeMap<&'static str, bool>,
    signature: String,
    revision: u64,
}

impl Graph {
    fn enabled(&self) -> Vec<&'static str> {
        GROUPS
            .iter()
            .copied()
            .filter(|group| self.groups.get(group).copied().unwrap_or(false))
            .collect()
    }

    fn tool_names(&self) -> Vec<&'static str> {
        let enabled = self.enabled();
        PUBLIC_TOOLS
            .iter()
            .copied()
            .filter(|name| {
                *name == "browser_status"
                    || corvus_mcp::surface::tool_group(name)
                        .is_some_and(|group| group_visible(group, &enabled))
            })
            .collect()
    }

    fn signature(&self) -> String {
        self.tool_names().join(",")
    }
}

#[derive(Default)]
struct RefStore {
    /// (browser_id, tab_id) -> provider reference -> opaque reference.
    maps: HashMap<(String, String), HashMap<String, String>>,
}

impl RefStore {
    fn replace(&mut self, browser_id: &str, tab_id: &str, provider_refs: &[String]) -> Vec<String> {
        let mut map = HashMap::new();
        let mut used = std::collections::HashSet::new();
        let mut opaque = Vec::with_capacity(provider_refs.len());
        for provider_ref in provider_refs {
            let mut candidate = String::new();
            for _ in 0..8 {
                let token = uuid::Uuid::new_v4().simple().to_string();
                candidate = format!("r-{}", &token[..8]);
                if !used.contains(&candidate) {
                    break;
                }
            }
            if candidate.is_empty() {
                candidate = format!("r-{:08x}", used.len());
            }
            used.insert(candidate.clone());
            map.insert(provider_ref.clone(), candidate.clone());
            opaque.push(candidate);
        }
        self.maps
            .insert((browser_id.to_string(), tab_id.to_string()), map);
        opaque
    }

    fn resolve(
        &self,
        browser_id: &str,
        tab_id: Option<&str>,
        opaque_ref: &str,
    ) -> Option<(String, String)> {
        if let Some(tab_id) = tab_id {
            let map = self
                .maps
                .get(&(browser_id.to_string(), tab_id.to_string()))?;
            return map
                .iter()
                .find(|(_, value)| value.as_str() == opaque_ref)
                .map(|(provider, _)| (tab_id.to_string(), provider.clone()));
        }
        self.maps
            .iter()
            .filter(|((browser, _), _)| browser == browser_id)
            .find_map(|((_, tab), map)| {
                map.iter()
                    .find(|(_, value)| value.as_str() == opaque_ref)
                    .map(|(provider, _)| (tab.clone(), provider.clone()))
            })
    }

    fn clear_tab(&mut self, browser_id: &str, tab_id: &str) {
        self.maps
            .remove(&(browser_id.to_string(), tab_id.to_string()));
    }
}

/// The live v2 runtime shared by MCP, REST and the background graph task.
pub struct P2Runtime {
    gateway: Arc<ProviderGateway>,
    cdp: Arc<CdpClient>,
    home: PathBuf,
    worker: Mutex<Option<WorkerSupervisor>>,
    graph: RwLock<Graph>,
    refs: Mutex<RefStore>,
    sink: StepSink,
    guardrails: Option<Arc<Guardrails>>,
}

impl P2Runtime {
    /// Build the runtime, starting the supervised browser worker when the
    /// activation variables are present. A worker failure is logged and the
    /// runtime still works for an already-connected browser endpoint.
    pub async fn new(
        gateway: Arc<ProviderGateway>,
        cdp: Arc<CdpClient>,
        home: PathBuf,
        sink: StepSink,
        guardrails: Option<Arc<Guardrails>>,
    ) -> Arc<Self> {
        let worker = match WorkerSupervisor::start_from_env().await {
            Ok(worker) => worker,
            Err(error) => {
                tracing::warn!(error = %error, "browser worker did not start");
                None
            }
        };
        let runtime = Arc::new(Self {
            gateway,
            cdp,
            home,
            worker: Mutex::new(worker),
            graph: RwLock::new(Graph::default()),
            refs: Mutex::new(RefStore::default()),
            sink,
            guardrails,
        });
        let graph = runtime.build_graph().await;
        runtime.publish(graph);
        runtime
    }

    /// Snapshot of the current browser graph, for tests and diagnostics.
    fn graph(&self) -> Graph {
        self.graph
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Effective public tool list.
    pub fn list_tools(&self) -> Vec<Value> {
        let graph = self.graph();
        graph.tool_names().into_iter().map(definition).collect()
    }

    /// Validate against the current graph.
    pub fn validate_tool(&self, name: &str, arguments: &Value) -> Result<Value, ErrorBody> {
        let graph = self.graph();
        let enabled = graph.enabled();
        validate_args(name, arguments, &enabled)
    }

    fn publish(&self, graph: Graph) {
        let mut slot = self
            .graph
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *slot = graph;
    }

    /// Rebuild the graph from the provider registry and browser endpoints.
    async fn refresh(&self) {
        let graph = self.build_graph().await;
        self.publish(graph);
    }

    async fn build_graph(&self) -> Graph {
        let providers = self.gateway.snapshot().await;
        let worker_status = {
            let worker = self.worker.lock().await;
            worker.as_ref().map(WorkerSupervisor::status)
        };
        let cdp_status = self.cdp.status();
        let mut browsers = Vec::new();

        let mut providers = providers;
        providers.sort_by(|left, right| left.provider_id.cmp(&right.provider_id));
        for provider in providers {
            let browser_id = browser_id_for(&provider.provider_id);
            let groups = provider_groups(&provider);
            let (family, version, profile) = provider_identity(&provider.browser);
            browsers.push(BrowserView {
                browser_id,
                family,
                version,
                profile,
                provider_id: Some(provider.provider_id.clone()),
                sources: provider_sources(&groups),
            });
        }

        browsers.extend(select_browser_views(worker_status.as_ref(), &cdp_status));

        browsers.sort_by(|left, right| left.browser_id.cmp(&right.browser_id));

        let groups = GROUPS
            .iter()
            .map(|group| {
                (
                    *group,
                    browsers
                        .iter()
                        .any(|browser| browser.sources.contains_key(group)),
                )
            })
            .collect();

        let mut graph = Graph {
            browsers,
            groups,
            signature: String::new(),
            revision: 0,
        };
        let signature = graph.signature();
        let revision = {
            let current = self
                .graph
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if current.signature == signature {
                current.revision
            } else {
                current.revision.saturating_add(1)
            }
        };
        graph.signature = signature;
        graph.revision = revision;
        graph
    }

    async fn invoke(
        &self,
        source: Source,
        provider_id: Option<&str>,
        action: &str,
        args: &Value,
        timeout: Duration,
    ) -> Result<Value, P2Failure> {
        match source {
            Source::Provider => {
                let provider_id = provider_id.ok_or_else(|| {
                    P2Failure::new("PROVIDER_LOST", "the browser connection was lost")
                })?;
                let result = self
                    .gateway
                    .invoke(provider_id, action, args.clone(), timeout)
                    .await
                    .map_err(P2Failure::from_gateway)?;
                if result.ok {
                    result
                        .value
                        .ok_or_else(|| P2Failure::internal("the browser returned no result"))
                } else {
                    let code = result
                        .error
                        .as_ref()
                        .map(|error| error.code.as_str())
                        .unwrap_or("INTERNAL");
                    Err(P2Failure::from_code(code))
                }
            }
            Source::Worker => {
                let mut worker = self.worker.lock().await;
                let worker = worker.as_mut().ok_or_else(|| {
                    P2Failure::new("PROVIDER_LOST", "the browser connection was lost")
                })?;
                worker
                    .invoke(
                        action,
                        args.clone(),
                        timeout.as_millis().clamp(1, 300_000) as u64,
                    )
                    .await
                    .map_err(P2Failure::from_worker)
            }
            Source::Direct => self
                .cdp
                .worker_action(action, args, timeout.as_millis().clamp(1, 300_000) as u64)
                .await
                .map_err(P2Failure::from_cdp),
        }
    }

    async fn call_tool(
        &self,
        name: &str,
        arguments: &Value,
        session: Option<&SessionContext>,
    ) -> Result<Value, ErrorBody> {
        let started = std::time::Instant::now();
        if let Some(guardrails) = &self.guardrails {
            if let Err(error) = guardrails
                .check_tool(name)
                .and_then(|()| guardrails.check_arguments(name, arguments))
            {
                self.record(session, name, arguments, Err(&error), started);
                return Err(error);
            }
        }
        let normalized = match self.validate_tool(name, arguments) {
            Ok(normalized) => normalized,
            Err(error) => {
                self.record(session, name, arguments, Err(&error), started);
                return Err(error);
            }
        };
        let outcome = self
            .dispatch(name, &normalized)
            .await
            .map_err(|error| error.body());
        let outcome = match (&self.guardrails, outcome) {
            (Some(guardrails), result) => guardrails.apply_result(name, result),
            (None, result) => result,
        };
        match outcome {
            Ok(result) => {
                self.record(session, name, &normalized, Ok(&result), started);
                Ok(result)
            }
            Err(error) => {
                self.record(session, name, &normalized, Err(&error), started);
                Err(error)
            }
        }
    }

    fn record(
        &self,
        session: Option<&SessionContext>,
        tool: &str,
        arguments: &Value,
        result: Result<&Value, &ErrorBody>,
        started: std::time::Instant,
    ) {
        let Some(session) = session else {
            return;
        };
        let (result_value, error_code, ok) = match result {
            Ok(value) => {
                let mut value = value.clone();
                strip_image_data(&mut value);
                (Some(value), None, true)
            }
            Err(error) => (None, Some(error.code.as_str().to_string()), false),
        };
        self.sink.record(StepRecord {
            session_id: session.session_id.clone(),
            ts: corvus_application::now_millis(),
            tool: tool.to_string(),
            args: arguments.clone(),
            result: result_value,
            error_code,
            duration_ms: started.elapsed().as_millis() as i64,
            browser_id: arguments
                .get("browser_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            tab_id: arguments
                .get("tab_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            ok,
        });
    }

    async fn dispatch(&self, name: &str, args: &Value) -> Result<Value, P2Failure> {
        let graph = self.graph();
        if name == "browser_status" {
            return self.browser_status(&graph, args);
        }
        let group = corvus_mcp::surface::tool_group(name)
            .ok_or_else(|| P2Failure::bad_args("unknown tool"))?;
        if !group_visible(group, &graph.enabled()) {
            return Err(P2Failure::new(
                "CAPABILITY_UNAVAILABLE",
                "the browser cannot perform this action",
            ));
        }
        let browser = select_browser(&graph, args.get("browser_id").and_then(Value::as_str))?;
        let source = browser.sources.get(group).copied().ok_or_else(|| {
            P2Failure::new(
                "CAPABILITY_UNAVAILABLE",
                "the browser cannot perform this action",
            )
        })?;
        match name {
            "tab_list" => {
                let value = self
                    .invoke(
                        source,
                        browser.provider_id.as_deref(),
                        "tabs.list",
                        &json!({}),
                        DEFAULT_ACTION_TIMEOUT,
                    )
                    .await?;
                normalize_tab_list(&browser.browser_id, value)
            }
            "tab_open" => {
                let payload = json!({
                    "url": args["url"],
                    "active": args["active"],
                });
                let value = self
                    .invoke(
                        source,
                        browser.provider_id.as_deref(),
                        "tabs.open",
                        &payload,
                        DEFAULT_ACTION_TIMEOUT,
                    )
                    .await?;
                normalize_tab(&browser.browser_id, value)
            }
            "tab_close" => {
                let tab_id = args["tab_id"].as_str().unwrap_or_default();
                let payload = json!({ "tab_id": tab_id });
                let value = self
                    .invoke(
                        source,
                        browser.provider_id.as_deref(),
                        "tabs.close",
                        &payload,
                        DEFAULT_ACTION_TIMEOUT,
                    )
                    .await?;
                self.refs
                    .lock()
                    .await
                    .clear_tab(&browser.browser_id, tab_id);
                normalize_tab(&browser.browser_id, value)
            }
            "page_navigate" => {
                let timeout = Duration::from_millis(args["timeout_ms"].as_u64().unwrap_or(30_000));
                let requested_tab = args.get("tab_id").and_then(Value::as_str);
                let mut payload = json!({
                    "url": args["url"],
                    "wait_until": args["wait_until"],
                    "timeout_ms": args["timeout_ms"],
                });
                if let Some(tab_id) = requested_tab {
                    payload["tab_id"] = json!(tab_id);
                }
                let value = self
                    .invoke(
                        source,
                        browser.provider_id.as_deref(),
                        "page.navigate",
                        &payload,
                        timeout,
                    )
                    .await?;
                let navigated_tab = value
                    .get("tab_id")
                    .and_then(Value::as_str)
                    .or(requested_tab);
                if let Some(tab_id) = navigated_tab {
                    self.refs
                        .lock()
                        .await
                        .clear_tab(&browser.browser_id, tab_id);
                }
                normalize_tab(&browser.browser_id, value)
            }
            "page_read" => {
                let mut payload = json!({
                    "mode": args["mode"],
                    "max_chars": args["max_chars"],
                });
                if let Some(tab_id) = args.get("tab_id") {
                    payload["tab_id"] = tab_id.clone();
                }
                if let Some(selector) = args.get("selector") {
                    payload["selector"] = selector.clone();
                }
                let value = self
                    .invoke(
                        source,
                        browser.provider_id.as_deref(),
                        "page.read",
                        &payload,
                        DEFAULT_ACTION_TIMEOUT,
                    )
                    .await?;
                normalize_tab(&browser.browser_id, value)
            }
            "page_snapshot" => {
                let mut payload = json!({
                    "max_nodes": args["max_nodes"],
                    "include_hidden": args["include_hidden"],
                });
                if let Some(tab_id) = args.get("tab_id") {
                    payload["tab_id"] = tab_id.clone();
                }
                let value = self
                    .invoke(
                        source,
                        browser.provider_id.as_deref(),
                        "page.snapshot",
                        &payload,
                        DEFAULT_ACTION_TIMEOUT,
                    )
                    .await?;
                self.normalize_snapshot(&browser.browser_id, value).await
            }
            "element_click" | "element_type" => {
                let opaque = args["ref"].as_str().unwrap_or_default();
                let requested_tab = args.get("tab_id").and_then(Value::as_str);
                let (tab_id, provider_ref) = self
                    .refs
                    .lock()
                    .await
                    .resolve(&browser.browser_id, requested_tab, opaque)
                    .ok_or_else(|| {
                        P2Failure::new(
                            "NOT_FOUND",
                            "the reference is not known; take a new snapshot",
                        )
                    })?;
                let mut payload = json!({
                    "ref": provider_ref,
                    "tab_id": tab_id,
                });
                if name == "element_click" {
                    let value = self
                        .invoke(
                            source,
                            browser.provider_id.as_deref(),
                            "input.click",
                            &payload,
                            DEFAULT_ACTION_TIMEOUT,
                        )
                        .await?;
                    let mut value = normalize_tab(&browser.browser_id, value)?;
                    value["ref"] = json!(opaque);
                    Ok(value)
                } else {
                    payload["text"] = args["text"].clone();
                    payload["clear"] = args["clear"].clone();
                    payload["submit"] = args["submit"].clone();
                    let value = self
                        .invoke(
                            source,
                            browser.provider_id.as_deref(),
                            "input.type",
                            &payload,
                            DEFAULT_ACTION_TIMEOUT,
                        )
                        .await?;
                    let mut value = normalize_tab(&browser.browser_id, value)?;
                    value["ref"] = json!(opaque);
                    Ok(value)
                }
            }
            "page_screenshot" => {
                if args["full_page"].as_bool().unwrap_or(false) {
                    return Err(P2Failure::new(
                        "CAPABILITY_UNAVAILABLE",
                        "full_page is not available",
                    ));
                }
                let mut payload = json!({
                    "format": args["format"],
                });
                if args["format"].as_str() == Some("jpeg") {
                    payload["quality"] = args["quality"].clone();
                }
                if let Some(tab_id) = args.get("tab_id") {
                    payload["tab_id"] = tab_id.clone();
                }
                let value = self
                    .invoke(
                        source,
                        browser.provider_id.as_deref(),
                        "capture.viewport",
                        &payload,
                        DEFAULT_ACTION_TIMEOUT,
                    )
                    .await?;
                self.finalize_screenshot(
                    &browser.browser_id,
                    value,
                    args["inline"].as_bool().unwrap_or(false),
                )
                .await
            }
            _ => Err(P2Failure::bad_args("unknown tool")),
        }
    }

    fn browser_status(&self, graph: &Graph, args: &Value) -> Result<Value, P2Failure> {
        let requested = args.get("browser_id").and_then(Value::as_str);
        let browsers: Vec<Value> = graph
            .browsers
            .iter()
            .filter(|browser| requested.is_none_or(|id| id == browser.browser_id))
            .map(|browser| {
                json!({
                    "browser_id": browser.browser_id,
                    "family": browser.family,
                    "version": browser.version,
                    "profile": browser.profile,
                    "connected": true,
                })
            })
            .collect();
        if requested.is_some() && browsers.is_empty() {
            return Err(P2Failure::bad_args("unknown browser_id"));
        }
        let mut value = json!({
            "browsers": browsers,
            "count": browsers.len(),
            "capabilities": GROUPS
                .iter()
                .map(|group| json!({
                    "group": group,
                    "available": graph.groups.get(group).copied().unwrap_or(false),
                }))
                .collect::<Vec<_>>(),
        });
        if value["count"].as_u64().unwrap_or(0) == 0 {
            value["hint"] =
                json!("No browser session is connected. Connect a supported browser and retry.");
        }
        Ok(value)
    }

    async fn normalize_snapshot(&self, browser_id: &str, value: Value) -> Result<Value, P2Failure> {
        let tab_id = value
            .get("tab_id")
            .and_then(Value::as_str)
            .ok_or_else(|| P2Failure::internal("the browser returned no tab"))?
            .to_string();
        let nodes = value
            .get("nodes")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| P2Failure::internal("the browser returned no nodes"))?;
        let refs: Vec<String> = nodes
            .iter()
            .map(|node| {
                node.get("ref")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| P2Failure::internal("the browser returned an unusable node"))
            })
            .collect::<Result<_, _>>()?;
        let opaque = self.refs.lock().await.replace(browser_id, &tab_id, &refs);
        let mut value = value;
        let nodes = value["nodes"].as_array_mut().expect("nodes array");
        for (node, opaque) in nodes.iter_mut().zip(opaque) {
            node["ref"] = json!(opaque);
        }
        value["browser_id"] = json!(browser_id);
        Ok(value)
    }

    async fn finalize_screenshot(
        &self,
        browser_id: &str,
        value: Value,
        inline: bool,
    ) -> Result<Value, P2Failure> {
        let data = value
            .get("data")
            .and_then(Value::as_str)
            .ok_or_else(|| P2Failure::internal("the browser returned no image"))?;
        if data.len() > MAX_SCREENSHOT_BASE64 {
            return Err(P2Failure::internal("Screenshot exceeds the 8 MiB limit."));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| P2Failure::internal("the browser returned an invalid image"))?;
        if bytes.len() > MAX_SCREENSHOT_BYTES {
            return Err(P2Failure::internal("Screenshot exceeds the 8 MiB limit."));
        }
        let format = value.get("format").and_then(Value::as_str).unwrap_or("png");
        if format != "png" && format != "jpeg" {
            return Err(P2Failure::internal(
                "the browser returned an unusable image",
            ));
        }
        let extension = if format == "png" { "png" } else { "jpg" };
        let mime_type = if format == "png" {
            "image/png"
        } else {
            "image/jpeg"
        };
        let width = value
            .get("width")
            .and_then(Value::as_u64)
            .ok_or_else(|| P2Failure::internal("the browser returned no image size"))?;
        let height = value
            .get("height")
            .and_then(Value::as_u64)
            .ok_or_else(|| P2Failure::internal("the browser returned no image size"))?;
        let directory = self.home.join("screenshots");
        std::fs::create_dir_all(&directory)
            .map_err(|_| P2Failure::internal("the screenshot could not be saved"))?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let path = directory.join(format!("shot-{id}.{extension}"));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| P2Failure::internal("the screenshot could not be saved"))?;
        use std::io::Write as _;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| P2Failure::internal("the screenshot could not be saved"))?;
        let sha256 = hex::encode(Sha256::digest(&bytes));
        let summary = format!("{width}x{height} {format} {} bytes", bytes.len());
        let path_text = path.to_string_lossy().to_string();
        let mut result = json!({
            "browser_id": browser_id,
            "tab_id": value.get("tab_id").cloned().unwrap_or_else(|| json!("")),
            "url": value.get("url").cloned().unwrap_or_else(|| json!("")),
            "format": format,
            "mime_type": mime_type,
            "width": width,
            "height": height,
            "bytes": bytes.len(),
            "sha256": sha256,
            "path": path_text,
            "inline": inline,
            "summary": summary,
        });
        if inline {
            result["image"] = json!({ "mime_type": mime_type, "data": data });
        }
        Ok(result)
    }

    /// Shut down the supervised worker, if one was started.
    pub async fn shutdown(&self) {
        let worker = self.worker.lock().await.take();
        if let Some(worker) = worker {
            worker.shutdown().await;
        }
    }
}

impl ToolSurface for P2Runtime {
    fn instructions(&self) -> &'static str {
        SURFACE_INSTRUCTIONS
    }

    fn tools(&self) -> Vec<Value> {
        self.list_tools()
    }

    fn validate(&self, name: &str, arguments: &Value) -> Result<Value, ErrorBody> {
        self.validate_tool(name, arguments)
    }

    fn signature(&self) -> String {
        self.graph().signature
    }
}

impl ToolCatalog for P2Runtime {
    fn definitions_for(&self, _category: Option<&str>, _detail: &str) -> Vec<Value> {
        self.list_tools()
    }

    fn validate(&self, name: &str, arguments: &Value) -> Result<Value, ErrorBody> {
        self.validate_tool(name, arguments)
    }
}

fn all_sources(source: Source) -> BTreeMap<&'static str, Source> {
    GROUPS.iter().map(|group| (*group, source)).collect()
}

fn provider_sources(groups: &[&'static str]) -> BTreeMap<&'static str, Source> {
    let has_page = groups.contains(&"page");
    groups
        .iter()
        .copied()
        .filter(|group| *group != "input" || has_page)
        .map(|group| (group, Source::Provider))
        .collect()
}

fn worker_browser_view(status: &WorkerStatus) -> BrowserView {
    let (family, version) = status
        .browser
        .as_ref()
        .map(|browser| split_product(&browser.version))
        .unwrap_or_else(|| ("unknown".to_string(), "unknown".to_string()));
    BrowserView {
        browser_id: "b-cd000001".to_string(),
        family,
        version,
        profile: "default".to_string(),
        provider_id: None,
        sources: all_sources(Source::Worker),
    }
}

fn direct_browser_view(status: &CdpStatus) -> BrowserView {
    let (family, version) = status
        .browser
        .as_deref()
        .map(split_product)
        .unwrap_or_else(|| ("unknown".to_string(), "unknown".to_string()));
    BrowserView {
        browser_id: "b-cd000002".to_string(),
        family,
        version,
        profile: "default".to_string(),
        provider_id: None,
        sources: all_sources(Source::Direct),
    }
}

/// A connected worker is the canonical view of its browser. The daemon may
/// also hold a direct endpoint to that same browser, but exposing both would
/// force every default call to choose between duplicate ids.
fn select_browser_views(worker: Option<&WorkerStatus>, direct: &CdpStatus) -> Vec<BrowserView> {
    if let Some(status) = worker.filter(|status| status.connected) {
        return vec![worker_browser_view(status)];
    }
    if direct.connected {
        vec![direct_browser_view(direct)]
    } else {
        Vec::new()
    }
}

fn select_browser<'a>(
    graph: &'a Graph,
    browser_id: Option<&str>,
) -> Result<&'a BrowserView, P2Failure> {
    match graph.browsers.as_slice() {
        [] => Err(P2Failure::new(
            "PROVIDER_LOST",
            "No browser session is connected. Connect a supported browser and retry.",
        )),
        [browser] if browser_id.is_none_or(|id| id == browser.browser_id) => Ok(browser),
        browsers => {
            if let Some(id) = browser_id {
                browsers
                    .iter()
                    .find(|browser| browser.browser_id == id)
                    .ok_or_else(|| P2Failure::bad_args("unknown browser_id"))
            } else {
                Err(P2Failure::new(
                    "AMBIGUOUS_BROWSER",
                    "more than one browser is connected; pass browser_id",
                ))
            }
        }
    }
}

fn browser_id_for(provider_id: &str) -> String {
    let digest = Sha256::digest(provider_id.as_bytes());
    let hex = hex::encode(digest);
    format!("b-{}", &hex[..8])
}

fn provider_identity(browser: &BrowserInfo) -> (String, String, String) {
    let fallback = |value: &str, default: &str| {
        if value.is_empty() {
            default.to_string()
        } else {
            value.to_string()
        }
    };
    (
        fallback(&browser.family, "unknown"),
        fallback(&browser.version, "unknown"),
        fallback(&browser.profile, "default"),
    )
}

fn split_product(product: &str) -> (String, String) {
    let (family, version) = product.split_once('/').unwrap_or((product, "unknown"));
    (family.to_string(), version.to_string())
}

fn provider_groups(provider: &ProviderSnapshot) -> Vec<&'static str> {
    let mut actions: Vec<String> = provider
        .manifest
        .capabilities
        .iter()
        .flat_map(|capability| {
            capability
                .actions
                .iter()
                .map(|action| format!("{}.{}", capability.id, action))
        })
        .collect();
    if provider.provider_id.starts_with("cdp-") {
        actions.extend(corvus_cdp::p2_capability_actions());
    }
    let mut groups = Vec::new();
    for group in GROUPS {
        let required = group_actions(group);
        if required
            .iter()
            .all(|action| actions.iter().any(|candidate| candidate == action))
        {
            groups.push(group);
        }
    }
    groups
}

fn group_actions(group: &str) -> &'static [&'static str] {
    match group {
        "tabs" => &["tabs.list", "tabs.open", "tabs.close"],
        "page" => &["page.navigate", "page.read", "page.snapshot"],
        "input" => &["input.click", "input.type"],
        "capture" => &["capture.viewport"],
        _ => &[],
    }
}

fn normalize_tab_list(browser_id: &str, value: Value) -> Result<Value, P2Failure> {
    let tabs = value
        .get("tabs")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| P2Failure::internal("the browser returned no tabs"))?;
    let mut rows: Vec<Value> = tabs
        .into_iter()
        .map(|mut tab| {
            tab["tab_id"] = json!(tab.get("tab_id").map(json_id).unwrap_or_default());
            tab
        })
        .collect();
    rows.sort_by(|left, right| {
        let window_left = left.get("window_id").and_then(Value::as_u64).unwrap_or(0);
        let window_right = right.get("window_id").and_then(Value::as_u64).unwrap_or(0);
        window_left.cmp(&window_right).then_with(|| {
            left.get("tab_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .cmp(
                    right
                        .get("tab_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                )
        })
    });
    rows.truncate(200);
    Ok(json!({
        "browser_id": browser_id,
        "count": rows.len(),
        "tabs": rows,
    }))
}

fn normalize_tab(browser_id: &str, mut value: Value) -> Result<Value, P2Failure> {
    if let Some(tab_id) = value.get("tab_id").cloned() {
        value["tab_id"] = json!(json_id(&tab_id));
    }
    value["browser_id"] = json!(browser_id);
    Ok(value)
}

fn json_id(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => String::new(),
    }
}

fn strip_image_data(value: &mut Value) {
    if let Some(image) = value.get_mut("image").and_then(Value::as_object_mut) {
        image.remove("data");
        if image.is_empty() {
            let _ = value.as_object_mut().map(|object| object.remove("image"));
        }
    }
    if let Some(object) = value.as_object_mut() {
        object.remove("data");
    }
}

/// One failure with a stable public code.
#[derive(Debug, Clone)]
pub struct P2Failure {
    code: String,
    message: String,
}

impl P2Failure {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: normalize_code(code),
            message: message.into(),
        }
    }

    fn bad_args(message: impl Into<String>) -> Self {
        Self::new("BAD_ARGS", message)
    }

    fn internal(message: impl Into<String>) -> Self {
        Self::new("INTERNAL", message)
    }

    fn from_code(code: &str) -> Self {
        let code = normalize_code(code);
        Self::new(&code, message_for(&code))
    }

    fn from_gateway(error: ProviderGatewayError) -> Self {
        match error {
            ProviderGatewayError::Timeout => Self::new("TIMEOUT", "the operation timed out"),
            ProviderGatewayError::NotConnected(_) | ProviderGatewayError::Closed => {
                Self::new("PROVIDER_LOST", "the browser connection was lost")
            }
            ProviderGatewayError::Protocol(_) => {
                Self::internal("the browser returned an unusable result")
            }
        }
    }

    fn from_worker(error: WorkerError) -> Self {
        match error {
            WorkerError::BadArgs(_) => Self::from_code("BAD_ARGS"),
            WorkerError::CapabilityUnavailable(_) => Self::from_code("CAPABILITY_UNAVAILABLE"),
            WorkerError::NotFound(_) => Self::from_code("NOT_FOUND"),
            WorkerError::NavigationFailed(_) => Self::from_code("NAVIGATION_FAILED"),
            WorkerError::Timeout(_) => Self::from_code("TIMEOUT"),
            WorkerError::ProviderLost => Self::from_code("PROVIDER_LOST"),
            WorkerError::Cdp(message) => {
                let code = message.split(':').next().unwrap_or("INTERNAL");
                Self::from_code(code)
            }
            WorkerError::ProtocolVersion(_) | WorkerError::Handshake(_) => {
                Self::from_code("PROVIDER_LOST")
            }
            _ => Self::from_code("INTERNAL"),
        }
    }

    fn from_cdp(error: P2ActionError) -> Self {
        Self::new(error.code(), error.message())
    }

    fn body(&self) -> ErrorBody {
        let code = match self.code.as_str() {
            "BAD_ARGS" | "AMBIGUOUS_BROWSER" => ErrorCode::BadArgs,
            "NOT_FOUND" => ErrorCode::NotFound,
            "TIMEOUT" => ErrorCode::Timeout,
            "CAPABILITY_UNAVAILABLE" => ErrorCode::CapabilityUnavailable,
            "PROVIDER_LOST" => ErrorCode::TransportDown,
            _ => ErrorCode::Internal,
        };
        ErrorBody::new(code, self.message.clone()).with_details(json!({
            "p2": { "code": self.code, "message": self.message }
        }))
    }
}

fn normalize_code(code: &str) -> String {
    let upper = code.trim().to_ascii_uppercase();
    match upper.as_str() {
        "BAD_ARGS"
        | "NOT_FOUND"
        | "NAVIGATION_FAILED"
        | "TIMEOUT"
        | "PROVIDER_LOST"
        | "CAPABILITY_UNAVAILABLE"
        | "AMBIGUOUS_BROWSER"
        | "INTERNAL" => upper,
        _ => "INTERNAL".to_string(),
    }
}

/// MCP backend that forwards the ten public tools into the runtime.
pub struct P2Backend {
    runtime: Arc<P2Runtime>,
}

impl P2Backend {
    /// Build a backend over the shared runtime.
    pub fn new(runtime: Arc<P2Runtime>) -> Self {
        Self { runtime }
    }

    /// Shared runtime handle, for lifecycle wiring.
    pub fn runtime(&self) -> &Arc<P2Runtime> {
        &self.runtime
    }
}

impl ToolBackend for P2Backend {
    fn call(&self, call: ToolCall) -> BackendFuture {
        let runtime = Arc::clone(&self.runtime);
        Box::pin(async move {
            runtime
                .call_tool(&call.name, &call.arguments, call.session.as_ref())
                .await
        })
    }
}

/// Catalog adapter for the REST API and legacy catalog call sites.
pub struct P2Catalog {
    runtime: Arc<P2Runtime>,
}

impl P2Catalog {
    /// Build a catalog over the shared runtime.
    pub fn new(runtime: Arc<P2Runtime>) -> Self {
        Self { runtime }
    }
}

impl ToolCatalog for P2Catalog {
    fn definitions_for(&self, _category: Option<&str>, _detail: &str) -> Vec<Value> {
        self.runtime.list_tools()
    }

    fn validate(&self, name: &str, arguments: &Value) -> Result<Value, ErrorBody> {
        self.runtime.validate_tool(name, arguments)
    }
}

impl P2Runtime {
    /// Refresh the graph every 250 ms until shutdown, so tools/list_changed
    /// observes provider connect/disconnect without a restart.
    pub fn spawn_graph_task(
        self: &Arc<Self>,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> tokio::task::JoinHandle<()> {
        let runtime = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(250));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            break;
                        }
                    }
                    _ = interval.tick() => runtime.refresh().await,
                }
            }
        })
    }
}

/// Current time in Unix milliseconds, re-exported for backend tests.
pub fn timestamp_ms() -> i64 {
    now_millis()
}

fn message_for(code: &str) -> String {
    match code {
        "BAD_ARGS" => "the browser rejected the request arguments".to_string(),
        "NOT_FOUND" => "the requested item was not found".to_string(),
        "NAVIGATION_FAILED" => "the page could not be loaded".to_string(),
        "TIMEOUT" => "the operation timed out".to_string(),
        "PROVIDER_LOST" => "the browser connection was lost".to_string(),
        "CAPABILITY_UNAVAILABLE" => "the browser cannot perform this action".to_string(),
        "AMBIGUOUS_BROWSER" => "more than one browser is connected; pass browser_id".to_string(),
        _ => "the browser returned an unusable result".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use corvus_provider::PairingToken;
    use corvus_record::{Recorder, Store};
    use corvus_worker::{
        read_frame, read_handshake, write_frame, SupervisorConfig, WorkerIo, WorkerLauncher,
    };
    use std::path::Path;

    const TINY_PNG: &str =
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAFgwJ/lK3w5QAAAABJRU5ErkJggg==";

    struct FakeWorkerLauncher;

    #[async_trait::async_trait]
    impl WorkerLauncher for FakeWorkerLauncher {
        async fn launch(&self, _config: &SupervisorConfig) -> Result<WorkerIo, WorkerError> {
            let (reader, mut worker_writer) = tokio::io::duplex(128 * 1024);
            let (mut worker_reader, writer) = tokio::io::duplex(128 * 1024);
            tokio::spawn(async move {
                let hello = match read_handshake(&mut worker_reader).await {
                    Ok(hello) => hello,
                    Err(_) => return,
                };
                let worker_id = hello["worker_id"].as_str().unwrap_or("w-test");
                let ack = json!({
                    "kind": "worker_ack",
                    "protocol": 1,
                    "worker_id": worker_id,
                    "provider_id": "cdp-fake",
                    "ok": true,
                    "browser": {
                        "version": "Edge/157.0.8103.78",
                        "protocol_version": "1.3"
                    },
                    "capabilities": ["cdp"]
                });
                if write_frame(&mut worker_writer, &ack).await.is_err() {
                    return;
                }
                loop {
                    let frame = match read_frame(&mut worker_reader).await {
                        Ok(frame) => frame,
                        Err(_) => return,
                    };
                    match frame["kind"].as_str().unwrap_or_default() {
                        "shutdown" => return,
                        "invoke" => {}
                        _ => continue,
                    }
                    let id = frame["id"].as_str().unwrap_or_default();
                    let action = frame["action"].as_str().unwrap_or_default();
                    let result = match action {
                        "tabs.list" => json!({
                            "kind": "result",
                            "id": id,
                            "ok": true,
                            "value": {
                                "tabs": [{
                                    "tab_id": "fake-tab",
                                    "window_id": 1,
                                    "title": "Fake",
                                    "url": "https://example.test/",
                                    "active": true
                                }],
                                "count": 1
                            }
                        }),
                        "capture.viewport" => json!({
                            "kind": "result",
                            "id": id,
                            "ok": true,
                            "value": {
                                "tab_id": "fake-tab",
                                "url": "https://example.test/",
                                "format": "png",
                                "mime_type": "image/png",
                                "width": 1,
                                "height": 1,
                                "data": TINY_PNG
                            }
                        }),
                        other => json!({
                            "kind": "result",
                            "id": id,
                            "ok": false,
                            "error": {
                                "code": "BAD_ARGS",
                                "message": format!("unknown action {other}"),
                                "retryable": false
                            }
                        }),
                    };
                    if write_frame(&mut worker_writer, &result).await.is_err() {
                        return;
                    }
                }
            });
            Ok(WorkerIo::from_parts(Box::new(reader), Box::new(writer)))
        }
    }

    fn supervisor_config(home: &Path) -> SupervisorConfig {
        SupervisorConfig {
            worker_program: PathBuf::from("fake-worker"),
            worker_args: Vec::new(),
            cdp_ws_endpoint: "ws://127.0.0.1:9500/devtools/browser/test".to_string(),
            home_dir: home.to_path_buf(),
            worker_id: "w-test".to_string(),
            browser_id: "browser-test".to_string(),
            log_level: "info".to_string(),
            handshake_timeout: Duration::from_secs(1),
            backoff: vec![Duration::from_millis(1)],
            max_restarts: 0,
        }
    }

    fn tool_names(runtime: &P2Runtime) -> Vec<String> {
        runtime
            .list_tools()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap_or_default().to_string())
            .collect()
    }

    #[test]
    fn provider_browser_metadata_is_used_literally() {
        let plain = BrowserInfo {
            family: "edge".to_string(),
            version: "157.0.8103.78".to_string(),
            profile: String::new(),
        };
        let (family, version, profile) = provider_identity(&plain);
        assert_eq!(family, "edge");
        assert_eq!(version, "157.0.8103.78");
        assert_eq!(profile, "default");

        let product = BrowserInfo {
            family: "edge".to_string(),
            version: "Chrome/157.0.8103.78".to_string(),
            profile: "work".to_string(),
        };
        let (family, version, profile) = provider_identity(&product);
        assert_eq!(family, "edge");
        assert_eq!(version, "Chrome/157.0.8103.78");
        assert_eq!(profile, "work");

        let empty = BrowserInfo {
            family: String::new(),
            version: String::new(),
            profile: String::new(),
        };
        let (family, version, profile) = provider_identity(&empty);
        assert_eq!(family, "unknown");
        assert_eq!(version, "unknown");
        assert_eq!(profile, "default");
    }

    #[test]
    fn a_connected_worker_hides_the_duplicate_direct_browser() {
        let worker = WorkerStatus {
            connected: true,
            worker_id: Some("w-test".to_string()),
            provider_id: Some("cdp-w-test".to_string()),
            browser: Some(corvus_worker::WorkerBrowser {
                version: "Chrome/157.0.8103.78".to_string(),
                protocol_version: "1.3".to_string(),
            }),
            pid: None,
            restarts: 0,
        };
        let direct = CdpStatus {
            connected: true,
            endpoint: Some("ws://127.0.0.1:9476/devtools/browser/test".to_string()),
            browser: Some("Chrome/157.0.8103.78".to_string()),
            protocol_version: Some("1.3".to_string()),
            targets: 1,
            last_error: None,
            connected_since_ms: Some(1),
            reconnects: 0,
        };
        let views = select_browser_views(Some(&worker), &direct);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].browser_id, "b-cd000001");
        assert!(views[0]
            .sources
            .values()
            .all(|source| *source == Source::Worker));

        let disconnected = WorkerStatus {
            connected: false,
            ..worker
        };
        let direct_only = select_browser_views(Some(&disconnected), &direct);
        assert_eq!(direct_only.len(), 1);
        assert_eq!(direct_only[0].browser_id, "b-cd000002");
        assert!(direct_only[0]
            .sources
            .values()
            .all(|source| *source == Source::Direct));

        assert_eq!(
            select_browser_views(None, &direct)[0].browser_id,
            "b-cd000002"
        );
        let offline = CdpStatus {
            connected: false,
            ..direct
        };
        assert!(select_browser_views(None, &offline).is_empty());
    }

    #[tokio::test]
    async fn fake_worker_joins_the_graph_and_serves_worker_actions() {
        let directory = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_in_memory().expect("store"));
        let recorder = Recorder::start(store);
        let runtime = Arc::new(P2Runtime {
            gateway: Arc::new(ProviderGateway::new(
                PairingToken::generate().expect("token"),
            )),
            cdp: Arc::new(CdpClient::new()),
            home: directory.path().to_path_buf(),
            worker: Mutex::new(None),
            graph: RwLock::new(Graph::default()),
            refs: Mutex::new(RefStore::default()),
            sink: recorder.sink(),
            guardrails: None,
        });
        runtime.publish(runtime.build_graph().await);
        assert_eq!(tool_names(&runtime), vec!["browser_status".to_string()]);

        let supervisor = WorkerSupervisor::start_with_launcher(
            supervisor_config(directory.path()),
            Arc::new(FakeWorkerLauncher),
        )
        .await
        .expect("worker");
        *runtime.worker.lock().await = Some(supervisor);
        runtime.refresh().await;

        let names = tool_names(&runtime);
        assert_eq!(names.len(), PUBLIC_TOOLS.len());
        assert_eq!(names.first().map(String::as_str), Some("browser_status"));
        for expected in PUBLIC_TOOLS {
            assert!(names.iter().any(|name| name == expected));
        }

        let status = runtime
            .call_tool("browser_status", &json!({}), None)
            .await
            .expect("status");
        assert_eq!(status["count"], 1);
        assert_eq!(status["browsers"][0]["family"], "Edge");
        assert_eq!(status["browsers"][0]["version"], "157.0.8103.78");
        assert!(status["capabilities"]
            .as_array()
            .expect("capabilities")
            .iter()
            .all(|group| group["available"] == true));

        let tabs = runtime
            .call_tool("tab_list", &json!({}), None)
            .await
            .expect("tabs");
        assert_eq!(tabs["count"], 1);
        assert_eq!(tabs["browser_id"], "b-cd000001");
        assert_eq!(tabs["tabs"][0]["tab_id"], "fake-tab");

        let shot = runtime
            .call_tool("page_screenshot", &json!({}), None)
            .await
            .expect("screenshot");
        assert_eq!(shot["bytes"], 70);
        assert_eq!(shot["summary"], "1x1 png 70 bytes");
        assert!(shot.get("image").is_none());
        let path = shot["path"].as_str().expect("path");
        assert!(path.contains("screenshots"));
        assert_eq!(std::fs::read(path).expect("screenshot file").len(), 70);

        let unavailable = runtime
            .call_tool("page_screenshot", &json!({ "full_page": true }), None)
            .await
            .expect_err("full page");
        assert_eq!(
            unavailable
                .details
                .as_ref()
                .and_then(|details| details.get("p2"))
                .and_then(|details| details["code"].as_str()),
            Some("CAPABILITY_UNAVAILABLE")
        );

        runtime.shutdown().await;
        runtime.refresh().await;
        assert_eq!(tool_names(&runtime), vec!["browser_status".to_string()]);
        recorder.shutdown().await;
    }
}
