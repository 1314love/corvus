//! Encrypted provider gateway on the daemon's loopback HTTP listener.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::ws::{CloseFrame, Message, Utf8Bytes, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use corvus_provider::{
    BrowserInfo, EstablishedProvider, Manifest, PairingToken, ProviderError, ProviderResult,
    ServerHandshake,
};
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot, watch, Mutex};
use uuid::Uuid;

/// HTTP path of the encrypted provider channel.
pub const PROVIDER_PATH: &str = corvus_provider::PROVIDER_PATH;
/// Maximum time for the full handshake, including the manifest.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// Maximum idle time after a manifest is accepted.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(45);

const COMMAND_QUEUE: usize = 8;
const PROVIDER_REPLACED_CLOSE_CODE: u16 = 1012;
const PROVIDER_IDLE_CLOSE_CODE: u16 = 1011;
const PROVIDER_TRANSPORT_CLOSE_CODE: u16 = 1011;

type ProviderSink = SplitSink<WebSocket, Message>;
type ProviderStream = SplitStream<WebSocket>;

/// A failure while invoking a connected provider.
#[derive(Debug, thiserror::Error)]
pub enum ProviderGatewayError {
    /// No provider with that identifier is connected.
    #[error("provider {0} is not connected")]
    NotConnected(String),
    /// The provider closed its connection while the invoke was in flight.
    #[error("provider connection closed")]
    Closed,
    /// The provider did not answer before the caller deadline.
    #[error("provider invoke timed out")]
    Timeout,
    /// The provider answered with a protocol error.
    #[error("provider protocol error: {0}")]
    Protocol(String),
}

/// A read-only view of one connected provider.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderSnapshot {
    /// Stable provider identifier.
    pub provider_id: String,
    /// Accepted manifest.
    pub manifest: Manifest,
    /// Browser metadata from the manifest.
    pub browser: BrowserInfo,
    /// Connection time in Unix milliseconds.
    pub connected_at_ms: u64,
    /// Number of encrypted inbound frames accepted on this connection.
    pub last_seq: u64,
}

struct ProviderEntry {
    connection_id: Uuid,
    manifest: Manifest,
    connected_at_ms: u64,
    last_seq: Arc<AtomicU64>,
    commands: mpsc::Sender<ProviderCommand>,
    cancel: watch::Sender<bool>,
}

#[derive(Default)]
struct Registry {
    providers: HashMap<String, ProviderEntry>,
}

enum ProviderCommand {
    Invoke {
        action: String,
        args: Value,
        deadline_ms: u64,
        reply: oneshot::Sender<Result<ProviderResult, ProviderGatewayError>>,
    },
}

enum CommandOutcome {
    Continue,
    Close(u16, String),
}

enum WaitFailure {
    Cancel,
    Provider(ProviderError),
}

/// Gateway that authenticates providers and keeps their live registrations.
pub struct ProviderGateway {
    token: Arc<PairingToken>,
    registry: Arc<Mutex<Registry>>,
}

impl ProviderGateway {
    /// Build a gateway around the pairing token loaded for this home.
    pub fn new(token: PairingToken) -> Self {
        Self {
            token: Arc::new(token),
            registry: Arc::new(Mutex::new(Registry::default())),
        }
    }

    /// Build the axum router carrying the encrypted provider route.
    pub fn router(state: Arc<Self>) -> Router {
        Router::new()
            .route(PROVIDER_PATH, get(provider_socket))
            .with_state(state)
    }

    /// Snapshot every connected provider.
    pub async fn snapshot(&self) -> Vec<ProviderSnapshot> {
        let registry = self.registry.lock().await;
        let mut snapshots = registry
            .providers
            .values()
            .map(|entry| ProviderSnapshot {
                provider_id: entry.manifest.provider_id.clone(),
                manifest: entry.manifest.clone(),
                browser: entry.manifest.browser.clone(),
                connected_at_ms: entry.connected_at_ms,
                last_seq: entry.last_seq.load(Ordering::Relaxed),
            })
            .collect::<Vec<_>>();
        snapshots.sort_by(|left, right| left.provider_id.cmp(&right.provider_id));
        snapshots
    }

