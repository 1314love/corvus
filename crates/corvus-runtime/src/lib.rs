//! Command routing between the daemon and connected browsers.
//!
//! The router owns everything that must be true exactly once per command:
//! identifier assignment, per browser serialization, the deadline, the
//! cancellation event and the idempotency table. Every failure leaves the
//! router as a typed [corvus_proto::ErrorBody].

pub mod router;
pub mod spec;

pub use corvus_record::SessionContext;
pub use router::{CommandRequest, CommandResult, Router, RouterConfig};
pub use spec::{lookup as lookup_tool, ToolSpec, TOOLS};
