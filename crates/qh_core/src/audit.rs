use crate::error::{AuditError, DataBaseError};
use serde::{Deserialize, Serialize};
use serde_json;
use sha2::{Digest, Sha256};
use sqlx::{query_as, query_scalar, sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous}, Executor, FromRow, QueryBuilder, SqlitePool};
use futures_util::TryStreamExt;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use hex;

/// 哈希链起点;
const GENESIS_HASH: &str = "0";
/// 当前落库格式版本;
const SCHEMA_VERSION: i64 = 1;
/// 默认条目哈希算法标识;
const DEFAULT_HASH_ALGO: &str = "sha256-json-v1";
/// SELECT 列表，与 `AuditRecord` 的字段顺序保持一致（derive(FromRow) 按名字取值。
const AUDIT_COLUMNS: &str = "id, created_at, actor, session_id, plugin_id, user_id, target, action, \
     parameters, outcome, error, duration_ms, trace_id, parent_span_id, category, level, ip, \
     tenant_id, policy_id, prev_hash, hash, schema_version, entry_hash_algo, \
     chain_anchor, anchor_signed, key_id";


/// actor + action + target + timestamp + outcome  parameters + trace_id + duration_ms
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct AuditRecord {
    pub id: i64,
    /// unix 时间戳
    pub created_at: i64,
    pub actor: String,
    pub session_id: Option<i64>,
    pub plugin_id: Option<i64>,
    pub user_id: Option<i64>,
    pub target: String,
    pub action: String,
    pub parameters: String,
    pub outcome: String,
    pub error: Option<String>,
    pub duration_ms: i64,
    pub trace_id: String,

    pub level: String, // debug / info / security
    pub parent_span_id: i64,
    pub category: String, // auth / data / config / admin
    pub ip: String,
    pub tenant_id: String,
    pub policy_id: String,

    pub prev_hash: String,
    pub hash: String,
    pub schema_version: i64,
    pub entry_hash_algo: String,
    /// 锚点填写
    pub chain_anchor: Option<String>,
    /// 锚点填写
    pub anchor_signed: Option<String>,
    /// 锚点填写
    pub key_id: Option<String>, // 签名/锚点用哪个密钥
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewAuditRecord {
    pub actor: String,
    pub session_id: Option<i64>,
    pub plugin_id: Option<i64>,
    pub user_id: Option<i64>,
    pub target: String,
    pub action: String,
    pub parameters: String,
    pub outcome: String,
    pub error: Option<String>,
    pub duration_ms: i64,
    pub trace_id: String,
    pub parent_span_id: i64,
    pub category: String,
    pub level: String,
    pub ip: String,
    pub tenant_id: String,
    pub policy_id: String,
    pub schema_version: i64,
    pub entry_hash_algo: String,
    pub chain_anchor: Option<String>,
    pub anchor_signed: Option<String>,
    pub key_id: Option<String>,
}

impl NewAuditRecord {
    /// 用默认 schema 版本与哈希算法构造，其余字段显式给。
    pub fn new(
        actor: impl Into<String>,
        target: impl Into<String>,
        action: impl Into<String>,
        outcome: impl Into<String>,
        trace_id: impl Into<String>,
    ) -> Self {
        Self {
            actor: actor.into(),
            session_id: None,
            plugin_id: None,
            user_id: None,
            target: target.into(),
            action: action.into(),
            parameters: "{}".to_owned(),
            outcome: outcome.into(),
            error: None,
            duration_ms: 0,
            trace_id: trace_id.into(),
            parent_span_id: 0,
            category: "data".to_owned(),
            level: "info".to_owned(),
            ip: String::new(),
            tenant_id: String::new(),
            policy_id: String::new(),
            schema_version: SCHEMA_VERSION,
            entry_hash_algo: DEFAULT_HASH_ALGO.to_owned(),
            chain_anchor: None,
            anchor_signed: None,
            key_id: None,
        }
    }
}

pub struct AuditStore {
    enabled: bool,
    connection: Option<SqlitePool>,
}

impl AuditStore {
    /// 审计关闭时不建库、不落盘，只留一个 no-op store。
    pub async fn new(enabled: bool, path: impl AsRef<Path>) -> Result<Self, AuditError> {
        if enabled {
            Ok(Self::open(path).await?)
        } else {
            Ok(Self::disabled())
        }
    }

    pub fn disabled() -> Self {
        Self {
            enabled: false,
            connection: None,
        }
    }

    // TODO 滚动策略， 验证入口， 完整性检查， 只写保护。
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, DataBaseError> {
        let opt = SqliteConnectOptions::new()
            .journal_mode(SqliteJournalMode::Wal)
            .page_size(4096)
            .create_if_missing(true)
            .filename(path.as_ref())
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5))
            .foreign_keys(false)
            .pragma("trusted_schema", "0")
            .pragma("secure_delete", "ON");
        let pool = SqlitePoolOptions::new() // TODO 单连接串行
            .max_connections(1)
            .min_connections(1)
            .max_lifetime(Duration::from_hours(1))
            .acquire_timeout(Duration::from_secs(10))
            .connect_with(opt)
            .await?;
        Self::after(pool).await
    }
    pub async fn after(pool: SqlitePool) -> Result<Self, DataBaseError> {
        let mut stream = pool.execute_many(
            r#"
            CREATE TABLE IF NOT EXISTS audit_records (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at      INTEGER NOT NULL,
                actor           TEXT    NOT NULL,
                session_id      INTEGER,
                plugin_id       INTEGER,
                user_id         INTEGER,
                target          TEXT    NOT NULL,
                action          TEXT    NOT NULL,
                parameters      TEXT    NOT NULL,
                outcome         TEXT    NOT NULL,
                error           TEXT,
                duration_ms     INTEGER NOT NULL,
                trace_id        TEXT    NOT NULL,
                parent_span_id  INTEGER NOT NULL DEFAULT 0,
                category        TEXT    NOT NULL,
                level           TEXT    NOT NULL,
                ip              TEXT    NOT NULL DEFAULT '',
                tenant_id       TEXT    NOT NULL DEFAULT '',
                policy_id       TEXT    NOT NULL DEFAULT '',
                schema_version  INTEGER NOT NULL DEFAULT 1,
                entry_hash_algo TEXT    NOT NULL,
                prev_hash       TEXT    NOT NULL,
                hash            TEXT    NOT NULL,
                chain_anchor    TEXT,
                anchor_signed   TEXT,
                key_id          TEXT
            ) STRICT;

            CREATE INDEX IF NOT EXISTS idx_audit_trace_id   ON audit_records(trace_id);
            CREATE INDEX IF NOT EXISTS idx_audit_created_at ON audit_records(created_at);
            CREATE INDEX IF NOT EXISTS idx_audit_session    ON audit_records(session_id, created_at);
            CREATE INDEX IF NOT EXISTS idx_audit_actor      ON audit_records(actor, created_at);
            CREATE INDEX IF NOT EXISTS idx_audit_level      ON audit_records(level, created_at);


            CREATE TRIGGER IF NOT EXISTS audit_no_update
            BEFORE UPDATE ON audit_records
            BEGIN SELECT RAISE(ABORT, 'audit records are append-only'); END;

            CREATE TRIGGER IF NOT EXISTS audit_no_delete
            BEFORE DELETE ON audit_records
            BEGIN SELECT RAISE(ABORT, 'audit records are append-only'); END;
        "#,
        );
        while let Some(_) = stream.try_next().await? {}
        Ok(Self {
            enabled: true,
            connection: Some(pool),
        })
    }
    fn pool(&self) -> Result<&SqlitePool, AuditError> {
        if !self.enabled {
            return Err(AuditError::NotEnabled);
        }
        self.connection.as_ref().ok_or_else(|| AuditError::NotConnected)
    }
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// 追加一条审计记录并返回落库后的完整记录。
    ///
    /// 「读链尾 → 算哈希 → 插入」在同一事务内完成，避免链分叉。
    pub async fn record(
        &self,
        input: NewAuditRecord,
    ) -> Result<Option<AuditRecord>, AuditError> {
        if !self.enabled {
            tracing::debug!(actor = %input.actor, action = %input.action, "audit disabled");
            return Ok(None);
        }
        let pool = self.pool()?;
        let mut tx = pool.begin().await.map_err(AuditError::from)?;

        // 链尾：按 id（= 审计序号）取，不用 created_at，避免同毫秒歧义。
        let previous: Option<(i64, String)> =
            query_as(r#"SELECT id, hash FROM audit_records ORDER BY id DESC LIMIT 1"#)
                .fetch_optional(&mut *tx)
                .await?;

        let prev_hash = previous
            .map(|(_, hash)| hash)
            .unwrap_or_else(|| GENESIS_HASH.to_owned());
        let created_at = now_unix_millis();
        let hash = calculate_hash(created_at, &input, &prev_hash)?;

        let row = query_scalar::<_, i64>(
            r#"INSERT INTO audit_records (
                   created_at, actor, session_id, plugin_id, user_id, target, action,
                   parameters, outcome, error, duration_ms, trace_id, parent_span_id,
                   category, level, ip, tenant_id, policy_id,
                   schema_version, entry_hash_algo, prev_hash, hash,
                   chain_anchor, anchor_signed, key_id
               ) VALUES (
                   ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                   ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25
               )
               RETURNING id"#,
        )
        .bind(created_at)
        .bind(&input.actor)
        .bind(input.session_id)
        .bind(input.plugin_id)
        .bind(input.user_id)
        .bind(&input.target)
        .bind(&input.action)
        .bind(&input.parameters)
        .bind(&input.outcome)
        .bind(&input.error)
        .bind(input.duration_ms)
        .bind(&input.trace_id)
        .bind(input.parent_span_id)
        .bind(&input.category)
        .bind(&input.level)
        .bind(&input.ip)
        .bind(&input.tenant_id)
        .bind(&input.policy_id)
        .bind(input.schema_version)
        .bind(&input.entry_hash_algo)
        .bind(&prev_hash)
        .bind(&hash)
        .bind(&input.chain_anchor)
        .bind(&input.anchor_signed)
        .bind(&input.key_id)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(Some(AuditRecord {
            id: row,
            created_at,
            actor: input.actor,
            session_id: input.session_id,
            plugin_id: input.plugin_id,
            user_id: input.user_id,
            target: input.target,
            action: input.action,
            parameters: input.parameters,
            outcome: input.outcome,
            error: input.error,
            duration_ms: input.duration_ms,
            trace_id: input.trace_id,
            parent_span_id: input.parent_span_id,
            category: input.category,
            level: input.level,
            ip: input.ip,
            tenant_id: input.tenant_id,
            policy_id: input.policy_id,
            prev_hash,
            hash,
            schema_version: input.schema_version,
            entry_hash_algo: input.entry_hash_algo,
            chain_anchor: input.chain_anchor,
            anchor_signed: input.anchor_signed,
            key_id: input.key_id,
        }))
    }

    pub async fn get(&self, id: i64) -> Result<Option<AuditRecord>, AuditError> {
        if !self.enabled {
            return Ok(None);
        }
        let pool = self.pool()?;
        let record = QueryBuilder::new("SELECT ")
            .push(AUDIT_COLUMNS)
            .push(" FROM audit_records WHERE id = ")
            .push_bind(id)
            .build_query_as::<AuditRecord>()
            .fetch_optional(pool)
            .await?;
        Ok(record)
    }

    pub async fn list_by_session(&self, session_id: i64,limit: i64)->Result<Vec<AuditRecord>, AuditError> {
        if !self.enabled{return Ok(Vec::new());}
        let pool = self.pool()?;
        let rows = QueryBuilder::new("SELECT ")
            .push(AUDIT_COLUMNS)
            .push(" FROM audit_records WHERE session_id = ")
            .push_bind(session_id)
            .push(" LIMIT ")
            .push_bind(limit)
            .build_query_as::<AuditRecord>()
            .fetch_all(pool)
            .await?;
        Ok(rows)
    }

    pub async fn chain_head(&self)->Result<Option<(i64,String)>, AuditError>{
        if !self.enabled { return Ok(None); }
        let pool = self.pool()?;
        let head = query_as::<_,(i64, String)>("SELECT id, hash FROM audit_records ORDER BY id DESC LIMIT 1")
            .fetch_optional(pool)
            .await?;
        Ok(head)
    }

    /// 验证哈希链
    /// 返回 false 表示哈希链异常
    pub async fn verify_chain(&self) -> Result<bool, AuditError> {
       if !self.enabled {
            return Ok(true);
        }
        let pool = self.pool()?;
        // 审计表按追加写、量级可控
        let records = QueryBuilder::new("SELECT ")
            .push(AUDIT_COLUMNS)
            .push(" FROM audit_records ORDER BY id ASC")
            .build_query_as::<AuditRecord>()
            .fetch_all(pool)
        .await?;

        let mut previous = GENESIS_HASH.to_owned();
        for record in &records {
            let expected = calculate_hash(record.created_at, &NewAuditRecord::from(record), &record.prev_hash)?;
            if record.prev_hash != previous || record.hash != expected {
                return Ok(false);
            }
            previous = record.hash.clone();
        }
        Ok(true)
    }
}


