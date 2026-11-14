//! The command router.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};

use corvus_proto::{ids, Envelope, ErrorBody, ErrorCode, Kind, Meta};
use corvus_transport::{BrowserConnection, Inbound, Registry};

use crate::spec::{self, ToolSpec};

/// Result of routing one command.
pub type CommandResult = Result<Value, ErrorBody>;

/// What an agent asks the router to do.
#[derive(Debug, Clone)]
pub struct CommandRequest {
    /// Browser identifier; when absent the router uses the only connected one.
    pub browser: Option<String>,
    /// Tool name.
    pub tool: String,
    /// Tool arguments, already validated by the caller.
    pub args: Value,
    /// Deadline requested by the caller.
    pub timeout_ms: Option<u64>,
    /// Key that makes a retry execute at most once.
    pub idempotency_key: Option<String>,
    /// Trace identifier propagated to the browser.
    pub trace_id: Option<String>,
}

impl CommandRequest {
    /// A request for the named tool.
    pub fn new(tool: impl Into<String>, args: Value) -> Self {
        Self {
            browser: None,
            tool: tool.into(),
            args,
            timeout_ms: None,
            idempotency_key: None,
            trace_id: None,
        }
    }

    /// Target a specific browser.
    pub fn with_browser(mut self, browser: impl Into<String>) -> Self {
        self.browser = Some(browser.into());
        self
    }

    /// Request a deadline.
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }

    /// Make the request idempotent.
    pub fn with_idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }

    /// Attach a trace identifier.
    pub fn with_trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
        self
    }
}

/// Router tuning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouterConfig {
    /// Deadline used when the caller does not pass one.
    pub default_timeout_ms: u64,
    /// Hard ceiling on any deadline.
    pub max_timeout_ms: u64,
    /// How many completed commands the idempotency table remembers.
    pub idempotency_capacity: usize,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            default_timeout_ms: 60_000,
            max_timeout_ms: 125_000,
            idempotency_capacity: 4_096,
        }
    }
}

impl RouterConfig {
    /// Force the configuration into a usable range.
    pub fn sanitized(mut self) -> Self {
        self.max_timeout_ms = self.max_timeout_ms.max(1);
        self.default_timeout_ms = self.default_timeout_ms.clamp(1, self.max_timeout_ms);
        self.idempotency_capacity = self.idempotency_capacity.max(1);
        self
    }
}

struct PendingCommand {
    browser: String,
    sender: oneshot::Sender<Envelope>,
}

enum IdempotencyEntry {
    Running(Vec<oneshot::Sender<CommandResult>>),
    Done(CommandResult),
}

#[derive(Default)]
struct IdempotencyState {
    entries: HashMap<String, IdempotencyEntry>,
    order: VecDeque<String>,
}

impl IdempotencyState {
    fn insert_running(&mut self, key: String) {
        self.order.push_back(key.clone());
        self.entries
            .insert(key, IdempotencyEntry::Running(Vec::new()));
    }

    fn insert_done(&mut self, key: String, result: CommandResult, capacity: usize) {
        self.order.push_back(key.clone());
        self.entries.insert(key, IdempotencyEntry::Done(result));
        while self.entries.len() > capacity {
            match self.order.pop_front() {
                Some(oldest) => {
                    let removable =
                        matches!(self.entries.get(&oldest), Some(IdempotencyEntry::Done(_)));
                    if removable {
                        self.entries.remove(&oldest);
                    } else {
                        self.order.push_back(oldest);
                        break;
                    }
                }
                None => break,
            }
        }
    }
}

struct Inner {
    pending: Mutex<HashMap<String, PendingCommand>>,
    gates: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    idempotency: Mutex<IdempotencyState>,
}

/// Routes commands to browsers and answers the caller with a typed result.
pub struct Router {
    registry: Arc<Registry>,
    config: RouterConfig,
    inner: Inner,
}

impl Router {
    /// Build a router over the given browser registry.
    pub fn new(registry: Arc<Registry>, config: RouterConfig) -> Arc<Self> {
        Arc::new(Self {
            registry,
            config: config.sanitized(),
            inner: Inner {
                pending: Mutex::new(HashMap::new()),
                gates: Mutex::new(HashMap::new()),
                idempotency: Mutex::new(IdempotencyState::default()),
            },
        })
    }

