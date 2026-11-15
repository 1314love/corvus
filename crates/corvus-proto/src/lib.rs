//! Wire types shared by the daemon, the MCP server and the browser extension.
#![warn(missing_docs)]
//!
//! Everything that crosses a transport is one [Envelope]. The protocol is
//! versioned: every envelope carries the version it was built with, the
//! handshake announces both sides, and a receiver refuses a version it does
//! not speak instead of guessing.

pub mod envelope;
pub mod error;
pub mod handshake;
pub mod ids;

pub use envelope::{Envelope, Kind, Meta};
pub use error::{ErrorBody, ErrorCode};
pub use handshake::{capability, CdpDescriptor, Hello, HelloAck, Limits};

/// Version of the wire protocol implemented by this build.
pub const PROTOCOL_VERSION: u16 = 1;

/// Name the daemon reports in the handshake.
pub const DAEMON_NAME: &str = "corvus";