    /// Invoke one action on a connected provider and await its result.
    pub async fn invoke(
        &self,
        provider_id: &str,
        action: &str,
        args: Value,
        timeout: Duration,
    ) -> Result<ProviderResult, ProviderGatewayError> {
        let commands = {
            let registry = self.registry.lock().await;
            registry
                .providers
                .get(provider_id)
                .map(|entry| entry.commands.clone())
                .ok_or_else(|| ProviderGatewayError::NotConnected(provider_id.to_string()))?
        };
        let deadline_ms = timeout.as_millis().clamp(1, 300_000) as u64;
        let (reply, response) = oneshot::channel();
        commands
            .send(ProviderCommand::Invoke {
                action: action.to_string(),
                args,
                deadline_ms,
                reply,
            })
            .await
            .map_err(|_| ProviderGatewayError::NotConnected(provider_id.to_string()))?;
        match tokio::time::timeout(timeout, response).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(ProviderGatewayError::Closed),
            Err(_) => Err(ProviderGatewayError::Timeout),
        }
    }

    async fn serve(self: Arc<Self>, socket: WebSocket) {
        let connection_id = Uuid::new_v4();
        let (mut sink, mut stream) = socket.split();
        let handshake = tokio::time::timeout(
            HANDSHAKE_TIMEOUT,
            Self::handshake(&mut sink, &mut stream, &self.token),
        )
        .await;
        let (mut provider, manifest) = match handshake {
            Ok(Ok(value)) => value,
            Ok(Err(error)) => {
                close_socket(
                    &mut sink,
                    &mut stream,
                    error.close_code(),
                    &error.to_string(),
                )
                .await;
                return;
            }
            Err(_) => {
                close_socket(
                    &mut sink,
                    &mut stream,
                    ProviderError::HandshakeTimeout.close_code(),
                    "provider handshake timed out",
                )
                .await;
                return;
            }
        };

        let provider_id = manifest.provider_id.clone();
        let last_seq = Arc::new(AtomicU64::new(2));
        let (commands, command_rx) = mpsc::channel(COMMAND_QUEUE);
        let (cancel, cancel_rx) = watch::channel(false);
        let entry = ProviderEntry {
            connection_id,
            manifest,
            connected_at_ms: now_ms(),
            last_seq: Arc::clone(&last_seq),
            commands,
            cancel,
        };
        let replaced = {
            let mut registry = self.registry.lock().await;
            registry.providers.insert(provider_id.clone(), entry)
        };
        if let Some(previous) = replaced {
            let _ = previous.cancel.send(true);
        }

        let acknowledgement = match provider.manifest_ack_frame() {
            Ok(frame) => frame,
            Err(error) => {
                close_socket(
                    &mut sink,
                    &mut stream,
                    error.close_code(),
                    &error.to_string(),
                )
                .await;
                self.unregister(&provider_id, connection_id).await;
                return;
            }
        };
        if let Err(error) = send_raw(&mut sink, &acknowledgement).await {
            close_socket(
                &mut sink,
                &mut stream,
                error.close_code(),
                &error.to_string(),
            )
            .await;
            self.unregister(&provider_id, connection_id).await;
            return;
        }

        let mut command_rx = command_rx;
        Self::serve_established(provider, sink, stream, &mut command_rx, cancel_rx, last_seq).await;
        self.unregister(&provider_id, connection_id).await;
    }

    async fn unregister(&self, provider_id: &str, connection_id: Uuid) {
        let mut registry = self.registry.lock().await;
        let current = registry
            .providers
            .get(provider_id)
            .is_some_and(|entry| entry.connection_id == connection_id);
        if current {
            registry.providers.remove(provider_id);
        }
    }

    async fn handshake(
        sink: &mut ProviderSink,
        stream: &mut ProviderStream,
        token: &PairingToken,
    ) -> Result<(EstablishedProvider, Manifest), ProviderError> {
        let (handshake, hello) = ServerHandshake::new(token.as_bytes())?;
        send_json(sink, &hello).await?;
        let client_hello = recv_text(stream, corvus_provider::MAX_HANDSHAKE_BYTES).await?;
        let pending = handshake.accept_raw(client_hello.as_bytes())?;
        let server_confirm = pending.server_confirm_frame().to_vec();
        send_raw(sink, &server_confirm).await?;
        let client_confirm = recv_text(stream, corvus_provider::MAX_FRAME_BYTES).await?;
        let mut established = pending.accept_client_confirm(client_confirm.as_bytes())?;
        let manifest_frame = recv_text(stream, corvus_provider::MAX_FRAME_BYTES).await?;
        let manifest = established
            .accept_manifest(manifest_frame.as_bytes())?
            .clone();
        Ok((established, manifest))
    }

    async fn serve_established(
        mut provider: EstablishedProvider,
        mut sink: ProviderSink,
        mut stream: ProviderStream,
        command_rx: &mut mpsc::Receiver<ProviderCommand>,
        mut cancel: watch::Receiver<bool>,
        last_seq: Arc<AtomicU64>,
    ) {
        loop {
            tokio::select! {
                _ = cancel.changed() => {
                    close_socket(
                        &mut sink,
                        &mut stream,
                        PROVIDER_REPLACED_CLOSE_CODE,
                        "provider replaced",
                    )
                    .await;
                    break;
                }
                incoming = stream.next() => {
                    match incoming {
                        Some(Ok(Message::Close(_))) | None => break,
                        Some(Err(_)) => break,
                        Some(Ok(Message::Text(_))) | Some(Ok(Message::Binary(_))) => {
                            close_socket(
                                &mut sink,
                                &mut stream,
                                ProviderError::Malformed("unexpected provider frame".into()).close_code(),
                                "unexpected provider frame",
                            )
                            .await;
                            break;
                        }
                        Some(Ok(_)) => {}
                    }
                }
                command = command_rx.recv() => {
                    let Some(command) = command else {
                        if *cancel.borrow() {
                            close_socket(
                                &mut sink,
                                &mut stream,
                                PROVIDER_REPLACED_CLOSE_CODE,
                                "provider replaced",
                            )
                            .await;
                        }
                        break;
                    };
                    match Self::handle_command(&mut provider, &mut sink, &mut stream, &mut cancel, &last_seq, command).await {
                        CommandOutcome::Continue => {}
                        CommandOutcome::Close(code, reason) => {
                            close_socket(&mut sink, &mut stream, code, &reason).await;
                            break;
                        }
                    }
                }
                _ = tokio::time::sleep(IDLE_TIMEOUT) => {
                    close_socket(
                        &mut sink,
                        &mut stream,
                        PROVIDER_IDLE_CLOSE_CODE,
                        "provider idle timeout",
                    )
                    .await;
                    break;
                }
            }
        }
    }

    async fn handle_command(
        provider: &mut EstablishedProvider,
        sink: &mut ProviderSink,
        stream: &mut ProviderStream,
        cancel: &mut watch::Receiver<bool>,
        last_seq: &AtomicU64,
        command: ProviderCommand,
    ) -> CommandOutcome {
        let ProviderCommand::Invoke {
            action,
            args,
            deadline_ms,
            reply,
        } = command;
        let frame = match provider.start_invoke(&action, &args, deadline_ms) {
            Ok(frame) => frame,
            Err(error) => {
                let _ = reply.send(Err(ProviderGatewayError::Protocol(error.to_string())));
                return CommandOutcome::Continue;
            }
        };
        if let Err(error) = send_raw(sink, &frame).await {
            let _ = reply.send(Err(ProviderGatewayError::Closed));
            return CommandOutcome::Close(PROVIDER_TRANSPORT_CLOSE_CODE, error.to_string());
        }
        let wait = Duration::from_millis(deadline_ms).min(IDLE_TIMEOUT);
        match tokio::time::timeout(wait, await_result(provider, stream, cancel)).await {
            Ok(Ok(result)) => {
                last_seq.fetch_add(1, Ordering::Relaxed);
                let _ = reply.send(Ok(result));
                CommandOutcome::Continue
            }
            Ok(Err(WaitFailure::Cancel)) => {
                let _ = reply.send(Err(ProviderGatewayError::Closed));
                CommandOutcome::Close(PROVIDER_REPLACED_CLOSE_CODE, "provider replaced".into())
            }
            Ok(Err(WaitFailure::Provider(error))) => {
                let _ = reply.send(Err(ProviderGatewayError::Protocol(error.to_string())));
                CommandOutcome::Close(error.close_code(), error.to_string())
            }
            Err(_) => {
                let _ = reply.send(Err(ProviderGatewayError::Timeout));
                CommandOutcome::Close(PROVIDER_IDLE_CLOSE_CODE, "provider invoke timed out".into())
            }
        }
    }
}

