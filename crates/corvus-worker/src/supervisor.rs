//! Process discovery, framed handshake and bounded restart supervision.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;
use std::process::Stdio;
use tokio::io::{AsyncRead, AsyncWrite};

use tokio::process::{Child, Command};
use tokio::time::timeout;
use uuid::Uuid;

use crate::error::WorkerError;
use crate::frame::{read_frame, write_frame};
use crate::protocol::{WorkerBrowser, WorkerMessage};

/// Default handshake deadline for one worker attempt.
pub const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// Default restart delays between worker attempts.
pub const DEFAULT_BACKOFF: [Duration; 6] = [
    Duration::from_millis(250),
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
];

/// Runtime configuration for a supervised CDP worker.
#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    /// Worker executable, or a test command such as cmd.exe.
    pub worker_program: PathBuf,
    /// Arguments passed to the worker executable.
    pub worker_args: Vec<String>,
    /// Loopback CDP WebSocket endpoint handed to the worker.
    pub cdp_ws_endpoint: String,
    /// Corvus home directory handed to the worker.
    pub home_dir: PathBuf,
    /// Gateway-assigned worker identifier.
    pub worker_id: String,
    /// Browser identifier served by this worker.
    pub browser_id: String,
    /// Requested worker log level.
    pub log_level: String,
    /// Per-attempt handshake deadline.
    pub handshake_timeout: Duration,
    /// Restart delays used in order, capped by the final value.
    pub backoff: Vec<Duration>,
    /// Maximum restart attempts after a loss.
    pub max_restarts: usize,
}

impl SupervisorConfig {
    /// Build the opt-in configuration from the environment.
    ///
    /// Returns `None` unless `CORVUS_V2_WORKER=1`. The endpoint must be a
    /// loopback `ws://` endpoint and port 9222 is refused so a developer's
    /// browser is never attached by accident.
    pub fn from_env() -> Result<Option<Self>, WorkerError> {
        if std::env::var("CORVUS_V2_WORKER").as_deref() != Ok("1") {
            return Ok(None);
        }
        let endpoint = std::env::var("CORVUS_V2_CDP_ENDPOINT").map_err(|_| {
            WorkerError::BadArgs(
                "CORVUS_V2_CDP_ENDPOINT is required when CORVUS_V2_WORKER=1".into(),
            )
        })?;
        if !permitted_loopback_endpoint(&endpoint) {
            return Err(WorkerError::BadArgs(
                "CORVUS_V2_CDP_ENDPOINT must be a loopback ws:// endpoint and must not use port 9222"
                    .into(),
            ));
        }
        let worker_program = match std::env::var_os("CORVUS_CDP_WORKER") {
            Some(value) => PathBuf::from(value),
            None => discover_worker_program()?,
        };
        let home_dir = match std::env::var_os("CORVUS_HOME") {
            Some(value) => PathBuf::from(value),
            None => std::env::current_dir().map_err(WorkerError::from)?,
        };
        Ok(Some(Self {
            worker_program,
            worker_args: Vec::new(),
            cdp_ws_endpoint: endpoint,
            home_dir,
            worker_id: format!("w-{}", Uuid::new_v4()),
            browser_id: std::env::var("CORVUS_V2_BROWSER_ID")
                .unwrap_or_else(|_| "browser-default".into()),
            log_level: std::env::var("CORVUS_LOG").unwrap_or_else(|_| "info".into()),
            handshake_timeout: DEFAULT_HANDSHAKE_TIMEOUT,
            backoff: DEFAULT_BACKOFF.to_vec(),
            max_restarts: DEFAULT_BACKOFF.len(),
        }))
    }
}

/// Locate `corvus-cdp-worker` beside the current executable.
pub fn discover_worker_program() -> Result<PathBuf, WorkerError> {
    let current = std::env::current_exe().map_err(WorkerError::from)?;
    let directory = current.parent().ok_or_else(|| {
        WorkerError::Handshake("current executable has no parent directory".into())
    })?;
    let name = if cfg!(windows) {
        "corvus-cdp-worker.exe"
    } else {
        "corvus-cdp-worker"
    };
    Ok(directory.join(name))
}

