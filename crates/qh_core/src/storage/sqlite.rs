use std::{
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

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

/// 会话行：`(id, title, model, created_at, updated_at, message_count, meta)`
type SessionRow = (i64, String, String, i64, i64, i64, String);
/// 消息行：`(id, session_id, role, content, created_at, sequence, meta)`
type MessageRow = (i64, i64, String, String, i64, i64, String);

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

    /// 新建会话，返回数据库分配的 SessionId。
    pub async fn create_session(
        &self,
        title: &str,
        model: &ModelId,
    ) -> Result<SessionId, StorageError> {
        let now = now_seconds();
        let result = sqlx::query(
            "INSERT INTO session_records (title, model, created_at, updated_at, message_count, meta)
             VALUES (?1, ?2, ?3, ?3, 0, '')",
        )
        .bind(title)
        .bind(model.0.as_str())
        .bind(now)
        .execute(&self.connection)
        .await
        .map_err(DataBaseError::from)?;

        Ok(SessionId::from(result.last_insert_rowid()))
    }

    /// 列出所有会话（按更新时间倒序）。
    pub async fn list_sessions(&self) -> Result<Vec<SessionRecord>, StorageError> {
        let rows = sqlx::query_as::<_, SessionRow>(
            "SELECT id, title, model, created_at, updated_at, message_count, meta
             FROM session_records ORDER BY updated_at DESC",
        )
        .fetch_all(&self.connection)
        .await
        .map_err(DataBaseError::from)?;

        Ok(rows.into_iter().map(into_session_record).collect())
    }

    /// 删除会话（消息随外键级联删除）。
    pub async fn delete_session(&self, id: SessionId) -> Result<(), StorageError> {
        let result = sqlx::query("DELETE FROM session_records WHERE id = ?1")
            .bind(id.0)
            .execute(&self.connection)
            .await
            .map_err(DataBaseError::from)?;

        if result.rows_affected() == 0 {
            return Err(StorageError::SessionNotFound(id.0));
        }
        Ok(())
    }

    /// 追加一条消息，sequence 自动递增，并刷新会话更新时间与消息计数。
    pub async fn append_message(
        &self,
        session_id: SessionId,
        role: &Role,
        content: &str,
    ) -> Result<i64, StorageError> {
        let now = now_seconds();
        let mut tx = self.connection.begin().await.map_err(DataBaseError::from)?;

        let next_sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(sequence), 0) + 1 FROM message_records WHERE session_id = ?1",
        )
        .bind(session_id.0)
        .fetch_one(&mut *tx)
        .await
        .map_err(DataBaseError::from)?;

        let result = sqlx::query(
            "INSERT INTO message_records (session_id, role, content, created_at, sequence, meta)
             VALUES (?1, ?2, ?3, ?4, ?5, '')",
        )
        .bind(session_id.0)
        .bind(role_to_str(role))
        .bind(content)
        .bind(now)
        .bind(next_sequence)
        .execute(&mut *tx)
        .await
        .map_err(DataBaseError::from)?;

        sqlx::query(
            "UPDATE session_records
             SET updated_at = ?1, message_count = message_count + 1
             WHERE id = ?2",
        )
        .bind(now)
        .bind(session_id.0)
        .execute(&mut *tx)
        .await
        .map_err(DataBaseError::from)?;

        tx.commit().await.map_err(DataBaseError::from)?;
        Ok(result.last_insert_rowid())
    }

    /// 加载某会话的全部消息（按 sequence 升序）。
    pub async fn load_messages(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<MessageRecord>, StorageError> {
        let rows = sqlx::query_as::<_, MessageRow>(
            "SELECT id, session_id, role, COALESCE(content, ''), created_at, sequence,
                    COALESCE(meta, '')
             FROM message_records WHERE session_id = ?1 ORDER BY sequence ASC",
        )
        .bind(session_id.0)
        .fetch_all(&self.connection)
        .await
        .map_err(DataBaseError::from)?;

        Ok(rows.into_iter().map(into_message_record).collect())
    }
}

fn into_session_record(row: SessionRow) -> SessionRecord {
    let (id, title, model, created_at, updated_at, message_count, meta) = row;
    SessionRecord {
        id: SessionId::from(id),
        title,
        model: ModelId::from(model),
        created_at,
        updated_at,
        message_count,
        meta,
    }
}

fn into_message_record(row: MessageRow) -> MessageRecord {
    let (id, session_id, role, content, created_at, sequence, meta) = row;
    MessageRecord {
        id,
        session_id: SessionId::from(session_id),
        role: role_from_str(&role),
        content,
        created_at,
        sequence,
        meta,
    }
}

fn role_to_str(role: &Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

fn role_from_str(value: &str) -> Role {
    match value {
        "system" => Role::System,
        "assistant" => Role::Assistant,
        _ => Role::User,
    }
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn memory_store() -> SqliteStore {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect memory sqlite");
        SqliteStore::after(pool).await.expect("init schema")
    }

    #[tokio::test]
    async fn persists_and_reloads_session() {
        let store = memory_store().await;
        let model = ModelId::from("test-model");

        let id = store.create_session("hello", &model).await.unwrap();
        store.append_message(id, &Role::User, "hi").await.unwrap();
        store
            .append_message(id, &Role::Assistant, "hello")
            .await
            .unwrap();

        let messages = store.load_messages(id).await.unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[0].content, "hi");
        assert_eq!(messages[1].sequence, 2);
        assert_eq!(messages[1].role, Role::Assistant);

        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, id);
        assert_eq!(sessions[0].message_count, 2);
    }

    #[tokio::test]
    async fn deletes_session_and_messages() {
        let store = memory_store().await;
        let id = store
            .create_session("t", &ModelId::from("m"))
            .await
            .unwrap();
        store.append_message(id, &Role::User, "x").await.unwrap();

        store.delete_session(id).await.unwrap();

        assert!(store.list_sessions().await.unwrap().is_empty());
        assert!(store.load_messages(id).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn missing_session_is_reported() {
        let store = memory_store().await;
        let result = store.delete_session(SessionId::from(404)).await;
        assert!(matches!(result, Err(StorageError::SessionNotFound(404))));
    }
}
