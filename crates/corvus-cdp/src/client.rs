//! Long-lived browser-level Chrome DevTools Protocol client.
//!
//! One WebSocket is kept open for the lifetime of the daemon. Requests are
//! correlated by numeric id, page targets are reached through flattened
//! Target.attachToTarget sessions, and reconnects back off between 250 ms
//! and 10 seconds. All endpoints are validated as loopback in
//! crate::endpoint.

#![allow(clippy::items_after_test_module)]

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, Notify};
use tokio_tungstenite::tungstenite::Message;

use crate::endpoint::validate_ws_endpoint;
use crate::{CdpError, CdpEvent, CdpEvents, CdpStatus, CdpTarget};

/// Number of protocol events retained for the events tool.
pub const EVENT_CAPACITY: usize = 1000;

const MIN_BACKOFF: Duration = Duration::from_millis(250);
const MAX_BACKOFF: Duration = Duration::from_secs(10);
const HEARTBEAT: Duration = Duration::from_secs(15);
const DEFAULT_TIMEOUT_MS: u64 = 10_000;
const MAX_TIMEOUT_MS: u64 = 60_000;

enum Command {
    SetEndpoint(String),
    ClearEndpoint,
    Send { text: String },
}

#[derive(Debug, Clone)]
struct TargetState {
    target_id: String,
    target_type: String,
    title: String,
    url: String,
    status: Option<String>,
}

#[derive(Default)]
struct EventRing {
    events: VecDeque<CdpEvent>,
    next_seq: u64,
    dropped: u64,
}

struct Inner {
    endpoint: Mutex<Option<String>>,
    connected: AtomicBool,
    browser: Mutex<Option<String>>,
    protocol_version: Mutex<Option<String>>,
    last_error: Mutex<Option<String>>,
    connected_since_ms: AtomicU64,
    reconnects: AtomicU64,
    targets: Mutex<HashMap<String, TargetState>>,
    sessions: Mutex<HashMap<String, String>>,
    active_target: Mutex<Option<String>>,
    events: Mutex<EventRing>,
    event_notify: Notify,
    state_notify: Notify,
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<Value, CdpError>>>>,
    next_id: AtomicU64,
}

impl Inner {
    fn new() -> Self {
        Self {
            endpoint: Mutex::new(None),
            connected: AtomicBool::new(false),
            browser: Mutex::new(None),
            protocol_version: Mutex::new(None),
            last_error: Mutex::new(None),
            connected_since_ms: AtomicU64::new(0),
            reconnects: AtomicU64::new(0),
            targets: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            active_target: Mutex::new(None),
            events: Mutex::new(EventRing::default()),
            event_notify: Notify::new(),
            state_notify: Notify::new(),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    fn set_error(&self, message: impl Into<String>) {
        if let Ok(mut guard) = self.last_error.lock() {
            *guard = Some(message.into());
        }
    }

    fn fail_pending(&self, error: CdpError) {
        let mut pending = match self.pending.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        for (_, sender) in pending.drain() {
            let _ = sender.send(Err(error.clone()));
        }
    }

    fn clear_targets(&self) {
        if let Ok(mut targets) = self.targets.lock() {
            targets.clear();
        }
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.clear();
        }
        if let Ok(mut active) = self.active_target.lock() {
            *active = None;
        }
    }

    fn mark_connected(&self, endpoint: &str) {
        // A fresh socket has a fresh target set. Drop everything observed on
        // the previous connection before Target.setDiscoverTargets repopulates
        // the cache, so a stale active target can never leak into a tool call.
        self.clear_targets();
        self.connected.store(true, Ordering::SeqCst);
        self.connected_since_ms.store(now_ms(), Ordering::SeqCst);
        if let Ok(mut guard) = self.endpoint.lock() {
            *guard = Some(endpoint.to_string());
        }
        self.state_notify.notify_waiters();
    }

    fn mark_disconnected(&self, reason: impl Into<String>) {
        self.connected.store(false, Ordering::SeqCst);
        self.connected_since_ms.store(0, Ordering::SeqCst);
        self.clear_targets();
        self.set_error(reason);
        self.fail_pending(CdpError::Transport("connection closed".into()));
        self.state_notify.notify_waiters();
    }

    fn push_event(&self, method: String, session_id: Option<String>, params: Value) {
        let mut ring = match self.events.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        ring.next_seq += 1;
        let seq = ring.next_seq;
        ring.events.push_back(CdpEvent {
            seq,
            ts: now_ms(),
            method,
            session_id,
            params,
        });
        while ring.events.len() > EVENT_CAPACITY {
            ring.events.pop_front();
            ring.dropped += 1;
        }
        drop(ring);
        self.event_notify.notify_waiters();
    }

    fn upsert_target(&self, info: &Value, session_id: Option<&str>) {
        let Some(target_id) = info.get("targetId").and_then(Value::as_str) else {
            return;
        };
        let target_type = info
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("page")
            .to_string();
        let state = TargetState {
            target_id: target_id.to_string(),
            target_type: target_type.clone(),
            title: info
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            url: info
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            status: None,
        };
        if let Ok(mut targets) = self.targets.lock() {
            targets.insert(target_id.to_string(), state);
        }
        if let Some(session_id) = session_id {
            if let Ok(mut sessions) = self.sessions.lock() {
                sessions.insert(target_id.to_string(), session_id.to_string());
            }
        }
        if target_type == "page" {
            if let Ok(mut active) = self.active_target.lock() {
                if active.is_none() {
                    *active = Some(target_id.to_string());
                }
            }
        }
    }

    fn is_page_target(&self, target_id: &str) -> bool {
        self.targets
            .lock()
            .map(|targets| {
                targets
                    .get(target_id)
                    .is_some_and(|target| target.target_type == "page")
            })
            .unwrap_or(false)
    }

    fn page_target_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .targets
            .lock()
            .map(|targets| {
                targets
                    .values()
                    .filter(|target| target.target_type == "page")
                    .map(|target| target.target_id.clone())
                    .collect()
            })
            .unwrap_or_default();
        ids.sort();
        ids
    }

    fn repair_active_target(&self) -> Option<String> {
        let pages = self.page_target_ids();
        let mut active = match self.active_target.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let valid = active
            .as_ref()
            .is_some_and(|target| pages.iter().any(|page| page == target));
        if !valid {
            *active = pages.first().cloned();
        }
        active.clone()
    }

