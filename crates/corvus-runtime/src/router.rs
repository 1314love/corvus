//! The command router.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, Notify, OwnedMutexGuard};

use corvus_proto::{ids, Envelope, ErrorBody, ErrorCode, Kind, Meta};
use corvus_record::{now_millis, SessionContext, StepRecord, StepSink};
use corvus_transport::{BrowserConnection, Inbound, Registry};

use crate::spec::{self, ToolSpec};

/// Result of routing one command.
pub type CommandResult = Result<Value, ErrorBody>;

/// Token that asks an in-flight dispatch to stop waiting.
///
/// The token is cheap to clone and can be cancelled from any task. A dispatch
/// that observes it removes its pending entry, tells the browser to cancel the
/// command and returns [DispatchFailure::Canceled] without waiting for a reply.
#[derive(Clone, Default)]
pub struct CommandCancel {
    state: Arc<CancelState>,
}

#[derive(Default)]
struct CancelState {
    cancelled: AtomicBool,
    notify: Notify,
}

impl CommandCancel {
    /// Create a token that has not been cancelled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Request cancellation and wake every waiter.
    pub fn cancel(&self) {
        if !self.state.cancelled.swap(true, Ordering::SeqCst) {
            self.state.notify.notify_waiters();
        }
    }

    /// Whether cancellation has already been requested.
    pub fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::SeqCst)
    }

    /// Resolve once cancellation is requested, including a request that landed
    /// between the check and the wait.
    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        let notified = self.state.notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }
}

/// Why a cancellable dispatch stopped before returning a tool result.
#[derive(Debug, Clone)]
pub enum DispatchFailure {
    /// The command failed with a typed error.
    Failed(ErrorBody),
    /// The caller cancelled the command before it produced a result.
    Canceled,
}

impl std::fmt::Display for DispatchFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Failed(error) => write!(formatter, "{error}"),
            Self::Canceled => formatter.write_str("request canceled"),
        }
    }
}

impl std::error::Error for DispatchFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Failed(error) => Some(error),
            Self::Canceled => None,
        }
    }
}

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
    /// MCP session that produced the call, used for recording.
    pub session: Option<SessionContext>,
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
            session: None,
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

    /// Record the call in the given MCP session.
    pub fn with_session(mut self, session: SessionContext) -> Self {
        self.session = Some(session);
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
    Running(Vec<oneshot::Sender<Result<Value, DispatchFailure>>>),
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

/// Removes one pending command when the caller, the deadline or a dropped
/// future ends the wait. Disarming it leaves the entry to the normal response
/// path.
struct PendingGuard<'a> {
    inner: &'a Inner,
    connection: Arc<BrowserConnection>,
    command_id: String,
    armed: bool,
}

impl<'a> PendingGuard<'a> {
    fn new(inner: &'a Inner, connection: Arc<BrowserConnection>, command_id: String) -> Self {
        Self {
            inner,
            connection,
            command_id,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }

    fn cleanup(&mut self) {
        if !self.armed {
            return;
        }
        self.armed = false;
        lock(&self.inner.pending).remove(&self.command_id);
        send_cancel_event(&self.connection, &self.command_id);
    }
}

impl Drop for PendingGuard<'_> {
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// Holds one gate for as long as the call runs and drops the map entry once the
/// last caller releases it. The guard is released before the entry is removed
/// so a queued caller that already cloned the mutex keeps a valid handle.
struct ScopedGate<'a> {
    inner: &'a Inner,
    key: String,
    guard: Option<OwnedMutexGuard<()>>,
}

impl Drop for ScopedGate<'_> {
    fn drop(&mut self) {
        self.guard.take();
        let mut gates = lock(&self.inner.gates);
        let removable = gates
            .get(&self.key)
            .is_some_and(|entry| Arc::strong_count(entry) == 1);
        if removable {
            gates.remove(&self.key);
        }
    }
}

