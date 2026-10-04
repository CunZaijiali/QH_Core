pub mod handle;
pub mod shutdown;

use std::sync::Arc;

use ractor::{Actor, ActorRef};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::config::CoreConfig;
use crate::domain::SessionId;
use crate::domain::message::{CompletionRequest, CompletionResponse, Message, Role};
use crate::error::{AdapterError, BootstrapError, CoreError, SessionError};
use crate::event::EventService;
use crate::http::HttpClient;
use crate::llm::CompletionAdapter;
use crate::llm::openai::OpenAiCompatibleAdapter;
use crate::logging::Logger;
use crate::runtime::root::{RootArgs, RootMsg, RootSupervisor};
use crate::security::AuditService;
use crate::storage::sqlite::{SessionRecord, SqliteStore};

/// qh_core 运行时。
///
/// 当前落地 Phase 1-6（基础设施 / 事件 / 核心服务 / 模型适配器 / 会话），
/// Phase 5、7（插件 / handle）逐步补齐。
pub struct AgentCore {
    config: Arc<CoreConfig>,
    logger: Logger,
    store: Arc<SqliteStore>,
    events: Arc<EventService>,
    audit: Arc<AuditService>,
    http: Arc<HttpClient>,
    adapter: Option<Arc<dyn CompletionAdapter>>,
    sessions: Option<ActorRef<RootMsg>>,
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
        let sessions = match &adapter {
            Some(adapter) => {
                let args = RootArgs {
                    model: adapter.model().clone(),
                    adapter: adapter.clone(),
                    store: store.clone(),
                };
                let (root, _handle) = Actor::spawn(None, RootSupervisor, args)
                    .await
                    .map_err(|e| BootstrapError::Runtime(format!("root supervisor: {e}")))?;
                Some(root)
            }
            None => None,
        };

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
            sessions,
            shutdown: CancellationToken::new(),
        })
    }

    /// 是否配置了可用的模型适配器。
    pub fn has_adapter(&self) -> bool {
        self.adapter.is_some()
    }

    /// 用默认模型发送一条用户消息（无会话状态）。
    pub async fn complete(&self, content: impl Into<String>) -> Result<CompletionResponse, CoreError> {
        let adapter = self.adapter.as_ref().ok_or_else(no_adapter)?;
        let request = CompletionRequest {
            model: adapter.model().clone(),
            messages: vec![Message::new(Role::User, content)],
            temperature: None,
            max_tokens: None,
        };
        Ok(adapter.complete(request, self.shutdown.clone()).await?)
    }

    /// 创建一个新会话。
    pub async fn create_session(&self) -> Result<SessionId, CoreError> {
        let root = self.root()?;
        let (tx, rx) = oneshot::channel();
        root.send_message(RootMsg::CreateSession { reply: tx })
            .map_err(|_| CoreError::Session(SessionError::Closed))?;
        let inner = rx.await.map_err(|_| CoreError::Session(SessionError::Closed))?;
        Ok(inner?)
    }

    /// 列出所有会话（含持久化元数据）。
    pub async fn list_sessions(&self) -> Result<Vec<SessionRecord>, CoreError> {
        let root = self.root()?;
        let (tx, rx) = oneshot::channel();
        root.send_message(RootMsg::ListSessions { reply: tx })
            .map_err(|_| CoreError::Session(SessionError::Closed))?;
        let inner = rx.await.map_err(|_| CoreError::Session(SessionError::Closed))?;
        Ok(inner?)
    }

    /// 删除一个会话。
    pub async fn delete_session(&self, id: SessionId) -> Result<(), CoreError> {
        let root = self.root()?;
        let (tx, rx) = oneshot::channel();
        root.send_message(RootMsg::DeleteSession { id, reply: tx })
            .map_err(|_| CoreError::Session(SessionError::Closed))?;
        let inner = rx.await.map_err(|_| CoreError::Session(SessionError::Closed))?;
        Ok(inner?)
    }

    /// 向指定会话发送消息，返回助手回复。
    pub async fn send_message(&self, id: SessionId, content: impl Into<String>) -> Result<String, CoreError> {
        let root = self.root()?;
        let (tx, rx) = oneshot::channel();
        root.send_message(RootMsg::SendMessage {
            id,
            content: content.into(),
            reply: tx,
        })
        .map_err(|_| CoreError::Session(SessionError::Closed))?;
        let inner = rx.await.map_err(|_| CoreError::Session(SessionError::Closed))?;
        Ok(inner?)
    }

    /// 读取指定会话的历史消息。
    pub async fn history(&self, id: SessionId) -> Result<Vec<Message>, CoreError> {
        let root = self.root()?;
        let (tx, rx) = oneshot::channel();
        root.send_message(RootMsg::History { id, reply: tx })
            .map_err(|_| CoreError::Session(SessionError::Closed))?;
        let inner = rx.await.map_err(|_| CoreError::Session(SessionError::Closed))?;
        Ok(inner?)
    }

    fn root(&self) -> Result<&ActorRef<RootMsg>, CoreError> {
        self.sessions.as_ref().ok_or_else(no_adapter)
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

fn no_adapter() -> CoreError {
    CoreError::Adapter(AdapterError::NoAdapterForModel(
        "no enabled adapter configured".into(),
    ))
}