impl From<&AuditRecord> for NewAuditRecord {
    fn from(record: &AuditRecord) -> Self {
        Self {
            actor: record.actor.clone(),
            session_id: record.session_id,
            plugin_id: record.plugin_id,
            user_id: record.user_id,
            target: record.target.clone(),
            action: record.action.clone(),
            parameters: record.parameters.clone(),
            outcome: record.outcome.clone(),
            error: record.error.clone(),
            duration_ms: record.duration_ms,
            trace_id: record.trace_id.clone(),
            parent_span_id: record.parent_span_id,
            category: record.category.clone(),
            level: record.level.clone(),
            ip: record.ip.clone(),
            tenant_id: record.tenant_id.clone(),
            policy_id: record.policy_id.clone(),
            schema_version: record.schema_version,
            entry_hash_algo: record.entry_hash_algo.clone(),
            chain_anchor: record.chain_anchor.clone(),
            anchor_signed: record.anchor_signed.clone(),
            key_id: record.key_id.clone(),
        }
    }
}

/// 计算条目哈希。
///
/// payload 用元组而非结构体：字段顺序在编译期固定，不依赖 JSON key 顺序；
/// prev_hash 放在最后，让链关系直接进入哈希输入。
fn calculate_hash(
    created_at: i64,
    input: &NewAuditRecord,
    prev_hash: &str,
) -> Result<String, AuditError> {
    let payload = serde_json::to_vec(&(input, created_at, prev_hash))?;
    Ok(hex::encode(Sha256::digest(payload)))
}

