//! Persistent recording of MCP sessions and their tool calls.
#![warn(missing_docs)]
//!
//! The store is a small SQLite database. Reads and session bookkeeping use a
//! shared connection behind a mutex; steps are funnelled through one writer
//! task so a tool call never waits for the disk.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, DbErr, QueryResult, Statement,
    TransactionTrait, Value as DbValue,
};
use sea_orm_migration::MigratorTrait;
use serde_json::Value;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

mod migrations;

pub use corvus_domain::recording::{
    now_millis, SessionContext, SessionRow, StepRecord, StepRow, MAX_PAYLOAD_BYTES, RETENTION_MS,
};

/// Current schema version.
pub const SCHEMA_VERSION: i64 = 1;

/// Why a recording operation failed.
#[derive(Debug, Error)]
pub enum RecordError {
    /// SQLite reported an error through SeaORM.
    #[error("recording database error: {0}")]
    Database(#[from] DbErr),
    /// The database pool could not be opened.
    #[error("recording database connection error: {0}")]
    Connection(#[from] sqlx::Error),
    /// The database directory could not be created.
    #[error("could not create the recording directory: {0}")]
    Io(#[from] std::io::Error),
    /// The database has a schema version this build does not understand.
    #[error("unsupported recording schema version {found}")]
    UnsupportedSchema {
        /// Version found in the file.
        found: i64,
    },
    /// The background writer has stopped.
    #[error("recording writer is no longer running")]
    WriterClosed,
    /// An existing database could not be backed up before a migration.
    #[error("could not back up the existing database at {path:?} to {backup:?}: {message}")]
    Backup {
        /// Existing database path.
        path: PathBuf,
        /// Destination backup path.
        backup: PathBuf,
        /// Underlying SQLite error.
        message: String,
    },
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
            tracing::warn!(reason = "writer gone", "tool step dropped");
        }
    }
}

impl corvus_application::ports::StepRecorder for StepSink {
    fn record(&self, step: StepRecord) {
        StepSink::record(self, step);
    }
}

impl corvus_application::ports::SessionRecording for Store {
    fn start_session(
        &self,
        id: &str,
        transport: &str,
        agent: Option<&str>,
        browser: Option<&str>,
    ) -> Result<(), String> {
        Store::start_session(self, id, transport, agent, browser).map_err(|error| error.to_string())
    }

    fn session_exists(&self, id: &str) -> Result<bool, String> {
        Store::session_exists(self, id).map_err(|error| error.to_string())
    }

    fn finish_session(&self, id: &str, status: &str) -> Result<(), String> {
        Store::finish_session(self, id, status).map_err(|error| error.to_string())
    }

    fn list_sessions(&self, limit: u32, since_ms: Option<i64>) -> Result<Vec<SessionRow>, String> {
        Store::list_sessions(self, limit, since_ms).map_err(|error| error.to_string())
    }

    fn record_step(&self, step: StepRecord) -> Result<(), String> {
        Store::record_step(self, step).map_err(|error| error.to_string())
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
            Ok(Err(error)) => tracing::warn!(error = %error, "recording writer failed"),
            Err(_) => tracing::warn!("recording writer shutdown timed out"),
        }
    }
}

/// The SQLite recording store.
pub struct Store {
    path: PathBuf,
    sender: Mutex<Option<std_mpsc::Sender<BoxedJob>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

type BoxedJob = Box<
    dyn FnOnce(DatabaseConnection) -> Pin<Box<dyn Future<Output = DatabaseConnection> + Send>>
        + Send,
>;

enum Target {
    File(PathBuf),
    Memory,
    ReadOnly(PathBuf),
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
        Self::spawn(path.clone(), Target::File(path))
    }

    /// Open a private in-memory store, used by tests.
    pub fn open_in_memory() -> Result<Self, RecordError> {
        Self::spawn(PathBuf::from(":memory:"), Target::Memory)
    }

