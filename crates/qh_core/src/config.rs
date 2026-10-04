use std::{
    collections::BTreeMap,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_with::{DurationSeconds, serde_as};
use directories::ProjectDirs;
use std::fs;

use crate::plugin::PluginKind;

const CONFIG_SCHEMA_VERSION: u32 = 1;
const QUALIFIER: &str = "com";
const ORGANISATION: &str = "cun-zai";
const APPLICATION: &str = "QH_Assistant";

fn default_data_dir() -> PathBuf {
    ProjectDirs::from(QUALIFIER, ORGANISATION, APPLICATION).unwrap().data_local_dir().to_path_buf()
}

fn default_workspace() -> PathBuf {
    ProjectDirs::from(QUALIFIER, ORGANISATION, APPLICATION).unwrap().data_local_dir().to_path_buf().join("default_workdir")
}

fn seconds(value: u64) -> Duration {
    Duration::from_secs(value)
}

fn default_shutdown_timeout() -> Duration {
    seconds(30)
}

fn default_turn_timeout() -> Duration {
    seconds(300)
}

fn default_tool_timeout() -> Duration {
    seconds(30)
}

fn default_http_timeout() -> Duration {
    seconds(30)
}

fn default_connection_timeout() -> Duration {
    seconds(10)
}

fn default_command_timeout() -> Duration {
    seconds(30)
}

fn default_plugin_start_timeout() -> Duration {
    seconds(10)
}

fn default_plugin_shutdown_timeout() -> Duration {
    seconds(10)
}

fn default_plugin_health_interval() -> Duration {
    seconds(10)
}

fn default_plugin_health_timeout() -> Duration {
    seconds(2)
}

fn default_extension_timeout() -> Duration {
    seconds(5)
}

/// The root configuration for the core runtime.
///
/// Defaults deliberately describe a standalone core. Plugins, external adapters and RPC are
/// optional capabilities and must not be required for bootstrap.
/// 
/// Minimal config.toml:
/// ```toml
/// [completion]
/// model="deepseek-flash"
/// base_url="https://api.deepseek.com"
/// api_key_env="DEEPSEEK_API_KEY"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CoreConfig {
    pub schema_version: u32,
    pub mode: RunMode,
    pub safe_mode: bool,
    pub data_dir: PathBuf,
    pub workspace: PathBuf,
    pub runtime: RuntimeConfig,
    pub storage: StorageConfig,
    pub http: HttpConfig,
    pub events: EventsConfig,
    pub logging: LoggingConfig,
    pub audit: AuditConfig,
    pub security: SecurityConfig,
    pub rpc: RpcConfig,
    pub adapters: Vec<AdapterConfig>,
    pub default_model: Option<String>,
    pub plugins: PluginsConfig,
    pub extensions: ExtensionsConfig,
}

/// Short name used by the bootstrap code and external callers.
pub type Config = CoreConfig;

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            mode: RunMode::default(),
            safe_mode: true,
            data_dir: default_data_dir(),
            workspace: default_workspace(),
            runtime: RuntimeConfig::default(),
            storage: StorageConfig::default(),
            http: HttpConfig::default(),
            events: EventsConfig::default(),
            logging: LoggingConfig::default(),
            audit: AuditConfig::default(),
            security: SecurityConfig::default(),
            rpc: RpcConfig::default(),
            adapters: Vec::new(),
            default_model: None,
            plugins: PluginsConfig::default(),
            extensions: ExtensionsConfig::default(),
        }
    }
}

impl CoreConfig {
    /// Parses a TOML document without touching the filesystem.
    pub fn from_toml(source: &str) -> Result<Self, ConfigError> {
        let config: Self = toml::from_str(source)?;
        config.validate()?;
        Ok(config)
    }

