//! Public snapshots returned by the CDP client.

use serde::Serialize;

/// Connection state of the browser-level CDP endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CdpStatus {
    /// Whether the WebSocket is currently usable.
    pub connected: bool,
    /// Endpoint the client is using or reconnecting to.
    pub endpoint: Option<String>,
    /// Browser name reported by Browser.getVersion.
    pub browser: Option<String>,
    /// Protocol version reported by Browser.getVersion.
    pub protocol_version: Option<String>,
    /// Number of page targets known to the client.
    pub targets: usize,
    /// Most recent transport or protocol error.
    pub last_error: Option<String>,
    /// Epoch milliseconds when the current connection was established.
    pub connected_since_ms: Option<u64>,
    /// Monotonic reconnect counter.
    pub reconnects: u64,
}

/// One page target known through Target.setDiscoverTargets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CdpTarget {
    /// CDP target identifier.
    pub target_id: String,
    /// Target type, usually "page".
    pub target_type: String,
    /// Document title.
    pub title: String,
    /// Document URL.
    pub url: String,
    /// Whether the client holds a flattened session for this target.
    pub attached: bool,
    /// Flattened session identifier when attached.
    pub session_id: Option<String>,
    /// Most recent reported target status.
    pub status: Option<String>,
}

/// One protocol event captured in the bounded ring.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CdpEvent {
    /// Monotonic sequence number assigned by the client.
    pub seq: u64,
    /// Epoch milliseconds when the event was received.
    pub ts: u64,
    /// CDP method name.
    pub method: String,
    /// Flattened session the event belongs to.
    pub session_id: Option<String>,
    /// Raw event payload.
    pub params: serde_json::Value,
}

/// Result of a bounded event read.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CdpEvents {
    /// Events in sequence order.
    pub events: Vec<CdpEvent>,
    /// Sequence to pass as since on the next read.
    pub next_seq: u64,
    /// Events evicted before they could be read.
    pub dropped: u64,
}
