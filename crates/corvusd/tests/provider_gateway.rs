//! Integration tests for the encrypted provider gateway and its in-memory registry.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

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

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

const PROVIDER_ID: &str = "provider-11111111-1111-4111-8111-111111111111";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Frame {
    seq: u64,
    ciphertext: String,
}

struct TestDaemon {
    daemon: Daemon,
    home: PathBuf,
    token: PairingToken,
}

async fn start_daemon() -> TestDaemon {
    let home = tempfile::tempdir().expect("temp home").keep();
    let daemon = Daemon::start(DaemonConfig {
        bind: "127.0.0.1".parse().expect("loopback"),
        ws_port: 0,
        http_port: 0,
        db_path: Some(home.join("corvus.sqlite")),
        home: Some(home.clone()),
        channel: ChannelMode::Extension,
        no_cdp: true,
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

async fn connect(daemon: &Daemon) -> Socket {
    let url = format!("ws://{}/provider/v2", daemon.http_addr());
    let (socket, response) = connect_async(url).await.expect("provider websocket");
    assert_eq!(response.status(), 101);
    socket
}

async fn send_json<T: Serialize>(socket: &mut Socket, value: &T) {
    let text = serde_json::to_string(value).expect("serialize frame");
    socket
        .send(Message::Text(text.into()))
        .await
        .expect("send frame");
}

async fn recv_json<T: DeserializeOwned>(socket: &mut Socket) -> T {
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

async fn expect_close(socket: &mut Socket) -> u16 {
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

struct HelloBundle {
    hello: ClientHello,
    shared: Vec<u8>,
    transcript: [u8; 32],
    salt: [u8; 32],
}

fn build_hello(server: &ServerHello, token: &[u8; 32]) -> HelloBundle {
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

struct PartialClient {
    session: ProviderSession,
    transcript: [u8; 32],
    client_confirm_key: [u8; 32],
}

fn manifest() -> Manifest {
    Manifest {
        provider_id: PROVIDER_ID.into(),
        version: "2.0.0".into(),
        browser: BrowserInfo {
            family: "edge".into(),
            version: "157.0.0.0".into(),
            profile: "default".into(),
        },
        capabilities: vec![
            Capability {
                id: "system.info".into(),
                version: 1,
                actions: vec!["info".into()],
            },
            Capability {
                id: "tabs.read".into(),
                version: 1,
                actions: vec!["list".into()],
            },
        ],
        subscriptions: vec!["tabs.updated".into()],
        limits: Limits {
            max_invoke_bytes: 1_048_576,
            max_result_bytes: 8_388_608,
        },
    }
}

async fn prepare(socket: &mut Socket, token: &PairingToken) -> PartialClient {
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

async fn send_encrypted(
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

async fn recv_text(socket: &mut Socket) -> String {
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

async fn recv_encrypted(socket: &mut Socket, session: &mut ProviderSession) -> ProviderMessage {
    let text = recv_text(socket).await;
    session.receive_message(text.as_bytes()).expect("decrypt")
}

async fn finish_handshake(
    socket: &mut Socket,
    mut partial: PartialClient,
    manifest: Manifest,
) -> ProviderSession {
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
            provider_id,
            accepted,
        } => {
            assert!(accepted);
            assert_eq!(provider_id, PROVIDER_ID);
        }
        other => panic!("expected manifest_ack, got {other:?}"),
    }
    partial.session
}

async fn establish(socket: &mut Socket, token: &PairingToken) -> ProviderSession {
    let partial = prepare(socket, token).await;
    finish_handshake(socket, partial, manifest()).await
}

async fn invoke_and_answer(
    socket: &mut Socket,
    session: &mut ProviderSession,
    gateway: &Arc<ProviderGateway>,
    action: &str,
    value: Value,
) -> Value {
    let task = tokio::spawn({
        let gateway = Arc::clone(gateway);
        let provider_id = PROVIDER_ID.to_string();
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn provider_handshake_and_invokes_round_trip() {
    let test = start_daemon().await;
    let mut socket = connect(&test.daemon).await;
    assert!(test.daemon.provider_gateway().snapshot().await.is_empty());
    let mut session = establish(&mut socket, &test.token).await;
    let snapshot = test.daemon.provider_gateway().snapshot().await;
    assert_eq!(snapshot.len(), 1);
    assert_eq!(snapshot[0].provider_id, PROVIDER_ID);
    assert_eq!(snapshot[0].manifest.version, "2.0.0");
    assert_eq!(snapshot[0].browser.family, "edge");

    let value = invoke_and_answer(
        &mut socket,
        &mut session,
        test.daemon.provider_gateway(),
        "system.info.info",
        json!({
            "browser_family": "edge",
            "browser_version": "157.0.0.0",
            "extension_version": "2.0.0",
            "capabilities": ["system.info", "tabs.read"]
        }),
    )
    .await;
    assert_eq!(value["browser_family"], "edge");

    let value = invoke_and_answer(
        &mut socket,
        &mut session,
        test.daemon.provider_gateway(),
        "tabs.read.list",
        json!([{
            "tab_id": "1",
            "window_id": 1,
            "title": "Example",
            "url": "https://example.test/",
            "active": true
        }]),
    )
    .await;
    assert_eq!(value.as_array().expect("tab array").len(), 1);

    drop(socket);
    test.daemon.shutdown().await;
    let _ = test.home;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wrong_token_is_rejected_before_a_manifest() {
    let test = start_daemon().await;
    let mut socket = connect(&test.daemon).await;
    let server: ServerHello = recv_json(&mut socket).await;
    let wrong = [9_u8; 32];
    let bundle = build_hello(&server, &wrong);
    send_json(&mut socket, &bundle.hello).await;
    assert_eq!(expect_close(&mut socket).await, 4401);
    test.daemon.shutdown().await;
    let _ = test.home;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tampered_manifest_ciphertext_is_rejected() {
    let test = start_daemon().await;
    let mut socket = connect(&test.daemon).await;
    let mut partial = prepare(&mut socket, &test.token).await;
    let proof = base64_encode(&hmac_sha256(
        &partial.client_confirm_key,
        &partial.transcript,
    ));
    send_encrypted(
        &mut socket,
        &mut partial.session,
        &ProviderMessage::ClientConfirm { proof },
    )
    .await;
    let raw = partial
        .session
        .send_message(&ProviderMessage::Manifest(manifest()))
        .expect("encrypt manifest");
    let mut frame: Frame = serde_json::from_slice(&raw).expect("manifest frame");
    assert_eq!(frame.seq, 1);
    let mut ciphertext = base64_decode(&frame.ciphertext).expect("ciphertext");
    let last = ciphertext.len() - 1;
    ciphertext[last] ^= 0x01;
    frame.ciphertext = base64_encode(&ciphertext);
    send_json(&mut socket, &frame).await;
    assert_eq!(expect_close(&mut socket).await, 4401);
    test.daemon.shutdown().await;
    let _ = test.home;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replayed_client_confirm_is_rejected() {
    let test = start_daemon().await;
    let mut socket = connect(&test.daemon).await;
    let mut partial = prepare(&mut socket, &test.token).await;
    let proof = base64_encode(&hmac_sha256(
        &partial.client_confirm_key,
        &partial.transcript,
    ));
    let raw = partial
        .session
        .send_message(&ProviderMessage::ClientConfirm { proof })
        .expect("encrypt confirm");
    let frame: Frame = serde_json::from_slice(&raw).expect("confirm frame");
    assert_eq!(frame.seq, 0);
    send_json(&mut socket, &frame).await;
    send_json(&mut socket, &frame).await;
    assert_eq!(expect_close(&mut socket).await, 4402);
    test.daemon.shutdown().await;
    let _ = test.home;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sequence_gap_is_rejected() {
    let test = start_daemon().await;
    let mut socket = connect(&test.daemon).await;
    let mut partial = prepare(&mut socket, &test.token).await;
    let proof = base64_encode(&hmac_sha256(
        &partial.client_confirm_key,
        &partial.transcript,
    ));
    send_encrypted(
        &mut socket,
        &mut partial.session,
        &ProviderMessage::ClientConfirm { proof },
    )
    .await;
    let raw = partial
        .session
        .send_message(&ProviderMessage::Manifest(manifest()))
        .expect("encrypt manifest");
    let mut frame: Frame = serde_json::from_slice(&raw).expect("manifest frame");
    assert_eq!(frame.seq, 1);
    frame.seq = 2;
    send_json(&mut socket, &frame).await;
    assert_eq!(expect_close(&mut socket).await, 4402);
    test.daemon.shutdown().await;
    let _ = test.home;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duplicate_provider_replaces_the_old_connection() {
    let test = start_daemon().await;
    let mut first = connect(&test.daemon).await;
    establish(&mut first, &test.token).await;

    let mut second = connect(&test.daemon).await;
    let mut session = establish(&mut second, &test.token).await;
    assert_eq!(test.daemon.provider_gateway().snapshot().await.len(), 1);
    let closed = tokio::time::timeout(Duration::from_secs(5), expect_close(&mut first))
        .await
        .expect("old connection closes");
    assert_eq!(closed, 1012);

    invoke_and_answer(
        &mut second,
        &mut session,
        test.daemon.provider_gateway(),
        "system.info.info",
        json!({
            "browser_family": "edge",
            "browser_version": "157.0.0.0",
            "extension_version": "2.0.0",
            "capabilities": ["system.info"]
        }),
    )
    .await;
    test.daemon.shutdown().await;
    let _ = test.home;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn handshake_timeout_is_reported_with_4408() {
    let test = start_daemon().await;
    let mut socket = connect(&test.daemon).await;
    let _server: ServerHello = recv_json(&mut socket).await;
    let started = Instant::now();
    let closed = tokio::time::timeout(Duration::from_secs(9), expect_close(&mut socket))
        .await
        .expect("handshake timeout close");
    assert_eq!(closed, 4408);
    let elapsed = started.elapsed();
    assert!(elapsed >= Duration::from_secs(4), "elapsed: {elapsed:?}");
    assert!(elapsed <= Duration::from_secs(9), "elapsed: {elapsed:?}");
    test.daemon.shutdown().await;
    let _ = test.home;
}