fn now_unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|t| t.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> NewAuditRecord {
        NewAuditRecord::new("core", "tool", "invoke", "success", "trace")
    }
    async fn memory_store()-> AuditStore{
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect memory sqlite");
        AuditStore::after(pool).await.expect("init schema")
    }
   #[tokio::test]
    async fn tampering_breaks_chain() {
        let store = memory_store().await;
        store.record(input()).await.unwrap();
        store.record(input()).await.unwrap();

        // 绕过触发器：直接改字节。触发器存在时 UPDATE 会 ABORT，所以先删触发器。
        let pool = store.pool().unwrap();
        sqlx::query("DROP TRIGGER audit_no_update")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("UPDATE audit_records SET outcome = 'tampered' WHERE id = 1")
            .execute(pool)
            .await
            .unwrap();

        assert!(!store.verify_chain().await.unwrap());
    }

    #[tokio::test]
    async fn append_only_triggers_block_update_and_delete() {
        let store = memory_store().await;
        store.record(input()).await.unwrap();
        let pool = store.pool().unwrap();

        assert!(sqlx::query("UPDATE audit_records SET outcome = 'x'")
            .execute(pool)
            .await
            .is_err());
        assert!(sqlx::query("DELETE FROM audit_records").execute(pool).await.is_err());
    }

    #[tokio::test]
    async fn disabled_store_is_noop() {
        let store = AuditStore::disabled();
        assert!(!store.is_enabled());
        assert_eq!(store.record(input()).await.unwrap(), None);
        assert!(store.verify_chain().await.unwrap());
        assert!(store.get(1).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn disabled_constructor_does_not_create_database() {
        let path = std::env::temp_dir().join(format!("qh-audit-disabled-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let store = AuditStore::new(false, &path).await.unwrap();
        assert!(!store.is_enabled());
        assert!(!path.exists());
    }
}
