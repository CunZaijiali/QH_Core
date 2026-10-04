当前代码已经暴露出需要裁剪的信号：`domain/llm.rs` 的 `Role` 包含 `Tool/Custom`，`AdapterCapabilities` 已包含 tool call、vision、reasoning、JSON mode 等远超单模态补全的能力，而 `plugin/protocol.rs` 和 `sidecar.rs` 仍为空，说明跨语言边界还没有被真正冻结。基于这一点，建议先定义一个严格的 Core Contract，再让所有扩展围绕它适配，而不是继续扩大 Adapter 抽象。

## 核心判断

你现在需要的不是继续完善一个“全能力 Adapter”，而是先定义：

> Core 到底必须知道什么，哪些内容一旦加入 Core 就会成为永久兼容负担。

建议把系统分成三层：

```text
Core Kernel
  最小运行能力和不可绕过的不变式

Core Protocol
  Core 与 Provider、Plugin、Extension 之间的稳定线协议

Optional Capabilities
  Plugins / Extensions / Sidecars 提供的所有可替换能力
```

核心目标不是“功能少”，而是：

```text
Core 能独立启动
Core 能完成基础文本对话
Core 能处理取消、超时、错误和关闭
Core 能加载可选扩展
Core 不依赖任何插件才能运行
```

## 一、建议的 Core 最小完备集

### 1. 进程生命周期

Core 必须支持：

- 启动
- 配置加载
- 初始化一个基础运行时
- 创建一个或多个内存会话
- 接收一条用户消息
- 调用一个默认模型适配器
- 返回一段文本
- 取消当前请求
- 超时处理
- 优雅关闭

暂时不要求：

- 完整 Actor 监督树
- 多租户
- Session 恢复
- 分布式调度
- 动态模型切换
- 插件热更新
- 多种存储后端

这些都可以在 Core 生命周期稳定以后增加。

### 2. 最小对话模型

建议核心只保留：

```rust
pub enum Role {
    System,
    User,
    Assistant,
}
```

```rust
pub struct Message {
    pub role: Role,
    pub content: String,
}
```

```rust
pub struct CompletionRequest {
    pub model: ModelId,
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}
```

```rust
pub struct CompletionResponse {
    pub content: String,
    pub finish_reason: FinishReason,
    pub usage: Option<Usage>,
}
```

这已经足够表达基础对话补全。

Core 不应包含：

```text
Tool role
Custom role
Image content
Audio content
Video content
ContentPart
ToolCall
ParallelToolCall
ReasoningBlock
MultimodalMessage
Provider-specific message variant
```

你的判断是正确的：`Role` 不需要追求最大兼容性。

Core 的职责是定义自己的最小语义，而不是把所有 Provider 的消息模型联合起来。

### 3. Role 的兼容策略

Core 只接受三种角色：

```text
system
user
assistant
```

Provider Adapter 内部负责转换：

```text
Core Role
  → Provider Request Role
```

例如：

```text
Core::System
  → OpenAI-compatible: system
  → 某些 Provider: system
  → 某些本地模型: prompt prefix

Core::Assistant
  → Provider-specific assistant message
```

如果某个 Provider 需要额外角色：

```text
tool
developer
function
reasoning
```

由对应 Adapter 自己解决，不进入 Core 的公共领域模型。

如果无法转换，返回：

```rust
AdapterError::UnsupportedMessageRole
```

而不是修改 Core 的 `Role` 枚举。

### 4. 模型适配器

Core 第一版只需要一个内置 Provider：

```text
OpenAI-compatible text completion adapter
```

它可以通过配置连接：

```text
OpenAI
DeepSeek
Local OpenAI-compatible server
企业内部兼容接口
```

不要在 Core 中增加：

```rust
ProviderKind::DeepSeek
ProviderKind::Anthropic
ProviderKind::Gemini
ProviderKind::Ollama
ProviderKind::Azure
```

这些都是 Provider 的具体实现或配置，不应该成为 Core 的永久枚举。

建议：

```rust
pub trait CompletionAdapter {
    async fn complete(
        &self,
        request: CompletionRequest,
        cancel: CancellationToken,
    ) -> Result<CompletionResponse, AdapterError>;
}
```

Core 只保留：

```text
一个默认 Adapter
一个 active model
请求超时
取消
错误映射
```

以下能力移出第一版：

- 多 Adapter 并行
- Provider Factory
- 模型动态切换
- Adapter Middleware
- Adapter Registry 热插拔
- 多 Provider fallback
- Provider 健康权重
- 成本路由
- 负载均衡

这些都是后续扩展能力。

## 二、哪些东西属于 Core，哪些不属于

可以使用下面的判断规则。

### 必须进入 Core

满足任意一条，就应留在 Core：

