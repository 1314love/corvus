//! Persistent recording of MCP sessions and their tool calls.
#![warn(missing_docs)]
//!
//! The store is a small SQLite database. Reads and session bookkeeping use a
//! shared connection behind a mutex; steps are funnelled through one writer
//! task so a tool call never waits for the disk.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

/// Largest JSON payload kept verbatim in the database.
pub const MAX_PAYLOAD_BYTES: usize = 262_144;
/// Sessions older than this are pruned on startup.
pub const RETENTION_MS: i64 = 30 * 24 * 60 * 60 * 1_000;

/// Current schema version.
pub const SCHEMA_VERSION: i64 = 1;

/// The MCP session that produced a tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionContext {
    /// Session identifier.
    pub session_id: String,
    /// Transport that carries the session, for example "http" or "stdio".
    pub transport: String,
    /// Agent that opened the session, when the client named one.
    pub agent: Option<String>,
}

impl SessionContext {
    /// Build a context.
    pub fn new(
        session_id: impl Into<String>,
        transport: impl Into<String>,
        agent: Option<String>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            transport: transport.into(),
            agent,
        }
    }
}

/// Why a recording operation failed.
#[derive(Debug, Error)]
pub enum RecordError {
    /// SQLite reported an error.
    #[error("recording database error: {0}")]
    Database(#[from] rusqlite::Error),
    /// The database directory could not be created.
    #[error("could not create the recording directory: {0}")]
    Io(#[from] std::io::Error),
    /// The database has a schema version this build does not understand.
    #[error("unsupported recording schema version {found}")]
    UnsupportedSchema {
        /// Version found in the file.
        found: i64,
    },
}

/// One recorded session row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    /// Session identifier.
    pub id: String,
    /// Start time in milliseconds since the Unix epoch.
    pub started_at: i64,
    /// End time in milliseconds since the Unix epoch.
    pub ended_at: Option<i64>,
    /// Browser id that served the session.
    pub browser: Option<String>,
    /// Browser profile label.
    pub profile: Option<String>,
    /// Agent that opened the session.
    pub agent: Option<String>,
    /// Transport name, for example "http" or "stdio".
    pub transport: String,
    /// Lifecycle state.
    pub status: String,
    /// Number of recorded steps.
    pub steps: i64,
    /// Number of failed steps.
    pub errors: i64,
}

/// One recorded step row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepRow {
    /// Row id.
    pub id: i64,
    /// Session that owns the step.
    pub session_id: String,
    /// Monotonic position inside the session.
    pub seq: i64,
    /// Time the step finished, in milliseconds since the Unix epoch.
    pub ts: i64,
    /// Tool name.
    pub tool: String,
    /// Stored arguments JSON, possibly truncated.
    pub args_json: Option<String>,
    /// Stored result JSON, possibly truncated.
    pub result_json: Option<String>,
    /// Error code when the step failed.
    pub error_code: Option<String>,
    /// Duration in milliseconds.
    pub duration_ms: i64,
    /// Browser that served the call.
    pub browser_id: Option<String>,
    /// Tab named by the call.
    pub tab_id: Option<String>,
    /// Whether the call succeeded.
    pub ok: bool,
    /// Whether the arguments were truncated before storage.
    pub args_truncated: bool,
    /// Whether the result was truncated before storage.
    pub result_truncated: bool,
}

/// A step waiting to be written.
#[derive(Debug, Clone)]
pub struct StepRecord {
    /// Session the step belongs to.
    pub session_id: String,
    /// Time the call finished, in milliseconds since the Unix epoch.
    pub ts: i64,
    /// Tool name.
    pub tool: String,
    /// Arguments passed to the tool.
    pub args: Value,
    /// Tool result, absent for failures.
    pub result: Option<Value>,
    /// Error code for a failed call.
    pub error_code: Option<String>,
    /// Duration in milliseconds.
    pub duration_ms: i64,
    /// Browser resolved by routing.
    pub browser_id: Option<String>,
    /// Tab named by the call.
    pub tab_id: Option<String>,
    /// Whether the call succeeded.
    pub ok: bool,
}