    fn set_active_target(&self, target_id: &str) -> bool {
        if !self.is_page_target(target_id) {
            return false;
        }
        if let Ok(mut active) = self.active_target.lock() {
            *active = Some(target_id.to_string());
        }
        true
    }

    fn remember_page_target(&self, target_id: &str, url: &str) {
        if let Ok(mut targets) = self.targets.lock() {
            targets
                .entry(target_id.to_string())
                .and_modify(|target| {
                    target.url = url.to_string();
                })
                .or_insert_with(|| TargetState {
                    target_id: target_id.to_string(),
                    target_type: "page".into(),
                    title: String::new(),
                    url: url.to_string(),
                    status: None,
                });
        }
    }

    fn forget_target(&self, target_id: &str) {
        if let Ok(mut targets) = self.targets.lock() {
            targets.remove(target_id);
        }
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.remove(target_id);
        }
        self.repair_active_target();
    }

    fn detach_target(&self, session_id: &str) {
        let target = self.sessions.lock().ok().and_then(|mut sessions| {
            let target = sessions
                .iter()
                .find(|(_, value)| value.as_str() == session_id)
                .map(|(key, _)| key.clone());
            if let Some(target) = target.as_deref() {
                sessions.remove(target);
            }
            target
        });
        if target.is_some() {
            self.repair_active_target();
        }
    }

    fn active_target(&self) -> Option<String> {
        self.repair_active_target()
    }
}

/// Shared state of the active Fetch interception loop.
#[derive(Default)]
pub(crate) struct InterceptRuntime {
    /// Rules applied to paused requests, in order.
    pub rules: Vec<Value>,
    /// Whether the background dispatcher is running.
    pub running: bool,
}

/// Cloneable handle to the shared CDP connection.
#[derive(Clone)]
pub struct CdpClient {
    inner: Arc<Inner>,
    commands: mpsc::Sender<Command>,
    attach_lock: Arc<tokio::sync::Mutex<()>>,
    pub(crate) intercept: Arc<Mutex<InterceptRuntime>>,
}

impl Default for CdpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl CdpClient {
    /// Start the supervisor. It stays idle until CdpClient::connect.
    pub fn new() -> Self {
        let inner = Arc::new(Inner::new());
        let (commands, receiver) = mpsc::channel(64);
        tokio::spawn(supervise(inner.clone(), receiver));
        Self {
            inner,
            commands,
            attach_lock: Arc::new(tokio::sync::Mutex::new(())),
            intercept: Arc::new(Mutex::new(InterceptRuntime::default())),
        }
    }

    /// Set or replace the browser-level endpoint and start connecting.
    pub fn connect(&self, endpoint: &str) -> Result<(), CdpError> {
        let endpoint = validate_ws_endpoint(endpoint)?;
        if let Ok(mut guard) = self.inner.endpoint.lock() {
            *guard = Some(endpoint.clone());
        }
        self.inner.mark_disconnected("endpoint changed");
        self.commands
            .try_send(Command::SetEndpoint(endpoint))
            .map_err(|error| CdpError::Transport(error.to_string()))
    }

    /// Alias for CdpClient::connect.
    pub fn replace_endpoint(&self, endpoint: &str) -> Result<(), CdpError> {
        self.connect(endpoint)
    }

    /// Stop the connection and forget the endpoint.
    pub fn disconnect(&self) {
        if let Ok(mut guard) = self.inner.endpoint.lock() {
            *guard = None;
        }
        self.inner.mark_disconnected("disconnected");
        let _ = self.commands.try_send(Command::ClearEndpoint);
    }

    /// Whether the endpoint is currently usable.
    pub fn is_connected(&self) -> bool {
        self.inner.connected.load(Ordering::SeqCst)
    }

