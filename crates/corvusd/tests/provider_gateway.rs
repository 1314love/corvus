//! Integration tests for the encrypted provider gateway and its in-memory registry.

mod support;

use std::time::{Duration, Instant};

use corvus_provider::crypto::{base64_decode, base64_encode, hmac_sha256};
use corvus_provider::{ProviderMessage, ServerHello};
use serde_json::json;
use support::*;

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