/// Test seam for worker process creation and I/O.
pub struct WorkerIo {
    reader: Box<dyn AsyncRead + Unpin + Send>,
    writer: Box<dyn AsyncWrite + Unpin + Send>,
    child: Option<Child>,
}

impl WorkerIo {
    /// Build I/O around an externally supplied pair, used by supervisor tests.
    pub fn from_parts(
        reader: Box<dyn AsyncRead + Unpin + Send>,
        writer: Box<dyn AsyncWrite + Unpin + Send>,
    ) -> Self {
        Self {
            reader,
            writer,
            child: None,
        }
    }

    /// Operating-system process id when this is a real child.
    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().and_then(Child::id)
    }
}

/// Creates one worker transport for a supervised attempt.
#[async_trait]
pub trait WorkerLauncher: Send + Sync {
    /// Launch and return a fresh worker transport.
    async fn launch(&self, config: &SupervisorConfig) -> Result<WorkerIo, WorkerError>;
}

/// Production launcher spawning the configured executable.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessLauncher;

#[async_trait]
impl WorkerLauncher for ProcessLauncher {
    async fn launch(&self, config: &SupervisorConfig) -> Result<WorkerIo, WorkerError> {
        let mut child = Command::new(&config.worker_program)
            .args(&config.worker_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| WorkerError::Io("worker stdin was not piped".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| WorkerError::Io("worker stdout was not piped".into()))?;
        Ok(WorkerIo {
            reader: Box::new(stdout),
            writer: Box::new(stdin),
            child: Some(child),
        })
    }
}

/// Snapshot of one supervised worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerStatus {
    /// Whether a handshake-complete worker is currently attached.
    pub connected: bool,
    /// Worker identifier from the hello.
    pub worker_id: Option<String>,
    /// Provider identifier from the acknowledgement.
    pub provider_id: Option<String>,
    /// Browser metadata from the acknowledgement.
    pub browser: Option<WorkerBrowser>,
    /// Operating-system process id when available.
    pub pid: Option<u32>,
    /// Successful and failed restart attempts since start.
    pub restarts: usize,
}

struct Session {
    io: WorkerIo,
    worker_id: String,
    provider_id: String,
    browser: Option<WorkerBrowser>,
}

/// Supervises one child worker, its handshake and serial invokes.
pub struct WorkerSupervisor {
    config: SupervisorConfig,
    launcher: Arc<dyn WorkerLauncher>,
    session: Option<Session>,
    restarts: usize,
    next_invoke: u64,
}

impl WorkerSupervisor {
    /// Start the configured worker with the production process launcher.
    pub async fn start(config: SupervisorConfig) -> Result<Self, WorkerError> {
        Self::start_with_launcher(config, Arc::new(ProcessLauncher)).await
    }

    /// Start with an injected launcher.
    pub async fn start_with_launcher(
        config: SupervisorConfig,
        launcher: Arc<dyn WorkerLauncher>,
    ) -> Result<Self, WorkerError> {
        let mut supervisor = Self {
            config,
            launcher,
            session: None,
            restarts: 0,
            next_invoke: 0,
        };
        supervisor.spawn_once().await?;
        Ok(supervisor)
    }

    /// Start only when the v2 worker opt-in is enabled.
    pub async fn start_from_env() -> Result<Option<Self>, WorkerError> {
        match SupervisorConfig::from_env()? {
            Some(config) => Ok(Some(Self::start(config).await?)),
            None => Ok(None),
        }
    }

    /// Current supervised status.
    pub fn status(&self) -> WorkerStatus {
        WorkerStatus {
            connected: self.session.is_some(),
            worker_id: self.session.as_ref().map(|s| s.worker_id.clone()),
            provider_id: self.session.as_ref().map(|s| s.provider_id.clone()),
            browser: self.session.as_ref().and_then(|s| s.browser.clone()),
            pid: self.session.as_ref().and_then(|s| s.io.pid()),
            restarts: self.restarts,
        }
    }

