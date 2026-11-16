//! Typed messages exchanged between the daemon and a compiled CDP worker.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Browser metadata reported by a worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerBrowser {
    /// Browser product/version string.
    pub version: String,
    /// CDP protocol version.
    pub protocol_version: String,
}

/// Error object carried by a worker acknowledgement or result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerErrorBody {
    /// Stable machine code.
    pub code: String,
    /// Human-readable message.
    pub message: String,
    /// Whether a retry could succeed.
    pub retryable: bool,
}

/// Every length-prefixed message in the worker protocol.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkerMessage {
    /// Gateway to worker first frame.
    WorkerHello {
        /// Worker protocol version, always 1.
        protocol: u16,
        /// Gateway-assigned worker identifier.
        worker_id: String,
        /// Browser identifier this worker serves.
        browser_id: String,
        /// Loopback CDP WebSocket endpoint.
        cdp_ws_endpoint: String,
        /// Corvus home directory.
        home_dir: String,
        /// Requested log level.
        log_level: String,
    },
    /// Worker to gateway handshake answer.
    WorkerAck {
        /// Worker protocol version, always 1.
        protocol: u16,
        /// Worker identifier echoed from hello.
        worker_id: String,
        /// Stable provider identifier.
        provider_id: String,
        /// Whether the worker is ready.
        ok: bool,
        /// Browser metadata on success.
        #[serde(skip_serializing_if = "Option::is_none")]
        browser: Option<WorkerBrowser>,
        /// Advertised capabilities.
        capabilities: Vec<String>,
        /// Error body on refusal.
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<WorkerErrorBody>,
    },
    /// One CDP invoke.
    Invoke {
        /// Unique invoke identifier.
        id: String,
        /// Action name.
        action: String,
        /// Action arguments.
        args: Value,
        /// Deadline in milliseconds.
        deadline_ms: u64,
    },
    /// One invoke result.
    Result {
        /// Invoke identifier being answered.
        id: String,
        /// Whether the invoke succeeded.
        ok: bool,
        /// Result value on success.
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<Value>,
        /// Error body on failure.
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<WorkerErrorBody>,
    },
    /// Cancel an in-flight invoke.
    Cancel {
        /// Invoke identifier.
        id: String,
    },
    /// One worker event.
    Event {
        /// Event topic.
        topic: String,
        /// Event sequence number.
        seq: u64,
        /// Event payload.
        payload: Value,
    },
    /// Clean shutdown request.
    Shutdown {},
    /// Protocol-level error.
    Error {
        /// Optional invoke identifier.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Stable machine code.
        code: String,
        /// Human-readable message.
        message: String,
        /// Whether a retry could succeed.
        retryable: bool,
    },
}

impl WorkerMessage {
    /// Stable wire kind.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::WorkerHello { .. } => "worker_hello",
            Self::WorkerAck { .. } => "worker_ack",
            Self::Invoke { .. } => "invoke",
            Self::Result { .. } => "result",
            Self::Cancel { .. } => "cancel",
            Self::Event { .. } => "event",
            Self::Shutdown {} => "shutdown",
            Self::Error { .. } => "error",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn worker_hello_and_ack_round_trip() {
        let hello = WorkerMessage::WorkerHello {
            protocol: 1,
            worker_id: "w-1".into(),
            browser_id: "b-1".into(),
            cdp_ws_endpoint: "ws://127.0.0.1:9476/devtools/browser/x".into(),
            home_dir: "C:/tmp".into(),
            log_level: "info".into(),
        };
        let value = serde_json::to_value(&hello).expect("serialize hello");
        assert_eq!(value["kind"], "worker_hello");
        let decoded: WorkerMessage = serde_json::from_value(value).expect("decode hello");
        assert_eq!(decoded, hello);
        let ack = WorkerMessage::WorkerAck {
            protocol: 1,
            worker_id: "w-1".into(),
            provider_id: "cdp-b-1".into(),
            ok: true,
            browser: Some(WorkerBrowser {
                version: "Corvus/157.0.0.0".into(),
                protocol_version: "1.3".into(),
            }),
            capabilities: vec!["cdp".into()],
            error: None,
        };
        let value = serde_json::to_value(&ack).expect("serialize ack");
        assert_eq!(value["kind"], "worker_ack");
        assert_eq!(value["browser"]["version"], "Corvus/157.0.0.0");
        let decoded: WorkerMessage = serde_json::from_value(value).expect("decode ack");
        assert_eq!(decoded, ack);
    }

    #[test]
    fn invoke_result_and_shutdown_round_trip() {
        let invoke = WorkerMessage::Invoke {
            id: "i-1".into(),
            action: "cdp.get_version".into(),
            args: json!({}),
            deadline_ms: 15000,
        };
        let decoded: WorkerMessage =
            serde_json::from_value(serde_json::to_value(&invoke).unwrap()).unwrap();
        assert_eq!(decoded, invoke);
        let result = WorkerMessage::Result {
            id: "i-1".into(),
            ok: true,
            value: Some(json!({"browser": "Corvus/157.0.0.0"})),
            error: None,
        };
        let decoded: WorkerMessage =
            serde_json::from_value(serde_json::to_value(&result).unwrap()).unwrap();
        assert_eq!(decoded, result);
        let shutdown = WorkerMessage::Shutdown {};
        let value = serde_json::to_value(&shutdown).expect("shutdown");
        assert_eq!(value["kind"], "shutdown");
    }
}
