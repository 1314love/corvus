//! Handshake payloads and capability negotiation.

use serde::{Deserialize, Serialize};

/// Capability names understood in protocol version 1.
pub mod capability {
    /// The extension can read and mutate the DOM of the active tab.
    pub const DOM: &str = "dom";
    /// The extension can run scripted actions through the scripting API.
    pub const SCRIPTING: &str = "scripting";
    /// The debugger channel is attached.
    pub const DEBUGGER: &str = "debugger";
    /// Network observation is available.
    pub const NETWORK: &str = "network";
    /// Downloads can be awaited.
    pub const DOWNLOADS: &str = "downloads";

    /// Every capability this daemon version understands.
    pub const SUPPORTED: [&str; 5] = [DOM, SCRIPTING, DEBUGGER, NETWORK, DOWNLOADS];

    /// Keep only the capabilities this daemon understands, in daemon order.
    pub fn negotiate(offered: &[String]) -> Vec<String> {
        SUPPORTED
            .iter()
            .filter(|name| offered.iter().any(|candidate| candidate == *name))
            .map(|name| (*name).to_string())
            .collect()
    }
}

/// What an extension sends in the hello envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    /// Browser profile name, for example "default".
    pub profile: String,
    /// Extension version.
    pub version: String,
    /// Capabilities the extension offers.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Optional extension build label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension: Option<String>,
}

impl Hello {
    /// Build a handshake payload.
    pub fn new(profile: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            profile: profile.into(),
            version: version.into(),
            capabilities: Vec::new(),
            extension: None,
        }
    }

    /// Advertise a capability.
    pub fn with_capability(mut self, name: impl Into<String>) -> Self {
        self.capabilities.push(name.into());
        self
    }

    /// The capabilities both sides speak.
    pub fn negotiated(&self) -> Vec<String> {
        capability::negotiate(&self.capabilities)
    }
}

/// What the daemon answers in the hello_ack envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloAck {
    /// Daemon name.
    pub daemon: String,
    /// Daemon version.
    pub version: String,
    /// Wire protocol version the daemon speaks.
    pub protocol: u16,
    /// Capabilities kept after negotiation.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Limits the extension must respect.
    pub limits: Limits,
}

/// Limits the daemon announces during the handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    /// Longest deadline accepted for one command.
    pub max_timeout_ms: u64,
    /// Deadline used when the caller does not pass one.
    pub default_timeout_ms: u64,
    /// Commands the daemon may keep in flight towards one browser.
    pub max_inflight: u32,
    /// Nodes a snapshot may return before it is truncated.
    pub max_snapshot_nodes: u32,
}

impl Limits {
    /// Protocol version 1 defaults.
    pub fn v1() -> Self {
        Self {
            max_timeout_ms: 125_000,
            default_timeout_ms: 60_000,
            max_inflight: 64,
            max_snapshot_nodes: 20_000,
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self::v1()
    }
}

#[cfg(test)]
mod tests {
    use super::Limits;

    #[test]
    fn limits_v1_matches_the_command_timeout_policy() {
        let limits = Limits::v1();
        assert_eq!(limits.max_timeout_ms, 125_000);
        assert_eq!(limits.default_timeout_ms, 60_000);
        assert_eq!(limits.max_inflight, 64);
        assert_eq!(limits.max_snapshot_nodes, 20_000);
    }
}
