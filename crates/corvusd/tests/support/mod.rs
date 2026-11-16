//! Shared encrypted provider client used by the daemon integration tests.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use corvus_provider::crypto::{
    base64_decode, base64_encode, base64url_encode, derive_keys, hmac_sha256, hmac_verify,
    p256_agree, p256_keypair, random_array, transcript,
};
use corvus_provider::{
    BrowserInfo, Capability, ClientHello, FramedSession, Limits, Manifest, PairingToken,
    ProviderMessage, ProviderSession, ServerHello,
};
use corvusd::channel::ChannelMode;
use corvusd::provider_gateway::ProviderGateway;
use corvusd::{Daemon, DaemonConfig};
use futures::{SinkExt, StreamExt};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

pub type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub const PROVIDER_ID: &str = "provider-11111111-1111-4111-8111-111111111111";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    pub seq: u64,
    pub ciphertext: String,
}

pub struct TestDaemon {
    pub daemon: Daemon,
    pub home: PathBuf,
    pub token: PairingToken,
}

pub async fn start_daemon() -> TestDaemon {
    start_daemon_with(false).await
}

pub async fn start_daemon_with(v2_surface: bool) -> TestDaemon {
    let home = tempfile::tempdir().expect("temp home").keep();
    let daemon = Daemon::start(DaemonConfig {
        bind: "127.0.0.1".parse().expect("loopback"),
        ws_port: 0,
        http_port: 0,
        db_path: Some(home.join("corvus.sqlite")),
        home: Some(home.clone()),
        channel: ChannelMode::Extension,
        no_cdp: true,
        v2_surface: Some(v2_surface),
        ..DaemonConfig::default()
    })
    .await
    .expect("daemon starts");
    let token = match std::env::var("CORVUS_PAIRING_TOKEN") {
        Ok(raw) => PairingToken::from_base64url(&raw).expect("env token"),
        Err(_) => corvus_provider::token::load_pairing_token(&home).expect("pairing token"),
    };
    TestDaemon {
        daemon,
        home,
        token,
    }
}

pub async fn connect(daemon: &Daemon) -> Socket {
    let url = format!("ws://{}/provider/v2", daemon.http_addr());
    let (socket, response) = connect_async(url).await.expect("provider websocket");
    assert_eq!(response.status(), 101);
    socket
}

pub async fn send_json<T: Serialize>(socket: &mut Socket, value: &T) {
    let text = serde_json::to_string(value).expect("serialize frame");
    socket
        .send(Message::Text(text.into()))
        .await
        .expect("send frame");
}

pub async fn recv_json<T: DeserializeOwned>(socket: &mut Socket) -> T {
    loop {
        match socket
            .next()
            .await
            .expect("frame")
            .expect("websocket frame")
        {
            Message::Text(text) => return serde_json::from_str(&text).expect("json frame"),
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("unexpected websocket message: {other:?}"),
        }
    }
}

pub async fn expect_close(socket: &mut Socket) -> u16 {
    loop {
        match socket
            .next()
            .await
            .expect("close frame")
            .expect("websocket")
        {
            Message::Close(Some(frame)) => {
                let _ = socket.close(None).await;
                return u16::from(frame.code);
            }
            Message::Ping(_) | Message::Pong(_) => continue,
            Message::Close(None) => panic!("close frame had no code"),
            other => panic!("expected close, got {other:?}"),
        }
    }
}

pub struct HelloBundle {
    pub hello: ClientHello,
    pub shared: Vec<u8>,
    pub transcript: [u8; 32],
    pub salt: [u8; 32],
}

pub fn build_hello(server: &ServerHello, token: &[u8; 32]) -> HelloBundle {
    let server_nonce: [u8; 32] = base64_decode(&server.server_nonce)
        .expect("server nonce")
        .try_into()
        .expect("32 bytes");
    let server_ephemeral = base64_decode(&server.server_ephemeral).expect("server ephemeral");
    let salt: [u8; 32] = base64_decode(&server.salt)
        .expect("salt")
        .try_into()
        .expect("32 bytes");
    let client_nonce = random_array::<32>().expect("client nonce");
    let client_id = random_array::<16>().expect("client id");
    let (private, client_public) = p256_keypair().expect("client keypair");
    let (private_ephemeral, client_ephemeral) = p256_keypair().expect("client ephemeral");
    let _ = private;
    let transcript = transcript(
        &server_nonce,
        &client_nonce,
        &server_ephemeral,
        &client_ephemeral,
        &client_id,
        &client_public,
    );
    let proof = hmac_sha256(token, &transcript);
    let shared = p256_agree(private_ephemeral, &server_ephemeral).expect("shared secret");
    HelloBundle {
        hello: ClientHello {
            kind: "client_hello".into(),
            protocol: 2,
            client_id: base64url_encode(&client_id),
            client_public_key: base64_encode(&client_public),
            client_nonce: base64_encode(&client_nonce),
            client_ephemeral: base64_encode(&client_ephemeral),
            pairing_proof: base64_encode(&proof),
        },
        shared,
        transcript,
        salt,
    }
}

pub struct PartialClient {
    pub session: ProviderSession,
    pub transcript: [u8; 32],
    pub client_confirm_key: [u8; 32],
}

pub fn capability(id: &str, actions: &[&str]) -> Capability {
    Capability {
        id: id.into(),
        version: 1,
        actions: actions.iter().map(|action| (*action).into()).collect(),
    }
}

pub fn manifest() -> Manifest {
    manifest_for(
        PROVIDER_ID,
        vec![
            capability("system.info", &["info"]),
            capability("tabs.read", &["list"]),
        ],
    )
}

