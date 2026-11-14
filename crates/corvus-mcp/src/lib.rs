//! Model Context Protocol server for Corvus.
//!
//! The server is transport agnostic: [protocol::McpServer] turns JSON-RPC
//! messages into answers, and both transports in this crate (stdio and
//! streamable HTTP) are thin adapters around it. Tool failures are reported
//! inside a normal tools/call result with isError set; a transport failure is
//! reserved for a broken connection, never for a rejected tool call.

pub mod backend;
pub mod http;
pub mod metrics;
pub mod protocol;
pub mod stdio;
pub mod tools;

pub use backend::{BackendFuture, ToolBackend, ToolCall};
pub use http::McpHttpServer;
pub use metrics::{CallGuard, Metrics};
pub use protocol::{McpServer, PROTOCOL_VERSION, SUPPORTED_VERSIONS};
pub use stdio::serve_stdio;