1. 删除后 Core 无法完成基础对话。
2. 该内容定义了跨模块不变式。
3. 该内容负责权限、预算、取消、超时或生命周期。
4. 该内容是安全边界。
5. 所有插件都必须依赖它才能通信。
6. 如果交给插件，插件就可能绕过系统控制。

因此 Core 必须拥有：

```text
CoreConfig
CoreError
Message / Role / CompletionRequest / CompletionResponse
Session 基础状态
CompletionAdapter 基础接口
CancellationToken
Timeout
RequestId / SessionId / TraceId
IPC framing
协议版本和握手
插件进程隔离
Capability 校验入口
最小审计入口
Core 生命周期
```

### 应该放入 Plugin 或 Extension

满足以下条件时，不应放入 Core：

1. 只是某种策略。
2. 只是某个 Provider 的差异。
3. 只是业务能力。
4. 需要额外运行时依赖。
5. 需要访问文件、网络、进程或系统能力。
6. 不同用户会有不同实现。
7. 失败时可以回退默认实现。

因此以下内容可以外置：

```text
工具调用
文件读写
代码执行
浏览器
数据库连接器
RAG
向量库
记忆系统
上下文压缩
Prompt 模板
模型路由
多模型 fallback
成本控制
重试策略
缓存
审计展示
多模态
语音
图像
视频
Agent-to-Agent
子 Agent
复杂推理模式
```

## 三、当前代码中需要明确裁剪的部分

### `domain/llm.rs`

当前包含：

```rust
Role::System
Role::User
Role::Assistant
Role::Tool
Role::Custom(String)
```

建议裁剪为：

```rust
Role::System
Role::User
Role::Assistant
```

同时把以下字段移出 Core 的基础 `LlmRequest`：

```rust
tools
tool_calls
```

可以建立两个层次：

```text
CoreCompletionRequest
  最小文本补全

ExtendedCompletionRequest
  由插件或扩展协议定义
```

不要让 `CoreCompletionRequest` 一开始就包含所有未来字段。

### `adapter.rs`

当前 `AdapterCapabilities` 已经包含：

```rust
streaming
tool_calls
parallel_tool_calls
vision
reasoning
json_mode
max_context
```

对于最小 Core，建议只保留：

```rust
pub struct AdapterCapabilities {
    pub streaming: bool,
}
```

甚至第一版可以不暴露 capabilities，只保留：

```text
adapter id
model id
provider label
protocol version
```

以下能力属于扩展声明：

```text
vision
tool_calls
parallel_tool_calls
reasoning
json_mode
```

如果以后需要，使用能力字符串或扩展能力文档：

```text
capabilities = [
    "completion.text",
    "completion.streaming",
    "tools.basic",
    "vision.image",
]
```

不要不断修改 Rust 枚举。

### `config.rs`

当前配置已经预留了大量未来能力：

```text
plugins
extensions
adapters
sandbox
rpc
audit
security
```

建议区分：

```text
CoreConfig
  Core 必需配置

PluginHostConfig
  插件系统配置

ExtensionHostConfig
  扩展点配置

ProviderConfig
  内置 Provider 配置
```

发布版 Core 的最小配置应尽量接近：

```toml
[core]
workspace = "."

[completion]
model = "deepseek-chat"
base_url = "https://api.deepseek.com"
api_key_env = "DEEPSEEK_API_KEY"
```

插件没有配置时，Core 仍然可启动。

### `plugin/protocol.rs` 和 `sidecar.rs`

当前这两个模块为空，说明跨语言能力还没有真正落地。

不能先设计大量 Rust Trait，再期待 Python、Node、Go 去兼容这些 Trait。

真正需要冻结的是：

```text
Wire Protocol
Schema
Handshake
Error Codes
Capability Names
Lifecycle
Version Policy
```

Rust Trait 只应是 Core 内部实现方式。

## 四、多语言 Plugin 和 Extension 的正确实现方式

### 核心原则

不要追求跨语言 ABI 兼容。

应该追求：

> 语言无关的进程协议兼容。

也就是说：

```text
Python Plugin
Node Plugin
Go Plugin
Rust Plugin
Java Plugin
C# Plugin
```

都不直接实现 Rust Trait，而是实现相同的协议。

推荐结构：

```text
qh_core
  ├── Core Domain
  ├── Builtin Completion Adapter
  ├── Extension Dispatcher
  └── Sidecar Host
          │
          ├── Python Process
          ├── Node Process
          ├── Go Process
          └── Rust Process
```

### 第一版协议建议

继续使用已有的：

```text
Length-prefixed JSON
```

当前 `ipc.rs` 已经实现了基础 framing：

```rust
IpcMessage::Request
IpcMessage::Response
IpcMessage::Error
IpcMessage::Event
IpcMessage::Ping
IpcMessage::Pong
```

