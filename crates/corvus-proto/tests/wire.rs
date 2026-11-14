//! Wire contract tests: every kind must survive a serde round trip and the
//! spelling must match the architecture document.

use corvus_proto::{
    capability, Envelope, ErrorBody, ErrorCode, Hello, HelloAck, Kind, Limits, Meta,
    PROTOCOL_VERSION,
};
use serde_json::{json, Value};

fn round_trip(envelope: &Envelope) -> Envelope {
    let text = serde_json::to_string(envelope).expect("serialize");
    serde_json::from_str(&text).expect("deserialize")
}

#[test]
fn command_round_trip_keeps_every_field() {
    let meta = Meta::new()
        .with_timeout_ms(5_000)
        .with_idempotency_key("agent-7:step-12")
        .with_trace_id("trace-9");
    let envelope = Envelope::command("cmd-1", "browser_click", json!({ "target": "@e12" }))
        .with_browser("edge:default")
        .with_session("s-1")
        .with_meta(meta);

    let parsed = round_trip(&envelope);
    assert_eq!(parsed, envelope);
    assert_eq!(parsed.kind, Kind::Command);
    assert_eq!(parsed.v, PROTOCOL_VERSION);
    assert_eq!(parsed.tool.as_deref(), Some("browser_click"));
    assert_eq!(parsed.browser.as_deref(), Some("edge:default"));
    assert_eq!(parsed.session.as_deref(), Some("s-1"));
    assert_eq!(parsed.meta.timeout_ms, Some(5_000));
    assert_eq!(
        parsed.meta.idempotency_key.as_deref(),
        Some("agent-7:step-12")
    );
    assert_eq!(parsed.meta.trace_id.as_deref(), Some("trace-9"));
}

#[test]
fn response_success_round_trip() {
    let envelope = Envelope::response_ok("cmd-1", json!({ "clicked": true }));
    let parsed = round_trip(&envelope);
    assert_eq!(parsed, envelope);
    assert!(parsed.is_response());
    assert!(parsed.error.is_none());
    assert_eq!(parsed.args, json!({ "clicked": true }));
}

#[test]
fn response_failure_carries_a_typed_error() {
    let envelope = Envelope::response_err(
        "cmd-2",
        ErrorBody::new(ErrorCode::NotFound, "selector @e99 is not in the page")
            .with_trace("trace-9"),
    );
    let parsed = round_trip(&envelope);
    assert_eq!(parsed.error_code(), Some(ErrorCode::NotFound));
    let body = parsed.error.expect("error body");
    assert_eq!(body.code.as_str(), "NOT_FOUND");
    assert!(body.retryable);
    assert_eq!(body.trace_id.as_deref(), Some("trace-9"));
}

#[test]
fn event_round_trip() {
    let envelope = Envelope::event(
        "evt-1",
        "navigation",
        json!({ "url": "https://example.test" }),
    )
    .with_browser("edge:default");
    let parsed = round_trip(&envelope);
    assert_eq!(parsed, envelope);
    assert_eq!(parsed.kind, Kind::Event);
    assert_eq!(parsed.tool.as_deref(), Some("navigation"));
}

#[test]
fn handshake_round_trip() {
    let hello = Hello::new("default", "0.1.0")
        .with_capability("dom")
        .with_capability("scripting")
        .with_capability("teleport");
    let envelope = Envelope::hello("h-1", "edge:default", &hello);
    let parsed = round_trip(&envelope);
    assert_eq!(parsed, envelope);
    assert_eq!(parsed.kind, Kind::Hello);
    let decoded: Hello = serde_json::from_value(parsed.args).expect("hello payload");
    assert_eq!(decoded.profile, "default");
    assert_eq!(
        decoded.negotiated(),
        vec!["dom".to_string(), capability::SCRIPTING.to_string()]
    );

    let ack = HelloAck {
        daemon: "corvus".to_string(),
        version: "0.1.0".to_string(),
        protocol: PROTOCOL_VERSION,
        capabilities: vec![capability::DOM.to_string()],
        limits: Limits::v1(),
    };
    let envelope = Envelope::hello_ack("h-1", ack).with_browser("edge:default");
    let parsed = round_trip(&envelope);
    assert_eq!(parsed, envelope);
    assert_eq!(parsed.kind, Kind::HelloAck);
    let decoded: HelloAck = serde_json::from_value(parsed.args).expect("hello_ack payload");
    assert_eq!(decoded.limits, Limits::v1());
}

#[test]
fn ping_and_pong_round_trip() {
    let ping = Envelope::ping("p-1");
    let pong = Envelope::pong("p-1");
    assert_eq!(round_trip(&ping), ping);
    assert_eq!(round_trip(&pong), pong);
    assert_eq!(ping.kind.as_str(), "ping");
    assert_eq!(pong.kind.as_str(), "pong");
}