async fn provider_socket(
    upgrade: WebSocketUpgrade,
    State(gateway): State<Arc<ProviderGateway>>,
) -> impl IntoResponse {
    upgrade.on_upgrade(move |socket| gateway.serve(socket))
}

async fn await_result(
    provider: &mut EstablishedProvider,
    stream: &mut ProviderStream,
    cancel: &mut watch::Receiver<bool>,
) -> Result<ProviderResult, WaitFailure> {
    loop {
        tokio::select! {
            _ = cancel.changed() => return Err(WaitFailure::Cancel),
            incoming = stream.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        if text.as_str().len() > corvus_provider::MAX_FRAME_BYTES {
                            return Err(WaitFailure::Provider(ProviderError::TooLarge));
                        }
                        return provider
                            .accept_result(text.as_str().as_bytes())
                            .map_err(WaitFailure::Provider);
                    }
                    Some(Ok(Message::Binary(_))) => {
                        return Err(WaitFailure::Provider(ProviderError::Malformed(
                            "text frames only".into(),
                        )));
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        return Err(WaitFailure::Provider(ProviderError::Io(
                            "provider connection closed".into(),
                        )));
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        return Err(WaitFailure::Provider(ProviderError::Io(error.to_string())));
                    }
                }
            }
        }
    }
}

