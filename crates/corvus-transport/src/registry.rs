//! Registry of connected browsers, keyed by browser identifier.
//!
//! A browser reconnecting replaces its previous entry. Removal is guarded by
//! the connection identifier so that a stale connection cannot evict the
//! connection that replaced it.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::sync::mpsc;

use corvus_proto::Envelope;

use crate::error::SendError;

/// Control frames the writer task understands.
#[derive(Debug)]
pub enum Outbound {
    /// Serialize and send this envelope.
    Envelope(Box<Envelope>),
    /// Close the WebSocket politely.
    Close,
}

/// Milliseconds since the Unix epoch.
pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// Public description of one connected browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BrowserInfo {
    /// Browser identifier, for example "edge:default".
    pub id: String,
    /// Profile the extension reported.
    pub profile: String,
    /// Extension version.
    pub version: String,
    /// Optional extension build label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension: Option<String>,
    /// Capabilities kept after negotiation.
    pub capabilities: Vec<String>,
    /// When the connection was accepted, in milliseconds since the epoch.
    pub connected_at_ms: u64,
    /// Remote socket address.
    pub remote: String,
}

/// One live WebSocket session with an extension.
#[derive(Debug)]
pub struct BrowserConnection {
    info: BrowserInfo,
    connection: String,
    outbound: mpsc::UnboundedSender<Outbound>,
}

impl BrowserConnection {
    /// Build a connection around an outbound channel.
    pub fn new(
        info: BrowserInfo,
        connection: impl Into<String>,
        outbound: mpsc::UnboundedSender<Outbound>,
    ) -> Self {
        Self {
            info,
            connection: connection.into(),
            outbound,
        }
    }

    /// Browser identifier.
    pub fn id(&self) -> &str {
        &self.info.id
    }

    /// Identifier of this particular socket session.
    pub fn connection(&self) -> &str {
        &self.connection
    }

    /// Public description of the browser.
    pub fn info(&self) -> &BrowserInfo {
        &self.info
    }

    /// Capabilities kept after negotiation.
    pub fn capabilities(&self) -> &[String] {
        &self.info.capabilities
    }

    /// Whether negotiation kept the named capability.
    pub fn supports(&self, capability: &str) -> bool {
        self.info.capabilities.iter().any(|name| name == capability)
    }

    /// Whether the writer task is still able to accept messages.
    pub fn is_alive(&self) -> bool {
        !self.outbound.is_closed()
    }

    /// Queue an envelope for delivery.
    pub fn send(&self, envelope: Envelope) -> Result<(), SendError> {
        self.outbound
            .send(Outbound::Envelope(Box::new(envelope)))
            .map_err(|_| SendError::disconnected(self.info.id.clone()))
    }

    /// Ask the transport to close this session.
    pub fn close(&self) {
        let _ = self.outbound.send(Outbound::Close);
    }
}

/// All browsers the daemon currently holds a connection for.
#[derive(Debug, Default)]
pub struct Registry {
    entries: RwLock<HashMap<String, Arc<BrowserConnection>>>,
}

impl Registry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a connection, returning the entry it replaced, if any.
    pub fn register(&self, connection: Arc<BrowserConnection>) -> Option<Arc<BrowserConnection>> {
        let mut entries = self
            .entries
            .write()
            .unwrap_or_else(|error| error.into_inner());
        entries.insert(connection.id().to_string(), connection)
    }

    /// Remove a connection if it is still the one that is registered.
    pub fn remove(&self, browser: &str, connection: &str) -> bool {
        let mut entries = self
            .entries
            .write()
            .unwrap_or_else(|error| error.into_inner());
        let matches = entries
            .get(browser)
            .map(|entry| entry.connection() == connection)
            .unwrap_or(false);
        if matches {
            entries.remove(browser);
        }
        matches
    }

    /// Look up a browser.
    pub fn get(&self, browser: &str) -> Option<Arc<BrowserConnection>> {
        let entries = self
            .entries
            .read()
            .unwrap_or_else(|error| error.into_inner());
        entries.get(browser).cloned()
    }

    /// Every connected browser, sorted by identifier.
    pub fn list(&self) -> Vec<BrowserInfo> {
        let entries = self
            .entries
            .read()
            .unwrap_or_else(|error| error.into_inner());
        let mut infos: Vec<BrowserInfo> =
            entries.values().map(|entry| entry.info().clone()).collect();
        infos.sort_by(|left, right| left.id.cmp(&right.id));
        infos
    }

    /// Number of connected browsers.
    pub fn len(&self) -> usize {
        let entries = self
            .entries
            .read()
            .unwrap_or_else(|error| error.into_inner());
        entries.len()
    }

    /// Whether no browser is connected.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection(
        browser: &str,
        session: &str,
    ) -> (Arc<BrowserConnection>, mpsc::UnboundedReceiver<Outbound>) {
        let (outbound, receiver) = mpsc::unbounded_channel();
        let info = BrowserInfo {
            id: browser.to_string(),
            profile: "default".to_string(),
            version: "0.1.0".to_string(),
            extension: None,
            capabilities: vec!["dom".to_string()],
            connected_at_ms: now_millis(),
            remote: "127.0.0.1:1234".to_string(),
        };
        (
            Arc::new(BrowserConnection::new(info, session, outbound)),
            receiver,
        )
    }

    #[test]
    fn register_reports_replaced_connection() {
        let registry = Registry::new();
        let (first, _first_receiver) = connection("edge:default", "conn-1");
        assert!(registry.register(first).is_none());
        let (second, _second_receiver) = connection("edge:default", "conn-2");
        let replaced = registry.register(second);
        assert_eq!(replaced.expect("replaced").connection(), "conn-1");
        assert_eq!(registry.len(), 1);
        assert_eq!(
            registry.get("edge:default").expect("entry").connection(),
            "conn-2"
        );
    }

    #[test]
    fn stale_connection_cannot_remove_its_successor() {
        let registry = Registry::new();
        let (first, _first_receiver) = connection("edge:default", "conn-1");
        registry.register(first);
        let (second, _second_receiver) = connection("edge:default", "conn-2");
        registry.register(second);
        assert!(!registry.remove("edge:default", "conn-1"));
        assert!(registry.remove("edge:default", "conn-2"));
        assert!(registry.is_empty());
    }

    #[test]
    fn capabilities_and_liveness_are_reported() {
        let (conn, _receiver) = connection("edge:default", "conn-1");
        assert!(conn.supports("dom"));
        assert!(!conn.supports("debugger"));
        assert!(conn.is_alive());
        conn.close();
    }
}