#[test]
fn kind_spellings_are_stable() {
    let spellings = [
        (Kind::Command, "command"),
        (Kind::Response, "response"),
        (Kind::Event, "event"),
        (Kind::Hello, "hello"),
        (Kind::HelloAck, "hello_ack"),
        (Kind::Ping, "ping"),
        (Kind::Pong, "pong"),
    ];
    for (kind, expected) in spellings {
        let encoded = serde_json::to_value(kind).expect("serialize kind");
        assert_eq!(encoded, Value::String(expected.to_string()));
        let decoded: Kind = serde_json::from_value(encoded).expect("deserialize kind");
        assert_eq!(decoded, kind);
    }
}

#[test]
fn every_error_code_round_trips() {
    for code in ErrorCode::ALL {
        let encoded = serde_json::to_value(code).expect("serialize code");
        assert_eq!(encoded, Value::String(code.as_str().to_string()));
        let decoded: ErrorCode = serde_json::from_value(encoded).expect("deserialize code");
        assert_eq!(decoded, code);
        assert_eq!(ErrorCode::parse(code.as_str()), Some(code));
    }
    assert_eq!(ErrorCode::ALL.len(), 10);
    assert!(!ErrorCode::BadArgs.retryable());
    assert!(ErrorCode::Timeout.retryable());
    assert!(ErrorCode::TransportDown.retryable());
    assert!(!ErrorCode::Cancelled.retryable());
}

#[test]
fn cancelled_error_code_parses_and_round_trips() {
    let raw = json!({
        "v": 1,
        "id": "cmd-cancelled",
        "kind": "response",
        "error": {
            "code": "CANCELLED",
            "message": "command cancelled before the browser answered",
            "retryable": false
        }
    });
    let envelope: Envelope = serde_json::from_value(raw).expect("CANCELLED must parse");
    let error = envelope.error.expect("the error body");
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_eq!(error.code.as_str(), "CANCELLED");
    assert!(!error.code.retryable());
    assert!(!error.retryable);

    let encoded = serde_json::to_value(&error).expect("serialize the error body");
    assert_eq!(
        encoded.get("code"),
        Some(&Value::String("CANCELLED".to_string()))
    );
    let decoded: ErrorBody = serde_json::from_value(encoded).expect("deserialize the error body");
    assert_eq!(decoded, error);
}

#[test]
fn unknown_fields_are_ignored_for_forward_compatibility() {
    let raw = json!({
        "v": 1,
        "id": "cmd-1",
        "kind": "command",
        "browser": "edge:default",
        "session": "s-1",
        "tool": "browser_click",
        "args": { "target": "@e12" },
        "meta": { "timeout_ms": 5000, "idempotency_key": "k", "trace_id": "t" },
        "annotation_from_the_future": true
    });
    let parsed: Envelope = serde_json::from_value(raw).expect("tolerant parse");
    assert_eq!(parsed.kind, Kind::Command);
    assert_eq!(parsed.meta.timeout_ms, Some(5_000));
}

#[test]
fn missing_optional_fields_are_accepted() {
    let raw = json!({ "v": 1, "id": "cmd-1", "kind": "ping" });
    let parsed: Envelope = serde_json::from_value(raw).expect("minimal parse");
    assert_eq!(parsed.kind, Kind::Ping);
    assert!(parsed.meta.timeout_ms.is_none());
    assert!(parsed.error.is_none());
}

#[test]
fn validation_rejects_broken_envelopes() {
    let mut wrong_version = Envelope::command("cmd-1", "browser_click", json!({}));
    wrong_version.v = 9;
    assert_eq!(
        wrong_version.validate().expect_err("version").code,
        ErrorCode::BadArgs
    );

    let mut no_id = Envelope::ping("  ");
    no_id.id = "  ".to_string();
    assert_eq!(no_id.validate().expect_err("id").code, ErrorCode::BadArgs);

    let mut no_tool = Envelope::command("cmd-1", "browser_click", json!({}));
    no_tool.tool = None;
    assert_eq!(
        no_tool.validate().expect_err("tool").code,
        ErrorCode::BadArgs
    );

    let mut no_browser = Envelope::hello("h-1", "edge:default", &Hello::new("default", "0.1.0"));
    no_browser.browser = None;
    assert_eq!(
        no_browser.validate().expect_err("browser").code,
        ErrorCode::BadArgs
    );

    let ok = Envelope::command("cmd-1", "browser_click", json!({}));
    assert!(ok.validate().is_ok());
}

#[test]
fn error_body_label_is_stable() {
    let body = ErrorBody::new(ErrorCode::Timeout, "no answer within 5000 ms");
    assert_eq!(body.label(), "TIMEOUT: no answer within 5000 ms");
    assert_eq!(body.to_string(), body.label());
}