    /// Wait until a connection exists or the timeout expires.
    pub async fn wait_connected(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if self.is_connected() {
                return true;
            }
            let notified = self.inner.state_notify.notified();
            if tokio::time::timeout_at(deadline.into(), notified)
                .await
                .is_err()
            {
                return self.is_connected();
            }
        }
    }

    /// Current connection snapshot.
    pub fn status(&self) -> CdpStatus {
        let targets = self
            .inner
            .targets
            .lock()
            .map(|guard| guard.values().filter(|t| t.target_type == "page").count())
            .unwrap_or(0);
        CdpStatus {
            connected: self.is_connected(),
            endpoint: self
                .inner
                .endpoint
                .lock()
                .ok()
                .and_then(|guard| guard.clone()),
            browser: self
                .inner
                .browser
                .lock()
                .ok()
                .and_then(|guard| guard.clone()),
            protocol_version: self
                .inner
                .protocol_version
                .lock()
                .ok()
                .and_then(|guard| guard.clone()),
            targets,
            last_error: self
                .inner
                .last_error
                .lock()
                .ok()
                .and_then(|guard| guard.clone()),
            connected_since_ms: match self.inner.connected_since_ms.load(Ordering::SeqCst) {
                0 => None,
                value => Some(value),
            },
            reconnects: self.inner.reconnects.load(Ordering::SeqCst),
        }
    }

    /// Number of events currently retained in the ring.
    pub fn event_count(&self) -> usize {
        self.inner
            .events
            .lock()
            .map(|ring| ring.events.len())
            .unwrap_or(0)
    }

    /// Known page targets, sorted by identifier.
    pub fn targets(&self) -> Vec<CdpTarget> {
        let sessions = self
            .inner
            .sessions
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        let mut targets: Vec<CdpTarget> = self
            .inner
            .targets
            .lock()
            .map(|guard| {
                guard
                    .values()
                    .filter(|target| target.target_type == "page")
                    .map(|target| CdpTarget {
                        target_id: target.target_id.clone(),
                        target_type: target.target_type.clone(),
                        title: target.title.clone(),
                        url: target.url.clone(),
                        attached: sessions.contains_key(&target.target_id),
                        session_id: sessions.get(&target.target_id).cloned(),
                        status: target.status.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        targets.sort_by(|left, right| left.target_id.cmp(&right.target_id));
        targets
    }

    /// Target used when the caller does not name one.
    pub fn default_target_id(&self) -> Option<String> {
        self.inner.active_target()
    }

    /// Whether a discovered target is a live page target.
    pub(crate) fn is_page_target(&self, target_id: &str) -> bool {
        self.inner.is_page_target(target_id)
    }

    /// Remember a page target that the browser just created.
    pub(crate) fn remember_page_target(&self, target_id: &str, url: &str) {
        self.inner.remember_page_target(target_id, url);
    }

    /// Select an existing page target as the active tab.
    pub(crate) fn set_active_target(&self, target_id: &str) -> bool {
        self.inner.set_active_target(target_id)
    }

    /// Forget a closed target and repair the active target if needed.
    pub(crate) fn forget_target(&self, target_id: &str) {
        self.inner.forget_target(target_id);
    }

    /// Send one CDP command and wait for its correlated response.
    pub async fn send(
        &self,
        method: &str,
        params: Value,
        session_id: Option<&str>,
        target_id: Option<&str>,
        timeout_ms: Option<u64>,
    ) -> Result<Value, CdpError> {
        let session = match (session_id, target_id) {
            (Some(session), _) => Some(session.to_string()),
            (None, Some(target)) => Some(
                self.attach(target, timeout_ms)
                    .await?
                    .ok_or_else(|| CdpError::TargetNotFound(target.to_string()))?,
            ),
            (None, None) => None,
        };
        self.send_raw(method, params, session.as_deref(), timeout_ms)
            .await
    }

    /// Resolve a flattened session for a tool call.
    ///
    /// An explicit session wins. An explicit target is attached on demand.
    /// Without either, the active page target is attached. When no page target
    /// is known the call is sent at browser level and None is returned.
    pub async fn resolve_session(
        &self,
        session_id: Option<&str>,
        target_id: Option<&str>,
        timeout_ms: Option<u64>,
    ) -> Result<Option<String>, CdpError> {
        if let Some(session) = session_id {
            return Ok(Some(session.to_string()));
        }
        if let Some(target) = target_id {
            return match self.attach(target, timeout_ms).await? {
                Some(session) => Ok(Some(session)),
                None => Err(CdpError::TargetNotFound(target.to_string())),
            };
        }
        match self.default_target_id() {
            Some(target) => match self.attach(&target, timeout_ms).await? {
                Some(session) => Ok(Some(session)),
                None => Err(CdpError::TargetNotFound(target)),
            },
            None => Ok(None),
        }
    }

    /// Attach to a page target once and cache the flattened session.
    pub async fn attach(
        &self,
        target_id: &str,
        timeout_ms: Option<u64>,
    ) -> Result<Option<String>, CdpError> {
        let _guard = self.attach_lock.lock().await;
        if !self.inner.is_page_target(target_id) {
            if let Ok(mut sessions) = self.inner.sessions.lock() {
                sessions.remove(target_id);
            }
            return Ok(None);
        }
        if let Some(session) = self
            .inner
            .sessions
            .lock()
            .ok()
            .and_then(|guard| guard.get(target_id).cloned())
        {
            return Ok(Some(session));
        }
        let value = match self
            .send_raw(
                "Target.attachToTarget",
                json!({ "targetId": target_id, "flatten": true }),
                None,
                timeout_ms,
            )
            .await
        {
            Ok(value) => value,
            Err(error) => {
                if is_missing_target_error(&error) {
                    if let Ok(mut sessions) = self.inner.sessions.lock() {
                        sessions.remove(target_id);
                    }
                    return Err(CdpError::TargetNotFound(target_id.to_string()));
                }
                return Err(error);
            }
        };
        let session = value
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| CdpError::Protocol("attachToTarget returned no sessionId".into()))?
            .to_string();
        if let Ok(mut sessions) = self.inner.sessions.lock() {
            sessions.insert(target_id.to_string(), session.clone());
        }
        // Event-driven tools are only useful when the relevant domains are
        // enabled for the session that was just attached. This is best
        // effort: an unsupported domain must not fail the attachment.
        let enable_timeout = Some(timeout_ms.unwrap_or(5_000).min(5_000));
        for method in ["Page.enable", "Runtime.enable", "Log.enable", "DOM.enable"] {
            let _ = self
                .send_raw(method, json!({}), Some(session.as_str()), enable_timeout)
                .await;
        }
        Ok(Some(session))
    }

    /// Read the bounded event ring, optionally waiting for a matching event.
    pub async fn events(
        &self,
        since: Option<u64>,
        limit: Option<usize>,
        method: Option<&str>,
        wait_ms: Option<u64>,
        clear: bool,
    ) -> CdpEvents {
        let since = since.unwrap_or(0);
        let limit = limit.unwrap_or(100).clamp(1, 500);
        let deadline = Instant::now() + Duration::from_millis(wait_ms.unwrap_or(0).min(30_000));
        loop {
            let (events, next_seq) = self.collect_events(since, limit, method, clear);
            if !events.is_empty() || Instant::now() >= deadline {
                let dropped = self
                    .inner
                    .events
                    .lock()
                    .map(|guard| guard.dropped)
                    .unwrap_or(0);
                return CdpEvents {
                    events,
                    next_seq,
                    dropped,
                };
            }
            let notified = self.inner.event_notify.notified();
            if tokio::time::timeout_at(deadline.into(), notified)
                .await
                .is_err()
            {
                let (events, next_seq) = self.collect_events(since, limit, method, false);
                let dropped = self
                    .inner
                    .events
                    .lock()
                    .map(|guard| guard.dropped)
                    .unwrap_or(0);
                return CdpEvents {
                    events,
                    next_seq,
                    dropped,
                };
            }
        }
    }

    fn collect_events(
        &self,
        since: u64,
        limit: usize,
        method: Option<&str>,
        clear: bool,
    ) -> (Vec<CdpEvent>, u64) {
        let mut ring = match self.inner.events.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let selected: Vec<CdpEvent> = ring
            .events
            .iter()
            .filter(|event| event.seq > since)
            .filter(|event| method.is_none_or(|wanted| event.method == wanted))
            .take(limit)
            .cloned()
            .collect();
        if clear && !selected.is_empty() {
            let highest = selected.last().map(|event| event.seq).unwrap_or(since);
            ring.events.retain(|event| event.seq > highest);
        }
        let next_seq = selected
            .last()
            .map(|event| event.seq)
            .unwrap_or_else(|| ring.next_seq.max(since));
        (selected, next_seq)
    }

    async fn send_raw(
        &self,
        method: &str,
        params: Value,
        session_id: Option<&str>,
        timeout_ms: Option<u64>,
    ) -> Result<Value, CdpError> {
        if method.trim().is_empty() {
            return Err(CdpError::Protocol("CDP method is empty".into()));
        }
        if !self.is_connected() {
            return Err(CdpError::NotConnected);
        }
        let timeout = Duration::from_millis(
            timeout_ms
                .unwrap_or(DEFAULT_TIMEOUT_MS)
                .clamp(1, MAX_TIMEOUT_MS),
        );
        let id = self.inner.next_id();
        let (sender, receiver) = oneshot::channel();
        if let Ok(mut pending) = self.inner.pending.lock() {
            pending.insert(id, sender);
        }
        let mut message = json!({ "id": id, "method": method, "params": params });
        if let Some(session) = session_id {
            message["sessionId"] = Value::String(session.to_string());
        }
        if self
            .commands
            .send(Command::Send {
                text: message.to_string(),
            })
            .await
            .is_err()
        {
            if let Ok(mut pending) = self.inner.pending.lock() {
                pending.remove(&id);
            }
            return Err(CdpError::NotConnected);
        }
        match tokio::time::timeout(timeout, receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(CdpError::Transport("request channel closed".into())),
            Err(_) => {
                if let Ok(mut pending) = self.inner.pending.lock() {
                    pending.remove(&id);
                }
                Err(CdpError::Timeout(timeout.as_millis() as u64))
            }
        }
    }
}

/// Apply one queued control command. Returns true when the target endpoint
/// changed, which resets the reconnect backoff.
fn apply_control(inner: &Inner, endpoint: &mut Option<String>, command: Command) -> bool {
    match command {
        Command::SetEndpoint(next) => {
            if let Ok(mut guard) = inner.endpoint.lock() {
                *guard = Some(next.clone());
            }
            *endpoint = Some(next);
            true
        }
        Command::ClearEndpoint => {
            if let Ok(mut guard) = inner.endpoint.lock() {
                *guard = None;
            }
            *endpoint = None;
            true
        }
        Command::Send { .. } => false,
    }
}

/// Drain queued control commands without blocking. A full command channel must
/// never pin the supervisor to an obsolete endpoint.
fn drain_controls(
    inner: &Inner,
    commands: &mut mpsc::Receiver<Command>,
    endpoint: &mut Option<String>,
) -> bool {
    let mut changed = false;
    while let Ok(command) = commands.try_recv() {
        changed |= apply_control(inner, endpoint, command);
    }
    changed
}

/// Sleep before a reconnect attempt, but wake immediately for a new endpoint.
/// `None` means the command channel closed and the supervisor should stop.
async fn wait_backoff(
    inner: &Inner,
    commands: &mut mpsc::Receiver<Command>,
    endpoint: &mut Option<String>,
    backoff: Duration,
) -> Option<bool> {
    let sleep = tokio::time::sleep(backoff);
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            _ = &mut sleep => return Some(false),
            command = commands.recv() => match command {
                Some(Command::Send { .. }) => continue,
                Some(command) => {
                    apply_control(inner, endpoint, command);
                    return Some(true);
                }
                None => return None,
            },
        }
    }
}

