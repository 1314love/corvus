//! Corvus v2 encrypted provider channel.
//!
//! This crate owns the server side of the authenticated P-256/AES-256-GCM
//! channel.  The legacy v1.7 plaintext transport is deliberately not part of
//! this crate.  No provider or channel wording is exposed through MCP here;
//! the daemon decides when a connection may be used.

#![warn(missing_docs)]

pub mod crypto;
pub mod error;
pub mod handshake;
pub mod message;
pub mod session;
pub mod token;

pub use error::ProviderError;
pub use handshake::{
    ClientHello, EstablishedProvider, PendingProvider, ServerHandshake, ServerHello,
};
pub use message::{
    BrowserInfo, Capability, Limits, Manifest, ManifestAck, ProviderErrorBody, ProviderMessage,
    ProviderResult,
};
pub use session::{Direction, FramedSession, ProviderSession};
pub use token::{load_or_create_pairing_token, PairingToken};

/// Wire protocol version for the encrypted provider channel.
pub const PROVIDER_PROTOCOL_VERSION: u16 = 2;

/// WebSocket path served by the daemon for encrypted provider connections.
pub const PROVIDER_PATH: &str = "/provider/v2";

/// Maximum plaintext application frame accepted before encryption.
pub const MAX_PLAINTEXT_BYTES: usize = 16 * 1024 * 1024;

/// Maximum outer encrypted frame accepted before base64 decoding.
pub const MAX_FRAME_BYTES: usize = 24 * 1024 * 1024;

/// Maximum plaintext handshake JSON accepted from a client.
pub const MAX_HANDSHAKE_BYTES: usize = 4096;
