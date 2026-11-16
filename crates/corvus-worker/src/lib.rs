//! Corvus CDP worker framing, protocol and supervisor.

#![warn(missing_docs)]

pub mod error;
pub mod frame;
pub mod protocol;
pub mod supervisor;

pub use error::WorkerError;
pub use frame::{read_frame, read_handshake, write_frame, MAX_FRAME_BYTES, MAX_HANDSHAKE_BYTES};
pub use protocol::{WorkerBrowser, WorkerErrorBody, WorkerMessage};
pub use supervisor::{
    discover_worker_program, ProcessLauncher, SupervisorConfig, WorkerIo, WorkerLauncher,
    WorkerStatus, WorkerSupervisor, DEFAULT_BACKOFF, DEFAULT_HANDSHAKE_TIMEOUT,
};