/// Routes commands to browsers and answers the caller with a typed result.
pub struct Router {
    registry: Arc<Registry>,
    config: RouterConfig,
    inner: Inner,
    step_sink: OnceLock<StepSink>,
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
            step_sink: OnceLock::new(),
        })
    }

    /// Attach the recording writer. Only the first sink wins.
    pub fn attach_step_sink(&self, sink: StepSink) -> bool {
        if self.step_sink.set(sink).is_ok() {
            true
        } else {
            tracing::warn!("a recording sink is already attached to the router");
            false
        }
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
    ///
    /// This wrapper never cancels; it keeps the original callers working while
    /// [Router::dispatch_with_cancel] exposes the cancellation facility.
    pub async fn dispatch(&self, request: CommandRequest) -> CommandResult {
        match self
            .dispatch_with_cancel(request, CommandCancel::default())
            .await
        {
            Ok(value) => Ok(value),
            Err(DispatchFailure::Failed(error)) => Err(error),
            Err(DispatchFailure::Canceled) => Err(ErrorBody::new(
                ErrorCode::Internal,
                "the command was canceled without a cancellation request",
            )),
        }
    }

    /// Route one command and stop waiting when the token is cancelled.
    ///
    /// Cancellation removes the pending entry immediately, sends the browser a
    /// cancel event and returns [DispatchFailure::Canceled]. A response that
    /// arrives later is dropped. Cancelling after completion is a no-op.
    pub async fn dispatch_with_cancel(
        &self,
        mut request: CommandRequest,
        cancel: CommandCancel,
    ) -> Result<Value, DispatchFailure> {
        let started = Instant::now();
        let session = request.session.clone();
        let tool = request.tool.clone();
        let recorded_args = request.args.clone();
        let (result, browser_id) = self.dispatch_inner(&mut request, &cancel).await;
        if let (Some(session), Some(sink)) = (session.as_ref(), self.step_sink.get()) {
            let (stored_result, error_code, ok) = match &result {
                Ok(value) => (Some(value.clone()), None, true),
                Err(DispatchFailure::Failed(error)) => {
                    (None, Some(error.code.as_str().to_string()), false)
                }
                Err(DispatchFailure::Canceled) => (None, Some("CANCELLED".to_string()), false),
            };
            sink.record(StepRecord {
                session_id: session.session_id.clone(),
                ts: now_millis(),
                tool,
                args: recorded_args,
                result: stored_result,
                error_code,
                duration_ms: started.elapsed().as_millis() as i64,
                browser_id,
                tab_id: tab_id_of(&request.args),
                ok,
            });
        }
        result
    }

    async fn dispatch_inner(
        &self,
        request: &mut CommandRequest,
        cancel: &CommandCancel,
    ) -> (Result<Value, DispatchFailure>, Option<String>) {
        let spec = match spec::lookup(&request.tool) {
            Some(spec) => spec,
            None => {
                let error =
                    ErrorBody::new(ErrorCode::BadArgs, format!("unknown tool {}", request.tool))
                        .with_details(json!({ "tool": request.tool, "known": spec::names() }));
                return (Err(DispatchFailure::Failed(error)), None);
            }
        };
        let argument_browser = take_browser_id(&mut request.args);
        let browser = request.browser.take().or(argument_browser);
        let connection = match self.resolve_browser(browser.as_deref()) {
            Ok(connection) => connection,
            Err(error) => return (Err(DispatchFailure::Failed(error)), None),
        };
        let browser_id = connection.id().to_string();
        if let Some(required) = spec.capability {
            if !connection.supports(required) {
                let error = ErrorBody::new(
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
                }));
                return (Err(DispatchFailure::Failed(error)), Some(browser_id));
            }
        }
        let result = match request.idempotency_key.clone() {
            None => self.execute(connection, spec, request, cancel).await,
            Some(key) => {
                let cache_key = format!("{}:{}:{}", connection.id(), request.tool, key);
                self.execute_once(cache_key, connection, spec, request.clone(), cancel)
                    .await
            }
        };
        (result, Some(browser_id))
    }

    fn resolve_browser(&self, browser: Option<&str>) -> Result<Arc<BrowserConnection>, ErrorBody> {
        if let Some(id) = browser {
            let connection = self.registry.get(id).ok_or_else(|| {
                ErrorBody::new(
                    ErrorCode::TransportDown,
                    format!("browser {id} is not connected"),
                )
                .with_details(json!({ "browser_id": id }))
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
                ErrorCode::BadArgs,
                "several browsers are connected; pass browser_id to choose one",
            )
            .with_details(json!({
                "reason": "multiple_browsers",
                "browser_ids": connected,
            }))),
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
        cancel: &CommandCancel,
    ) -> Result<Value, DispatchFailure> {
        enum Role {
            Owner,
            Waiter(oneshot::Receiver<Result<Value, DispatchFailure>>),
        }

        let role = {
            let mut state = lock(&self.inner.idempotency);
            match state.entries.get_mut(&key) {
                Some(IdempotencyEntry::Done(result)) => {
                    return match result.clone() {
                        Ok(value) => Ok(value),
                        Err(error) => Err(DispatchFailure::Failed(error)),
                    }
                }
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
            Role::Waiter(receiver) => {
                tokio::select! {
                    biased;
                    received = receiver => match received {
                        Ok(outcome) => outcome,
                        Err(_) => Err(DispatchFailure::Failed(ErrorBody::new(
                            ErrorCode::Internal,
                            "the command that owned this idempotency key disappeared",
                        ))),
                    },
                    _ = cancel.cancelled() => Err(DispatchFailure::Canceled),
                }
            }
            Role::Owner => {
                let outcome = self
                    .execute(Arc::clone(&connection), spec, &request, cancel)
                    .await;
                let waiters = {
                    let mut state = lock(&self.inner.idempotency);
                    let waiters = match state.entries.remove(&key) {
                        Some(IdempotencyEntry::Running(waiters)) => waiters,
                        _ => Vec::new(),
                    };
                    if let Some(cached) = cacheable_outcome(&outcome) {
                        state.insert_done(key, cached, self.config.idempotency_capacity);
                    }
                    waiters
                };
                for waiter in waiters {
                    let _ = waiter.send(outcome.clone());
                }
                outcome
            }
        }
    }

    async fn execute(
        &self,
        connection: Arc<BrowserConnection>,
        spec: &'static ToolSpec,
        request: &CommandRequest,
        cancel: &CommandCancel,
    ) -> Result<Value, DispatchFailure> {
        let _gate = if spec.serial {
            Some(tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(DispatchFailure::Canceled),
                gate = self.lock_gate(connection.id(), &request.tool, &request.args) => gate,
            })
        } else {
            None
        };
        if !connection.is_alive() {
            return Err(DispatchFailure::Failed(ErrorBody::new(
                ErrorCode::TransportDown,
                format!("browser {} disconnected", connection.id()),
            )));
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
        let mut pending =
            PendingGuard::new(&self.inner, Arc::clone(&connection), command_id.clone());
        if let Err(error) = connection.send(envelope) {
            pending.cleanup();
            return Err(DispatchFailure::Failed(error.error_body()));
        }
        tracing::debug!(command = %command_id, tool = %request.tool, browser = %connection.id(), "command dispatched");

        let response = tokio::select! {
            biased;
            response = tokio::time::timeout(Duration::from_millis(timeout_ms), receiver) => response,
            _ = cancel.cancelled() => {
                pending.cleanup();
                return Err(DispatchFailure::Canceled);
            }
        };
        match response {
            Ok(Ok(response)) => {
                pending.disarm();
                match response.error {
                    Some(error) => Err(DispatchFailure::Failed(error)),
                    None => Ok(response.args),
                }
            }
            Ok(Err(_)) => {
                pending.cleanup();
                Err(DispatchFailure::Failed(ErrorBody::new(
                    ErrorCode::Internal,
                    "the pending response channel closed unexpectedly",
                )))
            }
            Err(_) => {
                pending.cleanup();
                Err(DispatchFailure::Failed(
                    ErrorBody::new(
                        ErrorCode::Timeout,
                        format!(
                            "no response from browser {} within {} ms",
                            connection.id(),
                            timeout_ms
                        ),
                    )
                    .with_details(json!({ "command_id": command_id, "tool": request.tool })),
                ))
            }
        }
    }

    fn timeout_for(&self, requested: Option<u64>, spec: &ToolSpec, args: &Value) -> u64 {
        requested
            .unwrap_or_else(|| spec.timeout_ms(args, self.config.default_timeout_ms))
            .clamp(1, self.config.max_timeout_ms)
    }

    async fn lock_gate(&self, browser: &str, tool: &str, args: &Value) -> ScopedGate<'_> {
        let key = gate_key(browser, tool, args);
        let handle = {
            let mut gates = lock(&self.inner.gates);
            Arc::clone(
                gates
                    .entry(key.clone())
                    .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
            )
        };
        {
            let mut gates = lock(&self.inner.gates);
            if gates.len() > GATE_CAPACITY {
                gates.retain(|_, entry| Arc::strong_count(entry) > 1);
            }
        }
        let guard = handle.lock_owned().await;
        ScopedGate {
            inner: &self.inner,
            key,
            guard: Some(guard),
        }
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
        let prefix = format!("{browser}{}", SCOPE_SEPARATOR);
        lock(&self.inner.gates).retain(|key, _| !key.starts_with(&prefix) && key != browser);
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
}

/// Separates the browser from the tab scope inside a gate key.
const SCOPE_SEPARATOR: char = '\u{1f}';

/// Upper bound on the gate map. An idle entry created by a waiter that is
/// cancelled before it acquires the gate is removed by this sweep.
const GATE_CAPACITY: usize = 4096;

fn gate_key(browser: &str, tool: &str, args: &Value) -> String {
    format!("{browser}{SCOPE_SEPARATOR}{}", gate_scope(tool, args))
}

/// Gate scope inside one browser.
///
/// Tools that act on a tab serialize per tab and fall back to the active tab
/// when the caller does not name one. Browser-wide controls serialize on the
/// browser itself. Non-serial tools never reach this function.
fn gate_scope(tool: &str, args: &Value) -> String {
    if matches!(tool, "net_clear" | "net_intercept") {
        return tab_token(args).unwrap_or_else(|| "__browser__".to_string());
    }
    if tab_scoped(tool) {
        return tab_token(args).unwrap_or_else(|| "__active__".to_string());
    }
    "__browser__".to_string()
}

/// Tools whose tab_id argument selects the tab they act on.
fn tab_scoped(tool: &str) -> bool {
    matches!(
        tool,
        "browser_navigate"
            | "page_snapshot"
            | "browser_click"
            | "browser_type"
            | "page_text"
            | "page_html"
            | "page_find"
            | "browser_hover"
            | "browser_press"
            | "browser_scroll"
            | "browser_select"
            | "browser_eval"
            | "browser_wait"
            | "browser_history"
            | "net_capture"
            | "net_log"
            | "upload_file"
    )
}

/// Exact string form of a non-empty tab_id, whether it arrived as a string or
/// a number.
fn tab_token(args: &Value) -> Option<String> {
    match args.get("tab_id")? {
        Value::String(tab) if !tab.is_empty() => Some(tab.clone()),
        Value::Number(tab) => Some(tab.to_string()),
        _ => None,
    }
}

fn send_cancel_event(connection: &BrowserConnection, command_id: &str) {
    let cancel = Envelope::event(
        ids::command_id(),
        "cancel",
        json!({ "event": "cancel", "command_id": command_id }),
    )
    .with_browser(connection.id().to_string());
    if let Err(error) = connection.send(cancel) {
        tracing::debug!(error = %error, "could not deliver the cancel event");
    }
}

fn tab_id_of(args: &Value) -> Option<String> {
    tab_token(args)
}

fn take_browser_id(args: &mut Value) -> Option<String> {
    let value = args.as_object_mut()?.remove("browser_id")?;
    value.as_str().map(str::to_string)
}

fn cacheable(result: &CommandResult) -> bool {
    match result {
        Ok(_) => true,
        Err(error) => !matches!(error.code, ErrorCode::Timeout | ErrorCode::TransportDown),
    }
}

fn cacheable_outcome(outcome: &Result<Value, DispatchFailure>) -> Option<CommandResult> {
    match outcome {
        Ok(value) => Some(Ok(value.clone())),
        Err(DispatchFailure::Failed(error)) => {
            let result = Err(error.clone());
            cacheable(&result).then_some(result)
        }
        Err(DispatchFailure::Canceled) => None,
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