/// What lives on the recording writer channel.
enum WriterMessage {
    Step(Box<StepRecord>),
    Flush(oneshot::Sender<()>),
    Shutdown,
}

/// A cheap cloneable handle used by the router to report steps.
#[derive(Debug, Clone)]
pub struct StepSink {
    sender: mpsc::UnboundedSender<WriterMessage>,
}

impl StepSink {
    /// Queue a step. A closed writer is logged and the call continues.
    pub fn record(&self, step: StepRecord) {
        if self
            .sender
            .send(WriterMessage::Step(Box::new(step)))
            .is_err()
        {
            tracing::warn!("recording writer is gone; the step was not stored");
        }
    }
}

/// Owns the writing task and the shared store.
pub struct Recorder {
    store: Arc<Store>,
    sender: mpsc::UnboundedSender<WriterMessage>,
    task: tokio::task::JoinHandle<()>,
}

impl Recorder {
    /// Start the single writer task for a store.
    pub fn start(store: Arc<Store>) -> Self {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let writer = Arc::clone(&store);
        let task = tokio::spawn(async move {
            while let Some(message) = receiver.recv().await {
                match message {
                    WriterMessage::Step(step) => {
                        if let Err(error) = writer.record_step(*step) {
                            tracing::warn!(error = %error, "could not record a tool step");
                        }
                    }
                    WriterMessage::Flush(reply) => {
                        let _ = reply.send(());
                    }
                    WriterMessage::Shutdown => break,
                }
            }
        });
        Self {
            store,
            sender,
            task,
        }
    }

    /// The store behind the recorder.
    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    /// A handle for reporting steps.
    pub fn sink(&self) -> StepSink {
        StepSink {
            sender: self.sender.clone(),
        }
    }

    /// Wait until every queued step has been written.
    pub async fn flush(&self) -> bool {
        let (reply, wait) = oneshot::channel();
        if self.sender.send(WriterMessage::Flush(reply)).is_err() {
            return false;
        }
        tokio::time::timeout(Duration::from_secs(5), wait)
            .await
            .is_ok()
    }

    /// Stop the writer after draining the queue.
    pub async fn shutdown(self) {
        let _ = self.sender.send(WriterMessage::Shutdown);
        match tokio::time::timeout(Duration::from_secs(5), self.task).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!(error = %error, "recording writer ended badly"),
            Err(_) => tracing::warn!("recording writer did not stop in time"),
        }
    }
}

/// The SQLite recording store.
pub struct Store {
    path: PathBuf,
    connection: Mutex<Connection>,
}

