use crate::config::EventsConfig;
use crate::domain::{PluginId, SessionId};
use crate::error::EventError;
use async_trait::async_trait;
use qh_macros::define_id;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Instant, SystemTime};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

define_id!(pub ConsumerId, i64);
define_id!(pub EventId, i64);
define_id!(pub TraceId, i64);
define_id!(pub RequestId, i64);
define_id!(pub SubscriptionId, i64);
define_id!(pub StreamId, i64);

/// 提供四种通信原语
/// - Notification
/// - Command
/// - Stream
/// - Broadcast

pub struct Topic(String);
pub struct BroadcastTopic(String);
pub struct CommandKind(String);
#[async_trait]
trait NotificationStore: Send+Sync{
    async fn append(&self,n:&Notification)->Result<(),EventError>;
    async fn since(&self,n:&Notification)->Result<(),EventError>;
}
#[async_trait]
trait BroadcastStore: Send+Sync{
    async fn append(&self, b:&BroadcastEvent)->Result<(),EventError>;
    async fn since(&self, b:&BroadcastEvent)->Result<(),EventError>;
}

pub enum EventSource{
    Core,
    Session(SessionId),
    Plugin(PluginId),
}
pub enum Priority{
    Low,
    Normal,
    High,
    Critical,
}
struct PendingCommand{
    reply: oneshot::Sender<serde_json::Value>,
    deadline: Instant
}
struct CommandHandler{
    sender: mpsc::Sender<Command>
}

/// EventRouter
pub enum EventRouter {
    PointTo { target: ConsumerId },
    Broadcast { topic: Topic },
    Multicast { targets: Vec<ConsumerId> },
    Anycast { kind: CommandKind },
}



/// NotifacationBus
pub struct NotificationBus {
    /// 进程内: tokio broadcast 按 topic 分组
    channels: HashMap<String, broadcast::Sender<Notification>>,
    /// 跨进程: 转发给订阅的插件
    plugin_forwarders: HashMap<PluginId, Vec<Topic>>,
    /// 持久化
    store: Option<Arc<dyn NotificationStore>>,
}
pub struct Notification {
    pub id: EventId,
    pub topic: Topic,
    pub source: EventSource,
    pub payload: serde_json::Value,
    pub timestamp: SystemTime,
    pub trace_id: TraceId,
    pub priority: Priority,
}
impl NotificationBus{
    fn new(cfg:&EventsConfig) -> Self {
        let capacity:usize = cfg.notification_capacity;
        Self{
            channels: HashMap::with_capacity(capacity),
            plugin_forwarders: HashMap::with_capacity(capacity),
            store: None,
        }
    }
}



pub struct CommandBus {
    /// 挂起的请求 request_id -> oneshot sender
    pending: HashMap<RequestId, PendingCommand>,
    /// 目标路由 谁能处理哪类command
    routes: HashMap<CommandKind, Vec<CommandHandler>>,
}

pub struct Command {
    pub id: RequestId,
    pub kind: CommandKind,
    pub target: CommandTarget,
    pub payload: serde_json::Value,
    pub deadline: Instant,
    pub priority: Priority,
    pub cancel: CancellationToken,
}
pub enum CommandTarget {
    Plugin(PluginId),
    Session(SessionId),
    Core,
    AnyOf(Vec<CommandTarget>),
}
impl CommandBus {
    fn new(cfg:&EventsConfig) -> Self {
        let capacity: usize = cfg.command_capacity;
        let timeout = cfg.command_timeout;
        Self{
            pending: HashMap::with_capacity(capacity),
            routes: HashMap::with_capacity(capacity),
        }
    }
}



pub struct StreamHub {
    /// 每个 stream 都有一个 broadcast
    streams: HashMap<StreamId, broadcast::Sender<StreamItem>>,
    /// 订阅者管理
    subscribers: HashMap<StreamId, Vec<SubscriptionId>>,
}
pub struct StreamItem {
    pub stream_id: StreamId,
    pub sequence: u64,
    pub payload: serde_json::Value,
    pub timestamp: SystemTime,
}
impl StreamHub {
    fn new(cfg:&EventsConfig) -> Self {
        let capacity:usize = cfg.stream_capacity;
        Self{
            streams: HashMap::with_capacity(capacity),
            subscribers: HashMap::with_capacity(capacity),
        }
    }
}


pub struct BroadcastHub {
    channels: HashMap<BroadcastTopic, broadcast::Sender<BroadcastEvent>>,
    /// 持久化，用以实现延迟接收，离线订阅者上线后能够补收
    store: Option<Arc<dyn BroadcastStore>>,
}

pub struct BroadcastEvent {
    pub id: EventId,
    pub topic: BroadcastTopic,
    pub payload: serde_json::Value,
    pub version: u64,
    pub timestamp: SystemTime,
}
impl BroadcastHub {
    fn new(cfg:&EventsConfig) -> Self {
        let capacity:usize = cfg.broadcast_capacity;
        Self{
            channels: HashMap::with_capacity(capacity),
            store: None,
        }
    }
}



pub struct EventService {
    notification_bus: NotificationBus,
    command_bus: CommandBus,
    stream_hub: StreamHub,
    broadcast_hub: BroadcastHub,
}
impl EventService {
    pub fn new(cfg: &EventsConfig) -> Self {
        Self{
            notification_bus: NotificationBus::new(cfg),
            command_bus: CommandBus::new(cfg),
            stream_hub: StreamHub::new(cfg),
            broadcast_hub: BroadcastHub::new(cfg),
        }
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn publishes_and_stops_cleanly() {}
}
