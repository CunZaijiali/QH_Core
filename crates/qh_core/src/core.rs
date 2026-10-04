pub mod handle;
pub mod shutdown;

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::config::CoreConfig;
use crate::domain::message::{CompletionRequest, CompletionResponse, Message, Role};
use crate::error::{AdapterError, BootstrapError, CoreError};
use crate::event::EventService;
use crate::http::HttpClient;
use crate::llm::openai::OpenAiCompatibleAdapter;
use crate::llm::CompletionAdapter;
use crate::logging::Logger;
use crate::security::AuditService;
use crate::storage::sqlite::SqliteStore;

/// qh_core 运行时。
///
/// 当前落地 Phase 1-4（基础设施 / 事件 / 核心服务 / 模型适配器），
/// Phase 5-7（插件 / 会话 / handle）逐步补齐。
pub struct AgentCore {
    config: Arc<CoreConfig>,
    logger: Logger,
    store: Arc<SqliteStore>,
    events: Arc<EventService>,
    audit: Arc<AuditService>,
    http: Arc<HttpClient>,
    adapter: Option<Arc<dyn CompletionAdapter>>,
    shutdown: CancellationToken,
}

impl AgentCore {
    /// 构建运行时
    pub async fn bootstrap(config: CoreConfig) -> Result<Self, BootstrapError> {
        // TODO 0-7 启动流程
        // TODO 降级策略

        // Phase 1: 基础设施
        let logger = Logger::new();
        let config = Arc::new(config);
        let store = Arc::new(SqliteStore::new(&config.storage.path).await?);
        let http = Arc::new(HttpClient::new(&config.http)?);

        // Phase 2: 事件系统
        let events = Arc::new(EventService::new(&config.events));

        // Phase 3: 核心服务
        let audit = Arc::new(AuditService::new(&config.audit).await?);

        // Phase 4: 模型适配器
        let adapter: Option<Arc<dyn CompletionAdapter>> = config
            .adapters
            .iter()
            .find(|a| a.enabled)
            .map(|cfg| OpenAiCompatibleAdapter::from_config(cfg, http.clone()))
            .transpose()?
            .map(|a| Arc::new(a) as Arc<dyn CompletionAdapter>);

        // Phase 5: 插件系统
        // TODO plugins.enabled=false 时跳过

        // Phase 6: 会话与上下文
        // TODO RootSupervisor (ractor)

        // Phase 7: 组装 handle
        // TODO AgentCoreHandle

        Ok(Self {
            config,
            logger,
            store,
            events,
            audit,
            http,
            adapter,
            shutdown: CancellationToken::new(),
        })
    }

    /// 用默认模型发送一条用户消息，返回补全结果。
    pub async fn complete(&self, content: impl Into<String>) -> Result<CompletionResponse, CoreError> {
        let adapter = self.adapter.as_ref().ok_or_else(|| {
            CoreError::Adapter(AdapterError::NoAdapterForModel(
                "no enabled adapter configured".into(),
            ))
        })?;
        let request = CompletionRequest {
            model: adapter.model().clone(),
            messages: vec![Message::new(Role::User, content)],
            temperature: None,
            max_tokens: None,
        };
        Ok(adapter.complete(request, self.shutdown.clone()).await?)
    }

    /// main loop, waiting for shutdown signal
    pub async fn run(self) -> Result<(), CoreError> {
        let shutdown = self.shutdown.clone();
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Ctrl-C received.");
            }
            _ = shutdown.cancelled() => {
                tracing::info!("shutdown requested.");
            }
        }
        self.shutdown().await
    }

    /// shutdown
    async fn shutdown(self) -> Result<(), CoreError> {
        self.shutdown.cancel();
        // TODO
        // 1. stop rpc
        // 2. waiting for cleaning requests in waiting
        // 3. stop session
        // 4. stop plugins
        // 5. stop sandbox
        // 6. flush audit
        // 7. verify audit chain
        Ok(())
    }
}
