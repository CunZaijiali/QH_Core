// 一个会话实例
// 当前设计：
// Ractor框架，采用tokio并发模型
// 一个会话为一个 Actor

use std::sync::Arc;

use ractor::{Actor, ActorProcessingErr, ActorRef};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::domain::message::{CompletionRequest, Message, Role};
use crate::domain::{ModelId, SessionId};
use crate::error::SessionError;
use crate::llm::CompletionAdapter;
use crate::storage::sqlite::SqliteStore;

/// 单个会话的消息协议。
pub enum SessionMsg {
    /// 发送一条用户消息，返回助手回复文本。
    SendMessage {
        content: String,
        reply: oneshot::Sender<Result<String, SessionError>>,
    },
    /// 读取完整的历史消息。
    History {
        reply: oneshot::Sender<Vec<Message>>,
    },
    /// 取消当前正在进行的补全。
    Cancel,
}

/// 会话初始化参数。
pub struct SessionArgs {
    pub session_id: SessionId,
    pub model: ModelId,
    pub adapter: Arc<dyn CompletionAdapter>,
    pub store: Arc<SqliteStore>,
}

/// 会话状态：上下文消息、持久化句柄与取消令牌。
pub struct SessionState {
    session_id: SessionId,
    model: ModelId,
    adapter: Arc<dyn CompletionAdapter>,
    store: Arc<SqliteStore>,
    cancel: CancellationToken,
    history: Vec<Message>,
}

/// 会话 Actor，一个会话一个实例。
pub struct SessionActor;

#[ractor::async_trait]
impl Actor for SessionActor {
    type Msg = SessionMsg;
    type State = SessionState;
    type Arguments = SessionArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        // 从存储恢复历史；失败则降级为空上下文，不影响会话可用。
        let history = match args.store.load_messages(args.session_id).await {
            Ok(records) => records
                .into_iter()
                .map(|record| Message::new(record.role, record.content))
                .collect(),
            Err(err) => {
                tracing::warn!(session_id = %args.session_id, "load history failed: {err}");
                Vec::new()
            }
        };

        Ok(SessionState {
            session_id: args.session_id,
            model: args.model,
            adapter: args.adapter,
            store: args.store,
            cancel: CancellationToken::new(),
            history,
        })
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            SessionMsg::SendMessage { content, reply } => {
                let result = complete_turn(state, content).await;
                let _ = reply.send(result);
            }
            SessionMsg::History { reply } => {
                let _ = reply.send(state.history.clone());
            }
            SessionMsg::Cancel => {
                state.cancel.cancel();
                state.cancel = CancellationToken::new();
            }
        }
        Ok(())
    }
}

/// 追加用户消息 → 调用模型 → 追加助手回复，并逐条落库。
async fn complete_turn(state: &mut SessionState, content: String) -> Result<String, SessionError> {
    state
        .store
        .append_message(state.session_id, &Role::User, &content)
        .await?;
    state.history.push(Message::new(Role::User, content));

    let request = CompletionRequest {
        model: state.model.clone(),
        messages: state.history.clone(),
        temperature: None,
        max_tokens: None,
    };

    let response = state.adapter.complete(request, state.cancel.clone()).await?;

    state
        .store
        .append_message(state.session_id, &Role::Assistant, &response.content)
        .await?;
    state
        .history
        .push(Message::new(Role::Assistant, response.content.clone()));
    Ok(response.content)
}