    /// Open an existing store without creating or migrating it.
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self, RecordError> {
        let path = path.as_ref().to_path_buf();
        Self::spawn(path.clone(), Target::ReadOnly(path))
    }

    fn spawn(path: PathBuf, target: Target) -> Result<Self, RecordError> {
        let (ready_sender, ready_receiver) = std_mpsc::channel();
        let (sender, receiver) = std_mpsc::channel::<BoxedJob>();
        let worker = std::thread::Builder::new()
            .name("corvus-record".to_string())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = ready_sender.send(Err(RecordError::Io(error)));
                        return;
                    }
                };
                runtime.block_on(async move {
                    let mut connection = match open_connection(&target).await {
                        Ok(connection) => connection,
                        Err(error) => {
                            let _ = ready_sender.send(Err(error));
                            return;
                        }
                    };
                    let _ = ready_sender.send(Ok(()));
                    while let Ok(job) = receiver.recv() {
                        connection = job(connection).await;
                    }
                });
            })
            .map_err(RecordError::Io)?;
        match ready_receiver.recv() {
            Ok(Ok(())) => Ok(Self {
                path,
                sender: Mutex::new(Some(sender)),
                worker: Mutex::new(Some(worker)),
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(_) => {
                let _ = worker.join();
                Err(RecordError::WriterClosed)
            }
        }
    }

    fn run<T, F>(&self, work: F) -> Result<T, RecordError>
    where
        T: Send + 'static,
        F: FnOnce(
                DatabaseConnection,
            )
                -> Pin<Box<dyn Future<Output = (DatabaseConnection, Result<T, DbErr>)> + Send>>
            + Send
            + 'static,
    {
        let (reply, wait) = std_mpsc::channel();
        let job: BoxedJob = Box::new(move |connection| {
            Box::pin(async move {
                let (connection, result) = work(connection).await;
                let _ = reply.send(result);
                connection
            })
        });
        let sender = lock(&self.sender);
        let Some(sender) = sender.as_ref() else {
            return Err(RecordError::WriterClosed);
        };
        sender.send(job).map_err(|_| RecordError::WriterClosed)?;
        wait.recv()
            .map_err(|_| RecordError::WriterClosed)?
            .map_err(RecordError::from)
    }

    #[cfg(test)]
    fn test_execute(&self, sql: &'static str, value: i64) -> Result<(), RecordError> {
        self.run(move |connection| {
            Box::pin(async move {
                let result = async {
                    connection
                        .execute(Statement::from_sql_and_values(
                            DbBackend::Sqlite,
                            sql,
                            vec![DbValue::from(value)],
                        ))
                        .await?;
                    Ok(())
                }
                .await;
                (connection, result)
            })
        })
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
        self.run(move |connection| {
            Box::pin(async move {
                let result = maintenance(&connection).await;
                (connection, result)
            })
        })
    }

    /// Create a session or refresh its descriptive fields.
    pub fn start_session(
        &self,
        id: &str,
        transport: &str,
        agent: Option<&str>,
        browser: Option<&str>,
    ) -> Result<(), RecordError> {
        let id = id.to_string();
        let transport = transport.to_string();
        let agent = agent.map(str::to_string);
        let browser = browser.map(str::to_string);
        let now = now_millis();
        self.run(move |connection| {
            Box::pin(async move {
                let result = async {
                    connection
                        .execute(Statement::from_sql_and_values(
                            DbBackend::Sqlite,
                            "INSERT INTO sessions (id, started_at, browser, agent, transport, status)
                             VALUES (?1, ?2, ?3, ?4, ?5, 'active')
                             ON CONFLICT(id) DO UPDATE SET
                                 browser = excluded.browser,
                                 agent = excluded.agent,
                                 transport = excluded.transport",
                            vec![
                                DbValue::from(id),
                                DbValue::from(now),
                                DbValue::from(browser),
                                DbValue::from(agent),
                                DbValue::from(transport),
                            ],
                        ))
                        .await?;
                    Ok(())
                }
                .await;
                (connection, result)
            })
        })
    }

    /// Close a session.
    pub fn finish_session(&self, id: &str, status: &str) -> Result<(), RecordError> {
        let id = id.to_string();
        let status = status.to_string();
        let now = now_millis();
        self.run(move |connection| {
            Box::pin(async move {
                let result = async {
                    connection
                        .execute(Statement::from_sql_and_values(
                            DbBackend::Sqlite,
                            "UPDATE sessions SET ended_at = COALESCE(ended_at, ?2), status = ?3 WHERE id = ?1",
                            vec![DbValue::from(id), DbValue::from(now), DbValue::from(status)],
                        ))
                        .await?;
                    Ok(())
                }
                .await;
                (connection, result)
            })
        })
    }

    /// Insert one step and update the session counters.
    pub fn record_step(&self, step: StepRecord) -> Result<(), RecordError> {
        let StepRecord {
            session_id,
            ts,
            tool,
            args,
            result,
            error_code,
            duration_ms,
            browser_id,
            tab_id,
            ok,
        } = step;
        let (args_json, args_truncated) = encode_payload(&args);
        let (result_json, result_truncated) = match &result {
            Some(result) => {
                let (text, truncated) = encode_payload(result);
                (Some(text), truncated)
            }
            None => (None, false),
        };
        let failure = if ok { 0 } else { 1 };
        self.run(move |connection| {
            Box::pin(async move {
                let result = async {
                    let transaction = connection.begin().await?;
                    let sequence = transaction
                        .query_one(Statement::from_sql_and_values(
                            DbBackend::Sqlite,
                            "SELECT COALESCE(MAX(seq), 0) + 1 FROM steps WHERE session_id = ?1",
                            vec![DbValue::from(session_id.clone())],
                        ))
                        .await?;
                    let seq = sequence
                        .map(|row| row.try_get_by_index::<i64>(0))
                        .transpose()?
                        .unwrap_or(0);
                    transaction
                        .execute(Statement::from_sql_and_values(
                            DbBackend::Sqlite,
                            "INSERT INTO steps (session_id, seq, ts, tool, args_json, result_json,
                                                error_code, duration_ms, browser_id, tab_id, ok,
                                                args_truncated, result_truncated)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                            vec![
                                DbValue::from(session_id.clone()),
                                DbValue::from(seq),
                                DbValue::from(ts),
                                DbValue::from(tool),
                                DbValue::from(args_json),
                                DbValue::from(result_json),
                                DbValue::from(error_code),
                                DbValue::from(duration_ms),
                                DbValue::from(browser_id),
                                DbValue::from(tab_id),
                                DbValue::from(if ok { 1_i64 } else { 0_i64 }),
                                DbValue::from(if args_truncated { 1_i64 } else { 0_i64 }),
                                DbValue::from(if result_truncated { 1_i64 } else { 0_i64 }),
                            ],
                        ))
                        .await?;
                    transaction
                        .execute(Statement::from_sql_and_values(
                            DbBackend::Sqlite,
                            "UPDATE sessions SET steps = steps + 1, errors = errors + ?2 WHERE id = ?1",
                            vec![DbValue::from(session_id), DbValue::from(failure)],
                        ))
                        .await?;
                    transaction.commit().await?;
                    Ok(())
                }
                .await;
                (connection, result)
            })
        })
    }

    /// Newest sessions first.
    pub fn list_sessions(
        &self,
        limit: u32,
        since_ms: Option<i64>,
    ) -> Result<Vec<SessionRow>, RecordError> {
        self.run(move |connection| {
            Box::pin(async move {
                let result = async {
                    let rows = connection
                        .query_all(Statement::from_sql_and_values(
                            DbBackend::Sqlite,
                            "SELECT id, started_at, ended_at, browser, profile, agent, transport,
                                    status, steps, errors
                             FROM sessions
                             WHERE (?1 IS NULL OR started_at >= ?1)
                             ORDER BY started_at DESC, id DESC
                             LIMIT ?2",
                            vec![DbValue::from(since_ms), DbValue::from(i64::from(limit))],
                        ))
                        .await?;
                    rows.iter().map(map_session).collect()
                }
                .await;
                (connection, result)
            })
        })
    }

    /// Number of sessions, optionally restricted by start time.
    pub fn count_sessions(&self, since_ms: Option<i64>) -> Result<i64, RecordError> {
        self.run(move |connection| {
            Box::pin(async move {
                let result = async {
                    let row = connection
                        .query_one(Statement::from_sql_and_values(
                            DbBackend::Sqlite,
                            "SELECT COUNT(*) FROM sessions WHERE (?1 IS NULL OR started_at >= ?1)",
                            vec![DbValue::from(since_ms)],
                        ))
                        .await?;
                    row.map(|row| row.try_get_by_index::<i64>(0))
                        .transpose()
                        .map(|count| count.unwrap_or(0))
                }
                .await;
                (connection, result)
            })
        })
    }

    /// Whether a session exists.
    pub fn session_exists(&self, id: &str) -> Result<bool, RecordError> {
        let id = id.to_string();
        self.run(move |connection| {
            Box::pin(async move {
                let result = async {
                    let row = connection
                        .query_one(Statement::from_sql_and_values(
                            DbBackend::Sqlite,
                            "SELECT COUNT(*) FROM sessions WHERE id = ?1",
                            vec![DbValue::from(id)],
                        ))
                        .await?;
                    let count = row
                        .map(|row| row.try_get_by_index::<i64>(0))
                        .transpose()?
                        .unwrap_or(0);
                    Ok(count > 0)
                }
                .await;
                (connection, result)
            })
        })
    }

    /// One session by id.
    pub fn get_session(&self, id: &str) -> Result<Option<SessionRow>, RecordError> {
        let id = id.to_string();
        self.run(move |connection| {
            Box::pin(async move {
                let result = async {
                    let row = connection
                        .query_one(Statement::from_sql_and_values(
                            DbBackend::Sqlite,
                            "SELECT id, started_at, ended_at, browser, profile, agent, transport,
                                    status, steps, errors
                             FROM sessions WHERE id = ?1",
                            vec![DbValue::from(id)],
                        ))
                        .await?;
                    row.as_ref().map(map_session).transpose()
                }
                .await;
                (connection, result)
            })
        })
    }

    /// Steps of a session in ascending sequence order.
    pub fn get_session_steps(
        &self,
        session_id: &str,
        from_seq: i64,
        limit: u32,
    ) -> Result<Vec<StepRow>, RecordError> {
        let session_id = session_id.to_string();
        self.run(move |connection| {
            Box::pin(async move {
                let result = async {
                    let rows = connection
                        .query_all(Statement::from_sql_and_values(
                            DbBackend::Sqlite,
                            "SELECT id, session_id, seq, ts, tool, args_json, result_json, error_code,
                                    duration_ms, browser_id, tab_id, ok, args_truncated, result_truncated
                             FROM steps
                             WHERE session_id = ?1 AND seq >= ?2
                             ORDER BY seq ASC
                             LIMIT ?3",
                            vec![
                                DbValue::from(session_id),
                                DbValue::from(from_seq),
                                DbValue::from(i64::from(limit)),
                            ],
                        ))
                        .await?;
                    rows.iter().map(map_step).collect()
                }
                .await;
                (connection, result)
            })
        })
    }

    /// Number of steps recorded in a session.
    pub fn count_steps(&self, session_id: &str) -> Result<i64, RecordError> {
        let session_id = session_id.to_string();
        self.run(move |connection| {
            Box::pin(async move {
                let result = async {
                    let row = connection
                        .query_one(Statement::from_sql_and_values(
                            DbBackend::Sqlite,
                            "SELECT COUNT(*) FROM steps WHERE session_id = ?1",
                            vec![DbValue::from(session_id)],
                        ))
                        .await?;
                    row.map(|row| row.try_get_by_index::<i64>(0))
                        .transpose()
                        .map(|count| count.unwrap_or(0))
                }
                .await;
                (connection, result)
            })
        })
    }

    /// Total number of sessions in the store.
    pub fn sessions_total(&self) -> Result<i64, RecordError> {
        self.count_sessions(None)
    }

    /// Number of sessions that are still active.
    pub fn sessions_active(&self) -> Result<i64, RecordError> {
        self.run(move |connection| {
            Box::pin(async move {
                let result = async {
                    let row = connection
                        .query_one(Statement::from_string(
                            DbBackend::Sqlite,
                            "SELECT COUNT(*) FROM sessions WHERE ended_at IS NULL".to_string(),
                        ))
                        .await?;
                    row.map(|row| row.try_get_by_index::<i64>(0))
                        .transpose()
                        .map(|count| count.unwrap_or(0))
                }
                .await;
                (connection, result)
            })
        })
    }

    /// Total number of recorded steps.
    pub fn steps_total(&self) -> Result<i64, RecordError> {
        self.run(move |connection| {
            Box::pin(async move {
                let result = async {
                    let row = connection
                        .query_one(Statement::from_string(
                            DbBackend::Sqlite,
                            "SELECT COUNT(*) FROM steps".to_string(),
                        ))
                        .await?;
                    row.map(|row| row.try_get_by_index::<i64>(0))
                        .transpose()
                        .map(|count| count.unwrap_or(0))
                }
                .await;
                (connection, result)
            })
        })
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

impl Drop for Store {
    fn drop(&mut self) {
        let sender = lock(&self.sender).take();
        drop(sender);
        if let Some(worker) = lock(&self.worker).take() {
            let _ = worker.join();
        }
    }
}

async fn open_connection(target: &Target) -> Result<DatabaseConnection, RecordError> {
    let existed_before = match target {
        Target::File(path) => std::fs::metadata(path)
            .map(|metadata| metadata.len() > 0)
            .unwrap_or(false),
        Target::Memory | Target::ReadOnly(_) => false,
    };
    let (options, writable) = match target {
        Target::File(path) => (
            SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(true)
                .journal_mode(SqliteJournalMode::Wal),
            true,
        ),
        Target::Memory => (
            SqliteConnectOptions::new()
                .in_memory(true)
                .journal_mode(SqliteJournalMode::Wal),
            true,
        ),
        Target::ReadOnly(path) => (
            SqliteConnectOptions::new()
                .filename(path)
                .read_only(true)
                .create_if_missing(false),
            false,
        ),
    };
    let options = options
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    let connection = DatabaseConnection::from(pool);
    if writable {
        if let Target::File(path) = target {
            if existed_before && !migration_recorded(&connection).await? {
                let backup = backup_database(&connection, path).await?;
                tracing::info!(
                    database = %path.display(),
                    backup = %backup.display(),
                    "backed up recording database before migration"
                );
            }
        }
        migrations::Migrator::up(&connection, None).await?;
    }
    let found = read_schema_version(&connection).await?;
    if found != SCHEMA_VERSION {
        return Err(RecordError::UnsupportedSchema { found });
    }
    if writable {
        maintenance(&connection).await?;
    }
    Ok(connection)
}

async fn migration_recorded(connection: &DatabaseConnection) -> Result<bool, RecordError> {
    let table = connection
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'seaql_migrations'"
                .to_string(),
        ))
        .await?;
    if table.is_none() {
        return Ok(false);
    }
    let row = connection
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) FROM seaql_migrations".to_string(),
        ))
        .await?;
    Ok(row
        .map(|row| row.try_get_by_index::<i64>(0))
        .transpose()?
        .unwrap_or(0)
        > 0)
}