    /// Reads a TOML file, resolves relative paths against the file's directory and validates it.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let source = std::fs::read_to_string(path)?;
        let mut config = Self::from_toml(&source)?;
        let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
        config.resolve_paths(base_dir);
        config.validate()?;
        Ok(config)
    }

    /// Resolves all paths that belong to the core. Plugin-specific paths remain plugin-owned.
    pub fn resolve_paths(&mut self, base_dir: impl AsRef<Path>) {
        let base_dir = base_dir.as_ref();
        self.data_dir = resolve_path(base_dir, &self.data_dir);
        self.workspace = resolve_path(base_dir, &self.workspace);
        self.storage.path = resolve_path(&self.data_dir, &self.storage.path);
        self.audit.path = resolve_path(&self.data_dir, &self.audit.path);

        for directory in &mut self.plugins.directories {
            *directory = resolve_path(base_dir, directory);
        }

        if let RpcEndpoint::UnixSocket { path } = &mut self.rpc.endpoint {
            *path = resolve_path(base_dir, path);
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(ConfigError::InvalidValue(format!(
                "unsupported config schema version: {}",
                self.schema_version
            )));
        }
        if self.data_dir.as_os_str().is_empty() {
            return Err(ConfigError::InvalidValue(
                "data_dir must not be empty".into(),
            ));
        }
        if self.workspace.as_os_str().is_empty() {
            return Err(ConfigError::InvalidValue(
                "workspace must not be empty".into(),
            ));
        }

        self.runtime.validate()?;
        self.storage.validate()?;
        self.http.validate()?;
        self.events.validate()?;
        self.audit.validate()?;
        self.security.validate(self.safe_mode)?;
        self.rpc.validate(self.mode)?;
        self.plugins.validate(self.safe_mode)?;
        self.extensions.validate(self.plugins.allow_extensions)?;

        if let Some(model) = &self.default_model {
            if model.trim().is_empty() {
                return Err(ConfigError::InvalidValue(
                    "default_model must not be empty when specified".into(),
                ));
            }
        }

        let mut adapter_ids = std::collections::HashSet::new();
        for adapter in &self.adapters {
            adapter.validate()?;
            if !adapter_ids.insert(&adapter.id) {
                return Err(ConfigError::InvalidValue(format!(
                    "duplicate adapter id: {}",
                    adapter.id
                )));
            }
        }

        Ok(())
    }
}

fn resolve_path(base_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunMode {
    #[default]
    Desktop,
    Headless,
    OneShot,
    Test,
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RuntimeConfig {
    pub max_sessions: usize,
    pub max_turns_per_run: u32,
    pub max_tool_calls_per_run: u32,
    #[serde_as(as = "DurationSeconds<u64>")]
    pub default_turn_timeout: Duration,
    #[serde_as(as = "DurationSeconds<u64>")]
    pub default_tool_timeout: Duration,
    #[serde_as(as = "DurationSeconds<u64>")]
    pub shutdown_timeout: Duration,
    pub command_capacity: usize,
    pub worker_threads: Option<usize>,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            max_sessions: 64,
            max_turns_per_run: 32,
            max_tool_calls_per_run: 64,
            default_turn_timeout: default_turn_timeout(),
            default_tool_timeout: default_tool_timeout(),
            shutdown_timeout: default_shutdown_timeout(),
            command_capacity: 256,
            worker_threads: None,
        }
    }
}

impl RuntimeConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if self.max_sessions == 0
            || self.max_turns_per_run == 0
            || self.max_tool_calls_per_run == 0
            || self.command_capacity == 0
        {
            return Err(ConfigError::InvalidValue(
                "runtime limits and command_capacity must be greater than zero".into(),
            ));
        }
        if self.default_turn_timeout.is_zero()
            || self.default_tool_timeout.is_zero()
            || self.shutdown_timeout.is_zero()
        {
            return Err(ConfigError::InvalidValue(
                "runtime timeouts must be greater than zero".into(),
            ));
        }
        if self.worker_threads == Some(0) {
            return Err(ConfigError::InvalidValue(
                "worker_threads must be greater than zero when specified".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageBackend {
    Memory,
    Sqlite,
    Custom(String),
}

impl Default for StorageBackend {
    fn default() -> Self {
        Self::Sqlite
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StorageConfig {
    pub backend: StorageBackend,
    pub path: PathBuf,
    pub create_dirs: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            backend: StorageBackend::default(),
            path: PathBuf::from("state.db"),
            create_dirs: true,
        }
    }
}

impl StorageConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if matches!(&self.backend, StorageBackend::Sqlite) && self.path.as_os_str().is_empty() {
            return Err(ConfigError::InvalidValue(
                "storage.path must not be empty for sqlite storage".into(),
            ));
        }
        Ok(())
    }
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HttpConfig {
    #[serde_as(as = "DurationSeconds<u64>")]
    pub timeout: Duration,
    #[serde_as(as = "DurationSeconds<u64>")]
    pub connection_timeout: Duration,
    pub max_connections: usize,
    pub proxy: Option<String>,
    pub user_agent: String,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            timeout: default_http_timeout(),
            connection_timeout: default_connection_timeout(),
            max_connections: 32,
            proxy: None,
            user_agent: "qh-core/0.1".into(),
        }
    }
}