    /// The registry the router routes into.
    pub fn registry(&self) -> &Arc<Registry> {
        &self.registry
    }

    /// Effective configuration.
    pub fn config(&self) -> RouterConfig {
        self.config
    }

    /// Consume transport events until the transport stops.
    pub fn spawn(
        self: &Arc<Self>,
        mut inbound: mpsc::Receiver<Inbound>,
    ) -> tokio::task::JoinHandle<()> {
        let router = Arc::clone(self);
        tokio::spawn(async move {
            while let Some(event) = inbound.recv().await {
                match event {
                    Inbound::Message {
                        browser, envelope, ..
                    } => router.handle_inbound(&browser, *envelope),
                    Inbound::Closed { browser, .. } => router.fail_browser(&browser),
                }
            }
            tracing::debug!("transport channel closed");
        })
    }

    /// Route one command and wait for the browser answer.
    pub async fn dispatch(&self, request: CommandRequest) -> CommandResult {
        let spec = spec::lookup(&request.tool).ok_or_else(|| {
            ErrorBody::new(ErrorCode::BadArgs, format!("unknown tool {}", request.tool))
                .with_details(json!({ "tool": request.tool, "known": spec::names() }))
        })?;
        let connection = self.resolve_browser(request.browser.as_deref())?;
        if let Some(required) = spec.capability {
            if !connection.supports(required) {
                return Err(ErrorBody::new(
                    ErrorCode::CapabilityUnavailable,
                    format!(
                        "browser {} does not offer the {} capability",
                        connection.id(),
                        required
                    ),
                )
                .with_details(json!({
                    "browser": connection.id(),
                    "required": required,
                    "available": connection.capabilities(),
                })));
            }
        }
        match request.idempotency_key.clone() {
            None => self.execute(connection, spec, &request).await,
            Some(key) => {
                let cache_key = format!("{}:{}:{}", connection.id(), request.tool, key);
                self.execute_once(cache_key, connection, spec, request)
                    .await
            }
        }
    }

    fn resolve_browser(&self, browser: Option<&str>) -> Result<Arc<BrowserConnection>, ErrorBody> {
        if let Some(id) = browser {
            let connection = self.registry.get(id).ok_or_else(|| {
                ErrorBody::new(
                    ErrorCode::TransportDown,
                    format!("browser {id} is not connected"),
                )
                .with_details(json!({ "browser": id, "connected": self.registry_ids() }))
            })?;
            if !connection.is_alive() {
                return Err(ErrorBody::new(
                    ErrorCode::TransportDown,
                    format!("browser {id} is disconnecting"),
                ));
            }
            return Ok(connection);
        }
        let connected = self.registry_ids();
        match connected.len() {
            0 => Err(ErrorBody::new(
                ErrorCode::TransportDown,
                "no browser is connected",
            )),
            1 => self.resolve_browser(connected.first().map(String::as_str)),
            _ => Err(ErrorBody::new(
                ErrorCode::TransportDown,
                "several browsers are connected; name the one to use",
            )
            .with_details(json!({ "connected": connected }))),
        }
    }

    fn registry_ids(&self) -> Vec<String> {
        self.registry
            .list()
            .into_iter()
            .map(|info| info.id)
            .collect()
    }