async fn backup_database(
    connection: &DatabaseConnection,
    path: &Path,
) -> Result<PathBuf, RecordError> {
    let backup = backup_destination(path);
    if let Err(error) = connection
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "VACUUM INTO ?1",
            vec![DbValue::from(backup.to_string_lossy().to_string())],
        ))
        .await
    {
        return Err(RecordError::Backup {
            path: path.to_path_buf(),
            backup,
            message: error.to_string(),
        });
    }
    Ok(backup)
}

fn backup_destination(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("corvus.db");
    let stamp = now_millis();
    for attempt in 0..100 {
        let suffix = if attempt == 0 {
            format!(".backup-{stamp}.db")
        } else {
            format!(".backup-{stamp}-{attempt}.db")
        };
        let candidate = path.with_file_name(format!("{name}{suffix}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    path.with_file_name(format!("{name}.backup-{stamp}-fallback.db"))
}

async fn read_schema_version(connection: &DatabaseConnection) -> Result<i64, RecordError> {
    let row = connection
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT value FROM schema_meta WHERE key = 'schema_version'".to_string(),
        ))
        .await?;
    Ok(row
        .map(|row| row.try_get_by_index::<String>(0))
        .transpose()?
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0))
}

async fn maintenance(connection: &DatabaseConnection) -> Result<(), DbErr> {
    let now = now_millis();
    let cutoff = now.saturating_sub(RETENTION_MS);
    let transaction = connection.begin().await?;
    transaction
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE sessions SET ended_at = ?1, status = 'abandoned' WHERE ended_at IS NULL",
            vec![DbValue::from(now)],
        ))
        .await?;
    transaction
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM steps WHERE session_id IN (SELECT id FROM sessions WHERE started_at < ?1)",
            vec![DbValue::from(cutoff)],
        ))
        .await?;
    transaction
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM sessions WHERE started_at < ?1",
            vec![DbValue::from(cutoff)],
        ))
        .await?;
    transaction.commit().await?;
    Ok(())
}

