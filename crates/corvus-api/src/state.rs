//! Shared state for the Axum presentation adapter.

use std::sync::Arc;

use axum::http::HeaderMap;
use corvus_application::ports::{AccessClaims, BrowserDirectory, RecordingRepository, ToolCatalog};
use corvus_application::ToolBackend;
use serde_json::Value;

use crate::auth::{AuthError, AuthService};

/// Everything the REST handlers need, wired once by the composition root.
#[derive(Clone)]
pub struct AppState {
    /// Read-only browser directory.
    pub browsers: Arc<dyn BrowserDirectory>,
    /// Session recording repository.
    pub recordings: Arc<dyn RecordingRepository>,
    /// Shared tool executor; the same implementation serves MCP and REST.
    pub tools: Arc<dyn ToolBackend>,
    /// Tool catalogue used by the REST discovery endpoint.
    pub catalog: Arc<dyn ToolCatalog>,
    /// Health projection produced by the daemon.
    pub health: Arc<dyn Fn() -> Value + Send + Sync>,
    /// Optional local auth service. None means auth is disabled.
    pub auth: Option<Arc<AuthService>>,
    /// Build version reported by the API.
    pub version: String,
}

impl AppState {
    /// Build state from the already-constructed adapters.
    pub fn new(
        browsers: Arc<dyn BrowserDirectory>,
        recordings: Arc<dyn RecordingRepository>,
        tools: Arc<dyn ToolBackend>,
        catalog: Arc<dyn ToolCatalog>,
        health: Arc<dyn Fn() -> Value + Send + Sync>,
        auth: Option<Arc<AuthService>>,
        version: impl Into<String>,
    ) -> Self {
        Self {
            browsers,
            recordings,
            tools,
            catalog,
            health,
            auth,
            version: version.into(),
        }
    }

    /// Validate the bearer token when auth is enabled.
    ///
    /// Returns Ok(None) when auth is disabled, so callers can stay uniform.
    pub fn authorize(&self, headers: &HeaderMap) -> Result<Option<AccessClaims>, AuthError> {
        let Some(auth) = &self.auth else {
            return Ok(None);
        };
        auth.authorize(headers).map(Some)
    }
}
