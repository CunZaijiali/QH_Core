use thiserror::Error;

use crate::{capability::CapabilityError, config::ConfigError, ipc::IpcError};

/// Errors that can happen while constructing the runtime.
#[derive(Debug, Error)]
pub enum BootstrapError {
    #[error("environment load failed: {0}")]
    Environment(String), // TODO Environment Exception
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("event system initialization failed: {0}")]
    EventsSys(#[from] EventError),
    #[error("plugin system initialization failed: {0}")]
    PluginSys(#[from] PluginError),
    #[error("core component initialization failed: {0}")]
    Component(String),
    #[error("unexpected exception happened: {0}")]
    Other(String),
}

#[derive(Debug, Error)]
pub enum EventError {
    #[error("event over capacity")]
    OverCapacity,
    #[error("event error: {0}")]
    Other(String),
}

/// Errors emitted by an already running core.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("the root supervisor exited unexpectedly")]
    RootExited,
    #[error("the plugin manager exited unexpectedly")]
    PluginManagerExited,
    #[error("shutdown failed: {0}")]
    Shutdown(String),
    #[error(transparent)]
    Adapter(#[from] AdapterError),
    #[error(transparent)]
    Plugin(#[from] PluginError),
    #[error(transparent)]
    Session(#[from] SessionError),
}

/// Errors belonging to a single session.
#[derive(Debug, Error)]
pub enum SessionError {
    #[error("session was not found: {0}")]
    NotFound(String),
    #[error("session is already closed")]
    Closed,
    #[error("session is busy")]
    Busy,
    #[error("agent loop was cancelled")]
    Cancelled,
    #[error("agent loop failed: {0}")]
    AgentLoop(String),
    #[error(transparent)]
    Adapter(#[from] AdapterError),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// Errors crossing the plugin host boundary.
#[derive(Debug, Error)]
pub enum PluginError {
    #[error("plugin I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("plugin manifest TOML is invalid: {0}")]
    ManifestToml(#[from] toml::de::Error),
    #[error("invalid plugin manifest: {0}")]
    InvalidManifest(String),
    #[error("plugin process failed: {0}")]
    Process(String),
    #[error("plugin protocol failed: {0}")]
    Protocol(String),
    #[error("plugin request timed out: {0}")]
    Timeout(String),
    #[error("plugin capability was denied: {0}")]
    CapabilityDenied(String),
    #[error(transparent)]
    Ipc(#[from] IpcError),
}

/// Errors from an LLM adapter, independent of a particular provider.
#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("invalid adapter configuration: {0}")]
    InvalidConfig(String),
    #[error("adapter is not registered: {0}")]
    NotFound(String),
    #[error("no adapter is registered for model: {0}")]
    NoAdapterForModel(String),
    #[error("provider is unavailable: {0}")]
    ProviderUnavailable(String),
    #[error("model is not supported: {0}")]
    UnsupportedModel(String),
    #[error("message role is not supported: {0}")]
    UnsupportedMessageRole(String),
    #[error("adapter request failed: {0}")]
    Request(String),
    #[error("adapter stream failed: {0}")]
    Stream(String),
    #[error("adapter timed out: {0}")]
    Timeout(String),
    #[error("adapter request was cancelled")]
    Cancelled,
    #[error("adapter health check failed: {0}")]
    Unhealthy(String),
    #[error("adapter registry conflict: {0}")]
    RegistryConflict(String),
    #[error("adapter extension rejected the request: {0}")]
    ExtensionRejected(String),
    #[error("adapter extension failed: {0}")]
    Extension(String),
}

/// Errors from the authorization boundary.
#[derive(Debug, Error)]
pub enum PermissionError {
    #[error("capability denied")]
    Denied,
    #[error("capability signature is invalid")]
    InvalidSignature,
    #[error(transparent)]
    Evaluation(#[from] CapabilityError),
}

/// Errors from the sandbox boundary.
#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("no sandbox backend is available")]
    BackendUnavailable,
    #[error("sandbox initialization failed: {0}")]
    Initialization(String),
    #[error("sandbox rejected the operation: {0}")]
    Rejected(String),
    #[error("sandbox limit exceeded: {0}")]
    LimitExceeded(String),
    #[error("sandbox escape was detected")]
    EscapeDetected,
}

#[derive(Debug, Error)]
#[error("{0}")]
pub struct SecretKeyringError(
    #[from]
    #[source]
    keyring_core::Error,
);

#[derive(Debug, Error)]
#[error("{0}")]
pub struct DataBaseError(
    #[from]
    #[source]
    sqlx::Error,
);
fn classify_sqlite(db: &dyn sqlx::error::DatabaseError) -> DataBaseErrorKind {
    let code = db.code().unwrap_or_default();
    let msg = db.message();
    match code.as_ref() {
        "5" | "6" => DataBaseErrorKind::Busy,         // BUSY / LOCKED
        "8" | "14" => DataBaseErrorKind::Unavailable, // READONLY / CANTOPEN
        "10" | "11" | "13" => DataBaseErrorKind::Io,  // IOERR / CORRUPT / FULL
        "17" => DataBaseErrorKind::SchemaStable,      // SCHEMA
        "19" => {
            if msg.contains("UNIQUE") {
                DataBaseErrorKind::Conflict(ConflictKind::Unique)
            } else if (msg.contains("PRIMARY")) {
                DataBaseErrorKind::Conflict(ConflictKind::PrimaryKey)
            } else if (msg.contains("FOREIGN")) {
                DataBaseErrorKind::Conflict(ConflictKind::ForeignKey)
            } else if (msg.contains("NOT NULL")) {
                DataBaseErrorKind::Conflict(ConflictKind::NotNull)
            } else {
                DataBaseErrorKind::NotFound
            }
        }
        _ => DataBaseErrorKind::Other,
    }
}
impl DataBaseError {
    pub fn kind(&self) -> DataBaseErrorKind {
        use sqlx::Error::*;
        match &self.0 {
            PoolTimedOut | PoolClosed => DataBaseErrorKind::Unavailable,
            RowNotFound => DataBaseErrorKind::NotFound,
            Io(_) | Tls(_) => DataBaseErrorKind::Io,
            Database(db) => classify_sqlite(db.as_ref()),
            _ => DataBaseErrorKind::Other,
        }
    }
    pub fn is_retryable(&self) -> bool {
        self.kind().is_retryable()
    }
    pub fn raw(&self) -> &sqlx::Error {
        &self.0
    }
    pub fn into_parts(self) -> (DataBaseErrorKind, String) {
        let kind = self.kind();
        (kind, self.0.to_string())
    }
    pub fn wrap<T>(r: Result<T, sqlx::Error>) -> Result<T, Self> {
        r.map_err(Self::from)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataBaseErrorKind {
    Busy,
    Unavailable,
    NotFound,
    Io,
    SchemaStable,
    Conflict(ConflictKind),
    Other,
    Unique,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    Unique,
    PrimaryKey,
    ForeignKey,
    NotNull,
    Check,
}
impl DataBaseErrorKind {
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Busy | Self::SchemaStable)
    }
}

#[derive(Debug, Error)]
pub enum AuditError {
    #[error("database error: {0}")]
    DataBase(#[from] DataBaseError),
    #[error("审计事件序列化失败: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("audit is not enabled")]
    NotEnabled,
    #[error("audit connection is not initialized")]
    NotConnected,
}
impl From<sqlx::Error> for AuditError {
    fn from(e: sqlx::Error) -> Self {
        Self::DataBase(DataBaseError::from(e))
    }
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Database(#[from] DataBaseError),
    #[error("会话 {0} 不存在")]
    SessionNotFound(i64),
    #[error("会话 {session_id} 内 sequence {sequence} 已被占用")]
    SequenceConflict { session_id: u64, sequence: u64 },
}
impl From<sqlx::Error> for StorageError {
    fn from(e: sqlx::Error) -> Self {
        Self::Database(DataBaseError::from(e))
    }
}

#[derive(Debug, Error)]
pub enum SecurityError {
    #[error("authentication failed")]
    Authentication,
    #[error("secret key error")]
    KeyRing(#[from] SecretKeyringError),
}
impl From<keyring_core::Error> for SecurityError {
    fn from(e: keyring_core::Error) -> Self {
        Self::KeyRing(SecretKeyringError::from(e))
    }
}

/// Top-level error for callers that do not need to distinguish lifecycle layers.
#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Bootstrap(#[from] BootstrapError),
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Adapter(#[from] AdapterError),
    #[error(transparent)]
    Plugin(#[from] PluginError),
    #[error(transparent)]
    Permission(#[from] PermissionError),
    #[error(transparent)]
    Sandbox(#[from] SandboxError),
    #[error(transparent)]
    Ipc(#[from] IpcError),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Audit(#[from] AuditError),
    #[error(transparent)]
    Event(#[from] EventError),
    #[error(transparent)]
    Security(#[from] SecurityError),
}
