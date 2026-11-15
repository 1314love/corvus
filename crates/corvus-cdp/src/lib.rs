//! Chrome DevTools Protocol client used by the Corvus daemon.
#![warn(missing_docs)]
//!
//! The client owns one browser-level WebSocket, attaches to page targets with
//! flattened sessions, keeps a bounded event ring, and exposes typed tool
//! handlers that mirror the extension transport result shapes.

pub mod client;
pub mod endpoint;
pub mod error;
pub mod tools;
pub mod types;

pub use client::{CdpClient, EVENT_CAPACITY};
pub use endpoint::{
    is_loopback_host, validate_descriptor, validate_http_endpoint, validate_ws_endpoint,
};
pub use error::CdpError;
pub use tools::{is_cdp_tool, CDP_TOOL_NAMES};
pub use types::{CdpEvent, CdpEvents, CdpStatus, CdpTarget};