    async fn spawn_once(&mut self) -> Result<(), WorkerError> {
        let mut io = self.launcher.launch(&self.config).await?;
        let hello = WorkerMessage::WorkerHello {
            protocol: 1,
            worker_id: self.config.worker_id.clone(),
            browser_id: self.config.browser_id.clone(),
            cdp_ws_endpoint: self.config.cdp_ws_endpoint.clone(),
            home_dir: self.config.home_dir.to_string_lossy().into_owned(),
            log_level: self.config.log_level.clone(),
        };
        write_frame(&mut io.writer, &serde_json::to_value(&hello)?).await?;
        let frame = timeout(
            self.config.handshake_timeout,
            crate::frame::read_handshake(&mut io.reader),
        )
        .await
        .map_err(|_| WorkerError::Timeout("worker handshake".into()))??;
        let ack: WorkerMessage = serde_json::from_value(frame)?;
        let (worker_id, provider_id, browser) = match ack {
            WorkerMessage::WorkerAck {
                protocol,
                worker_id,
                provider_id,
                ok,
                browser,
                error,
                ..
            } => {
                if protocol != 1 {
                    return Err(WorkerError::ProtocolVersion(protocol));
                }
                if !ok {
                    let detail = error
                        .map(|e| format!("{}: {}", e.code, e.message))
                        .unwrap_or_else(|| "worker refused the handshake".into());
                    return Err(WorkerError::Handshake(detail));
                }
                if worker_id != self.config.worker_id {
                    return Err(WorkerError::Handshake("worker id mismatch".into()));
                }
                (worker_id, provider_id, browser)
            }
            other => {
                return Err(WorkerError::Handshake(format!(
                    "expected worker_ack, got {}",
                    other.kind()
                )))
            }
        };
        self.session = Some(Session {
            io,
            worker_id,
            provider_id,
            browser,
        });
        Ok(())
    }

    /// Invoke one action and wait for its matching result.
    pub async fn invoke(
        &mut self,
        action: &str,
        args: serde_json::Value,
        deadline_ms: u64,
    ) -> Result<serde_json::Value, WorkerError> {
        let session = self.session.as_mut().ok_or(WorkerError::ProviderLost)?;
        self.next_invoke += 1;
        let id = format!("i-{}", self.next_invoke);
        let invoke = WorkerMessage::Invoke {
            id: id.clone(),
            action: action.into(),
            args,
            deadline_ms,
        };
        if let Err(error) =
            write_frame(&mut session.io.writer, &serde_json::to_value(&invoke)?).await
        {
            self.session = None;
            return Err(error);
        }
        let deadline = Duration::from_millis(deadline_ms.max(1));
        loop {
            let frame = match timeout(deadline, read_frame(&mut session.io.reader)).await {
                Ok(Ok(frame)) => frame,
                Ok(Err(WorkerError::Io(_))) => {
                    self.session = None;
                    return Err(WorkerError::ProviderLost);
                }
                Ok(Err(error)) => {
                    self.session = None;
                    return Err(error);
                }
                Err(_) => return Err(WorkerError::Timeout(format!("invoke {id}"))),
            };
            let message: WorkerMessage = serde_json::from_value(frame)?;
            match message {
                WorkerMessage::Result {
                    id: result_id,
                    ok,
                    value,
                    error,
                } if result_id == id => {
                    if ok {
                        return Ok(value.unwrap_or(serde_json::Value::Null));
                    }
                    let body = error.unwrap_or(crate::protocol::WorkerErrorBody {
                        code: "WORKER_ERROR".into(),
                        message: "worker returned no error body".into(),
                        retryable: false,
                    });
                    return Err(WorkerError::Cdp(format!("{}: {}", body.code, body.message)));
                }
                WorkerMessage::Event { .. } => continue,
                WorkerMessage::Error { code, message, .. } => {
                    return Err(WorkerError::Cdp(format!("{code}: {message}")))
                }
                other => {
                    return Err(WorkerError::Malformed(format!(
                        "unexpected worker frame {}",
                        other.kind()
                    )))
                }
            }
        }
    }

