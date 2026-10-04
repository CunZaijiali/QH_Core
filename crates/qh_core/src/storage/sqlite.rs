use std::{path::Path, time::Duration};

use crate::domain::{ModelId, SessionId, message::Role};
use crate::error::{DataBaseError, StorageError};

use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
use sqlx::{Executor, SqlitePool, sqlite::SqlitePoolOptions};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: SessionId,
    pub title: String,
    pub model: ModelId,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: i64,
    pub meta: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageRecord {
    pub id: i64,
    pub session_id: SessionId,
    pub role: Role,
    pub content: String,
    pub created_at: i64,
    pub sequence: i64,
    pub meta: String,
}

pub struct SqliteStore {
    connection: SqlitePool,
}

impl SqliteStore {
    pub async fn new(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let opt = SqliteConnectOptions::new()
            .journal_mode(SqliteJournalMode::Wal)
            .page_size(4096)
            .create_if_missing(true)
            .filename(path.as_ref())
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5))
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .min_connections(1)
            .max_lifetime(Duration::from_hours(1))
            .acquire_timeout(Duration::from_secs(10))
            .connect_with(opt)
            .await?;
        Ok(Self::after(pool).await?)
    }

    /// 建表 / 迁移 / 检查
    /// 维护默认表
    async fn after(pool: SqlitePool) -> Result<Self, DataBaseError> {
        let mut stream = pool.execute_many(r#"
            CREATE TABLE IF NOT EXISTS session_records (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                title         TEXT,
                model         TEXT    NOT NULL,
                created_at    INTEGER NOT NULL,
                updated_at    INTEGER NOT NULL DEFAULT 0,
                message_count INTEGER NOT NULL DEFAULT 0,
                meta          TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_session_records_update
                ON session_records(updated_at DESC);

            CREATE TABLE IF NOT EXISTS message_records (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id INTEGER NOT NULL REFERENCES session_records(id) ON DELETE CASCADE,
                role       TEXT    NOT NULL,
                content    TEXT,
                created_at INTEGER NOT NULL,
                sequence   INTEGER NOT NULL,
                meta       TEXT
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_message_records_seq_unique
                ON message_records(session_id, sequence);
        "#);
        while let Some(_) = stream.try_next().await? {}
        Ok(Self { connection: pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.connection
    }
}