但还需要在其上增加协议层：

```text
Hello
Ready
Describe
Invoke
Cancel
Shutdown
Health
```

握手示例：

```json
{
  "kind": "hello",
  "protocol": "qh.plugin",
  "version": 1,
  "plugin_id": "example.optimizer",
  "runtime": "python",
  "capabilities": [
    "extension.adapter.request_transform"
  ]
}
```

Core 返回：

```json
{
  "kind": "ready",
  "protocol": "qh.plugin",
  "version": 1,
  "grants": [
    "extension.adapter.request_transform"
  ],
  "limits": {
    "timeout_ms": 5000,
    "max_payload_bytes": 8388608
  }
}
```

### JSON 是否足够

第一版建议使用 JSON，因为：

- Python、Node、Go、Rust 都容易实现
- 调试简单
- 协议可读
- 方便保存 Golden Test
- 适合低频扩展点调用
- Sidecar 启动和控制消息本身不需要极致性能

后续如果遇到大 payload，再增加：

```text
MessagePack
CBOR
Protobuf
共享内存
```

但不要一开始同时支持多种编码。

可以把编码抽象成：

```text
Transport
  framing

Codec
  JSON / MessagePack / CBOR

Protocol
  semantic messages
```

这样不会把协议和编码耦合。

## 五、Plugin 和 Extension 的边界

### Plugin

Plugin 是能力提供者，通常运行在独立进程：

```text
Plugin
  提供工具
  提供 Provider
  提供外部服务
  提供业务能力
  订阅事件
```

Plugin 可以拥有：

- 自己的状态
- 自己的运行时
- 自己的依赖
- 自己的配置
- 自己的语言生态

但不能直接访问：

- Core 内存
- Core ActorRef
- Core 数据库连接
- Core 权限状态
- Core shutdown token

### Extension

Extension 是 Core 某个生命周期或决策点的参与者：

```text
Extension
  修改请求
  修改响应
  提供上下文压缩策略
  提供 Prompt 构造策略
  提供工具选择策略
  提供 Loop 决策建议
```

Extension 不应该成为任意代码注入点。

Core 负责：

```text
调用时机
超时
取消
顺序
失败回退
预算
审计
最终校验
```

Extension 只负责：

```text
输入 → 输出
```

这也是为什么 `AdapterExtension::transform_request` 比让插件直接接管整个 Adapter 更合适。

## 六、如何让插件修改 Adapter 效果

建议提供三种独立协议能力。

### 1. Provider Plugin

插件实现一个新的 Provider：

```text
plugin → describe_provider
plugin → complete
plugin → stream
plugin → health
```

Core 视其为远程 Adapter。

适合：

```text
本地模型
特殊云服务
企业内部模型
自研推理服务
非 HTTP 模型
```

### 2. Request Transform Extension

插件修改请求数据：

```text
before_completion
```

可以修改：

```text
messages
model
temperature
max_tokens
provider_options
```

不能修改：

```text
timeout upper bound
cancellation
audit policy
permission policy
session ownership
```

### 3. Response Transform Extension

插件修改响应：

```text
after_completion
```

可以用于：

```text
内容过滤
格式规范化
脱敏
后处理
响应翻译
结果标注
```

对于远程插件，Core 仍然拥有流程控制权。

不要让远程插件实现真正的 Middleware，因为 Middleware 可以决定：

```text
是否调用 Adapter
调用几次
是否重试
是否返回缓存
是否吞掉错误
```

这种能力只适合：

```text
Core 内置
Rust 进程内
WASM
```

远程插件可以提出建议，但最终由 Core 执行。

## 七、如何尽可能提高多语言兼容性

### 1. 不把 Rust Trait 作为跨语言接口

Rust Trait 只用于 Core 内部：

```rust
trait CompletionAdapter
trait AdapterExtension
trait ToolProvider
```

跨语言接口统一使用：

```text
JSON Schema
JSON-RPC-like messages
Versioned envelopes
```

### 2. 生成多语言 SDK

过程宏只服务于 Rust：

```rust
#[qh_extension]
struct ContextCompressor;
```

但跨语言 SDK 应该由协议生成或手写薄封装：

```text
qh-plugin-sdk-python
qh-plugin-sdk-node
qh-plugin-sdk-go
qh-plugin-sdk-rust
```

每个 SDK 只负责：

```text
连接 stdin/stdout 或 socket
处理 framing
处理 handshake
序列化请求
发送响应
处理取消
```

不要让 SDK 携带 Core 的全部实现。

### 3. 协议优先于 SDK

正确顺序是：

```text
协议文档
→ JSON Schema
→ Golden fixtures
→ Rust host
→ Python SDK
→ Node SDK
→ Go SDK
```

