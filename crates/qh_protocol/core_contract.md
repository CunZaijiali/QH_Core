# Core v1 Contract

> 版本 `v1` · 状态：冻结草案 · 关联文档：`protocols.md`（跨语言插件协议）、`capability_schema.json`（能力授权模型）

本文档冻结 `qh_core` 的**最小功能边界**：Core 到底必须知道什么、哪些内容一旦进入 Core 就会成为永久兼容负担。所有后续实现以本文档为基线。

## 1. 定位与分层

`qh_core` 是一个**不依赖任何插件即可运行的单模态文本对话运行时**。插件与 Extension 只负责让 Core「更强」，不负责让它「变得可运行」。

| 层 | 内容 | 稳定性 |
|---|---|---|
| **Core Kernel** | 最小领域模型 + 不可绕过的不变式（生命周期/权限/预算/取消/超时/审计） | 冻结后不轻易改 |
| **Core Protocol** | Core ↔ Provider / Plugin / Extension 的线协议 | 版本化，向后兼容 |
| **Optional Capabilities** | Plugin / Extension / Sidecar 提供的可替换能力 | 可增删，不进 Core |

## 2. 领域模型（冻结）

### Role

```rust
pub enum Role { System, User, Assistant }
```

只这三种。`tool` / `developer` / `function` / `reasoning` / `custom` 由 Provider Adapter 内部转换；无法转换时返回 `AdapterError::UnsupportedMessageRole`，**不改 Core 的 `Role` 枚举**。

### Message

```rust
pub struct Message {
    pub role: Role,
    pub content: String,
}
```

v1 不含 `name` / `tool_call_id` / `content_parts` / `image_url` / `audio` / `annotations` / `reasoning_content`。

### CompletionRequest

```rust
pub struct CompletionRequest {
    pub model: ModelId,
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}
```

v1 不含 `tools` / `response_format` / `parallel_tool_calls` / `top_p` / `logprobs` / `seed` / `provider_options`。

### CompletionResponse

```rust
pub struct CompletionResponse {
    pub content: String,
    pub finish_reason: FinishReason,
    pub usage: Option<Usage>,
}

pub enum FinishReason { Stop, Length, Cancelled, Error }

pub struct Usage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}
```

响应不含 `tool_calls`。

## 3. 身份与 ID

```rust
// 强类型 ID（由 qh_macros::define_id! 生成）
SessionId · ModelId · RequestId · TraceId
```

## 4. 生命周期（最小行为）

```
启动 → 读取配置 → 创建内存 Session → 接收 system/user/assistant 消息
     → 调用内置 Adapter → 返回文本 → 取消 / 超时 / 错误 → 优雅关闭
```

**无插件目录、无 Extension、无 RPC、无工具、无多模态时，Core 仍必须完成上述闭环。**

v1 暂不要求：完整 Actor 监督树、多租户、Session 恢复、分布式调度、动态模型切换、插件热更新、多存储后端。

## 5. Adapter 契约（最小）

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

- 内置**一个** OpenAI-compatible HTTP Adapter，经配置连 OpenAI / DeepSeek / 本地兼容服务 / 企业接口。
- 只维护一个 active adapter + 一个默认模型 + `complete`。
- `AdapterProvider` 只保留 `OpenAiCompatible`；不加 `DeepSeek` / `Anthropic` / `Gemini` / `Ollama` / `Azure` 枚举。
- Core 在 Adapter 之上封装：超时、取消、审计、错误映射。

最小配置：

```toml
[adapter]
base_url = "https://api.deepseek.com"
model = "deepseek-chat"
api_key_source = "environment:DEEPSEEK_API_KEY"
```

无 Adapter 配置时 Core 仍能启动，完成请求返回 `NoModelConfigured`。

**流式（v1 决定）**：对外先只保 `complete`；Adapter 内部可 stream 并由 Core 聚合。`TextDelta` 流式事件留作协议 v1 的**可选**能力，不阻塞最小实现。

## 6. 明确禁止项（不进 Core）

以下任何一项进入 Core 都会成为永久兼容负担，v1 一律排除，由 Plugin / Extension / Sidecar 承担：

| 类别 | 排除项 |
|---|---|
| 角色 | `Tool` / `Custom` / `Developer` / `Function` / `Reasoning` |
| 内容 | Image / Audio / Video / `ContentPart` / `MultimodalMessage` |
| 工具 | `ToolCall` / `ParallelToolCall` / `ToolSpec` / JSON Schema 工具参数 / MCP / A2A |
| 推理 | `ReasoningBlock` / `reasoning_content` / `thinking_budget` |
| Provider 字段 | `response_format` / `tool_choice` / `top_p` / `logprobs` / `seed` |
| 策略 | 上下文压缩 / Prompt 模板 / 工具选择 / 模型路由 / fallback / 重试 / 缓存 / 限流 |
| 能力 | RAG / 记忆 / 浏览器 / 代码沙箱 / 子 Agent / 多 Agent / 多模态 / 语音 |

## 7. 进 Core 判定表

新增任一字段 / Trait / 模块前，逐项回答：

| 问题 | 是 | 否 |
|---|---|---|
| 删了它基础文本补全还能完成吗？ | 不进（Plugin） | 继续判断 |
| 是否维护不可绕过的不变式（权限/预算/取消/超时/生命周期）？ | Core | 继续判断 |
| 是否只是某种策略？ | Extension | 继续判断 |
| 是否依赖某语言运行时 / 系统库？ | Sidecar Plugin | 继续判断 |
| 是否有多个合理实现？ | Extension/Plugin | Core |
| 是否需要流程控制权？ | Core / WASM | 远程插件不允许 |
| 删了是否只影响可选体验？ | Plugin | Core |

## 8. 兼容与版本策略

- 协议带版本号（`protocol = "qh.plugin"`，`version = 1`）。
- 能力用**字符串名**（`"completion.text"`、`"extension.context_compressor"`），不用 Rust 枚举作为跨语言协议；新增能力只加字符串，不升级所有 SDK。
- Core 领域模型改动走 schema 版本递增，破坏性改动需要迁移。
- 跨语言接口 = JSON Schema + 版本化 envelope；Rust Trait 只用于 Core 内部。
