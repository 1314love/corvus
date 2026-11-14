//! WebSocket server: accepts extension connections, performs the handshake,
//! answers liveness probes and forwards everything else to the runtime.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

use corvus_proto::ids;
use corvus_proto::{
    Envelope, ErrorBody, ErrorCode, Hello, HelloAck, Kind, Limits, DAEMON_NAME, PROTOCOL_VERSION,
};

use crate::registry::{now_millis, BrowserConnection, BrowserInfo, Outbound, Registry};

/// Something the transport hands to the runtime.
#[derive(Debug)]
pub enum Inbound {
    /// A response or an event sent by a connected browser.
    Message {
        /// Browser the message came from.
        browser: String,
        /// Socket session the message came from.
        connection: String,
        /// The decoded envelope.
        envelope: Box<Envelope>,
    },
    /// A browser connection ended, for whatever reason.
    Closed {
        /// Browser that disconnected.
        browser: String,
        /// Socket session that ended.
        connection: String,
    },
}

/// A WebSocket listener waiting to be served.
#[derive(Debug)]
pub struct WsServer {
    listener: TcpListener,
    local_addr: SocketAddr,
}

impl WsServer {
    /// Bind the listener. Use port 0 to let the operating system choose.
    pub async fn bind(addr: SocketAddr) -> std::io::Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        Ok(Self {
            listener,
            local_addr,
        })
    }

    /// The address the listener is bound to.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Serve connections until the shutdown signal flips.
    pub async fn serve(
        self,
        registry: Arc<Registry>,
        inbound: mpsc::Sender<Inbound>,
        mut shutdown: watch::Receiver<bool>,
    ) {
        tracing::info!(addr = %self.local_addr, "websocket transport listening");
        loop {
            if *shutdown.borrow() {
                break;
            }
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
                accepted = self.listener.accept() => {
                    match accepted {
                        Ok((stream, remote)) => {
                            tokio::spawn(handle_connection(
                                stream,
                                remote,
                                Arc::clone(&registry),
                                inbound.clone(),
                            ));
                        }
                        Err(error) => {
                            tracing::warn!(error = %error, "websocket accept failed");
                            tokio::time::sleep(Duration::from_millis(50)).await;
                        }
                    }
                }
            }
        }
        tracing::debug!("websocket transport stopped");
    }

    /// Serve on a background task.
    pub fn spawn(
        self,
        registry: Arc<Registry>,
        inbound: mpsc::Sender<Inbound>,
        shutdown: watch::Receiver<bool>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.serve(registry, inbound, shutdown))
    }
}