pub fn full_manifest(provider_id: &str) -> Manifest {
    manifest_for(
        provider_id,
        vec![
            capability("system.info", &["info"]),
            capability("tabs", &["list", "open", "close"]),
            capability("page", &["navigate", "read", "snapshot"]),
            capability("input", &["click", "type"]),
            capability("capture", &["viewport"]),
        ],
    )
}

pub fn manifest_for(provider_id: &str, capabilities: Vec<Capability>) -> Manifest {
    Manifest {
        provider_id: provider_id.into(),
        version: "2.0.0".into(),
        browser: BrowserInfo {
            family: "edge".into(),
            version: "157.0.0.0".into(),
            profile: "default".into(),
        },
        capabilities,
        subscriptions: vec!["tabs.updated".into()],
        limits: Limits {
            max_invoke_bytes: 1_048_576,
            max_result_bytes: 8_388_608,
        },
    }
}

pub async fn prepare(socket: &mut Socket, token: &PairingToken) -> PartialClient {
    let server: ServerHello = recv_json(socket).await;
    assert_eq!(server.kind, "server_hello");
    assert_eq!(server.protocol, 2);
    assert!(server.pairing_required);
    let bundle = build_hello(&server, token.as_bytes());
    send_json(socket, &bundle.hello).await;

    let text = recv_text(socket).await;
    let frame: Frame = serde_json::from_str(&text).expect("server confirm frame");
    assert_eq!(frame.seq, 0);
    let keys = derive_keys(&bundle.shared, &bundle.salt, token.as_bytes());
    let mut session = ProviderSession::new(FramedSession::client(keys.c2s, keys.s2c));
    let message = session
        .receive_message(text.as_bytes())
        .expect("server confirm");
    let ProviderMessage::ServerConfirm { proof, .. } = message else {
        panic!("expected server confirm");
    };
    let proof = base64_decode(&proof).expect("proof bytes");
    assert!(hmac_verify(
        &keys.server_confirm,
        &bundle.transcript,
        &proof
    ));
    PartialClient {
        session,
        transcript: bundle.transcript,
        client_confirm_key: keys.client_confirm,
    }
}

pub async fn send_encrypted(
    socket: &mut Socket,
    session: &mut ProviderSession,
    message: &ProviderMessage,
) {
    let frame = session.send_message(message).expect("encrypt");
    let text = String::from_utf8(frame).expect("utf8 frame");
    socket
        .send(Message::Text(text.into()))
        .await
        .expect("send frame");
}

pub async fn recv_text(socket: &mut Socket) -> String {
    loop {
        match socket
            .next()
            .await
            .expect("frame")
            .expect("websocket frame")
        {
            Message::Text(text) => return text.to_string(),
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("unexpected websocket message: {other:?}"),
        }
    }
}

pub async fn recv_encrypted(socket: &mut Socket, session: &mut ProviderSession) -> ProviderMessage {
    let text = recv_text(socket).await;
    session.receive_message(text.as_bytes()).expect("decrypt")
}

pub async fn finish_handshake(
    socket: &mut Socket,
    mut partial: PartialClient,
    manifest: Manifest,
) -> ProviderSession {
    let provider_id = manifest.provider_id.clone();
    let proof = base64_encode(&hmac_sha256(
        &partial.client_confirm_key,
        &partial.transcript,
    ));
    send_encrypted(
        socket,
        &mut partial.session,
        &ProviderMessage::ClientConfirm { proof },
    )
    .await;
    send_encrypted(
        socket,
        &mut partial.session,
        &ProviderMessage::Manifest(manifest),
    )
    .await;
    let ack = recv_encrypted(socket, &mut partial.session).await;
    match ack {
        ProviderMessage::ManifestAck {
            provider_id: accepted_id,
            accepted,
        } => {
            assert!(accepted);
            assert_eq!(accepted_id, provider_id);
        }
        other => panic!("expected manifest_ack, got {other:?}"),
    }
    partial.session
}

pub async fn establish(socket: &mut Socket, token: &PairingToken) -> ProviderSession {
    let partial = prepare(socket, token).await;
    finish_handshake(socket, partial, manifest()).await
}

pub async fn establish_with(
    socket: &mut Socket,
    token: &PairingToken,
    manifest: Manifest,
) -> ProviderSession {
    let partial = prepare(socket, token).await;
    finish_handshake(socket, partial, manifest).await
}

pub async fn invoke_and_answer(
    socket: &mut Socket,
    session: &mut ProviderSession,
    gateway: &Arc<ProviderGateway>,
    action: &str,
    value: Value,
) -> Value {
    invoke_and_answer_for(socket, session, gateway, PROVIDER_ID, action, value).await
}

pub async fn invoke_and_answer_for(
    socket: &mut Socket,
    session: &mut ProviderSession,
    gateway: &Arc<ProviderGateway>,
    provider_id: &str,
    action: &str,
    value: Value,
) -> Value {
    let task = tokio::spawn({
        let gateway = Arc::clone(gateway);
        let provider_id = provider_id.to_string();
        let action = action.to_string();
        async move {
            gateway
                .invoke(&provider_id, &action, json!({}), Duration::from_secs(5))
                .await
        }
    });
    let invoke = recv_encrypted(socket, session).await;
    let ProviderMessage::Invoke {
        id,
        action: received,
        ..
    } = invoke
    else {
        panic!("expected invoke, got {invoke:?}");
    };
    assert_eq!(received, action);
    send_encrypted(
        socket,
        session,
        &ProviderMessage::Result {
            id,
            ok: true,
            value: Some(value.clone()),
            error: None,
        },
    )
    .await;
    let result = task.await.expect("join invoke").expect("gateway invoke");
    assert!(result.ok);
    assert_eq!(result.value, Some(value));
    result.value.expect("value")
}