    async fn execute_once(
        &self,
        key: String,
        connection: Arc<BrowserConnection>,
        spec: &'static ToolSpec,
        request: CommandRequest,
    ) -> CommandResult {
        enum Role {
            Owner,
            Waiter(oneshot::Receiver<CommandResult>),
        }

        let role = {
            let mut state = lock(&self.inner.idempotency);
            match state.entries.get_mut(&key) {
                Some(IdempotencyEntry::Done(result)) => return result.clone(),
                Some(IdempotencyEntry::Running(waiters)) => {
                    let (sender, receiver) = oneshot::channel();
                    waiters.push(sender);
                    Role::Waiter(receiver)
                }
                None => {
                    state.insert_running(key.clone());
                    Role::Owner
                }
            }
        };

        match role {
            Role::Waiter(receiver) => match receiver.await {
                Ok(result) => result,
                Err(_) => Err(ErrorBody::new(
                    ErrorCode::Internal,
                    "the command that owned this idempotency key disappeared",
                )),
            },
            Role::Owner => {
                let result = self.execute(Arc::clone(&connection), spec, &request).await;
                let waiters = {
                    let mut state = lock(&self.inner.idempotency);
                    let waiters = match state.entries.remove(&key) {
                        Some(IdempotencyEntry::Running(waiters)) => waiters,
                        _ => Vec::new(),
                    };
                    if cacheable(&result) {
                        state.insert_done(key, result.clone(), self.config.idempotency_capacity);
                    }
                    waiters
                };
                for waiter in waiters {
                    let _ = waiter.send(result.clone());
                }
                result
            }
        }
    }

    async fn execute(
        &self,
        connection: Arc<BrowserConnection>,
        spec: &'static ToolSpec,
        request: &CommandRequest,
    ) -> CommandResult {
        let _gate = if spec.serial {
            Some(self.gate(connection.id()).lock_owned().await)
        } else {
            None
        };
        if !connection.is_alive() {
            return Err(ErrorBody::new(
                ErrorCode::TransportDown,
                format!("browser {} disconnected", connection.id()),
            ));
        }

        let timeout_ms = self.timeout_for(request.timeout_ms, spec, &request.args);
        let mut meta = Meta::new().with_timeout_ms(timeout_ms);
        if let Some(key) = &request.idempotency_key {
            meta = meta.with_idempotency_key(key.clone());
        }
        if let Some(trace_id) = &request.trace_id {
            meta = meta.with_trace_id(trace_id.clone());
        }

        let command_id = ids::command_id();
        let envelope = Envelope::command(
            command_id.clone(),
            request.tool.clone(),
            request.args.clone(),
        )
        .with_browser(connection.id().to_string())
        .with_meta(meta);

        let (sender, receiver) = oneshot::channel();
        lock(&self.inner.pending).insert(
            command_id.clone(),
            PendingCommand {
                browser: connection.id().to_string(),
                sender,
            },
        );
        if let Err(error) = connection.send(envelope) {
            lock(&self.inner.pending).remove(&command_id);
            return Err(error.error_body());
        }
        tracing::debug!(command = %command_id, tool = %request.tool, browser = %connection.id(), "command dispatched");

        match tokio::time::timeout(Duration::from_millis(timeout_ms), receiver).await {
            Ok(Ok(response)) => match response.error {
                Some(error) => Err(error),
                None => Ok(response.args),
            },
            Ok(Err(_)) => {
                lock(&self.inner.pending).remove(&command_id);
                Err(ErrorBody::new(
                    ErrorCode::Internal,
                    "the pending response channel closed unexpectedly",
                ))
            }
            Err(_) => {
                lock(&self.inner.pending).remove(&command_id);
                self.send_cancel(&connection, &command_id);
                Err(ErrorBody::new(
                    ErrorCode::Timeout,
                    format!(
                        "no response from browser {} within {} ms",
                        connection.id(),
                        timeout_ms
                    ),
                )
                .with_details(json!({ "command_id": command_id, "tool": request.tool })))
            }
        }
    }

    fn timeout_for(&self, requested: Option<u64>, spec: &ToolSpec, args: &Value) -> u64 {
        requested
            .unwrap_or_else(|| spec.timeout_ms(args, self.config.default_timeout_ms))
            .clamp(1, self.config.max_timeout_ms)
    }

    fn gate(&self, browser: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut gates = lock(&self.inner.gates);
        Arc::clone(
            gates
                .entry(browser.to_string())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
        )
    }

    fn handle_inbound(&self, browser: &str, envelope: Envelope) {
        if envelope.kind == Kind::Response {
            let pending = lock(&self.inner.pending).remove(&envelope.id);
            match pending {
                Some(pending) => {
                    let _ = pending.sender.send(envelope);
                }
                None => tracing::debug!(
                    id = %envelope.id,
                    browser = %browser,
                    "dropping a response nobody is waiting for"
                ),
            }
        } else {
            tracing::debug!(
                kind = %envelope.kind.as_str(),
                browser = %browser,
                "ignoring an inbound event"
            );
        }
    }

