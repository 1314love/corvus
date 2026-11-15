//! WebSocket transport for the browser extension, plus the registry of
#![warn(missing_docs)]
//! connected browsers.
//!
//! The transport owns exactly two responsibilities: it terminates the
//! WebSocket protocol (handshake, framing, liveness) and it keeps an
//! up-to-date registry of browsers. Routing commands is not its job; every
//! inbound envelope is forwarded to the runtime through a channel.

pub mod application;
pub mod error;
pub mod registry;
pub mod server;

pub use error::SendError;
pub use registry::{now_millis, BrowserConnection, BrowserInfo, Registry};
pub use server::{Inbound, WsServer};