不要反过来先写 Rust Trait，再让其他语言猜协议。

### 4. 使用能力协商

插件不能假设 Core 支持所有能力。

握手时协商：

```text
plugin capabilities
core capabilities
granted capabilities
```

例如：

```text
extension.adapter.request_transform
extension.adapter.response_transform
provider.completion.text
provider.completion.streaming
```

不支持的能力不应通过字段猜测，而应在握手阶段明确拒绝。

### 5. 使用稳定的字符串能力名

不要使用会频繁变动的 Rust 枚举作为跨语言协议：

```rust
enum Capability {
    ToolCalls,
    Vision,
    Reasoning,
}
```

推荐：

```json
{
  "capabilities": [
    "completion.text",
    "completion.streaming"
  ]
}
```

新能力可以新增字符串，不需要升级所有 SDK。

## 八、如何确认某个能力不应该进入 Core

可以为每个候选能力填写一张决策表：

| 问题                               | 是               | 否             |
| ---------------------------------- | ---------------- | -------------- |
| 没有它，基础文本补全还能完成吗？   | 倾向 Plugin      | 继续判断       |
| 它是否维护 Core 不可绕过的不变式？ | Core             | 继续判断       |
| 它是否涉及权限、取消、超时、预算？ | Core             | 继续判断       |
| 它是否只是某种策略？               | Extension        | 继续判断       |
| 它是否依赖某个语言运行时或系统库？ | Sidecar Plugin   | 继续判断       |
| 是否有多个合理实现？               | Extension/Plugin | Core           |
| 是否必须拥有流程控制权？           | Core/WASM        | 远程插件不允许 |
| 删除它是否只影响可选体验？         | Plugin           | Core           |

### 例子：Tool Call

基础对话不需要 Tool Call，因此：

```text
Core：不包含 Tool role、ToolCall、ToolSpec
Plugin：提供工具
Extension：可以建议工具选择
Core：未来再定义通用工具执行闸门
```

### 例子：Vision

单模态 Core 不需要：

```text
ImageContent
MimePart
VisionCapability
ImageToken
```

将来由多模态 Adapter 或 Plugin 提供：

```text
completion.multimodal.image
```

### 例子：流式输出

建议采用以下折中：

```text
Core 公共最小 API：complete
Adapter 内部可以 stream
Core 可以聚合 stream 得到 complete
```

这样 Core 不依赖每个 Provider 都支持 streaming。

如果 Tauri UI 后续强依赖实时输出，再把：

```text
CompletionChunk
```

加入协议 v1 的可选能力，而不是让所有 Adapter 都必须支持。

## 九、推荐的最小发布版本

建议第一个正式发布版本只包含：

```text
Core
  配置加载
  生命周期
  内存 Session
  system/user/assistant 三种 Role
  文本 CompletionRequest
  文本 CompletionResponse
  一个 OpenAI-compatible Adapter
  一个默认模型
  超时
  取消
  分层错误
  基础日志
  最小审计
  优雅关闭
```

可选但不阻塞启动：

```text
Plugin Host
Sidecar IPC
Extension Dispatcher
RPC Endpoint
持久化 Store
```

明确排除：

```text
Tool Calling
Vision
Audio
Video
RAG
Memory
Browser
Code Sandbox
Sub-agent
Multi-agent
Provider fallback
Dynamic routing
Parallel tool calls
```

你的目标可以定义为：

> `qh_core` 是一个不依赖插件即可运行的单模态对话运行时；插件和 Extensions 不负责让 Core “变得可运行”，只负责让 Core “变得更强”。

这意味着需要先冻结一个非常小的 Core Contract，而不是继续把所有未来能力提前放进 `LlmRequest`、`Role`、`AdapterCapabilities` 或 `AgentCore`。

## 一、先定义“最小完备集”

最小完备集不是“功能最少”，而是：

> 删除任何一个组件后，Core 就无法完成基础对话闭环，或者无法保证核心生命周期、安全和协议不变式。

建议 Core v1 只保证这一条链路：

```text
启动
→ 读取配置
→ 创建一个或多个 Session
→ 接收 system/user/assistant 消息
→ 调用一个内置文本模型 Adapter
→ 得到文本补全
→ 返回结果
→ 超时、取消、错误处理
→ 正常关闭
```

### Core 必须包含

