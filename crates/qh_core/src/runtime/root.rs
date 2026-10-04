use std::collections::HashMap;
use std::sync::Arc;

use ractor::{Actor, ActorProcessingErr, ActorRef};
use tokio::sync::oneshot;

use crate::domain::message::Message;
use crate::domain::{ModelId, SessionId};
use crate::error::SessionError;
use crate::llm::CompletionAdapter;
use crate::runtime::session::{SessionActor, SessionArgs, SessionMsg};
use crate::storage::sqlite::{SessionRecord, SqliteStore};

/// 会话根监督者的消息协议。
pub enum RootMsg {
    CreateSession {
        reply: oneshot::Sender<Result<SessionId, SessionError>>,
    },
    ListSessions {
        reply: oneshot::Sender<Result<Vec<SessionRecord>, SessionError>>,
    },
    DeleteSession {
        id: SessionId,
        reply: oneshot::Sender<Result<(), SessionError>>,
    },
    SendMessage {
        id: SessionId,
        content: String,
        reply: oneshot::Sender<Result<String, SessionError>>,
    },
    History {
        id: SessionId,
        reply: oneshot::Sender<Result<Vec<Message>, SessionError>>,
    },
}

/// 根监督者初始化参数。
pub struct RootArgs {
    pub model: ModelId,
    pub adapter: Arc<dyn CompletionAdapter>,
    pub store: Arc<SqliteStore>,
}

/// 根监督者状态：活跃会话表 + 持久化句柄。
pub struct RootState {
    model: ModelId,
    adapter: Arc<dyn CompletionAdapter>,
    store: Arc<SqliteStore>,
    sessions: HashMap<SessionId, ActorRef<SessionMsg>>,
}

/// 会话根监督者：管理所有会话 Actor 的生命周期与消息路由。
pub struct RootSupervisor;

#[ractor::async_trait]
impl Actor for RootSupervisor {
    type Msg = RootMsg;
    type State = RootState;
    type Arguments = RootArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let mut sessions = HashMap::new();

        // 启动时恢复已持久化的会话；单个会话失败只记日志，不影响整体启动。
        match args.store.list_sessions().await {
            Ok(records) => {
                for record in records {
                    let session_args = SessionArgs {
                        session_id: record.id,
                        model: args.model.clone(),
                        adapter: args.adapter.clone(),
                        store: args.store.clone(),
                    };
                    match Actor::spawn(None, SessionActor, session_args).await {
                        Ok((actor, _handle)) => {
                            sessions.insert(record.id, actor);
                        }
                        Err(err) => {
                            tracing::warn!(session_id = %record.id, "restore session failed: {err}");
                        }
                    }
                }
                if !sessions.is_empty() {
                    tracing::info!("restored {} session(s)", sessions.len());
                }
            }
            Err(err) => {
                tracing::warn!("restore sessions failed: {err}");
            }
        }

        Ok(RootState {
            model: args.model,
            adapter: args.adapter,
            store: args.store,
            sessions,
        })
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            RootMsg::CreateSession { reply } => {
                // 先落库拿到持久化 id，再拉起会话 Actor。
                match state.store.create_session("", &state.model).await {
                    Ok(id) => {
                        let args = SessionArgs {
                            session_id: id,
                            model: state.model.clone(),
                            adapter: state.adapter.clone(),
                            store: state.store.clone(),
                        };
                        match Actor::spawn(None, SessionActor, args).await {
                            Ok((actor, _handle)) => {
                                state.sessions.insert(id, actor);
                                let _ = reply.send(Ok(id));
                            }
                            Err(err) => {
                                let _ = reply.send(Err(SessionError::AgentLoop(err.to_string())));
                            }
                        }
                    }
                    Err(err) => {
                        let _ = reply.send(Err(SessionError::Storage(err)));
                    }
                }
            }
            RootMsg::ListSessions { reply } => {
                let result = state.store.list_sessions().await.map_err(SessionError::Storage);
                let _ = reply.send(result);
            }
            RootMsg::DeleteSession { id, reply } => {
                if let Some(actor) = state.sessions.remove(&id) {
                    actor.stop(None);
                }
                let result = state
                    .store
                    .delete_session(id)
                    .await
                    .map_err(SessionError::Storage);
                let _ = reply.send(result);
            }
            RootMsg::SendMessage { id, content, reply } => {
                match state.sessions.get(&id).cloned() {
                    Some(actor) => {
                        let (tx, rx) = oneshot::channel();
                        if actor
                            .send_message(SessionMsg::SendMessage { content, reply: tx })
                            .is_err()
                        {
                            let _ = reply.send(Err(SessionError::Closed));
                            return Ok(());
                        }
                        // 异步转发，避免在 handler 里阻塞整个监督者。
                        tokio::spawn(async move {
                            let result = rx.await.unwrap_or(Err(SessionError::Closed));
                            let _ = reply.send(result);
                        });
                    }
                    None => {
                        let _ = reply.send(Err(SessionError::NotFound(id.to_string())));
                    }
                }
            }
            RootMsg::History { id, reply } => match state.sessions.get(&id).cloned() {
                Some(actor) => {
                    let (tx, rx) = oneshot::channel();
                    if actor.send_message(SessionMsg::History { reply: tx }).is_err() {
                        let _ = reply.send(Err(SessionError::Closed));
                        return Ok(());
                    }
                    tokio::spawn(async move {
                        let result = rx.await.map_err(|_| SessionError::Closed);
                        let _ = reply.send(result);
                    });
                }
                None => {
                    let _ = reply.send(Err(SessionError::NotFound(id.to_string())));
                }
            },
        }
        Ok(())
    }
}
