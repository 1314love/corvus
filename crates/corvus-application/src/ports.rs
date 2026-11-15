//! Ports implemented by outbound adapters.

use std::sync::Arc;

use async_trait::async_trait;
use corvus_domain::recording::{SessionRow, StepRecord, StepRow};
use corvus_proto::{Envelope, ErrorBody};
use serde_json::Value;

use crate::dto::{BrowserSummary, UserAccount, UserView};

/// Read-only browser directory used by API and reporting use cases.
#[async_trait]
pub trait BrowserDirectory: Send + Sync {
    /// Lists registered browsers in stable id order.
    async fn list(&self) -> Vec<BrowserSummary>;

    /// Looks up one browser by id.
    async fn get(&self, id: &str) -> Option<BrowserSummary>;
}

/// Session recording repository.
#[async_trait]
pub trait RecordingRepository: Send + Sync {
    /// Lists recent sessions.
    async fn list_sessions(
        &self,
        limit: u32,
        since_ms: Option<i64>,
    ) -> Result<Vec<SessionRow>, String>;

    /// Loads one session.
    async fn get_session(&self, id: &str) -> Result<Option<SessionRow>, String>;

    /// Loads ordered steps for one session.
    async fn get_steps(
        &self,
        session_id: &str,
        from_seq: i64,
        limit: u32,
    ) -> Result<Vec<StepRow>, String>;

    /// Appends one recorded step.
    async fn record_step(&self, step: StepRecord) -> Result<(), String>;
}

/// Synchronous session bookkeeping used by the MCP presentation adapter.
///
/// The adapter records sessions and cancelled steps through this port, so it
/// never names the concrete recording store.
pub trait SessionRecording: Send + Sync {
    /// Opens or refreshes one recorded session.
    fn start_session(
        &self,
        id: &str,
        transport: &str,
        agent: Option<&str>,
        browser: Option<&str>,
    ) -> Result<(), String>;

    /// Whether a session id is already recorded.
    fn session_exists(&self, id: &str) -> Result<bool, String>;

    /// Closes one session.
    fn finish_session(&self, id: &str, status: &str) -> Result<(), String>;

    /// Lists recent sessions.
    fn list_sessions(&self, limit: u32, since_ms: Option<i64>) -> Result<Vec<SessionRow>, String>;

    /// Appends one recorded step.
    fn record_step(&self, step: StepRecord) -> Result<(), String>;
}

/// One live browser connection as the application sees it.
///
/// Adapters implement this over their concrete transport handle. The
/// application never names the transport crate.
pub trait BrowserSession: Send + Sync {
    /// Stable browser identifier.
    fn id(&self) -> &str;

    /// Capabilities the browser currently advertises.
    fn capabilities(&self) -> Vec<String>;

    /// Whether the browser advertises one capability.
    fn supports(&self, capability: &str) -> bool;

    /// Whether the connection is still usable.
    fn is_alive(&self) -> bool;

    /// Sends one protocol envelope to the browser.
    fn send(&self, envelope: Envelope) -> Result<(), ErrorBody>;
}

/// Directory of live browser connections used by command routing.
pub trait BrowserRegistry: Send + Sync {
    /// Every connected browser, in stable order.
    fn list(&self) -> Vec<Arc<dyn BrowserSession>>;

    /// Looks up one browser by id.
    fn get(&self, id: &str) -> Option<Arc<dyn BrowserSession>>;
}

/// Sink for completed tool steps.
pub trait StepRecorder: Send + Sync {
    /// Queues one step; recording failures must not fail the tool call.
    fn record(&self, step: StepRecord);
}

/// Tool catalogue used by inbound presentation adapters.
pub trait ToolCatalog: Send + Sync {
    /// JSON definitions for the requested detail level.
    fn definitions_for(&self, category: Option<&str>, detail: &str) -> Vec<Value>;

    /// Validates and normalises arguments for one tool.
    fn validate(&self, name: &str, arguments: &Value) -> Result<Value, ErrorBody>;
}

/// Event delivered from a transport into the router.
#[derive(Debug)]
pub enum BrowserEvent {
    /// One protocol envelope arrived from a browser.
    Message {
        /// Browser identifier.
        browser: String,
        /// Envelope received from the browser.
        envelope: Box<Envelope>,
    },
    /// A browser connection closed.
    Closed {
        /// Browser identifier.
        browser: String,
    },
}

/// User and refresh-token repository.
#[async_trait]
pub trait AuthRepository: Send + Sync {
    /// Creates a local user and returns its id.
    async fn create_user(
        &self,
        username: &str,
        password_hash: &str,
        role: &str,
    ) -> Result<String, String>;

    /// Finds a local user by username.
    async fn find_user(&self, username: &str) -> Result<Option<UserAccount>, String>;

    /// Returns a public user view by id.
    async fn user_view(&self, id: &str) -> Result<Option<UserView>, String>;

    /// Stores a hashed refresh token.
    async fn store_refresh(
        &self,
        id: &str,
        user_id: &str,
        token_hash: &str,
        expires_at: i64,
        created_at: i64,
    ) -> Result<(), String>;

    /// Loads a hashed refresh token plus account details.
    async fn find_refresh(&self, id: &str) -> Result<Option<RefreshTokenRecord>, String>;

    /// Marks a refresh token as revoked and optionally links its replacement.
    async fn revoke_refresh(
        &self,
        id: &str,
        revoked_at: i64,
        replaced_by: Option<&str>,
    ) -> Result<(), String>;
}

/// A stored refresh token and the account it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshTokenRecord {
    /// Token row id.
    pub id: String,
    /// Owning user id.
    pub user_id: String,
    /// Argon2id hash of the secret half.
    pub token_hash: String,
    /// Expiry as epoch milliseconds.
    pub expires_at: i64,
    /// Revocation timestamp, if any.
    pub revoked_at: Option<i64>,
    /// Replacement token id, if any.
    pub replaced_by: Option<String>,
    /// Username for the owning account.
    pub username: String,
    /// Role for the owning account.
    pub role: String,
}

/// Password hashing port.
pub trait PasswordHasher: Send + Sync {
    /// Hashes a password using the adapter's memory-hard parameters.
    fn hash(&self, password: &str) -> Result<String, String>;

    /// Verifies a password against a stored hash.
    fn verify(&self, password: &str, hash: &str) -> Result<bool, String>;
}

/// Access-token issuance and validation port.
pub trait TokenService: Send + Sync {
    /// Issues a signed access token.
    fn issue_access(&self, user: &UserView) -> Result<String, String>;

    /// Validates a signed access token and returns its claims.
    fn verify_access(&self, token: &str) -> Result<AccessClaims, String>;
}

/// Claims carried by an access token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessClaims {
    /// Subject user id.
    pub subject: String,
    /// Username.
    pub username: String,
    /// Role name.
    pub role: String,
    /// Expiry as epoch seconds.
    pub expires_at: usize,
}

/// Wall-clock port.
pub trait Clock: Send + Sync {
    /// Current time as epoch milliseconds.
    fn now_ms(&self) -> i64;
}

/// Metrics sink port.
pub trait MetricsSink: Send + Sync {
    /// Increments a counter.
    fn increment(&self, name: &str, labels: &[(&str, &str)]);
}

/// A read-only health projection.
pub trait HealthReadModel: Send + Sync {
    /// Returns a JSON health document.
    fn health(&self) -> Value;
}