| 能力                |  是否进入 Core | 原因                               |
| ------------------- | -------------: | ---------------------------------- |
| 配置读取与校验      |             是 | 没有配置就无法启动                 |
| Core 生命周期       |             是 | 启动、运行、关闭是不变式           |
| Session 最小状态    |             是 | 对话上下文必须有唯一所有者         |
| 基础消息模型        |             是 | 对话闭环的领域模型                 |
| 文本补全请求/响应   |             是 | Core 的最小业务能力                |
| 一个内置 Adapter    |             是 | 发布后 Core 能直接运行             |
| 超时与取消          |             是 | 核心控制边界                       |
| 基础错误模型        |             是 | 所有外部能力都依赖它               |
| 基础事件或响应通道  |             是 | 客户端必须拿到结果                 |
| 最小日志            |             是 | 启动和故障排查                     |
| 最小审计            |             是 | 至少记录启动、失败、取消和外部边界 |
| Adapter 注册接口    | 是，但保持最小 | 插件需要扩展模型供应商             |
| 插件握手和进程隔离  |     是基础设施 | 插件边界属于 Core 的安全职责       |
| Capability 校验入口 |     是基础设施 | 插件不能绕过授权                   |

### Core v1 不应该包含

| 能力                       | 放置位置                          |
| -------------------------- | --------------------------------- |
| Tool Calling               | Plugin / Extension                |
| `Tool` Role                | Plugin Protocol 或未来协议版本    |
| Vision / Image / Audio     | 多模态 Adapter Plugin             |
| Reasoning 专用字段         | Provider Plugin                   |
| Parallel Tool Calls        | Tool Plugin                       |
| JSON Schema 工具参数       | Tool Plugin                       |
| Context 压缩策略           | Extension                         |
| Prompt 模板策略            | Extension                         |
| Tool 选择                  | Extension                         |
| Retry / Cache / Rate Limit | Core Middleware 或 WASM Extension |
| 多 Provider 特殊字段       | Adapter Plugin                    |
| Provider-specific Role     | Adapter Plugin 内部转换           |
| MCP、A2A 等外部协议        | Plugin                            |
| 文件系统工具               | Plugin + Sandbox                  |
| 浏览器自动化               | Plugin + Sandbox                  |
| 远程部署                   | Host / Sidecar / Deployment Layer |
| 多模态消息结构             | 后续协议扩展                      |
| 模型路由和智能选择         | Extension                         |

关键点是：

> Core 可以有扩展点，但 Core 不需要实现所有扩展点的业务能力。

例如 Core 可以定义 `ContextCompressor` 接口，但默认实现只提供 `NoopCompressor`。这样 Core 可运行，插件可以替换策略。

## 二、建议的 Core v1 LLM 模型

当前 `domain/llm.rs` 中的模型过宽：

```rust
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
    Custom(String),
}
```

对于 Core v1，建议改成：

```rust
pub enum Role {
    System,
    User,
    Assistant,
}
```

不要在 Core 中保留：

```text
Tool
Custom
Developer
Function
Reasoning
Image
Audio
```

原因不是这些概念没有价值，而是它们会污染 Core 的最小不变式。

### Core v1 的消息

```rust
pub struct Message {
    pub role: Role,
    pub content: String,
}
```

第一版不必包含：

```rust
name
tool_call_id
content_parts
image_url
audio
annotations
reasoning_content
```

这些字段都可以在插件协议或未来版本中扩展。

### Core v1 的请求

```rust
pub struct CompletionRequest {
    pub model: ModelId,
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}
```

第一版可以暂时不包含：

```text
tools
response_format
parallel_tool_calls
top_p
logprobs
seed
vision inputs
provider-specific options
```

如果确实需要 Provider 扩展参数，不要直接把大量字段加入 Core，而是保留一个明确的扩展区：

```rust
pub struct CompletionRequest {
    pub model: ModelId,
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub extensions: BTreeMap<String, serde_json::Value>,
}
```

但这个字段只能由 Adapter Plugin 使用，Core 不解释其中的内容，也不把它当作稳定语义。

### Core v1 的响应

```rust
pub struct CompletionResponse {
    pub content: String,
    pub finish_reason: FinishReason,
    pub usage: Option<Usage>,
}
```

```rust
pub enum FinishReason {
    Stop,
    Length,
    Cancelled,
    Error,
}
```

对于 Core v1，响应不需要包含 ToolCall。

### 关于流式响应

我的建议是：

- Core 对外先保证 `complete`
- Adapter 内部可以支持 `stream`
- Core 可以把内部 stream 聚合为 `CompletionResponse`
- 不要让“流式事件模型”成为 Core v1 的前置依赖

这样可以降低最小模型复杂度。

后续如果 Tauri UI 强依赖实时输出，再增加：

```rust
pub enum CompletionEvent {
    TextDelta { text: String },
    Finished { response: CompletionResponse },
}
```

这仍然不需要引入 Tool、Vision 或复杂角色。

## 三、Core 只支持一个内置 Adapter

目前 `AdapterProvider` 包含：

```rust
OpenAiCompatible
DeepSeek
Custom(String)
```

建议 Core v1 不要把 `DeepSeek` 作为核心 Provider 类型。

更好的设计是：