    fn fail_browser(&self, browser: &str) {
        lock(&self.inner.gates).remove(browser);
        let failed: Vec<(String, PendingCommand)> = {
            let mut pending = lock(&self.inner.pending);
            let ids: Vec<String> = pending
                .iter()
                .filter(|(_, command)| command.browser == browser)
                .map(|(id, _)| id.clone())
                .collect();
            ids.into_iter()
                .filter_map(|id| pending.remove(&id).map(|command| (id, command)))
                .collect()
        };
        for (id, command) in failed {
            let error = ErrorBody::new(
                ErrorCode::BrowserGone,
                format!("browser {browser} disconnected before answering"),
            );
            let _ = command.sender.send(Envelope::response_err(id, error));
        }
    }

    fn send_cancel(&self, connection: &BrowserConnection, command_id: &str) {
        let cancel = Envelope::event(
            ids::command_id(),
            "cancel",
            json!({ "command_id": command_id }),
        )
        .with_browser(connection.id().to_string());
        if let Err(error) = connection.send(cancel) {
            tracing::debug!(error = %error, "could not deliver the cancel event");
        }
    }
}

fn cacheable(result: &CommandResult) -> bool {
    match result {
        Ok(_) => true,
        Err(error) => !matches!(error.code, ErrorCode::Timeout | ErrorCode::TransportDown),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unknown_tool_is_rejected_before_routing() {
        let router = Router::new(Arc::new(Registry::new()), RouterConfig::default());
        let error = router
            .dispatch(CommandRequest::new("browser_teleport", json!({})))
            .await
            .expect_err("unknown tool");
        assert_eq!(error.code, ErrorCode::BadArgs);
    }

    #[tokio::test]
    async fn a_missing_browser_maps_to_transport_down() {
        let router = Router::new(Arc::new(Registry::new()), RouterConfig::default());
        let error = router
            .dispatch(CommandRequest::new(
                "browser_click",
                json!({ "target": "@e1" }),
            ))
            .await
            .expect_err("no browser");
        assert_eq!(error.code, ErrorCode::TransportDown);
        assert!(error.retryable);
    }

    #[tokio::test]
    async fn timeouts_are_clamped_to_the_configured_ceiling() {
        let router = Router::new(
            Arc::new(Registry::new()),
            RouterConfig {
                default_timeout_ms: 1_000,
                max_timeout_ms: 5_000,
                idempotency_capacity: 8,
            },
        );
        let click = spec::lookup("browser_click").expect("click spec");
        let eval = spec::lookup("browser_eval").expect("eval spec");
        assert_eq!(router.timeout_for(None, click, &json!({})), 1_000);
        assert_eq!(router.timeout_for(Some(60_000), click, &json!({})), 5_000);
        assert_eq!(router.timeout_for(Some(0), click, &json!({})), 1);
        assert_eq!(router.timeout_for(None, eval, &json!({})), 5_000);
        assert_eq!(router.config().idempotency_capacity, 8);
        let defaults = RouterConfig::default();
        assert_eq!(defaults.default_timeout_ms, 60_000);
        assert_eq!(defaults.max_timeout_ms, 125_000);
    }

    #[tokio::test]
    async fn completed_results_are_cached_and_running_ones_are_shared() {
        assert!(cacheable(&Ok(json!({ "ok": true }))));
        assert!(cacheable(&Err(ErrorBody::new(ErrorCode::NotFound, "gone"))));
        assert!(!cacheable(&Err(ErrorBody::new(ErrorCode::Timeout, "slow"))));
        assert!(!cacheable(&Err(ErrorBody::new(
            ErrorCode::TransportDown,
            "down"
        ))));

        let mut state = IdempotencyState::default();
        state.insert_done("a".to_string(), Ok(json!(1)), 2);
        state.insert_done("b".to_string(), Ok(json!(2)), 2);
        state.insert_done("c".to_string(), Ok(json!(3)), 2);
        assert_eq!(state.entries.len(), 2);
        assert!(!state.entries.contains_key("a"));
        assert!(state.entries.contains_key("c"));
    }
}