async fn handle_connection(
    stream: TcpStream,
    remote: SocketAddr,
    registry: Arc<Registry>,
    inbound: mpsc::Sender<Inbound>,
) {
    if let Err(error) = stream.set_nodelay(true) {
        tracing::debug!(error = %error, "could not disable Nagle algorithm");
    }
    let websocket = match accept_async(stream).await {
        Ok(websocket) => websocket,
        Err(error) => {
            tracing::debug!(remote = %remote, error = %error, "websocket handshake failed");
            return;
        }
    };
    let (mut sink, mut source) = websocket.split();
    let (outbound, mut outbox) = mpsc::unbounded_channel::<Outbound>();

    let writer = tokio::spawn(async move {
        while let Some(frame) = outbox.recv().await {
            let message = match frame {
                Outbound::Envelope(envelope) => match serde_json::to_string(&envelope) {
                    Ok(text) => Message::text(text),
                    Err(error) => {
                        tracing::error!(error = %error, "failed to serialize envelope");
                        continue;
                    }
                },
                Outbound::Close => {
                    let _ = sink.send(Message::Close(None)).await;
                    break;
                }
            };
            if let Err(error) = sink.send(message).await {
                tracing::debug!(error = %error, "websocket write failed");
                break;
            }
        }
        let _ = sink.close().await;
    });

    let mut session: Option<(String, String)> = None;
    while let Some(frame) = source.next().await {
        let frame = match frame {
            Ok(frame) => frame,
            Err(error) => {
                tracing::debug!(error = %error, "websocket read failed");
                break;
            }
        };
        let text = match frame {
            Message::Text(text) => text.as_str().to_string(),
            Message::Binary(_) => {
                tracing::warn!("binary frames are not supported");
                break;
            }
            Message::Close(_) => break,
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
        };
        let envelope: Envelope = match serde_json::from_str(&text) {
            Ok(envelope) => envelope,
            Err(error) => {
                tracing::warn!(error = %error, "rejecting malformed envelope");
                reject(
                    &outbound,
                    "malformed",
                    ErrorCode::BadArgs,
                    format!("malformed envelope: {error}"),
                );
                break;
            }
        };
        if let Err(error) = envelope.validate() {
            tracing::warn!(code = %error.code, "rejecting invalid envelope");
            let id = envelope.id.clone();
            let _ = outbound.send(Outbound::Envelope(Box::new(Envelope::response_err(
                id, error,
            ))));
            break;
        }

        match &session {
            None => {
                if envelope.kind != Kind::Hello {
                    reject(
                        &outbound,
                        envelope.id.clone(),
                        ErrorCode::BadArgs,
                        "the first envelope must be hello",
                    );
                    break;
                }
                match complete_handshake(&envelope, remote, &registry, &outbound) {
                    Ok((browser, connection, answer)) => {
                        let _ = outbound.send(Outbound::Envelope(Box::new(answer)));
                        session = Some((browser, connection));
                    }
                    Err(error) => {
                        tracing::warn!(code = %error.code, message = %error.message, "handshake rejected");
                        let id = envelope.id.clone();
                        let _ = outbound.send(Outbound::Envelope(Box::new(
                            Envelope::response_err(id, error),
                        )));
                        break;
                    }
                }
            }
            Some((browser, _)) => match envelope.kind {
                Kind::Ping => {
                    let pong = Envelope::pong(envelope.id.clone()).with_browser(browser.clone());
                    let _ = outbound.send(Outbound::Envelope(Box::new(pong)));
                }
                Kind::Pong => {}
                Kind::Response | Kind::Event => {
                    let browser = browser.clone();
                    let connection = session
                        .as_ref()
                        .map(|(_, connection)| connection.clone())
                        .unwrap_or_default();
                    if inbound
                        .send(Inbound::Message {
                            browser,
                            connection,
                            envelope: Box::new(envelope),
                        })
                        .await
                        .is_err()
                    {
                        tracing::debug!("runtime is gone; closing connection");
                        break;
                    }
                }
                Kind::Command => reject(
                    &outbound,
                    envelope.id.clone(),
                    ErrorCode::BadArgs,
                    "commands originate from the daemon",
                ),
                Kind::Hello | Kind::HelloAck => reject(
                    &outbound,
                    envelope.id.clone(),
                    ErrorCode::BadArgs,
                    "the handshake is already complete",
                ),
            },
        }
    }

    match session {
        Some((browser, connection)) => {
            if registry.remove(&browser, &connection) {
                tracing::info!(browser = %browser, "browser disconnected");
            }
            let _ = outbound.send(Outbound::Close);
            drop(outbound);
            let _ = writer.await;
            let _ = inbound
                .send(Inbound::Closed {
                    browser,
                    connection,
                })
                .await;
        }
        None => {
            drop(outbound);
        }
    }
}

fn complete_handshake(
    envelope: &Envelope,
    remote: SocketAddr,
    registry: &Arc<Registry>,
    outbound: &mpsc::UnboundedSender<Outbound>,
) -> Result<(String, String, Envelope), ErrorBody> {
    let browser = envelope.browser.clone().unwrap_or_default();
    if browser.trim().is_empty() {
        return Err(ErrorBody::new(
            ErrorCode::BadArgs,
            "hello without a browser identifier",
        ));
    }
    let hello: Hello = serde_json::from_value(envelope.args.clone()).map_err(|error| {
        ErrorBody::new(
            ErrorCode::BadArgs,
            format!("invalid hello payload: {error}"),
        )
    })?;
    if hello.profile.trim().is_empty() {
        return Err(ErrorBody::new(
            ErrorCode::BadArgs,
            "hello without a profile name",
        ));
    }
    let capabilities = hello.negotiated();
    let connection = ids::connection_id();
    let info = BrowserInfo {
        id: browser.clone(),
        profile: hello.profile.clone(),
        version: hello.version.clone(),
        extension: hello.extension.clone(),
        capabilities: capabilities.clone(),
        connected_at_ms: now_millis(),
        remote: remote.to_string(),
    };
    tracing::info!(
        browser = %browser,
        profile = %hello.profile,
        version = %hello.version,
        offered = ?hello.capabilities,
        negotiated = ?capabilities,
        remote = %remote,
        "browser connected"
    );
    let connection_state = Arc::new(BrowserConnection::new(
        info,
        connection.clone(),
        outbound.clone(),
    ));
    if let Some(previous) = registry.register(connection_state) {
        tracing::info!(browser = %browser, "replacing an older session");
        previous.close();
    }
    let ack = HelloAck {
        daemon: DAEMON_NAME.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        protocol: PROTOCOL_VERSION,
        capabilities,
        limits: Limits::v1(),
    };
    let answer = Envelope::hello_ack(envelope.id.clone(), ack).with_browser(browser.clone());
    Ok((browser, connection, answer))
}

fn reject(
    outbound: &mpsc::UnboundedSender<Outbound>,
    id: impl Into<String>,
    code: ErrorCode,
    message: impl Into<String>,
) {
    let failure = Envelope::response_err(id, ErrorBody::new(code, message));
    let _ = outbound.send(Outbound::Envelope(Box::new(failure)));
}
