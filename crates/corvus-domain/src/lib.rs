//! Pure Corvus domain types and policies.
//!
//! This crate contains no I/O and no framework dependencies. Adapters and
//! application services depend on it, never the other way around.

#![warn(missing_docs)]

pub mod error;
pub mod model;
pub mod policy;
pub mod recording;

pub use error::PolicyError;
pub use model::{BrowserId, SessionId, ToolName};
pub use policy::{
    classify_target_id, route_tool, AuthMode, AuthPolicy, BatchPolicy, ChannelMode, Route,
    SnapshotPolicy, TargetIdKind, TimeoutPolicy, ToolFamily, CDP_TAB_HINT, EXTENSION_TAB_HINT,
};
pub use recording::{
    now_millis, SessionContext, SessionRow, StepRecord, StepRow, MAX_PAYLOAD_BYTES, RETENTION_MS,
};