fn map_session(row: &QueryResult) -> Result<SessionRow, DbErr> {
    Ok(SessionRow {
        id: row.try_get_by_index(0)?,
        started_at: row.try_get_by_index(1)?,
        ended_at: row.try_get_by_index(2)?,
        browser: row.try_get_by_index(3)?,
        profile: row.try_get_by_index(4)?,
        agent: row.try_get_by_index(5)?,
        transport: row.try_get_by_index(6)?,
        status: row.try_get_by_index(7)?,
        steps: row.try_get_by_index(8)?,
        errors: row.try_get_by_index(9)?,
    })
}

fn map_step(row: &QueryResult) -> Result<StepRow, DbErr> {
    Ok(StepRow {
        id: row.try_get_by_index(0)?,
        session_id: row.try_get_by_index(1)?,
        seq: row.try_get_by_index(2)?,
        ts: row.try_get_by_index(3)?,
        tool: row.try_get_by_index(4)?,
        args_json: row.try_get_by_index(5)?,
        result_json: row.try_get_by_index(6)?,
        error_code: row.try_get_by_index(7)?,
        duration_ms: row.try_get_by_index(8)?,
        browser_id: row.try_get_by_index(9)?,
        tab_id: row.try_get_by_index(10)?,
        ok: row.try_get_by_index::<i64>(11)? != 0,
        args_truncated: row.try_get_by_index::<i64>(12)? != 0,
        result_truncated: row.try_get_by_index::<i64>(13)? != 0,
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
    use std::time::{SystemTime, UNIX_EPOCH};

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

    fn seed_legacy_database(path: &Path) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(
                    SqliteConnectOptions::new()
                        .filename(path)
                        .create_if_missing(true),
                )
                .await
                .expect("seed connection");
            let schema = [
                "CREATE TABLE schema_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
                "CREATE TABLE sessions (id TEXT PRIMARY KEY, started_at INTEGER NOT NULL, ended_at INTEGER, browser TEXT, profile TEXT, agent TEXT, transport TEXT NOT NULL, status TEXT NOT NULL, steps INTEGER NOT NULL DEFAULT 0, errors INTEGER NOT NULL DEFAULT 0)",
                "CREATE TABLE steps (id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL, seq INTEGER NOT NULL, ts INTEGER NOT NULL, tool TEXT NOT NULL, args_json TEXT, result_json TEXT, error_code TEXT, duration_ms INTEGER NOT NULL, browser_id TEXT, tab_id TEXT, ok INTEGER NOT NULL, args_truncated INTEGER NOT NULL DEFAULT 0, result_truncated INTEGER NOT NULL DEFAULT 0, UNIQUE(session_id, seq))",
                "CREATE INDEX steps_session_seq ON steps(session_id, seq)",
            ];
            for statement in schema {
                sqlx::query(statement)
                    .execute(&pool)
                    .await
                    .expect("legacy schema");
            }
            sqlx::query(
                "INSERT INTO schema_meta (key, value) VALUES ('schema_version', '1')",
            )
            .execute(&pool)
            .await
            .expect("schema marker");
            sqlx::query(
                "INSERT INTO sessions (id, started_at, ended_at, transport, status, steps, errors) VALUES ('s-legacy', ?1, ?2, 'http', 'closed', 1, 0)",
            )
            .bind(now_millis() - 1_000)
            .bind(now_millis())
            .execute(&pool)
            .await
            .expect("legacy session");
            sqlx::query(
                "INSERT INTO steps (session_id, seq, ts, tool, args_json, result_json, duration_ms, ok) VALUES ('s-legacy', 1, ?1, 'browser_click', '{\"target\":\"@e1\"}', '{\"clicked\":true}', 5, 1)",
            )
            .bind(now_millis())
            .execute(&pool)
            .await
            .expect("legacy step");
            pool.close().await;
        });
    }

    fn backup_files(path: &Path) -> Vec<PathBuf> {
        let parent = path.parent().expect("parent");
        let prefix = format!(
            "{}.backup-",
            path.file_name().expect("file name").to_string_lossy()
        );
        let mut files: Vec<PathBuf> = std::fs::read_dir(parent)
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|candidate| {
                candidate
                    .file_name()
                    .map(|name| name.to_string_lossy().starts_with(&prefix))
                    .unwrap_or(false)
            })
            .collect();
        files.sort();
        files
    }

    #[test]
    fn existing_v13_database_is_backed_up_before_migration() {
        let path = temp_path("backup");
        seed_legacy_database(&path);
        assert!(backup_files(&path).is_empty());
        {
            let store = Store::open(&path).expect("upgrade");
            let session = store
                .get_session("s-legacy")
                .expect("read")
                .expect("legacy session");
            assert_eq!(session.steps, 1);
            assert_eq!(store.schema_version(), 1);
        }
        let backups = backup_files(&path);
        assert_eq!(backups.len(), 1, "one pre-migration backup");
        let backup = Store::open_read_only(&backups[0]).expect("read backup");
        let session = backup
            .get_session("s-legacy")
            .expect("read")
            .expect("legacy session in backup");
        assert_eq!(session.steps, 1);
        assert_eq!(
            backup
                .get_session_steps("s-legacy", 0, 10)
                .expect("steps")
                .len(),
            1
        );
        drop(backup);
        let _ = std::fs::remove_file(&path);
        for file in backups {
            let _ = std::fs::remove_file(file);
        }
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }

    #[test]
    fn startup_marks_active_sessions_abandoned_and_prunes_old_ones() {
        let path = temp_path("maintenance");
        {
            let store = Store::open(&path).expect("open");
            store
                .start_session("s-live", "http", None, None)
                .expect("session");
            store
                .test_execute(
                    "INSERT INTO sessions (id, started_at, transport, status)
                     VALUES ('s-old', ?1, 'http', 'closed')",
                    now_millis() - RETENTION_MS - 1,
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