impl HttpConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if self.timeout.is_zero() || self.connection_timeout.is_zero() || self.max_connections == 0
        {
            return Err(ConfigError::InvalidValue(
                "http timeouts and max_connections must be greater than zero".into(),
            ));
        }
        if self.user_agent.trim().is_empty() {
            return Err(ConfigError::InvalidValue(
                "http.user_agent must not be empty".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlowConsumerPolicy {
    #[default]
    DropOldest,
    Disconnect,
    Block,
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EventsConfig {
    pub notification_capacity: usize,
    pub command_capacity: usize,
    pub stream_capacity: usize,
    pub broadcast_capacity: usize,
    #[serde_as(as = "DurationSeconds<u64>")]
    pub command_timeout: Duration,
    pub slow_consumer_policy: SlowConsumerPolicy,
}

impl Default for EventsConfig {
    fn default() -> Self {
        Self {
            notification_capacity: 256,
            command_capacity: 256,
            stream_capacity: 256,
            broadcast_capacity: 256,
            command_timeout: default_command_timeout(),
            slow_consumer_policy: SlowConsumerPolicy::default(),
        }
    }
}

impl EventsConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if self.notification_capacity == 0
            || self.command_capacity == 0
            || self.stream_capacity == 0
            || self.broadcast_capacity == 0
            || self.command_timeout.is_zero()
        {
            return Err(ConfigError::InvalidValue(
                "event capacities and command_timeout must be greater than zero".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Debug,
    #[default]
    Info,
    Warn,
    Error,
    Off,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogOutput {
    Stdout,
    Stderr,
    File { path: PathBuf },
}

impl Default for LogOutput {
    fn default() -> Self {
        Self::Stdout
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogFormat {
    #[default]
    Compact,
    Pretty,
    Json,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RotationPolicy {
    #[default]
    Never,
    Daily,
    SizeMb(u64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    pub level: LogLevel,
    pub outputs: Vec<LogOutput>,
    pub format: LogFormat,
    pub rotation: RotationPolicy,
    pub include_spans: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: LogLevel::default(),
            outputs: vec![LogOutput::default()],
            format: LogFormat::default(),
            rotation: RotationPolicy::default(),
            include_spans: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditLevel {
    Minimal,
    #[default]
    Standard,
    Strict,
    Forensic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditBackend {
    Sqlite,
    None,
    Custom(String),
}

impl Default for AuditBackend {
    fn default() -> Self {
        Self::Sqlite
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventKind {
    CapabilityDenied,
    SignatureInvalid,
    SandboxViolation,
    PluginCrash,
    ToolInvocation,
    LlmRequest,
    LlmResponse,
    PluginLifecycle,
    ExtensionDecision,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionPolicy {
    KeepForever,
    Days(u32),
    MaxRecords(u64),
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self::KeepForever
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AuditConfig {
    pub enabled: bool,
    pub level: AuditLevel,
    pub backend: AuditBackend,
    pub path: PathBuf,
    pub retention: RetentionPolicy,
    pub sync_write_events: Vec<AuditEventKind>,
    pub alert_events: Vec<AuditEventKind>,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            level: AuditLevel::default(),
            backend: AuditBackend::default(),
            path: PathBuf::from("audit.db"),
            retention: RetentionPolicy::default(),
            sync_write_events: vec![
                AuditEventKind::CapabilityDenied,
                AuditEventKind::SignatureInvalid,
                AuditEventKind::SandboxViolation,
                AuditEventKind::PluginCrash,
            ],
            alert_events: vec![
                AuditEventKind::CapabilityDenied,
                AuditEventKind::SignatureInvalid,
                AuditEventKind::SandboxViolation,
            ],
        }
    }
}

impl AuditConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if matches!(&self.backend, AuditBackend::Sqlite) && self.path.as_os_str().is_empty() {
            return Err(ConfigError::InvalidValue(
                "audit.path must not be empty for sqlite audit storage".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionModel {
    #[default]
    DenyByDefault,
    ExplicitAllow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretSource {
    Environment(String),
    File(PathBuf),
    Generated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SigningKeySource {
    Environment(String),
    File(PathBuf),
    Generated,
}

impl Default for SigningKeySource {
    fn default() -> Self {
        Self::Generated
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SecurityConfig {
    pub permission_model: PermissionModel,
    pub signing_key: SigningKeySource,
    pub require_signed_capabilities: bool,
    pub sandbox: SandboxPolicy,
    pub default_audit_level: AuditLevel,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            permission_model: PermissionModel::default(),
            signing_key: SigningKeySource::default(),
            require_signed_capabilities: true,
            sandbox: SandboxPolicy::default(),
            default_audit_level: AuditLevel::default(),
        }
    }
}

impl SecurityConfig {
    fn validate(&self, safe_mode: bool) -> Result<(), ConfigError> {
        if safe_mode && matches!(self.sandbox.default_kind, SandboxKind::None) {
            return Err(ConfigError::InvalidCombination(
                "safe_mode cannot use sandbox.default_kind = none".into(),
            ));
        }
        self.sandbox.validate()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxKind {
    Auto,
    None,
    Landlock,
    Wasm,
    Container,
    MicroVm,
    Custom,
}

impl Default for SandboxKind {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ResourceLimits {
    pub max_memory_bytes: Option<u64>,
    pub max_cpu_seconds: Option<u64>,
    pub max_output_bytes: u64,
    pub max_processes: u32,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_memory_bytes: Some(512 * 1024 * 1024),
            max_cpu_seconds: Some(300),
            max_output_bytes: 8 * 1024 * 1024,
            max_processes: 16,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EscapeDetectionPolicy {
    Disabled,
    #[default]
    Standard,
    Strict,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SandboxPolicy {
    pub default_kind: SandboxKind,
    pub verify_isolation: bool,
    pub require_for_plugins: bool,
    pub default_limits: ResourceLimits,
    pub escape_detection: EscapeDetectionPolicy,
}

impl Default for SandboxPolicy {
    fn default() -> Self {
        Self {
            default_kind: SandboxKind::default(),
            verify_isolation: true,
            require_for_plugins: true,
            default_limits: ResourceLimits::default(),
            escape_detection: EscapeDetectionPolicy::default(),
        }
    }
}

impl SandboxPolicy {
    fn validate(&self) -> Result<(), ConfigError> {
        if self.default_limits.max_output_bytes == 0 || self.default_limits.max_processes == 0 {
            return Err(ConfigError::InvalidValue(
                "sandbox output and process limits must be greater than zero".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum RpcEndpoint {
    Disabled,
    UnixSocket { path: PathBuf },
    NamedPipe { name: String },
    Tcp { addr: SocketAddr },
}

impl Default for RpcEndpoint {
    fn default() -> Self {
        Self::Disabled
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum RpcAuth {
    None,
    Token { source: SecretSource },
    FilePermission,
}

impl Default for RpcAuth {
    fn default() -> Self {
        Self::FilePermission
    }
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RpcConfig {
    pub enabled: bool,
    pub endpoint: RpcEndpoint,
    pub auth: RpcAuth,
    #[serde_as(as = "DurationSeconds<u64>")]
    pub request_timeout: Duration,
    pub max_concurrent: usize,
    pub read_only: bool,
}

impl Default for RpcConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: RpcEndpoint::default(),
            auth: RpcAuth::default(),
            request_timeout: default_command_timeout(),
            max_concurrent: 64,
            read_only: false,
        }
    }
}

impl RpcConfig {
    fn validate(&self, mode: RunMode) -> Result<(), ConfigError> {
        if self.enabled && matches!(&self.endpoint, RpcEndpoint::Disabled) {
            return Err(ConfigError::InvalidCombination(
                "rpc.enabled requires a non-disabled endpoint".into(),
            ));
        }
        if self.enabled && self.request_timeout.is_zero()
            || self.enabled && self.max_concurrent == 0
        {
            return Err(ConfigError::InvalidValue(
                "enabled RPC requires a positive timeout and max_concurrent".into(),
            ));
        }
        if self.enabled
            && matches!(&self.endpoint, RpcEndpoint::Tcp { .. })
            && matches!(&self.auth, RpcAuth::None)
            && !matches!(mode, RunMode::Test)
        {
            return Err(ConfigError::InvalidCombination(
                "TCP RPC requires authentication outside test mode".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterProvider {
    OpenAiCompatible,
}

impl Default for AdapterProvider {
    fn default() -> Self {
        Self::OpenAiCompatible
    }
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AdapterConfig {
    pub id: String,
    pub provider: AdapterProvider,
    pub enabled: bool,
    pub models: Vec<String>,
    pub base_url: Option<String>,
    pub api_key_source: Option<SecretSource>,
    #[serde_as(as = "DurationSeconds<u64>")]
    pub timeout: Duration,
    pub max_retries: u32,
    pub extra: BTreeMap<String, toml::Value>,
}

impl Default for AdapterConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            provider: AdapterProvider::default(),
            enabled: true,
            models: Vec::new(),
            base_url: None,
            api_key_source: None,
            timeout: default_http_timeout(),
            max_retries: 2,
            extra: BTreeMap::new(),
        }
    }
}

impl AdapterConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if self.id.trim().is_empty() {
            return Err(ConfigError::InvalidValue(
                "adapter.id must not be empty".into(),
            ));
        }
        if self.enabled && self.models.is_empty() {
            return Err(ConfigError::InvalidValue(format!(
                "enabled adapter {} must declare at least one model",
                self.id
            )));
        }
        if self.timeout.is_zero() {
            return Err(ConfigError::InvalidValue(format!(
                "adapter {} timeout must be greater than zero",
                self.id
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoStartPolicy {
    #[default]
    Disabled,
    Enabled,
    Required,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestartPolicy {
    Never,
    #[default]
    OnFailure,
    Always,
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginLifecycleConfig {
    #[serde_as(as = "DurationSeconds<u64>")]
    pub start_timeout: Duration,
    #[serde_as(as = "DurationSeconds<u64>")]
    pub shutdown_timeout: Duration,
    pub restart_policy: RestartPolicy,
    pub max_restarts: u32,
}

impl Default for PluginLifecycleConfig {
    fn default() -> Self {
        Self {
            start_timeout: default_plugin_start_timeout(),
            shutdown_timeout: default_plugin_shutdown_timeout(),
            restart_policy: RestartPolicy::default(),
            max_restarts: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GrantPolicy {
    pub auto_grant_low: bool,
    pub confirm_medium: bool,
    pub confirm_high_every_time: bool,
}

impl Default for GrantPolicy {
    fn default() -> Self {
        Self {
            auto_grant_low: false,
            confirm_medium: true,
            confirm_high_every_time: true,
        }
    }
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HealthCheckConfig {
    pub enabled: bool,
    #[serde_as(as = "DurationSeconds<u64>")]
    pub interval: Duration,
    #[serde_as(as = "DurationSeconds<u64>")]
    pub timeout: Duration,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval: default_plugin_health_interval(),
            timeout: default_plugin_health_timeout(),
        }
    }
}

/// 显式登记的单个插件条目（v1 不扫目录，逐个列出）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginEntryConfig {
    /// 插件唯一标识。
    pub id: String,
    /// 插件分类。
    pub kind: PluginKind,
    /// 入口：Native 为注册表标识，Sidecar 为目录或入口路径。
    pub entry: String,
    /// 是否随 Core 启动预热（默认懒加载）。
    pub auto_start: bool,
    /// 是否启用该条目。
    pub enabled: bool,
}

impl Default for PluginEntryConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            kind: PluginKind::default(),
            entry: String::new(),
            auto_start: false,
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginsConfig {
    pub enabled: bool,
    pub directories: Vec<PathBuf>,
    /// 显式插件列表。
    pub entries: Vec<PluginEntryConfig>,
    pub auto_start: AutoStartPolicy,
    pub lifecycle: PluginLifecycleConfig,
    pub grant_policy: GrantPolicy,
    pub allow_third_party: bool,
    pub allow_extensions: bool,
    pub health_check: HealthCheckConfig,
}

impl Default for PluginsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            directories: Vec::new(),
            entries: Vec::new(),
            auto_start: AutoStartPolicy::default(),
            lifecycle: PluginLifecycleConfig::default(),
            grant_policy: GrantPolicy::default(),
            allow_third_party: false,
            allow_extensions: false,
            health_check: HealthCheckConfig::default(),
        }
    }
}

impl PluginsConfig {
    fn validate(&self, safe_mode: bool) -> Result<(), ConfigError> {
        let mut ids = std::collections::HashSet::new();
        for entry in &self.entries {
            if entry.id.trim().is_empty() || entry.entry.trim().is_empty() {
                return Err(ConfigError::InvalidValue(
                    "plugin entry id and entry must not be empty".into(),
                ));
            }
            if !ids.insert(&entry.id) {
                return Err(ConfigError::InvalidValue(format!(
                    "duplicate plugin entry id: {}",
                    entry.id
                )));
            }
        }
        if self.enabled
            && matches!(self.auto_start, AutoStartPolicy::Required)
            && self.directories.is_empty()
        {
            return Err(ConfigError::InvalidCombination(
                "required plugin autostart needs at least one plugin directory".into(),
            ));
        }
        if safe_mode && self.allow_third_party && !self.enabled {
            return Err(ConfigError::InvalidCombination(
                "allow_third_party requires plugins.enabled".into(),
            ));
        }
        if self.lifecycle.start_timeout.is_zero()
            || self.lifecycle.shutdown_timeout.is_zero()
            || self.health_check.interval.is_zero()
            || self.health_check.timeout.is_zero()
        {
            return Err(ConfigError::InvalidValue(
                "plugin lifecycle and health-check timeouts must be greater than zero".into(),
            ));
        }
        Ok(())
    }
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExtensionsConfig {
    pub context_compressor: ExtensionBinding<ContextCompressorKind>,
    pub tool_selector: ExtensionBinding<ToolSelectorKind>,
    pub loop_decision: ExtensionBinding<LoopDecisionKind>,
    pub prompt_builder: ExtensionBinding<PromptBuilderKind>,
    pub allow_plugin_override: bool,
    #[serde_as(as = "DurationSeconds<u64>")]
    pub invocation_timeout: Duration,
    pub fallback_to_builtin: bool,
}

impl Default for ExtensionsConfig {
    fn default() -> Self {
        Self {
            context_compressor: ExtensionBinding::new(ContextCompressorKind::SlidingWindow),
            tool_selector: ExtensionBinding::new(ToolSelectorKind::Default),
            loop_decision: ExtensionBinding::new(LoopDecisionKind::BudgetAndCompletion),
            prompt_builder: ExtensionBinding::new(PromptBuilderKind::Default),
            allow_plugin_override: false,
            invocation_timeout: default_extension_timeout(),
            fallback_to_builtin: true,
        }
    }
}

impl ExtensionsConfig {
    fn validate(&self, plugins_allow_extensions: bool) -> Result<(), ConfigError> {
        if self.invocation_timeout.is_zero() {
            return Err(ConfigError::InvalidValue(
                "extension invocation_timeout must be greater than zero".into(),
            ));
        }
        if self.allow_plugin_override && !plugins_allow_extensions {
            return Err(ConfigError::InvalidCombination(
                "extensions.allow_plugin_override requires plugins.allow_extensions".into(),
            ));
        }
        if self.allow_plugin_override && !self.fallback_to_builtin {
            return Err(ConfigError::InvalidCombination(
                "plugin extension overrides must keep builtin fallback enabled".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionBinding<K> {
    pub default: K,
    pub override_priority: i32,
    pub overridable: bool,
}

impl<K> ExtensionBinding<K> {
    pub fn new(default: K) -> Self {
        Self {
            default,
            override_priority: 0,
            overridable: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextCompressorKind {
    SlidingWindow,
    Noop,
    Plugin(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSelectorKind {
    Default,
    Plugin(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopDecisionKind {
    BudgetAndCompletion,
    Plugin(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptBuilderKind {
    Default,
    Plugin(String),
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read configuration: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse TOML configuration: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("invalid configuration value: {0}")]
    InvalidValue(String),
    #[error("invalid configuration combination: {0}")]
    InvalidCombination(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_standalone() {
        let config = CoreConfig::default();

        assert!(config.validate().is_ok());
        assert!(!config.plugins.enabled);
        assert!(!config.plugins.allow_extensions);
        assert!(config.adapters.is_empty());
        assert!(!config.rpc.enabled);
        assert!(config.extensions.fallback_to_builtin);
    }

    #[test]
    fn parses_partial_toml_and_keeps_core_defaults() {
        let config = CoreConfig::from_toml(
            r#"
                safe_mode = true
                workspace = "./workspace"

                [runtime]
                max_sessions = 8

                [plugins]
                enabled = true
                directories = ["./plugins"]
                allow_extensions = true

                [extensions]
                allow_plugin_override = true
            "#,
        )
        .unwrap();

        assert_eq!(config.runtime.max_sessions, 8);
        assert_eq!(config.runtime.max_turns_per_run, 32);
        assert_eq!(config.workspace, PathBuf::from("./workspace"));
        assert!(config.plugins.enabled);
        assert!(config.extensions.allow_plugin_override);
    }

    #[test]
    fn parses_duration_seconds_from_toml() {
        let config = CoreConfig::from_toml(
            r#"
                [runtime]
                shutdown_timeout = 12

                [http]
                timeout = 45
            "#,
        )
        .unwrap();

        assert_eq!(config.runtime.shutdown_timeout, Duration::from_secs(12));
        assert_eq!(config.http.timeout, Duration::from_secs(45));
    }

    #[test]
    fn loads_toml_file_and_resolves_relative_paths() {
        let directory = std::env::temp_dir().join(format!("qh-config-test-{}", std::process::id()));
        let path = directory.join("config.toml");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(&path, "data_dir = \"data\"\nworkspace = \"workspace\"\n").unwrap();

        let config = CoreConfig::load(&path).unwrap();

        assert_eq!(config.data_dir, directory.join("data"));
        assert_eq!(config.workspace, directory.join("workspace"));
        assert_eq!(config.storage.path, directory.join("data/state.db"));
        assert_eq!(config.audit.path, directory.join("data/audit.db"));

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn resolves_paths_against_config_directory() {
        let mut config = CoreConfig {
            data_dir: PathBuf::from(".qh-assistant"),
            workspace: PathBuf::from("."),
            ..CoreConfig::default()
        };
        config.resolve_paths("/tmp/qh-config");

        assert_eq!(
            config.data_dir,
            PathBuf::from("/tmp/qh-config/.qh-assistant")
        );
        assert_eq!(config.workspace, PathBuf::from("/tmp/qh-config"));
        assert_eq!(
            config.storage.path,
            PathBuf::from("/tmp/qh-config/.qh-assistant/state.db")
        );
        assert_eq!(
            config.audit.path,
            PathBuf::from("/tmp/qh-config/.qh-assistant/audit.db")
        );
    }

    #[test]
    fn rejects_unsafe_configuration_combinations() {
        let mut config = CoreConfig::default();
        config.security.sandbox.default_kind = SandboxKind::None;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidCombination(_))
        ));

        let mut config = CoreConfig::default();
        config.extensions.allow_plugin_override = true;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidCombination(_))
        ));
    }

    #[test]
    fn rejects_enabled_adapter_without_models() {
        let mut config = CoreConfig::default();
        config.adapters.push(AdapterConfig {
            id: "default".into(),
            ..AdapterConfig::default()
        });

        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidValue(message)) if message.contains("at least one model")
        ));
    }
}
