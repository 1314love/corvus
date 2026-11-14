//! The Corvus daemon: one runtime that owns the WebSocket transport, the
//! command router and the MCP server.

pub mod doctor;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use corvus_mcp::{BackendFuture, McpHttpServer, McpServer, ToolBackend, ToolCall};
use corvus_proto::ids;
use corvus_runtime::{CommandRequest, Router};

pub use corvus_runtime::RouterConfig;
use corvus_transport::{Registry, WsServer};

/// Default port of the WebSocket transport.
pub const DEFAULT_WS_PORT: u16 = 7330;
/// Default port of the MCP HTTP endpoint.
pub const DEFAULT_HTTP_PORT: u16 = 7331;
/// Default loopback address the daemon binds to.
pub const DEFAULT_BIND: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// Everything needed to start a daemon.
#[derive(Debug, Clone, Copy)]
pub struct DaemonConfig {
    /// Address the listeners bind to.
    pub bind: IpAddr,
    /// WebSocket port; zero lets the operating system choose.
    pub ws_port: u16,
    /// MCP HTTP port; zero lets the operating system choose.
    pub http_port: u16,
    /// Router tuning.
    pub router: RouterConfig,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            bind: DEFAULT_BIND,
            ws_port: DEFAULT_WS_PORT,
            http_port: DEFAULT_HTTP_PORT,
            router: RouterConfig::default(),
        }
    }
}

/// Executes MCP tool calls by routing them to the browser.
pub struct RouterBackend {
    router: Arc<Router>,
    registry: Arc<Registry>,
}

impl RouterBackend {
    /// Build a backend around a router and the live browser registry.
    pub fn new(router: Arc<Router>, registry: Arc<Registry>) -> Self {
        Self { router, registry }
    }
}

impl ToolBackend for RouterBackend {
    fn call(&self, call: ToolCall) -> BackendFuture {
        if call.name == "browser_list" {
            let browsers: Vec<Value> = self
                .registry
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
            return Box::pin(async move { Ok(json!({ "browsers": browsers })) });
        }
        let router = Arc::clone(&self.router);
        Box::pin(async move {
            let trace_id = ids::next_id("trace");
            let request = CommandRequest::new(call.name, call.arguments).with_trace_id(trace_id);
            let result = router.dispatch(request).await;
            if let Err(error) = &result {
                tracing::info!(code = %error.code, message = %error.message, "tool call failed");
            }
            result
        })
    }
}

/// A running daemon.
pub struct Daemon {
    registry: Arc<Registry>,
    router: Arc<Router>,
    mcp: Arc<McpServer>,
    ws_addr: SocketAddr,
    http_addr: SocketAddr,
    shutdown: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
}

impl Daemon {
    /// Bind the listeners, start the background tasks and log the addresses.
    pub async fn start(config: DaemonConfig) -> std::io::Result<Self> {
        let registry = Arc::new(Registry::new());
        let (inbound_sender, inbound_receiver) = mpsc::channel(256);
        let router = Router::new(Arc::clone(&registry), config.router);
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
        ));
        let http_addr_slot = Arc::new(std::sync::OnceLock::<SocketAddr>::new());
        let health_registry = Arc::clone(&registry);
        let health_http_addr = Arc::clone(&http_addr_slot);
        let health = Arc::new(move || {
            let mut document = json!({
                "status": "ok",
                "ws_addr": ws_addr.to_string(),
                "browsers": health_registry.list(),
            });
            if let Some(addr) = health_http_addr.get() {
                document["http_addr"] = json!(addr.to_string());
            }
            document
        });
        let mcp = Arc::new(McpServer::new(backend).with_health(health));

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
            "corvusd serving"
        );

        Ok(Self {
            registry,
            router,
            mcp,
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

    /// The health document the daemon reports.
    pub fn health(&self) -> Value {
        self.mcp.health()
    }

    /// Stop the background tasks and wait for them, bounded by a short grace.
    pub async fn shutdown(self) {
        let _ = self.shutdown.send(true);
        for task in self.tasks {
            if tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .is_err()
            {
                tracing::debug!("a background task did not stop within the grace period");
            }
        }
    }
}
