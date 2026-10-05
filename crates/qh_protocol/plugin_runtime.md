# 插件运行时设计（v0.4 · 决策已定）

> 状态：**决策已定，待实现** · 关联：[core_contract.md](core_contract.md)、[architecture.md](architecture.md)、[protocols.md](protocols.md)

---

## 0. 决策记录

| # | 决策点 | 结论 |
|---|---|---|
| 1 | Native 加载方式 | **静态注册**（编译进 Core，`with_plugin(...)` 注册） |
| 2 | 插件发现方式 | **显式列表**（配置里逐个列出，不扫目录） |
| 3 | v1 开放范围 | **开放全部扩展点**（为后续把 Core 扩展成个人助手做准备） |
| 4 | Sidecar 协议 | **统一 `ipc.rs`**（transport 与协议层都走它） |
| 5 | 启动时机 | **懒加载**（首次调用才拉起/初始化） |
| 6 | 参考插件语言 | **Rust / Node(TS) / Python** 三种 |
| 7 | 插件间调用 | **v1 不允许**；且任何跨插件交互都必须**走 Core 中转的通信**，不允许直接调用 |
| 8 | 默认能力 | **一切皆插件**（adapter / context compressor 等都是插件）；默认实现**随 Core 提供**，开箱可用，可被覆盖 / 重写 |
| 9 | Session 归属 | **Kernel 持有内存会话状态**（保证定序）；持久化可以是插件，但只能经 Kernel 受控接口 |
| 10 | 调用管线 | **硬编码**在 Kernel；插件只能在固定位置选择「用 / 不用」，不能改顺序 |
| 11 | 物理布局 | **独立 crate**：`qh_plugin_api`（接口）/ `qh_core`（kernel）/ `qh_plugins`（默认集） |

---

## 1. 设计目标

1. **Core 不依赖插件即可运行**——插件只让 Core「更强」。
2. **跨语言边界是协议**——`ipc.rs` 统一承载，Python / Node / Rust 用同一套消息。
3. **两类插件**：Rust 原生（进程内静态注册）与其他语言（独立进程）。
4. **插件不能绕过 Core 的控制**：权限、预算、取消、超时、审计始终由 Core 持有。
5. **插件是 Core 的扩展机制**：目标是靠插件把 Core 长成一个「切实可用的个人助手」。

---

## 2. 分类模型（已落地）

```rust
pub enum PluginKind {
    Native,                      // Rust 进程内，静态注册
    Sidecar { runtime: String }, // "python" / "node" / "rust" / ...
}
```

已在 `plugin.rs` 落地：`PluginKind` + `PluginManager::native_plugins()` / `sidecar_plugins()`。

---

## 3. 统一生命周期

```
Discovered ──▶ Loading ──▶ Handshaking ──▶ Ready ──▶ Running
                  │              │                      │
                  └──────────────┴──────▶ Failed ──────┘
                                                         │
                              Stopping ◀─────────────────┘
                                 │
                              Stopped
```

| 阶段 | Native | Sidecar |
|---|---|---|
| Loading | 静态注册（进程内，无需进程） | 懒加载时拉起子进程 |
| Handshaking | 无需 | `IpcMessage` 握手协商 |
| Ready | 立即可用 | 协议就绪 |
| Stopping | drop（无进程） | `Shutdown` + 宽限期内强杀 |

---

## 4. Native 插件（静态注册）

### 4.1 注册接口

```rust
/// Native 插件向 Core 注册自己提供的扩展点。
pub trait NativePlugin: Send + Sync + 'static {
    fn id(&self) -> &str;
    fn version(&self) -> &str;
    /// Core 装配阶段调用；只暴露可注册的扩展点，不暴露 Core 内部状态。
    fn register(&self, ctx: &mut PluginContext);
}

pub struct PluginContext<'a> { /* register_* 方法族 */ }
```

装配（示意）：

```rust
let core = AgentCoreBuilder::new(config)
    .with_plugin(MyCompressor)      // Native 插件
    .build()
    .await?;
```

### 4.2 Native 特有能力

- **Middleware**：retry / cache / 限流 / 指标——**只有 Core 内置或 Native 能实现**，因为要控制调用流程。
- 其余扩展点 Native 与 Sidecar 都能实现（见 §6）。

---

## 5. Sidecar 插件（统一 ipc）

### 5.1 分层（全部走 `ipc.rs`）

```
协议层  语义消息（Hello / Ready / Invoke / Response / Error / Cancel / Event / Shutdown）
   ▲
编码层  JSON（v1 仅此一种）
   ▲
传输层  src/ipc.rs —— 长度前缀帧 + IpcMessage 枚举
```

`ipc.rs` 现有的 `IpcMessage::{Request, Response, Error, Event, Ping, Pong}` 作为**帧类型**，协议层在其 `Request.method` / `Event.topic` 里区分语义（`hello` / `ready` / `invoke` / ...）。

### 5.2 握手

plugin → core：

