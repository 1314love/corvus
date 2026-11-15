//! Corvus Axum HTTP API, OpenAPI document and optional local auth.

#![warn(missing_docs)]

pub mod auth;
pub mod routes;
pub mod state;

pub use auth::{AuthConfig, AuthMode, AuthService};
pub use routes::router;
pub use state::AppState;

/// Presentation-layer alias for the shared API state.
pub use state::AppState as ApiState;