impl Store {
    /// Open or create a store at the given path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RecordError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let connection = Connection::open(&path)?;
        Self::prepare(connection, path)
    }

    /// Open a private in-memory store, used by tests.
    pub fn open_in_memory() -> Result<Self, RecordError> {
        let connection = Connection::open_in_memory()?;
        Self::prepare(connection, PathBuf::from(":memory:"))
    }

    /// Open an existing store without creating or migrating it.
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self, RecordError> {
        let path = path.as_ref().to_path_buf();
        let connection = Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                | rusqlite::OpenFlags::SQLITE_OPEN_URI
                | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let version: Option<String> = connection
            .query_row(
                "SELECT value FROM schema_meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let found = version
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(0);
        if found != SCHEMA_VERSION {
            return Err(RecordError::UnsupportedSchema { found });
        }
        Ok(Self {
            path,
            connection: Mutex::new(connection),
        })
    }

    fn prepare(connection: Connection, path: PathBuf) -> Result<Self, RecordError> {
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_meta (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS sessions (
                 id TEXT PRIMARY KEY,
                 started_at INTEGER NOT NULL,
                 ended_at INTEGER,
                 browser TEXT,
                 profile TEXT,
                 agent TEXT,
                 transport TEXT NOT NULL,
                 status TEXT NOT NULL,
                 steps INTEGER NOT NULL DEFAULT 0,
                 errors INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS steps (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 session_id TEXT NOT NULL,
                 seq INTEGER NOT NULL,
                 ts INTEGER NOT NULL,
                 tool TEXT NOT NULL,
                 args_json TEXT,
                 result_json TEXT,
                 error_code TEXT,
                 duration_ms INTEGER NOT NULL,
                 browser_id TEXT,
                 tab_id TEXT,
                 ok INTEGER NOT NULL,
                 args_truncated INTEGER NOT NULL DEFAULT 0,
                 result_truncated INTEGER NOT NULL DEFAULT 0,
                 UNIQUE(session_id, seq)
             );
             CREATE INDEX IF NOT EXISTS steps_session_seq ON steps(session_id, seq);",
        )?;
        let existing: Option<String> = connection
            .query_row(
                "SELECT value FROM schema_meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        match existing {
            Some(value) => {
                let found = value.parse::<i64>().unwrap_or(0);
                if found != SCHEMA_VERSION {
                    return Err(RecordError::UnsupportedSchema { found });
                }
            }
            None => {
                connection.execute(
                    "INSERT INTO schema_meta (key, value) VALUES ('schema_version', ?1)",
                    params![SCHEMA_VERSION.to_string()],
                )?;
            }
        }
        let store = Self {
            path,
            connection: Mutex::new(connection),
        };
        store.startup_maintenance()?;
        Ok(store)
    }

    /// Path of the database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Schema version stored in the file.
    pub fn schema_version(&self) -> i64 {
        SCHEMA_VERSION
    }

    /// Mark leftover sessions abandoned and prune old sessions.
    pub fn startup_maintenance(&self) -> Result<(), RecordError> {
        let now = now_millis();
        let cutoff = now.saturating_sub(RETENTION_MS);
        let mut connection = lock(&self.connection);
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE sessions SET ended_at = ?1, status = 'abandoned' WHERE ended_at IS NULL",
            params![now],
        )?;
        transaction.execute(
            "DELETE FROM steps WHERE session_id IN (SELECT id FROM sessions WHERE started_at < ?1)",
            params![cutoff],
        )?;
        transaction.execute(
            "DELETE FROM sessions WHERE started_at < ?1",
            params![cutoff],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Create a session or refresh its descriptive fields.
    pub fn start_session(
        &self,
        id: &str,
        transport: &str,
        agent: Option<&str>,
        browser: Option<&str>,
    ) -> Result<(), RecordError> {
        let connection = lock(&self.connection);
        connection.execute(
            "INSERT INTO sessions (id, started_at, browser, agent, transport, status)
             VALUES (?1, ?2, ?3, ?4, ?5, 'active')
             ON CONFLICT(id) DO UPDATE SET
                 browser = excluded.browser,
                 agent = excluded.agent,
                 transport = excluded.transport",
            params![id, now_millis(), browser, agent, transport],
        )?;
        Ok(())
    }

    /// Close a session.
    pub fn finish_session(&self, id: &str, status: &str) -> Result<(), RecordError> {
        let connection = lock(&self.connection);
        connection.execute(
            "UPDATE sessions SET ended_at = COALESCE(ended_at, ?2), status = ?3 WHERE id = ?1",
            params![id, now_millis(), status],
        )?;
        Ok(())
    }

    /// Insert one step and update the session counters.
    pub fn record_step(&self, step: StepRecord) -> Result<(), RecordError> {
        let (args_json, args_truncated) = encode_payload(&step.args);
        let (result_json, result_truncated) = match &step.result {
            Some(result) => {
                let (text, truncated) = encode_payload(result);
                (Some(text), truncated)
            }
            None => (None, false),
        };
        let mut connection = lock(&self.connection);
        let transaction = connection.transaction()?;
        let seq: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM steps WHERE session_id = ?1",
            params![step.session_id],
            |row| row.get(0),
        )?;
        transaction.execute(
            "INSERT INTO steps (session_id, seq, ts, tool, args_json, result_json,
                                error_code, duration_ms, browser_id, tab_id, ok,
                                args_truncated, result_truncated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                step.session_id,
                seq,
                step.ts,
                step.tool,
                args_json,
                result_json,
                step.error_code,
                step.duration_ms,
                step.browser_id,
                step.tab_id,
                if step.ok { 1 } else { 0 },
                if args_truncated { 1 } else { 0 },
                if result_truncated { 1 } else { 0 },
            ],
        )?;
        transaction.execute(
            "UPDATE sessions SET steps = steps + 1, errors = errors + ?2 WHERE id = ?1",
            params![step.session_id, if step.ok { 0 } else { 1 }],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Newest sessions first.
    pub fn list_sessions(
        &self,
        limit: u32,
        since_ms: Option<i64>,
    ) -> Result<Vec<SessionRow>, RecordError> {
        let connection = lock(&self.connection);
        let mut statement = connection.prepare(
            "SELECT id, started_at, ended_at, browser, profile, agent, transport,
                    status, steps, errors
             FROM sessions
             WHERE (?1 IS NULL OR started_at >= ?1)
             ORDER BY started_at DESC, id DESC
             LIMIT ?2",
        )?;
        let rows = statement.query_map(params![since_ms, i64::from(limit)], map_session)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Number of sessions, optionally restricted by start time.
    pub fn count_sessions(&self, since_ms: Option<i64>) -> Result<i64, RecordError> {
        let connection = lock(&self.connection);
        let count = connection.query_row(
            "SELECT COUNT(*) FROM sessions WHERE (?1 IS NULL OR started_at >= ?1)",
            params![since_ms],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Whether a session exists.
    pub fn session_exists(&self, id: &str) -> Result<bool, RecordError> {
        let connection = lock(&self.connection);
        let count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM sessions WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// One session by id.
    pub fn get_session(&self, id: &str) -> Result<Option<SessionRow>, RecordError> {
        let connection = lock(&self.connection);
        let row = connection
            .query_row(
                "SELECT id, started_at, ended_at, browser, profile, agent, transport,
                        status, steps, errors
                 FROM sessions WHERE id = ?1",
                params![id],
                map_session,
            )
            .optional()?;
        Ok(row)
    }

    /// Steps of a session in ascending sequence order.
    pub fn get_session_steps(
        &self,
        session_id: &str,
        from_seq: i64,
        limit: u32,
    ) -> Result<Vec<StepRow>, RecordError> {
        let connection = lock(&self.connection);
        let mut statement = connection.prepare(
            "SELECT id, session_id, seq, ts, tool, args_json, result_json, error_code,
                    duration_ms, browser_id, tab_id, ok, args_truncated, result_truncated
             FROM steps
             WHERE session_id = ?1 AND seq >= ?2
             ORDER BY seq ASC
             LIMIT ?3",
        )?;
        let rows =
            statement.query_map(params![session_id, from_seq, i64::from(limit)], map_step)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Number of steps recorded in a session.
    pub fn count_steps(&self, session_id: &str) -> Result<i64, RecordError> {
        let connection = lock(&self.connection);
        let count = connection.query_row(
            "SELECT COUNT(*) FROM steps WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Total number of sessions in the store.
    pub fn sessions_total(&self) -> Result<i64, RecordError> {
        self.count_sessions(None)
    }

    /// Number of sessions that are still active.
    pub fn sessions_active(&self) -> Result<i64, RecordError> {
        let connection = lock(&self.connection);
        let count = connection.query_row(
            "SELECT COUNT(*) FROM sessions WHERE ended_at IS NULL",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Total number of recorded steps.
    pub fn steps_total(&self) -> Result<i64, RecordError> {
        let connection = lock(&self.connection);
        let count = connection.query_row("SELECT COUNT(*) FROM steps", [], |row| row.get(0))?;
        Ok(count)
    }

    /// All rows of a session, in export order.
    pub fn export_rows(
        &self,
        session_id: &str,
    ) -> Result<Option<(SessionRow, Vec<StepRow>)>, RecordError> {
        let Some(session) = self.get_session(session_id)? else {
            return Ok(None);
        };
        let steps = self.get_session_steps(session_id, 0, u32::MAX)?;
        Ok(Some((session, steps)))
    }
}

fn map_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRow> {
    Ok(SessionRow {
        id: row.get(0)?,
        started_at: row.get(1)?,
        ended_at: row.get(2)?,
        browser: row.get(3)?,
        profile: row.get(4)?,
        agent: row.get(5)?,
        transport: row.get(6)?,
        status: row.get(7)?,
        steps: row.get(8)?,
        errors: row.get(9)?,
    })
}

fn map_step(row: &rusqlite::Row<'_>) -> rusqlite::Result<StepRow> {
    Ok(StepRow {
        id: row.get(0)?,
        session_id: row.get(1)?,
        seq: row.get(2)?,
        ts: row.get(3)?,
        tool: row.get(4)?,
        args_json: row.get(5)?,
        result_json: row.get(6)?,
        error_code: row.get(7)?,
        duration_ms: row.get(8)?,
        browser_id: row.get(9)?,
        tab_id: row.get(10)?,
        ok: row.get::<_, i64>(11)? != 0,
        args_truncated: row.get::<_, i64>(12)? != 0,
        result_truncated: row.get::<_, i64>(13)? != 0,
    })
}

fn encode_payload(value: &Value) -> (String, bool) {
    let text = serde_json::to_string(value).unwrap_or_else(|_| "null".to_string());
    if text.len() <= MAX_PAYLOAD_BYTES {
        return (text, false);
    }
    let mut cut = MAX_PAYLOAD_BYTES;
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    (text[..cut].to_string(), true)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

/// Milliseconds since the Unix epoch.
pub fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// Resolve the database path: an explicit path, then CORVUS_HOME, then
/// CORVUS_DB_PATH, then the per-user default.
///
/// A configured home owns the database, so CORVUS_DB_PATH is ignored while
/// CORVUS_HOME is set. The daemon applies the same precedence for `--home`.
pub fn resolve_path(explicit: Option<PathBuf>) -> PathBuf {
    if let Some(path) = explicit {
        return path;
    }
    if configured_home_root().is_some() {
        return default_path();
    }
    if let Ok(path) = std::env::var("CORVUS_DB_PATH") {
        if !path.trim().is_empty() {
            return PathBuf::from(path);
        }
    }
    default_path()
}

/// The per-user default database path under the Corvus home.
pub fn default_path() -> PathBuf {
    let root = home_root().unwrap_or_else(|| PathBuf::from(".").join(".corvus"));
    root.join("data").join("corvus.db")
}

/// Resolve the platform Corvus home. Keep this in sync with
/// `crates/corvusd/src/home.rs`; corvus-record must not depend on corvusd.
fn configured_home_root() -> Option<PathBuf> {
    non_empty_var("CORVUS_HOME")
}

fn home_root() -> Option<PathBuf> {
    if let Some(root) = configured_home_root() {
        return Some(root);
    }
    if cfg!(windows) {
        return non_empty_var("LOCALAPPDATA")
            .map(|path| path.join("Corvus"))
            .or_else(|| non_empty_var("USERPROFILE").map(|path| path.join(".corvus")));
    }
    if cfg!(target_os = "macos") {
        return non_empty_var("HOME").map(|path| {
            path.join("Library")
                .join("Application Support")
                .join("Corvus")
        });
    }
    non_empty_var("XDG_DATA_HOME")
        .map(|path| path.join("corvus"))
        .or_else(|| {
            non_empty_var("HOME").map(|path| path.join(".local").join("share").join("corvus"))
        })
}

fn non_empty_var(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_path(name: &str) -> PathBuf {
        let unique = format!(
            "corvus-record-{}-{}-{name}.db",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        );
        std::env::temp_dir().join(unique)
    }

    fn step(session: &str, tool: &str, ok: bool) -> StepRecord {
        StepRecord {
            session_id: session.to_string(),
            ts: now_millis(),
            tool: tool.to_string(),
            args: json!({ "target": "@e1" }),
            result: ok.then(|| json!({ "clicked": true })),
            error_code: (!ok).then(|| "NOT_FOUND".to_string()),
            duration_ms: 12,
            browser_id: Some("edge:default".to_string()),
            tab_id: Some("1".to_string()),
            ok,
        }
    }

    #[test]
    fn schema_is_created_and_reopening_keeps_the_version() {
        let path = temp_path("schema");
        {
            let store = Store::open(&path).expect("open");
            assert_eq!(store.schema_version(), 1);
            store
                .start_session("s-1", "http", Some("test"), None)
                .expect("session");
        }
        let reopened = Store::open(&path).expect("reopen");
        assert_eq!(reopened.schema_version(), 1);
        assert_eq!(reopened.sessions_total().expect("count"), 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn startup_marks_active_sessions_abandoned_and_prunes_old_ones() {
        let path = temp_path("maintenance");
        {
            let store = Store::open(&path).expect("open");
            store
                .start_session("s-live", "http", None, None)
                .expect("session");
            let connection = lock(&store.connection);
            connection
                .execute(
                    "INSERT INTO sessions (id, started_at, transport, status)
                     VALUES ('s-old', ?1, 'http', 'closed')",
                    params![now_millis() - RETENTION_MS - 1],
                )
                .expect("old session");
        }
        let store = Store::open(&path).expect("reopen");
        let live = store.get_session("s-live").expect("read").expect("live");
        assert_eq!(live.status, "abandoned");
        assert!(live.ended_at.is_some());
        assert!(store.get_session("s-old").expect("read").is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn steps_get_sequences_and_session_counters() {
        let store = Store::open_in_memory().expect("open");
        store
            .start_session("s-1", "http", None, None)
            .expect("session");
        store
            .record_step(step("s-1", "browser_click", true))
            .expect("ok");
        store
            .record_step(step("s-1", "browser_type", false))
            .expect("err");
        let rows = store.get_session_steps("s-1", 0, 10).expect("steps");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].seq, 1);
        assert_eq!(rows[1].seq, 2);
        assert!(rows[0].ok);
        assert!(!rows[1].ok);
        assert_eq!(rows[1].error_code.as_deref(), Some("NOT_FOUND"));
        let session = store.get_session("s-1").expect("read").expect("session");
        assert_eq!(session.steps, 2);
        assert_eq!(session.errors, 1);
    }

    #[test]
    fn oversize_payloads_are_truncated_on_a_character_boundary() {
        let (text, truncated) = encode_payload(&json!("é".repeat(MAX_PAYLOAD_BYTES)));
        assert!(truncated);
        assert!(text.len() <= MAX_PAYLOAD_BYTES);
        assert!(std::str::from_utf8(text.as_bytes()).is_ok());
    }

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct SavedEnv(&'static str, Option<std::ffi::OsString>);

    impl SavedEnv {
        fn capture(key: &'static str) -> Self {
            Self(key, std::env::var_os(key))
        }
    }

    impl Drop for SavedEnv {
        fn drop(&mut self) {
            match self.1.take() {
                Some(value) => std::env::set_var(self.0, value),
                None => std::env::remove_var(self.0),
            }
        }
    }

    #[test]
    fn resolve_path_prefers_configured_home_over_the_database_environment() {
        let _env_lock = lock(&ENV_LOCK);
        let _home = SavedEnv::capture("CORVUS_HOME");
        let _database = SavedEnv::capture("CORVUS_DB_PATH");
        std::env::remove_var("CORVUS_HOME");
        std::env::set_var("CORVUS_DB_PATH", "environment-corvus.db");
        assert_eq!(resolve_path(None), PathBuf::from("environment-corvus.db"));
        std::env::set_var("CORVUS_HOME", "configured-corvus-home");
        assert_eq!(
            resolve_path(None),
            PathBuf::from("configured-corvus-home")
                .join("data")
                .join("corvus.db")
        );
        assert_eq!(
            resolve_path(Some(PathBuf::from("explicit-corvus.db"))),
            PathBuf::from("explicit-corvus.db")
        );
    }

    #[test]
    fn list_sessions_returns_newest_first() {
        let store = Store::open_in_memory().expect("open");
        store.start_session("old", "http", None, None).expect("one");
        std::thread::sleep(Duration::from_millis(2));
        store
            .start_session("new", "stdio", None, None)
            .expect("two");
        let rows = store.list_sessions(10, None).expect("list");
        assert_eq!(rows.first().map(|row| row.id.as_str()), Some("new"));
        assert_eq!(rows.len(), 2);
    }
}