```json
{ "kind": "request", "id": "1", "method": "handshake.hello",
  "payload": { "protocol": "qh.plugin", "version": 1, "plugin_id": "example.echo",
               "implementation": { "language": "python", "version": "0.1.0" },
               "capabilities": ["extension.context_compressor"] } }
```

core → plugin：

```json
{ "kind": "response", "id": "1",
  "result": { "granted": ["extension.context_compressor"],
              "limits": { "timeout_ms": 5000, "max_payload_bytes": 8388608 } } }
```

### 5.3 调用与取消

```json
{ "kind": "request", "id": "req-7", "method": "extension.invoke",
  "payload": { "point": "context_compressor", "args": { ... } } }

{ "kind": "response", "id": "req-7", "result": { ... } }
{ "kind": "error",    "id": "req-7", "code": 1001, "message": "..." }
{ "kind": "request",  "id": "cancel-7", "method": "invoke.cancel", "payload": { "target": "req-7" } }
```

- 每次调用带 correlation id；Core 施加 timeout，超时发 cancel 并丢弃迟到响应。

---

## 6. v1 开放的全部扩展点

> 这是本次最重要的范围变更：**不再只开放 Extension**。下面全部扩展点 v1 都开放，Native 与 Sidecar 都能实现（除标注外）。

| 扩展点 | 语义 | 谁可实现 |
|---|---|---|
| `ContextCompressor` | 上下文压缩 | Native / Sidecar |
| `PromptBuilder` | 构造 prompt | Native / Sidecar |
| `CompletionRequestTransformer` | 修改请求（messages / temperature / model 等） | Native / Sidecar |
| `CompletionResponseTransformer` | 修改响应（过滤 / 脱敏 / 规范化） | Native / Sidecar |
| `ModelSelector` | 建议用哪个模型 | Native / Sidecar |
| `LoopDecision` | 是否继续 / 是否重试（**建议**，最终由 Core 决定） | Native / Sidecar |
| `CompletionAdapter` | 远程 Provider（本地模型 / 特殊 API） | Native / Sidecar |
| `ToolProvider` | 提供工具 + 执行工具 | Native / Sidecar |
| `EventSubscriber` | 订阅 Core 事件 | Native / Sidecar |
| `Middleware` | retry / cache / 限流 / 成本控制 | **仅 Native / Core** |

**约束（不因开放而放弃）**：

- 插件只做「输入 → 输出」的数据变换，或提供「被 Core 调度的能力」；
- **流程控制权始终在 Core**：是否调用、调用几次、是否重试、是否吞错误，由 Core 决定；
- 涉及安全不变式的（预算 / 取消 / 权限 / 审计 / 沙箱 / 循环检测 / 最大深度）**只由 Core 强制**。

### 6.1 关于 `ToolProvider` 与最小消息模型的关系（需你留意）

这是「开放全部」带来的一个张力点：

- `core_contract.md` 明确规定 **Core 的 `CompletionRequest` 不含 `tools` / `tool_calls`**，`Role` 只有三种；
- 但 `ToolProvider` 要开放，意味着 Core 需要一个「工具执行闸门」。

我的处理方式（**不改最小消息模型**）：

1. Core 定义 `ToolProvider` 扩展点 + 一个**工具注册表**（只在 Core 内部，不进领域模型）；
2. 工具的实际调用由**插件或 Extension 触达**：例如插件通过 `EventSubscriber` 观察到需要工具，或 Core 未来定义独立的「工具调用协议 v2」；
3. Core 负责：工具调用的**闸门、超时、权限校验、审计**，但**不把 tool 语义写进 `CompletionRequest`**。

> 也就是说：**开放 ToolProvider ≠ 把 Tool Role 塞回 Core**。前者是「Core 提供执行闸门」，后者才是「污染最小契约」。如果你希望 v1 就让模型原生 tool_call，我们需要改 `core_contract.md`——这一点请你明确。

### 6.2 默认实现与覆盖机制（核心决策）

**一切皆插件**：`CompletionAdapter`、`ContextCompressor`、`PromptBuilder`、`LoopDecision`、`ModelSelector` 等**全部以插件接口实现**——**包括 Core 自带的默认实现**。

**默认插件**（编译进 Core，随发行版一起，默认启用）：

| 扩展点 | 默认实现 |
|---|---|
| `CompletionAdapter` | OpenAI-compatible HTTP adapter |
| `ContextCompressor` | `SlidingWindow` |
| `PromptBuilder` | `Default`（仅拼接 system / user / assistant） |
| `LoopDecision` | `BudgetAndCompletion` |
| `ModelSelector` | 唯一模型直选 |
| `ToolProvider` | 无（默认为空注册表） |

**覆盖顺序**：

```
① 配置显式指定插件（外部 native / sidecar）→ 覆盖默认
② 未指定 → 用 Core 自带的默认实现
③ 显式禁用 → 允许「无该能力」模式（如 adapter = none，则 complete 返回 NoAdapterForModel）
```

**约束**：

- 默认实现走**与外部插件完全相同**的接口，**没有任何特权路径**；
- 覆盖发生在装配阶段（bootstrap），运行期不可换（v1）；
- 默认实现的存在本身就是**对插件接口完备性的验证**——连默认 adapter 都能用插件接口写出，说明接口够用。

