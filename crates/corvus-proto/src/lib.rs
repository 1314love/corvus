//! Wire types shared by the daemon and the extension bridge.

use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Hello,
    HelloAck,
    Command,
    Response,
    Event,
    Ping,
    Pong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    BadArgs,
    NotFound,
    NotInteractable,
    Timeout,
    NavigationLost,
    CapabilityUnavailable,
    BrowserGone,
    TransportDown,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meta {
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
    #[serde(default)]
    pub trace_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u16,
    pub id: String,
    pub kind: Kind,
    #[serde(default)]
    pub browser: Option<String>,
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub args: serde_json::Value,
    #[serde(default)]
    pub meta: Option<Meta>,
}

impl Envelope {
    pub fn command(
        id: impl Into<String>,
        tool: impl Into<String>,
        args: serde_json::Value,
    ) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            id: id.into(),
            kind: Kind::Command,
            browser: None,
            session: None,
            tool: Some(tool.into()),
            args,
            meta: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_roundtrip() {
        let env = Envelope::command(
            "01",
            "browser_click",
            serde_json::json!({ "selector": "@e1" }),
        );
        let text = serde_json::to_string(&env).unwrap();
        let back: Envelope = serde_json::from_str(&text).unwrap();
        assert_eq!(back.kind, Kind::Command);
        assert_eq!(back.tool.as_deref(), Some("browser_click"));
    }
}
