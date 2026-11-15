//! Versioned schema migrations for the recording store.
//!
//! The baseline migration reproduces the v1 schema exactly. Additive migrations
//! may follow it; the legacy `schema_meta` marker stays at version 1 so a v1.3
//! binary can still open and read the database after an upgrade.

use sea_orm_migration::prelude::*;

const BASELINE: &str = r#"
CREATE TABLE IF NOT EXISTS schema_meta (
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
CREATE INDEX IF NOT EXISTS steps_session_seq ON steps(session_id, seq);
"#;

/// Baseline migration: create the v1 schema without touching existing rows.
#[derive(DeriveMigrationName)]
pub struct Baseline;

#[async_trait::async_trait]
impl MigrationTrait for Baseline {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(BASELINE)
            .await?;
        manager
            .get_connection()
            .execute_unprepared(
                "INSERT OR IGNORE INTO schema_meta (key, value) VALUES ('schema_version', '1')",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The baseline is destructive to roll back and is shared with v1.0-v1.3.
        // Rolling it back would drop user data, so down is intentionally a no-op.
        Ok(())
    }
}

/// The recording store migrator.
pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(Baseline)]
    }
}