    /// Kill the current transport, used before restart and in tests.
    pub fn terminate(&mut self) {
        self.session = None;
    }

    /// Restart with bounded exponential backoff.
    pub async fn restart(&mut self) -> Result<(), WorkerError> {
        self.session = None;
        let attempts = self.config.max_restarts.max(1);
        for attempt in 0..attempts {
            let delay = self
                .config
                .backoff
                .get(attempt.min(self.config.backoff.len().saturating_sub(1)))
                .copied()
                .unwrap_or(Duration::from_secs(8));
            tokio::time::sleep(delay).await;
            self.restarts += 1;
            if self.spawn_once().await.is_ok() {
                return Ok(());
            }
        }
        Err(WorkerError::ProviderLost)
    }

    /// Ask the worker to shut down and enforce a two-second exit deadline.
    pub async fn shutdown(mut self) {
        if let Some(mut session) = self.session.take() {
            let _ = write_frame(&mut session.io.writer, &json!({"kind": "shutdown"})).await;
            if let Some(child) = session.io.child.as_mut() {
                if timeout(Duration::from_secs(2), child.wait()).await.is_err() {
                    let _ = child.kill().await;
                }
            }
        }
    }
}

/// Whether an endpoint is a loopback `ws://` URL on a non-user port.
fn permitted_loopback_endpoint(endpoint: &str) -> bool {
    let Some(rest) = endpoint.strip_prefix("ws://") else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or_default();
    let (host, port) = if let Some(after) = authority.strip_prefix('[') {
        let Some((host, tail)) = after.split_once(']') else {
            return false;
        };
        let Some(port) = tail.strip_prefix(':') else {
            return false;
        };
        (host, port)
    } else {
        let Some((host, port)) = authority.rsplit_once(':') else {
            return false;
        };
        (host, port)
    };
    let loopback = matches!(host, "127.0.0.1" | "localhost" | "::1");
    let port = port.parse::<u16>().ok();
    loopback && matches!(port, Some(value) if value != 9222)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use async_trait::async_trait;
    use serde_json::json;
    use tokio::io::DuplexStream;

    use super::*;
    use crate::frame::{read_frame, read_handshake};

    fn config(program: &str) -> SupervisorConfig {
        SupervisorConfig {
            worker_program: PathBuf::from(program),
            worker_args: Vec::new(),
            cdp_ws_endpoint: "ws://127.0.0.1:9476/devtools/browser/test".into(),
            home_dir: std::env::temp_dir(),
            worker_id: "w-test".into(),
            browser_id: "b-test".into(),
            log_level: "info".into(),
            handshake_timeout: Duration::from_secs(1),
            backoff: vec![Duration::from_millis(1), Duration::from_millis(1)],
            max_restarts: 2,
        }
    }

    fn spawn_fake_worker() -> (WorkerIo, tokio::task::JoinHandle<()>) {
        let (supervisor_side, worker_side) = tokio::io::duplex(64 * 1024);
        let (reader, writer) = tokio::io::split(supervisor_side);
        let (worker_reader, worker_writer) = tokio::io::split(worker_side);
        let task = tokio::spawn(fake_worker_loop(worker_reader, worker_writer));
        (
            WorkerIo::from_parts(Box::new(reader), Box::new(writer)),
            task,
        )
    }

    async fn fake_worker_loop(
        mut reader: tokio::io::ReadHalf<DuplexStream>,
        mut writer: tokio::io::WriteHalf<DuplexStream>,
    ) {
        let hello = read_handshake(&mut reader).await.expect("hello");
        assert_eq!(hello["kind"], "worker_hello");
        let worker_id = hello["worker_id"].as_str().expect("worker id");
        let ack = json!({
            "kind": "worker_ack",
            "protocol": 1,
            "worker_id": worker_id,
            "provider_id": "cdp-test",
            "ok": true,
            "browser": {"version": "Corvus/157.0.0.0", "protocol_version": "1.3"},
            "capabilities": ["cdp"]
        });
        write_frame(&mut writer, &ack).await.expect("ack");
        loop {
            let frame = match read_frame(&mut reader).await {
                Ok(frame) => frame,
                Err(_) => return,
            };
            match frame["kind"].as_str() {
                Some("invoke") => {
                    let id = frame["id"].as_str().expect("invoke id");
                    let result = json!({
                        "kind": "result",
                        "id": id,
                        "ok": true,
                        "value": {"browser": "Corvus/157.0.0.0", "protocol_version": "1.3"}
                    });
                    write_frame(&mut writer, &result).await.expect("result");
                }
                Some("shutdown") => return,
                _ => {}
            }
        }
    }

    struct FakeLauncher {
        attempts: AtomicUsize,
        fail_after: usize,
    }

    #[async_trait]
    impl WorkerLauncher for FakeLauncher {
        async fn launch(&self, _config: &SupervisorConfig) -> Result<WorkerIo, WorkerError> {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst) + 1;
            if attempt > self.fail_after {
                return Err(WorkerError::Handshake("test launcher refused".into()));
            }
            Ok(spawn_fake_worker().0)
        }
    }

    #[tokio::test]
    async fn starts_handshakes_and_invokes_one_action() {
        let launcher = Arc::new(FakeLauncher {
            attempts: AtomicUsize::new(0),
            fail_after: 10,
        });
        let mut supervisor = WorkerSupervisor::start_with_launcher(config("fake"), launcher)
            .await
            .expect("start");
        let status = supervisor.status();
        assert!(status.connected);
        assert_eq!(status.provider_id.as_deref(), Some("cdp-test"));
        let value = supervisor
            .invoke("cdp.get_version", json!({}), 1_000)
            .await
            .expect("invoke");
        assert_eq!(value["browser"], "Corvus/157.0.0.0");
        supervisor.shutdown().await;
    }

    #[tokio::test]
    async fn killed_child_reports_provider_lost() {
        let launcher = Arc::new(FakeLauncher {
            attempts: AtomicUsize::new(0),
            fail_after: 1,
        });
        let mut supervisor = WorkerSupervisor::start_with_launcher(config("fake"), launcher)
            .await
            .expect("start");
        supervisor.terminate();
        let error = supervisor
            .invoke("cdp.get_version", json!({}), 1_000)
            .await
            .expect_err("lost worker");
        assert_eq!(error, WorkerError::ProviderLost);
        assert!(!supervisor.status().connected);
    }

    #[tokio::test]
    async fn restart_is_bounded_and_reports_provider_lost() {
        let launcher = Arc::new(FakeLauncher {
            attempts: AtomicUsize::new(0),
            fail_after: 1,
        });
        let mut supervisor = WorkerSupervisor::start_with_launcher(config("fake"), launcher)
            .await
            .expect("start");
        let attempts = supervisor.config.max_restarts;
        supervisor.terminate();
        let error = supervisor.restart().await.expect_err("no restart");
        assert_eq!(error, WorkerError::ProviderLost);
        assert_eq!(supervisor.status().restarts, attempts);
        assert!(!supervisor.status().connected);
    }

    #[test]
    fn endpoint_validation_only_accepts_loopback_and_refuses_user_edge() {
        assert!(permitted_loopback_endpoint(
            "ws://127.0.0.1:9476/devtools/browser/test"
        ));
        assert!(permitted_loopback_endpoint("ws://localhost:9477/x"));
        assert!(permitted_loopback_endpoint(
            "ws://[::1]:9478/devtools/browser/test"
        ));
        assert!(!permitted_loopback_endpoint(
            "ws://127.0.0.1:9222/devtools/browser/test"
        ));
        assert!(!permitted_loopback_endpoint(
            "ws://192.0.2.1:9476/devtools/browser/test"
        ));
        assert!(!permitted_loopback_endpoint("https://127.0.0.1:9476/x"));
    }
}