```rust
AdapterProvider::OpenAiCompatible
```

Core 内置一个 OpenAI-compatible HTTP Adapter，用户通过配置：

```toml
[adapter]
base_url = "https://api.deepseek.com"
model = "deepseek-chat"
api_key_source = "environment:DEEPSEEK_API_KEY"
```

这样：

- Core 只维护一个 HTTP 协议实现
- DeepSeek、OpenAI、兼容服务都可以复用
- 新的特殊供应商由 Plugin 提供
- Core 不需要持续增加 Provider 枚举

Core v1 可以只支持：

```text
一个 active adapter
一个默认 model
文本输入
文本输出
system/user/assistant
complete
```

不需要立即实现：

```text
AdapterRegistry 的复杂热切换
多个 Provider 竞争同一模型
自动模型路由
Provider fallback
并行 Adapter
动态模型选择
```

这些都属于之后的扩展能力。

## 四、Adapter Trait 也应该缩小

当前 `LlmAdapter` 已经包含：

- stream
- complete
- health
- shutdown
- capability metadata
- 扩展和 middleware 机制
- Provider Factory
- generation 排空
- active adapter 切换

这些设计可以保留为未来架构，但不应全部成为 Core v1 的启动依赖。

建议 Core v1 的内部接口先缩小为：

```rust
#[async_trait]
pub trait CompletionAdapter: Send + Sync {
    fn model(&self) -> &ModelId;

    async fn complete(
        &self,
        request: CompletionRequest,
        cancel: CancellationToken,
    ) -> Result<CompletionResponse, AdapterError>;
}
```

然后 Core 内部封装：

```text
CompletionAdapter
  → timeout
  → cancellation
  → audit
  → error mapping
```

未来扩展到插件时，再增加：

```rust
trait AdapterFactory
trait AdapterExtension
trait AdapterMiddleware
trait StreamingAdapter
```

这些应该是扩展层协议，而不是 Core 最小领域模型。

## 五、任意语言 Plugin 的关键方法

你希望用户可以用任意语言编写 Plugin，这一点不能通过 Rust Trait 实现。

Rust Trait 只适合：

```text
同一进程
同一 ABI
同一语言
同一版本依赖
```

跨语言 Plugin 的真正边界必须是：

```text
版本化的 Wire Protocol
```

推荐结构：

```text
Rust Core
  ↓ JSON-RPC / framed RPC
Python Plugin
Node Plugin
Go Plugin
Rust Plugin
Java Plugin
C# Plugin
WASM Extension
```

### 第一版协议建议

使用现有 `IpcConnection` 作为传输层基础：

```text
length-prefixed JSON
```

然后在其上定义协议层：

```text
Handshake
Describe
Invoke
Response
Event
Cancel
Ping
Pong
Shutdown
```

当前 `src/ipc.rs` 已经有：

```rust
IpcMessage::Request
IpcMessage::Response
IpcMessage::Error
IpcMessage::Event
IpcMessage::Ping
IpcMessage::Pong
```

但还需要补：

- 协议版本
- Plugin ID
- 实现版本
- 能力声明
- 请求超时
- Capability 声明
- correlation ID
- 错误分类
- 取消语义
- 握手状态
- 最大 payload
- 心跳和关闭原因

### 推荐握手消息

```json
{
  "kind": "handshake",
  "protocol": "qh.plugin",
  "version": 1,
  "plugin_id": "example.context",
  "implementation": {
    "language": "python",
    "version": "0.1.0"
  },
  "capabilities": [
    "extension.context_compressor"
  ]
}
```

Core 返回：

```json
{
  "kind": "handshake_ack",
  "accepted": true,
  "core_protocol": 1,
  "granted_capabilities": [
    "extension.context_compressor"
  ]
}
```

### 为什么不用 FFI

不建议：

```text
Rust Core ↔ C ABI Plugin
```

因为这会引入：

- ABI 稳定性问题
- 内存所有权问题
- 崩溃污染 Core
- 多语言绑定成本
- 版本升级困难
- 安全边界弱化

更适合：

```text
Sidecar Process + IPC
```

这也符合你原先确定的进程级隔离原则。

## 六、Plugin 和 Extension 要分成两个概念

### Plugin

Plugin 是能力提供者，通常是独立进程。

例如：

```text
Python Browser Plugin
Go Git Plugin
Node MCP Plugin
Rust Local Model Plugin
```

它可以提供：

- Tool
- Adapter
- Extension
- Event Subscriber
- 外部协议桥接

### Extension

Extension 是 Core 某个明确扩展点的实现。

例如：

```text
ContextCompressor
PromptBuilder
CompletionRequestTransformer
CompletionResponseTransformer
ModelSelector
```

Extension 不一定是独立进程：

