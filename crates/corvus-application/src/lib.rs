//! Corvus application layer: use cases, ports and transport-neutral DTOs.
//!
//! The layer owns orchestration and depends only on the domain, the wire
//! vocabulary and abstract ports. Concrete browsers, databases and HTTP
//! frameworks live in adapter crates.

#![warn(missing_docs)]

pub mod backend;
pub mod dto;
pub mod ports;
pub mod router;
pub mod spec;

pub use backend::{BackendFuture, ToolBackend, ToolCall};
pub use corvus_domain::recording::{now_millis, SessionContext, StepRecord};
pub use router::{
    CommandCancel, CommandRequest, CommandResult, DispatchFailure, Router, RouterConfig,
};
pub use spec::{lookup as lookup_tool, ToolSpec, TOOLS};