async fn supervise(inner: Arc<Inner>, mut commands: mpsc::Receiver<Command>) {
    let mut endpoint: Option<String> = None;
    let mut backoff = MIN_BACKOFF;
    let mut ever_connected = false;
    loop {
        // The public connect/disconnect methods update inner.endpoint before
        // they queue a command, so this also recovers when the channel is full.
        if let Ok(guard) = inner.endpoint.lock() {
            if guard.as_deref() != endpoint.as_deref() {
                endpoint = guard.clone();
                backoff = MIN_BACKOFF;
            }
        }
        if drain_controls(&inner, &mut commands, &mut endpoint) {
            backoff = MIN_BACKOFF;
        }
        let Some(url) = endpoint.clone() else {
            match commands.recv().await {
                Some(Command::Send { .. }) => {}
                Some(command) => {
                    apply_control(&inner, &mut endpoint, command);
                    backoff = MIN_BACKOFF;
                }
                None => return,
            }
            continue;
        };
        match tokio_tungstenite::connect_async(url.as_str()).await {
            Ok((socket, _)) => {
                backoff = MIN_BACKOFF;
                if ever_connected {
                    inner.reconnects.fetch_add(1, Ordering::SeqCst);
                }
                ever_connected = true;
                inner.mark_connected(&url);
                if let Ok(mut error) = inner.last_error.lock() {
                    *error = None;
                }
                let (mut sink, mut stream) = socket.split();
                let outcome =
                    run_connection(&inner, &mut sink, &mut stream, &mut commands, &mut endpoint)
                        .await;
                inner.mark_disconnected(match &outcome {
                    Ok(()) => "connection closed".to_string(),
                    Err(error) => error.to_string(),
                });
                let switched = drain_controls(&inner, &mut commands, &mut endpoint)
                    || endpoint.as_deref() != Some(url.as_str());
                if endpoint.is_none() || switched {
                    backoff = MIN_BACKOFF;
                    continue;
                }
                match wait_backoff(&inner, &mut commands, &mut endpoint, backoff).await {
                    None => return,
                    Some(true) => backoff = MIN_BACKOFF,
                    Some(false) => backoff = (backoff * 2).min(MAX_BACKOFF),
                }
            }
            Err(error) => {
                inner.set_error(format!("connect failed: {error}"));
                inner.state_notify.notify_waiters();
                let switched = drain_controls(&inner, &mut commands, &mut endpoint)
                    || endpoint.as_deref() != Some(url.as_str());
                if switched && endpoint.is_some() {
                    backoff = MIN_BACKOFF;
                    continue;
                }
                match wait_backoff(&inner, &mut commands, &mut endpoint, backoff).await {
                    None => return,
                    Some(true) => backoff = MIN_BACKOFF,
                    Some(false) => backoff = (backoff * 2).min(MAX_BACKOFF),
                }
            }
        }
    }
}

type SocketStream = futures::stream::SplitStream<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
>;
type SocketSink = futures::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    Message,
>;