```text
Core 内置 Extension
Rust in-process Extension
WASM Extension
Sidecar Extension
```

建议定义如下层级：

```text
Extension Contract
    ↓
In-process Rust implementation
WASM implementation
Sidecar RPC implementation
```

也就是说，Extension 是能力契约，不是语言绑定。

## 七、不同扩展模式的语言支持

| 模式       | 适合实现               | 支持语言  | 是否允许远程 Sidecar |
| ---------- | ---------------------- | --------- | -------------------- |
| Strategy   | Prompt、压缩、模型选择 | 任意语言  | 是                   |
| Hook       | 审计、过滤、参数建议   | 任意语言  | 是                   |
| Decision   | 是否继续、是否切换     | 任意语言  | 是                   |
| Middleware | 重试、缓存、流程控制   | Rust/WASM | 默认否               |

关键边界：

> Sidecar 可以修改数据，但不能直接控制 Core 流程。

例如 Python Extension 可以修改请求：

```text
CompletionRequest
→ Python extension
→ 修改后的 CompletionRequest
→ Core 校验
→ Adapter
```

但 Python Extension 不能决定：

```text
是否无限重试
是否绕过取消
是否突破 max_tokens
是否绕过权限
是否改变审计
```

如果需要重试，应该发送一个“建议”：

```json
{
  "decision": "retry",
  "reason": "transient_provider_error"
}
```

最终是否重试由 Core 根据预算决定。

## 八、Adapter 修改能力的推荐分层

### 1. Provider Adapter Plugin

插件完整实现一个 Provider：

```text
describe
complete
health
```

适合：

- 本地模型
- 特殊 API
- 企业模型
- 非 OpenAI-compatible API

Core 控制：

- 超时
- 取消
- 模型
- 预算
- 审计
- Capability
- 进程生命周期

### 2. Request Transformer Extension

适合：

- 注入 system message
- 修改 temperature
- 自动选择模型
- Prompt 模板
- 上下文压缩
- 安全过滤

这是最适合多语言扩展的方式。

### 3. Response Transformer Extension

适合：

- 输出清洗
- 敏感信息过滤
- 格式转换
- 结果后处理
- 语言转换

### 4. Core Middleware

适合：

- Retry
- Cache
- Rate Limit
- 指标
- 成本控制

因为 Middleware 能控制调用流程，建议只允许：

```text
Core 内置
Rust in-process
WASM
```

不让普通远程插件直接实现流程型 Middleware。

## 九、如何判断某项能力“不应该进 Core”

建议每新增一个字段、Trait 或模块，都回答以下问题。

### 问题一：删除后 Core 还能完成基础对话吗？

如果不能，进入 Core。

例如：

```text
Message
CompletionRequest
CompletionResponse
Cancellation
Timeout
```

如果能，继续判断。

### 问题二：它是在描述机制，还是描述策略？

机制进入 Core：

```text
调用
取消
超时
状态所有权
IPC
权限验证
审计
```

策略进入 Extension：

```text
如何选模型
如何构造 Prompt
如何压缩上下文
如何重试
如何过滤响应
```

### 问题三：它是否改变安全不变式？

如果会改变以下内容，就不能完全交给插件：

```text
预算
取消
权限
审计
沙箱
进程生命周期
循环检测
最大深度
```

这些只能由 Core 强制。

### 问题四：它是否只是某个 Provider 的特殊语义？

如果是，放 Adapter Plugin。

例如：

```text
reasoning_content
thinking_budget
provider_role
response_format
tool_choice
```

不要把 Provider 特殊字段提升到 Core。

### 问题五：它是否需要控制调用流程？

如果只是修改数据：

```text
远程 Extension 可以实现
```

如果需要控制流程：

```text
Core Middleware 或 WASM
```

如果需要访问系统资源：

```text
Sidecar Plugin + Sandbox
```

## 十、建议的 Core v1 目录

可以将当前结构裁剪为：

```text
crates/qh_core/src/
├── lib.rs
├── config.rs
├── error.rs
├── runtime/
│   ├── core.rs
│   ├── session.rs
│   └── completion.rs
├── domain/
│   ├── id.rs
│   ├── message.rs
│   └── completion.rs
├── adapter/
│   ├── mod.rs
│   ├── builtin.rs
│   └── contract.rs
├── ipc/
│   ├── framing.rs
│   └── protocol.rs
├── security/
│   ├── capability.rs
│   └── audit.rs
└── extension/
    └── contract.rs
```

暂时不要把以下模块作为 Core v1 启动依赖：

```text
tool
sandbox backend
sidecar manager
plugin manager
extension registry
adapter factory registry
multimodal
reasoning
MCP
remote RPC
```

这些可以存在，但应当是：

```text
optional module
optional feature
lazy service
独立 crate
```