> 这一条同时守住了两个承诺：**架构上一切皆插件**（可替换、可重写），**产品上开箱即用**（默认实现编译进 Core）。

---

## 7. 能力授权集成

```
manifest.permissions ──▶ Capability(subject = plugin:<id>)
                                │
握手时              capability::evaluate(caps, request)
                                │
                        Decision::Allow / Deny / NotGranted
```

- `ResourcePattern::Extension { point }` 授权扩展点；`ResourcePattern::Plugin { target, actions }` 授权插件相关操作。
- 显式 `Deny` 优先；委托不允许越权；未 `granted` 的能力调用直接拒绝。

---

## 8. 进程与沙箱

| 项 | 做法 |
|---|---|
| 隔离 | Sidecar 独立子进程，崩溃不污染 Core |
| 超时 | 每次 invoke 带 deadline，超时即 cancel |
| 资源 | 复用 `SandboxPolicy`（内存 / CPU / 输出 / 进程数） |
| 启动 | **懒加载**：首次调用才拉起；`auto_start` 可主动预热 |
| 关闭 | `shutdown` → 宽限期 → 强杀 |

---

## 9. 配置与目录布局（显式列表）

```toml
[plugins]
enabled = true

# 显式列表：v1 不扫目录，逐个登记
[[plugins.entries]]
id = "example.compressor"
kind = "native"                       # 静态注册的登记名
entry = "compressor"                  # Native：注册表里的标识

[[plugins.entries]]
id = "example.echo"
kind = "sidecar"
runtime = "python"
entry = "./plugins/echo"              # 目录或入口脚本
auto_start = false                    # 默认懒加载

[plugins.lifecycle]
start_timeout = 10
shutdown_timeout = 10
restart_policy = "on_failure"         # never | on_failure | always
max_restarts = 3

[plugins.health]
enabled = true
interval = 10
timeout = 2
```

目录（建议，非强制）：

```text
plugins/
├── echo/                # python 参考插件
│   ├── plugin.toml
│   └── echo.py
├── echo-ts/             # node(ts) 参考插件
│   ├── plugin.toml
│   └── index.ts
└── echo-rs/             # rust 参考插件（独立进程）
    ├── plugin.toml
    └── Cargo.toml
```

---

## 10. 健康检查与重启

- **心跳**：Core 定期 `Ping`，插件回 `Pong`（复用 `IpcMessage::Ping/Pong`）。
- **崩溃**：子进程退出即 `Failed`，按 `restart_policy` 重启；超过 `max_restarts` 放弃并告警（`AuditEventKind::PluginCrash`）。

---

## 11. 分阶段落地计划

| 阶段 | 内容 | 依赖 |
|---|---|---|
| **P5.1** ✅ | 插件分类（`PluginKind`）+ manifest 声明 | 完成 |
| **P5.2** | 显式列表配置 + `PluginManager` 生命周期状态机 + 懒加载调度 | P5.1 |
| **P5.3** | Sidecar 宿主 `plugin/sidecar.rs` + `plugin/protocol.rs`（统一 ipc 上的协议层） | `ipc.rs` |
| **P5.4** | 参考插件 **Python**（echo + 一个 extension） | P5.3 |
| **P5.5** | 参考插件 **Node(TS)** | P5.3 |
| **P5.6** | 参考插件 **Rust**（独立进程） | P5.3 |
| **P5.7** | Native 注册接口 `NativePlugin` + `PluginContext` + 各扩展点 trait | P5.1 |
| **P5.8** | 能力授权联调（manifest → Capability → grants） | P5.2 / P5.3 |
| **P5.9** | 健康检查 + 重启策略 | P5.3 |

**物理布局**：Kernel 与默认插件的 crate 划分见 [architecture.md](architecture.md) §5 ——
`qh_plugin_api`（扩展点 trait + 共享领域类型）/ `qh_core`（kernel）/ `qh_plugins`（默认集）。
P5.7 的 trait 定义落在 `qh_plugin_api`，并且**由 `qh_plugins` 的实现驱动**（接口从真实需求长出来，不先抽象）。

---

## 12. v1 明确不做

- 插件热更新（重启 Core 才生效）
- 插件之间直接调用（必须经 Core 中转）
- 动态库 ABI 兼容
- 多编码协商（v1 只有 JSON）
- Middleware 型远程插件

---

## 13. 待确认

1. **`ToolProvider` 与最小消息模型**（§6.1）：v1 是否保持「Core 不含 tool 语义、只提供执行闸门」？若要模型原生 tool_call，需改 `core_contract.md`。
2. **扩展点 trait 的具体签名**：P5.7 需要逐个定 trait（`ContextCompressor::compress` 等），这部分我建议在实现 P5.7 时逐个出草案给你。
3. **显式列表的 `plugin.toml` 是否仍需要**：既然配置里已显式登记，manifest 是否只保留 `permissions` / `capabilities` 部分？