async fn run_connection(
    inner: &Arc<Inner>,
    sink: &mut SocketSink,
    stream: &mut SocketStream,
    commands: &mut mpsc::Receiver<Command>,
    endpoint: &mut Option<String>,
) -> Result<(), CdpError> {
    let version_id = inner.next_id();
    sink.send(Message::Text(
        json!({ "id": version_id, "method": "Browser.getVersion", "params": {} })
            .to_string()
            .into(),
    ))
    .await
    .map_err(|error| CdpError::Transport(error.to_string()))?;
    sink.send(Message::Text(
        json!({
            "id": inner.next_id(),
            "method": "Target.setDiscoverTargets",
            "params": { "discover": true }
        })
        .to_string()
        .into(),
    ))
    .await
    .map_err(|error| CdpError::Transport(error.to_string()))?;

    // Download tools depend on browser-level download events. Record a
    // warning when the browser rejects the command, but keep the connection:
    // every other CDP feature still works without download reporting.
    let download_id = inner.next_id();
    let (download_sender, download_receiver) = oneshot::channel();
    if let Ok(mut pending) = inner.pending.lock() {
        pending.insert(download_id, download_sender);
    }
    sink.send(Message::Text(
        json!({
            "id": download_id,
            "method": "Browser.setDownloadBehavior",
            "params": { "behavior": "default", "eventsEnabled": true }
        })
        .to_string()
        .into(),
    ))
    .await
    .map_err(|error| CdpError::Transport(error.to_string()))?;
    let warning_inner = inner.clone();
    tokio::spawn(async move {
        match tokio::time::timeout(Duration::from_secs(5), download_receiver).await {
            Ok(Ok(Err(error))) => {
                tracing::warn!(
                    method = "Browser.setDownloadBehavior",
                    error = %error,
                    "could not enable download behavior"
                );
                if warning_inner.connected.load(Ordering::SeqCst) {
                    warning_inner.set_error(format!(
                        "Browser.setDownloadBehavior is unavailable: {error}"
                    ));
                }
            }
            Ok(Ok(Ok(_))) | Ok(Err(_)) => {}
            Err(_) => {
                if let Ok(mut pending) = warning_inner.pending.lock() {
                    pending.remove(&download_id);
                }
            }
        }
    });

    let mut heartbeat = tokio::time::interval(HEARTBEAT);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await;
    loop {
        tokio::select! {
            command = commands.recv() => {
                match command {
                    Some(command @ Command::SetEndpoint(_))
                    | Some(command @ Command::ClearEndpoint) => {
                        apply_control(inner, endpoint, command);
                        return Ok(());
                    }
                    Some(Command::Send { text }) => {
                        sink.send(Message::Text(text.into()))
                            .await
                            .map_err(|error| CdpError::Transport(error.to_string()))?;
                    }
                    None => return Ok(()),
                }
            }
            frame = stream.next() => {
                match frame {
                    Some(Ok(Message::Text(text))) => {
                        handle_text(inner, text.as_str(), version_id);
                    }
                    Some(Ok(Message::Binary(bytes))) => {
                        if let Ok(text) = std::str::from_utf8(&bytes) {
                            handle_text(inner, text, version_id);
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        sink.send(Message::Pong(payload))
                            .await
                            .map_err(|error| CdpError::Transport(error.to_string()))?;
                    }
                    Some(Ok(Message::Close(_))) | None => return Ok(()),
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        return Err(CdpError::Transport(error.to_string()));
                    }
                }
            }
            _ = heartbeat.tick() => {
                sink.send(Message::Text(
                    json!({ "id": inner.next_id(), "method": "Browser.getVersion", "params": {} })
                        .to_string()
                        .into(),
                ))
                .await
                .map_err(|error| CdpError::Transport(error.to_string()))?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone, Default)]
    struct MockOptions {
        close_first: bool,
        close_on_eval: bool,
        dialog_not_showing: bool,
        attach_not_found: bool,
        create_targets: bool,
        log: Option<Arc<Mutex<Vec<Value>>>>,
    }

    async fn mock_cdp(close_first: bool) -> (String, Arc<AtomicUsize>) {
        let options = MockOptions {
            close_first,
            ..MockOptions::default()
        };
        let (endpoint, connections, _) = mock_cdp_with(options).await;
        (endpoint, connections)
    }

    async fn mock_cdp_with(
        options: MockOptions,
    ) -> (String, Arc<AtomicUsize>, Option<Arc<Mutex<Vec<Value>>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock cdp");
        let address = listener.local_addr().expect("mock address");
        let connections = Arc::new(AtomicUsize::new(0));
        let counter = connections.clone();
        let log = options.log.clone();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let number = counter.fetch_add(1, Ordering::SeqCst) + 1;
                tokio::spawn(serve_mock(stream, number, options.clone()));
            }
        });
        (
            format!("ws://{address}/devtools/browser/mock"),
            connections,
            log,
        )
    }

    async fn wait_for_page(client: &CdpClient) {
        for _ in 0..80 {
            if !client.targets().is_empty() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("mock page target was not discovered");
    }

    async fn serve_mock(stream: tokio::net::TcpStream, connection: usize, options: MockOptions) {
        let Ok(socket) = tokio_tungstenite::accept_async(stream).await else {
            return;
        };
        let (mut sink, mut stream) = socket.split();
        let close_at = if options.close_first && connection == 1 {
            tokio::time::Instant::now() + Duration::from_millis(250)
        } else {
            tokio::time::Instant::now() + Duration::from_secs(3600)
        };
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(close_at) => {
                    let _ = sink.close().await;
                    return;
                }
                frame = stream.next() => {
                    match frame {
                        Some(Ok(Message::Text(text))) => {
                            let Ok(value) = serde_json::from_str::<Value>(text.as_str()) else {
                                continue;
                            };
                            let Some(id) = value.get("id").and_then(Value::as_u64) else {
                                continue;
                            };
                            let method = value
                                .get("method")
                                .and_then(Value::as_str)
                                .unwrap_or("unknown");
                            if let Some(log) = options.log.as_ref() {
                                if let Ok(mut entries) = log.lock() {
                                    entries.push(value.clone());
                                }
                            }
                            if method == "Runtime.evaluate" && options.close_on_eval {
                                let _ = sink.close().await;
                                return;
                            }
                            if method == "Page.handleJavaScriptDialog" && options.dialog_not_showing {
                                let response = json!({
                                    "id": id,
                                    "error": {
                                        "code": -32602,
                                        "message": "No dialog is showing",
                                    }
                                });
                                if sink
                                    .send(Message::Text(response.to_string().into()))
                                    .await
                                    .is_err()
                                {
                                    return;
                                }
                                continue;
                            }
                            if method == "Target.createTarget" && options.create_targets {
                                let url = value
                                    .pointer("/params/url")
                                    .and_then(Value::as_str)
                                    .unwrap_or("about:blank");
                                let event = json!({
                                    "method": "Target.targetCreated",
                                    "params": { "targetInfo": {
                                        "targetId": "T2",
                                        "type": "page",
                                        "title": "Mock created",
                                        "url": url,
                                    }}
                                });
                                let _ = sink.send(Message::Text(event.to_string().into())).await;
                                let response = json!({
                                    "id": id,
                                    "result": { "targetId": "T2" }
                                });
                                if sink
                                    .send(Message::Text(response.to_string().into()))
                                    .await
                                    .is_err()
                                {
                                    return;
                                }
                                continue;
                            }
                            if method == "Target.attachToTarget" && options.attach_not_found {
                                let response = json!({
                                    "id": id,
                                    "error": {
                                        "code": -32602,
                                        "message": "No target with given id found",
                                    }
                                });
                                if sink
                                    .send(Message::Text(response.to_string().into()))
                                    .await
                                    .is_err()
                                {
                                    return;
                                }
                                continue;
                            }
                            if method == "Target.setDiscoverTargets" {
                                let event = json!({
                                    "method": "Target.targetCreated",
                                    "params": { "targetInfo": {
                                        "targetId": "T1",
                                        "type": "page",
                                        "title": "Mock page",
                                        "url": "https://example.test/",
                                    }}
                                });
                                let _ = sink.send(Message::Text(event.to_string().into())).await;
                            }
                            let result = match method {
                                "Browser.getVersion" => json!({
                                    "product": "MockChrome/1.0",
                                    "protocolVersion": "1.3",
                                }),
                                "Target.attachToTarget" => json!({ "sessionId": "SESSION-1" }),
                                _ => json!({
                                    "method": method,
                                    "params": value.get("params").cloned().unwrap_or(Value::Null),
                                    "sessionId": value.get("sessionId").cloned().unwrap_or(Value::Null),
                                }),
                            };
                            let response = json!({ "id": id, "result": result }).to_string();
                            if sink.send(Message::Text(response.into())).await.is_err() {
                                return;
                            }
                        }
                        Some(Ok(Message::Close(_))) | None => return,
                        Some(Ok(_)) => {}
                        Some(Err(_)) => return,
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn correlates_ids_captures_events_and_attaches_a_session() {
        let (endpoint, connections) = mock_cdp(false).await;
        let client = CdpClient::new();
        client.connect(&endpoint).expect("connect");
        assert!(client.wait_connected(Duration::from_secs(2)).await);

        let calls = (0..20).map(|index| {
            let client = client.clone();
            async move {
                client
                    .send(
                        "Test.echo",
                        json!({ "index": index }),
                        None,
                        None,
                        Some(2000),
                    )
                    .await
            }
        });
        let results = futures::future::join_all(calls).await;
        for (index, result) in results.into_iter().enumerate() {
            let value = result.expect("echo response");
            assert_eq!(
                value.get("method").and_then(Value::as_str),
                Some("Test.echo")
            );
            assert_eq!(
                value
                    .get("params")
                    .and_then(|params| params.get("index"))
                    .and_then(Value::as_u64),
                Some(index as u64)
            );
        }

        let events = client
            .events(
                Some(0),
                Some(10),
                Some("Target.targetCreated"),
                Some(1000),
                false,
            )
            .await;
        assert!(!events.events.is_empty(), "target event was not captured");
        assert_eq!(events.events[0].method, "Target.targetCreated");

        let attached = client
            .send("Page.navigate", json!({}), None, Some("T1"), Some(2000))
            .await
            .expect("attach response");
        assert_eq!(
            attached.get("sessionId").and_then(Value::as_str),
            Some("SESSION-1")
        );
        assert_eq!(client.targets().len(), 1);

        let status = client.status();
        assert!(status.connected);
        assert_eq!(status.browser.as_deref(), Some("MockChrome/1.0"));
        assert_eq!(status.protocol_version.as_deref(), Some("1.3"));
        assert_eq!(connections.load(Ordering::SeqCst), 1);
        client.disconnect();
    }

    #[tokio::test]
    async fn replaces_the_endpoint_while_the_supervisor_is_connected() {
        let (first, first_connections) = mock_cdp(false).await;
        let (second, second_connections) = mock_cdp(false).await;
        let client = CdpClient::new();
        client.connect(&first).expect("first connect");
        assert!(client.wait_connected(Duration::from_secs(2)).await);
        assert!(first_connections.load(Ordering::SeqCst) >= 1);

        client.connect(&second).expect("replace endpoint");
        let mut switched = false;
        for _ in 0..80 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let status = client.status();
            if status.connected && status.endpoint.as_deref() == Some(second.as_str()) {
                switched = true;
                break;
            }
        }
        assert!(
            switched,
            "client did not reconnect to the replacement endpoint: {:?}",
            client.status()
        );
        assert!(second_connections.load(Ordering::SeqCst) >= 1);
        assert!(
            client.status().reconnects >= 1,
            "replacement did not increment the reconnect counter: {:?}",
            client.status()
        );
        let echo = client
            .send(
                "Test.echo",
                json!({ "after": true }),
                None,
                None,
                Some(2000),
            )
            .await
            .expect("command after endpoint replacement");
        assert_eq!(
            echo.get("method").and_then(Value::as_str),
            Some("Test.echo")
        );
        client.disconnect();
    }

    #[tokio::test]
    async fn reconnects_after_the_socket_closes() {
        let (endpoint, connections) = mock_cdp(true).await;
        let client = CdpClient::new();
        client.connect(&endpoint).expect("connect");
        assert!(client.wait_connected(Duration::from_secs(2)).await);

        let mut recovered = false;
        for _ in 0..60 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let status = client.status();
            if status.connected && status.reconnects >= 1 {
                recovered = true;
                break;
            }
        }
        assert!(recovered, "client did not reconnect: {:?}", client.status());
        assert!(connections.load(Ordering::SeqCst) >= 2);
        client.disconnect();
    }

    #[test]
    fn disconnect_clears_cached_targets_and_the_active_target() {
        let inner = Inner::new();
        inner.upsert_target(
            &json!({
                "targetId": "T1",
                "type": "page",
                "title": "One",
                "url": "https://one.test/",
            }),
            Some("S1"),
        );
        assert!(inner.set_active_target("T1"));
        assert_eq!(inner.active_target().as_deref(), Some("T1"));
        inner.mark_disconnected("closed");
        assert!(inner.page_target_ids().is_empty());
        assert!(inner.sessions.lock().expect("sessions").is_empty());
        assert!(inner.active_target().is_none());
    }

    #[test]
    fn forgetting_the_active_target_repairs_to_a_remaining_page() {
        let inner = Inner::new();
        for id in ["T1", "T2"] {
            inner.upsert_target(
                &json!({
                    "targetId": id,
                    "type": "page",
                    "title": id,
                    "url": "https://example.test/",
                }),
                None,
            );
        }
        assert!(inner.set_active_target("T2"));
        assert_eq!(inner.active_target().as_deref(), Some("T2"));
        assert!(!inner.set_active_target("missing"));
        assert_eq!(inner.active_target().as_deref(), Some("T2"));
        inner.forget_target("T2");
        assert_eq!(inner.active_target().as_deref(), Some("T1"));
    }

    #[test]
    fn target_destroyed_event_repairs_the_active_target() {
        let inner = Arc::new(Inner::new());
        for id in ["T1", "T2"] {
            inner.upsert_target(
                &json!({
                    "targetId": id,
                    "type": "page",
                    "title": id,
                    "url": "https://example.test/",
                }),
                None,
            );
        }
        assert!(inner.set_active_target("T2"));
        handle_text(
            &inner,
            &json!({
                "method": "Target.targetDestroyed",
                "params": { "targetId": "T2" },
            })
            .to_string(),
            0,
        );
        assert_eq!(inner.active_target().as_deref(), Some("T1"));
    }

    #[tokio::test]
    async fn page_tools_map_an_unknown_target_to_not_found() {
        let (endpoint, _) = mock_cdp(false).await;
        let client = CdpClient::new();
        client.connect(&endpoint).expect("connect");
        assert!(client.wait_connected(Duration::from_secs(2)).await);
        wait_for_page(&client).await;
        let error = client
            .call_tool(
                "page_text",
                &json!({ "tab_id": "0123456789abcdef0123456789abcdef" }),
            )
            .await
            .expect_err("unknown target should fail");
        assert!(
            matches!(error, CdpError::TargetNotFound(_)),
            "unexpected error: {error:?}"
        );
        client.disconnect();
    }

    #[tokio::test]
    async fn attach_maps_a_missing_target_to_not_found() {
        let (endpoint, _, _) = mock_cdp_with(MockOptions {
            attach_not_found: true,
            ..MockOptions::default()
        })
        .await;
        let client = CdpClient::new();
        client.connect(&endpoint).expect("connect");
        assert!(client.wait_connected(Duration::from_secs(2)).await);
        wait_for_page(&client).await;
        let error = client
            .attach("T1", Some(2_000))
            .await
            .expect_err("attach to missing target should fail");
        assert!(
            matches!(error, CdpError::TargetNotFound(_)),
            "unexpected error: {error:?}"
        );
        client.disconnect();
    }

    #[tokio::test]
    async fn tab_tools_keep_the_active_target_valid() {
        let (endpoint, _, _) = mock_cdp_with(MockOptions {
            create_targets: true,
            ..MockOptions::default()
        })
        .await;
        let client = CdpClient::new();
        client.connect(&endpoint).expect("connect");
        assert!(client.wait_connected(Duration::from_secs(2)).await);
        wait_for_page(&client).await;
        assert_eq!(client.default_target_id().as_deref(), Some("T1"));

        let created = client
            .call_tool(
                "tab_new",
                &json!({ "url": "https://two.test/", "active": true }),
            )
            .await
            .expect("tab_new");
        assert_eq!(created["tab_id"], "T2");
        assert_eq!(client.default_target_id().as_deref(), Some("T2"));

        let switched = client
            .call_tool("tab_switch", &json!({ "tab_id": "T1" }))
            .await
            .expect("tab_switch");
        assert_eq!(switched["active"], true);
        assert_eq!(client.default_target_id().as_deref(), Some("T1"));

        let closed = client
            .call_tool("tab_close", &json!({ "tab_id": "T1" }))
            .await
            .expect("tab_close");
        assert_eq!(closed["next_tab_id"], "T2");
        assert_eq!(client.default_target_id().as_deref(), Some("T2"));

        let error = client
            .call_tool(
                "tab_switch",
                &json!({ "tab_id": "0123456789abcdef0123456789abcdef" }),
            )
            .await
            .expect_err("stale tab id should fail");
        assert!(
            matches!(error, CdpError::TargetNotFound(_)),
            "unexpected error: {error:?}"
        );
        client.disconnect();
    }

    #[tokio::test]
    async fn enables_download_events_on_every_connection() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (endpoint, _, _) = mock_cdp_with(MockOptions {
            close_first: true,
            log: Some(log.clone()),
            ..MockOptions::default()
        })
        .await;
        let client = CdpClient::new();
        client.connect(&endpoint).expect("connect");
        assert!(client.wait_connected(Duration::from_secs(2)).await);
        let mut calls = 0;
        let mut recovered = false;
        for _ in 0..100 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            calls = log
                .lock()
                .expect("command log")
                .iter()
                .filter(|value| {
                    value.get("method").and_then(Value::as_str)
                        == Some("Browser.setDownloadBehavior")
                        && value.pointer("/params/behavior").and_then(Value::as_str)
                            == Some("default")
                        && value
                            .pointer("/params/eventsEnabled")
                            .and_then(Value::as_bool)
                            == Some(true)
                })
                .count();
            if calls >= 2 && client.status().reconnects >= 1 {
                recovered = true;
                break;
            }
        }
        assert!(
            recovered,
            "download behavior was not enabled on each connection: calls={calls} status={:?}",
            client.status()
        );
        client.disconnect();
    }

    #[tokio::test]
    async fn attach_enables_event_domains_for_the_new_session() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (endpoint, _, _) = mock_cdp_with(MockOptions {
            log: Some(log.clone()),
            ..MockOptions::default()
        })
        .await;
        let client = CdpClient::new();
        client.connect(&endpoint).expect("connect");
        assert!(client.wait_connected(Duration::from_secs(2)).await);
        wait_for_page(&client).await;
        client
            .send(
                "Test.echo",
                json!({ "after_attach": true }),
                None,
                Some("T1"),
                Some(2000),
            )
            .await
            .expect("command after attach");
        let entries = log.lock().expect("command log");
        let sequence: Vec<&str> = entries
            .iter()
            .filter(|value| value.get("sessionId").and_then(Value::as_str) == Some("SESSION-1"))
            .filter_map(|value| value.get("method").and_then(Value::as_str))
            .collect();
        assert_eq!(
            sequence,
            vec![
                "Page.enable",
                "Runtime.enable",
                "Log.enable",
                "DOM.enable",
                "Test.echo"
            ]
        );
        drop(entries);
        client.disconnect();
    }

    #[tokio::test]
    async fn page_snapshot_uses_the_accessibility_and_dom_snapshot_domains() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (endpoint, _, _) = mock_cdp_with(MockOptions {
            log: Some(log.clone()),
            ..MockOptions::default()
        })
        .await;
        let client = CdpClient::new();
        client.connect(&endpoint).expect("connect");
        assert!(client.wait_connected(Duration::from_secs(2)).await);
        wait_for_page(&client).await;
        let snapshot = client
            .call_tool("page_snapshot", &json!({ "max_nodes": 40 }))
            .await
            .expect("snapshot result");
        assert_eq!(snapshot["format"], "compact");
        assert_eq!(snapshot["mode"], "interactive");
        assert_eq!(snapshot["node_count"], 0);
        assert_eq!(snapshot["text"], "");
        let entries = log.lock().expect("command log");
        let methods: Vec<&str> = entries
            .iter()
            .filter(|value| value.get("sessionId").and_then(Value::as_str) == Some("SESSION-1"))
            .filter_map(|value| value.get("method").and_then(Value::as_str))
            .collect();
        assert!(
            methods.contains(&"Accessibility.getFullAXTree"),
            "missing AX tree request: {methods:?}"
        );
        assert!(
            methods.contains(&"DOMSnapshot.captureSnapshot"),
            "missing DOM snapshot request: {methods:?}"
        );
        let dom_request = entries
            .iter()
            .find(|value| {
                value.get("method").and_then(Value::as_str) == Some("DOMSnapshot.captureSnapshot")
            })
            .expect("DOM snapshot request");
        assert_eq!(dom_request["params"]["includeDOMRects"], true);
        assert_eq!(dom_request["params"]["includePaintOrder"], false);
        drop(entries);
        client.disconnect();
    }

    #[tokio::test]
    async fn browser_dialog_without_a_dialog_is_not_found() {
        let (endpoint, _, _) = mock_cdp_with(MockOptions {
            dialog_not_showing: true,
            ..MockOptions::default()
        })
        .await;
        let client = CdpClient::new();
        client.connect(&endpoint).expect("connect");
        assert!(client.wait_connected(Duration::from_secs(2)).await);
        wait_for_page(&client).await;
        let error = client
            .call_tool("browser_dialog", &json!({ "accept": true }))
            .await
            .expect_err("no dialog is showing");
        assert!(matches!(error, CdpError::DialogNotFound), "{error:?}");
        assert_eq!(error.code(), "NOT_FOUND");
        assert_eq!(error.to_string(), "no dialog is showing");
        client.disconnect();
    }

    #[tokio::test]
    async fn wait_tool_propagates_transport_failure() {
        let (endpoint, _, _) = mock_cdp_with(MockOptions {
            close_on_eval: true,
            ..MockOptions::default()
        })
        .await;
        let client = CdpClient::new();
        client.connect(&endpoint).expect("connect");
        assert!(client.wait_connected(Duration::from_secs(2)).await);
        wait_for_page(&client).await;
        let started = Instant::now();
        let error = client
            .call_tool(
                "browser_wait",
                &json!({ "selector": "#ready", "timeout_ms": 5000 }),
            )
            .await
            .expect_err("transport loss must not look like a timeout");
        assert!(matches!(error, CdpError::Transport(_)), "{error:?}");
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "wait tool swallowed the transport failure until its deadline"
        );
        client.disconnect();
    }
}