尤其是当前 `core.rs` 依赖了大量尚未实现的：

```text
Store
AuditService
PermissionService
SandboxManager
FileService
LlmAdapters
ToolRegistry
RootMsg
RootSupervisor
```

如果继续以这些组件作为 `AgentCore::bootstrap` 的必需字段，Core 永远无法收敛到最小完备集。建议把 `AgentCore` 先缩小为真正可运行的核心服务，而不是保留所有未来字段。

## 十一、推荐的发布版最小行为

发布时应满足：

```text
无插件目录
无 Extension
无 RPC
无工具
无多模态
无模型路由
无 Sidecar
无 WASM
```

仍然可以：

```text
启动 Core
读取 TOML
配置一个内置 OpenAI-compatible Adapter
创建 Session
发送 system/user/assistant 消息
获取文本补全
取消请求
处理超时
记录错误
正常关闭
```

一个最小配置可以是：

```toml
[adapter]
base_url = "https://api.example.com/v1"
model = "example-chat"
api_key_source = "environment:MODEL_API_KEY"
```

如果没有 Adapter 配置：

```text
Core 仍能启动
但完成请求返回 NoModelConfigured
```

这比“没有模型就无法启动”更适合作为基础运行时。

## 十二、接下来建议按这个顺序做

### 第一步：冻结 Core v1 Contract

先写一份不超过一页的协议定义：

```text
CoreRole = System | User | Assistant
CompletionRequest
CompletionResponse
CompletionError
Session
ModelId
RequestId
```

同时明确禁止：

```text
Tool
Vision
Audio
Reasoning
Custom Role
Provider-specific fields
```

### 第二步：裁剪当前 LLM 模型

重点处理：

```text
crates/qh_core/src/domain/llm.rs
crates/qh_core/src/llm/adapter.rs
crates/qh_core/src/config.rs
```

建议：

- `Role` 只保留三种
- `Message` 只保留 role/content
- Core 请求移除 tools
- Core 响应移除 tool_calls
- `AdapterCapabilities` 只保留 `streaming` 和 `max_context`，甚至第一版可以先移除
- Provider 枚举只保留 OpenAI-compatible
- 一个 active Adapter
- 一个默认模型
- `complete` 作为核心能力

### 第三步：实现单 Adapter 垂直切片

```text
Config
→ BuiltinAdapter
→ Session
→ CompletionRequest
→ HTTP
→ CompletionResponse
```

不要先实现：

```text
AdapterRegistry
AdapterFactory
AdapterMiddleware
动态切换
插件 Adapter
```

### 第四步：冻结跨语言协议

基于现有 `src/ipc.rs`，增加：

```text
protocol version
handshake
describe
invoke
response
error
cancel
ping/pong
```

先用 JSON，后续再增加 MessagePack/CBOR，不要一开始支持多套编码。

### 第五步：实现一个跨语言参考插件

建议使用 Python，测试以下能力：

```text
Python Plugin
→ 启动
→ 握手
→ 声明一个 Extension
→ 接收 CompletionRequest
→ 修改 system message
→ 返回结果
```

然后再实现 Node 或 Go SDK。

协议正确后，语言数量只是 SDK 工作量，而不是 Core 设计问题。

### 第六步：补兼容性测试矩阵

必须建立协议 Fixture：

```text
tests/protocol/
├── handshake.json
├── completion_request.json
├── completion_response.json
├── error.json
├── cancel.json
└── extension_invoke.json
```

所有语言 SDK 都读取同一组 Fixture：

```text
Python
Node
Go
Rust
```

这样兼容性由协议测试保证，而不是依赖某个语言的 Trait 实现。

## 最终建议

你当前最需要做的不是继续扩展 Adapter，而是完成一次“核心降维”：

```text
Core = 文本对话运行时
Plugin = 外部能力提供者
Extension = Core 策略插槽
Sidecar = 跨语言和进程隔离边界
Rust Trait = Core 内部机制
Wire Protocol = 跨语言公共契约
```

对于 `Role`，你的判断是正确的。Core v1 不需要追求最大兼容性，应明确只支持：

```text
system
user
assistant
```

兼容更多角色不是 Core 的职责，而是 Adapter Plugin 的协议转换职责。

当前最推荐的单一决策是：

> Core v1 只实现一个 OpenAI-compatible 文本补全 Adapter 和三种 Role；所有 Tool、Vision、Reasoning、特殊 Provider 字段和复杂流控都推迟到 Plugin/Extension 协议。

另外需要尽早决定一个细节：Core v1 对外是否必须原生流式输出。我的建议是先保证 `complete`，Adapter 内部允许 stream 并由 Core 聚合；等基础闭环稳定后，再把 `TextDelta` 作为协议 v1 的可选能力，而不是让流式事件阻塞最小实现。