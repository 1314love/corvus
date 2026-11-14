//! The wire envelope: one message shape for every transport.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{ErrorBody, ErrorCode};
use crate::handshake::{Hello, HelloAck};
use crate::ids;
use crate::PROTOCOL_VERSION;

/// What an envelope is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Daemon to extension: run this tool.
    Command,
    /// Extension to daemon: the outcome of a command.
    Response,
    /// Extension to daemon: something happened, unsolicited.
    Event,
    /// Extension to daemon: first message on a connection.
    Hello,
    /// Daemon to extension: handshake accepted.
    HelloAck,
    /// Liveness probe.
    Ping,
    /// Liveness answer.
    Pong,
}

impl Kind {
    /// The wire spelling of this kind.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Command => "command",
            Kind::Response => "response",
            Kind::Event => "event",
            Kind::Hello => "hello",
            Kind::HelloAck => "hello_ack",
            Kind::Ping => "ping",
            Kind::Pong => "pong",
        }
    }
}

/// Per envelope metadata. Every field is optional so that old extensions keep
/// working when the daemon starts sending more of them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meta {
    /// Deadline the receiver should honour, in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// Key that makes a retried command execute at most once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    /// Identifier that ties one agent request to every hop it causes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
}

impl Meta {
    /// Meta with nothing set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the deadline.
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }

    /// Set the idempotency key.
    pub fn with_idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }

    /// Set the trace identifier.
    pub fn with_trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
        self
    }
}

/// One protocol message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    /// Wire protocol version.
    pub v: u16,
    /// Message identifier, assigned by the daemon for commands and echoed back.
    pub id: String,
    /// What this message is for.
    pub kind: Kind,
    /// Browser the message belongs to, for example "edge:default".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser: Option<String>,
    /// Recording session the message belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// Tool name for commands and responses, event name for events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// Tool arguments for a command, tool result for a response, payload for
    /// an event, and the handshake payload for hello and hello_ack.
    #[serde(default)]
    pub args: Value,
    /// Metadata.
    #[serde(default)]
    pub meta: Meta,
    /// Typed failure; absent on success, and never a bare string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
}

impl Envelope {
    /// An envelope with the current protocol version and no payload.
    pub fn new(id: impl Into<String>, kind: Kind) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            id: id.into(),
            kind,
            browser: None,
            session: None,
            tool: None,
            args: Value::Null,
            meta: Meta::default(),
            error: None,
        }
    }

    /// A command for the extension to run.
    pub fn command(id: impl Into<String>, tool: impl Into<String>, args: Value) -> Self {
        let mut envelope = Self::new(id, Kind::Command);
        envelope.tool = Some(tool.into());
        envelope.args = args;
        envelope
    }

    /// A successful response carrying a result.
    pub fn response_ok(id: impl Into<String>, result: Value) -> Self {
        let mut envelope = Self::new(id, Kind::Response);
        envelope.args = result;
        envelope
    }

    /// A failed response carrying a typed error.
    pub fn response_err(id: impl Into<String>, error: ErrorBody) -> Self {
        let mut envelope = Self::new(id, Kind::Response);
        envelope.error = Some(error);
        envelope
    }

    /// An unsolicited event.
    pub fn event(id: impl Into<String>, name: impl Into<String>, payload: Value) -> Self {
        let mut envelope = Self::new(id, Kind::Event);
        envelope.tool = Some(name.into());
        envelope.args = payload;
        envelope
    }

    /// A handshake request from an extension.
    pub fn hello(id: impl Into<String>, browser: impl Into<String>, payload: &Hello) -> Self {
        let mut envelope = Self::new(id, Kind::Hello);
        envelope.browser = Some(browser.into());
        envelope.args = serde_json::to_value(payload).unwrap_or(Value::Null);
        envelope
    }

    /// A handshake answer from the daemon.
    pub fn hello_ack(id: impl Into<String>, payload: HelloAck) -> Self {
        let mut envelope = Self::new(id, Kind::HelloAck);
        envelope.args = serde_json::to_value(payload).unwrap_or(Value::Null);
        envelope
    }

    /// A liveness probe.
    pub fn ping(id: impl Into<String>) -> Self {
        Self::new(id, Kind::Ping)
    }

    /// A liveness answer.
    pub fn pong(id: impl Into<String>) -> Self {
        Self::new(id, Kind::Pong)
    }

    /// Attach the browser identifier.
    pub fn with_browser(mut self, browser: impl Into<String>) -> Self {
        self.browser = Some(browser.into());
        self
    }

    /// Attach the session identifier.
    pub fn with_session(mut self, session: impl Into<String>) -> Self {
        self.session = Some(session.into());
        self
    }

    /// Replace the metadata.
    pub fn with_meta(mut self, meta: Meta) -> Self {
        self.meta = meta;
        self
    }

    /// Record the trace identifier.
    pub fn with_trace(mut self, trace_id: impl Into<String>) -> Self {
        self.meta.trace_id = Some(trace_id.into());
        self
    }

    /// Record the deadline in metadata.
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.meta.timeout_ms = Some(timeout_ms);
        self
    }

    /// The error code carried by this envelope, if any.
    pub fn error_code(&self) -> Option<ErrorCode> {
        self.error.as_ref().map(|error| error.code)
    }

    /// Whether this envelope answers a command.
    pub fn is_response(&self) -> bool {
        self.kind == Kind::Response
    }

    /// Check the fields that every peer must agree on before acting.
    ///
    /// This is deliberately strict about the version and the identifier and
    /// deliberately lax about new fields, so that a newer peer can still talk
    /// to an older one.
    pub fn validate(&self) -> Result<(), ErrorBody> {
        if self.v != PROTOCOL_VERSION {
            return Err(ErrorBody::new(
                ErrorCode::BadArgs,
                format!(
                    "unsupported protocol version {} (this build speaks {})",
                    self.v, PROTOCOL_VERSION
                ),
            ));
        }
        if self.id.trim().is_empty() {
            return Err(ErrorBody::new(ErrorCode::BadArgs, "missing envelope id"));
        }
        match self.kind {
            Kind::Command | Kind::Event => {
                if self.tool.as_deref().unwrap_or_default().trim().is_empty() {
                    return Err(ErrorBody::new(
                        ErrorCode::BadArgs,
                        format!("{} envelope without a tool name", self.kind.as_str()),
                    ));
                }
            }
            Kind::Hello => {
                if self
                    .browser
                    .as_deref()
                    .unwrap_or_default()
                    .trim()
                    .is_empty()
                {
                    return Err(ErrorBody::new(
                        ErrorCode::BadArgs,
                        "hello envelope without a browser identifier",
                    ));
                }
            }
            Kind::Response | Kind::HelloAck | Kind::Ping | Kind::Pong => {}
        }
        Ok(())
    }
}

/// Build a fresh identifier for a command envelope.
pub fn new_command_id() -> String {
    ids::command_id()
}