fn handle_text(inner: &Arc<Inner>, text: &str, version_id: u64) {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        inner.set_error("CDP sent malformed JSON");
        return;
    };
    if let Some(id) = value.get("id").and_then(Value::as_u64) {
        let outcome = match value.get("error") {
            Some(error) if !error.is_null() => Err(CdpError::Remote {
                code: error.get("code").and_then(Value::as_i64).unwrap_or(-1),
                message: error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown CDP error")
                    .to_string(),
            }),
            _ => Ok(value.get("result").cloned().unwrap_or(Value::Null)),
        };
        let sender = inner
            .pending
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(&id));
        if let Some(sender) = sender {
            let _ = sender.send(outcome);
        }
        if id == version_id {
            if let Some(result) = value.get("result") {
                if let Some(product) = result.get("product").and_then(Value::as_str) {
                    if let Ok(mut browser) = inner.browser.lock() {
                        *browser = Some(product.to_string());
                    }
                }
                if let Some(version) = result.get("protocolVersion").and_then(Value::as_str) {
                    if let Ok(mut protocol) = inner.protocol_version.lock() {
                        *protocol = Some(version.to_string());
                    }
                }
            }
        }
        return;
    }
    let Some(method) = value.get("method").and_then(Value::as_str) else {
        return;
    };
    let session_id = value
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::to_string);
    let params = value.get("params").cloned().unwrap_or(Value::Null);
    match method {
        "Target.targetCreated" | "Target.targetInfoChanged" => {
            if let Some(info) = params.get("targetInfo") {
                inner.upsert_target(info, None);
            }
        }
        "Target.targetDestroyed" => {
            if let Some(target_id) = params.get("targetId").and_then(Value::as_str) {
                if let Ok(mut targets) = inner.targets.lock() {
                    targets.remove(target_id);
                }
                if let Ok(mut sessions) = inner.sessions.lock() {
                    sessions.remove(target_id);
                }
                inner.repair_active_target();
            }
        }
        "Target.attachedToTarget" => {
            if let (Some(info), Some(session)) = (
                params.get("targetInfo"),
                params.get("sessionId").and_then(Value::as_str),
            ) {
                inner.upsert_target(info, Some(session));
            }
        }
        "Target.detachedFromTarget" => {
            if let Some(session) = params.get("sessionId").and_then(Value::as_str) {
                inner.detach_target(session);
            }
        }
        _ => {}
    }
    inner.push_event(method.to_string(), session_id, params);
}

fn is_missing_target_error(error: &CdpError) -> bool {
    matches!(
        error,
        CdpError::Remote { code: -32602, message }
            if message.to_ascii_lowercase().contains("no target with given id")
    )
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