async fn recv_text(stream: &mut ProviderStream, max: usize) -> Result<String, ProviderError> {
    loop {
        match stream.next().await {
            Some(Ok(Message::Text(text))) => {
                if text.as_str().len() > max {
                    return Err(ProviderError::TooLarge);
                }
                return Ok(text.as_str().to_string());
            }
            Some(Ok(Message::Binary(_))) => {
                return Err(ProviderError::Malformed("text frames only".into()));
            }
            Some(Ok(Message::Close(_))) | None => {
                return Err(ProviderError::Io("provider connection closed".into()));
            }
            Some(Ok(_)) => {}
            Some(Err(error)) => return Err(ProviderError::Io(error.to_string())),
        }
    }
}

async fn send_raw(sink: &mut ProviderSink, frame: &[u8]) -> Result<(), ProviderError> {
    let text = std::str::from_utf8(frame)
        .map_err(|_| ProviderError::Malformed("provider frame is not UTF-8".into()))?;
    sink.send(Message::Text(text.to_owned().into()))
        .await
        .map_err(|error| ProviderError::Io(error.to_string()))
}

async fn send_json<T: Serialize>(sink: &mut ProviderSink, value: &T) -> Result<(), ProviderError> {
    let frame = serde_json::to_vec(value)?;
    send_raw(sink, &frame).await
}

async fn close_socket(
    sink: &mut ProviderSink,
    stream: &mut ProviderStream,
    code: u16,
    reason: &str,
) {
    let reason = Utf8Bytes::from(reason);
    let _ = sink
        .send(Message::Close(Some(CloseFrame { code, reason })))
        .await;
    let _ = sink.flush().await;
    let _ = tokio::time::timeout(Duration::from_millis(500), async {
        while let Some(message) = stream.next().await {
            match message {
                Ok(Message::Close(_)) | Err(_) => break,
                _ => {}
            }
        }
    })
    .await;
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}
