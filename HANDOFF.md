# QH Assistant — 交接上下文

> **写给新会话**：这份文档自包含，读完就能接着干，不需要回溯聊天记录。
> 最后更新：2026-10-06 · 项目根：`D:\XiangMu\QH_Assistant` · 代码根：`qh_core/`

---

## 0. 一句话现状

`qh_core` 已从「一个想和 DSH 竞争的 agent runtime」**重新定位为「个人项目的基石 + QH 系列的内核」**。

代码进度：**Phase 1-4 与 Phase 6 完成，Phase 5（插件运行时）进行中**。可编译、可运行、21 个测试通过、已推 GitHub。

---

## 1. 定位的演变（最重要，先读这节）

### 1.1 起点

最初目标是做「一个 agent runtime」，希望它有**独特性**、并且对**他人**也有价值。

### 1.2 探索与结论

经过一轮系统分析（对照 DSH / Cordis、LangChain、Claude Code、MCP、rig/swiftide 等），得到几个诚实的结论：

- **设计范式上没有首创**：薄内核 + 一切皆插件（DSH/Cordis）、跨语言协议（MCP）、能力授权（OS / WASM）、哈希链审计——全部有先例
- **「控制硬、能力软」不构成差异**：多数框架的控制本来就不给普通插件替换；DSH 反而更彻底
- **通用 runtime 处于「平台层」**：赢家通吃，个人/小团队的胜算极低
- **扫描出的可能空位**（仅记录，未采纳）：编译期强制的控制边界、单二进制 + 默认完整、多语言对等的 runtime 扩展点、能力安全在 agent 层的落地

### 1.3 决定

**降级为个人项目**，定位改为：

> **qh_core = 个人开发的基础 kernel + QH 项目系列的基石。**

含义：

- 不再追求「独特」，也不对标通用框架
- 追求「**自己拥有 + 可任意扩展**」
- 未来在它之上做**垂直应用**（沙箱、debug 等）与个人系列产品
- 成功标准是「**一直没停**」，而不是「做完了」或「有人用了」

### 1.4 相关现实背景（影响决策节奏）

- 用户同时面临**就业/变现**压力，kernel 是长线（悲观估计：1 年自己用顺，2 年+ 别人能用）
- **两者必须解耦**：kernel 不承担赚钱 KPI，否则会因为「救不了急」被放弃
- 变现主线另走（国际远程/外包、AI 生态小产品、高门槛技术服务），不在本项目的范围内

---

## 2. 技术聚焦（接下来重点做什么）

用户明确指定接下来的重心：

### 2.1 事件传播模型 ⭐ 重点

当前 `event.rs` 只有**四总线骨架**（Notification / Command / Stream / Broadcast），所有字段 `never read`，发布/订阅尚未实现。

要做：

- 发布 / 订阅 / 背压策略
- 持久化（离线订阅者上线可补收）
- 与调用管线的集成（谁在什么阶段发事件）

**参考**：DSH 的五档分发（`emit` / `waterfall` / `parallel` / `serial` / `bail`）比四总线更精确，其中 `waterfall`（环绕中间件）和 `bail`（短路）值得借鉴。

### 2.2 多语言插件系统 ⭐ 重点

- **Native**：Rust 进程内，静态注册
- **Sidecar**：其他语言，独立进程 + framed IPC
- **关键主张**：native 与 sidecar 实现**同一批扩展点**、能力对等；唯一的刻意不对称是 **middleware（流程控制）只能进程内**
- 协议层建在现有 `ipc.rs` 之上（复用 transport）
- 参考插件语言：**Python → Node(TS) → Rust**

### 2.3 MCP 支持

- MCP 作为一种插件 / 工具来源接入
- **待设计**：MCP 与自有多语言插件协议的关系（复用 MCP？桥接？还是只把 MCP 当 ToolProvider 的一种）

---

## 3. 当前代码状态

### 3.1 进度

| Phase | 内容 | 状态 |
|---|---|---|
| 1 | 基础设施：logging / SQLite / HTTP client | ✅ |
| 2 | 事件系统：四总线**骨架**（未接线） | 🚧 |
| 3 | 核心服务：哈希链审计 / keyring 密钥 | ✅ |
| 4 | 模型适配器：OpenAI 兼容 adapter | ✅ |
| 5 | 插件运行时：分类 ✅ / 显式列表 ✅ / 状态机 ✅ / 宿主与协议 ⏳ | 🚧 |
| 6 | 会话：ractor actors / 多轮 / 持久化 / 启动恢复 | ✅ |
| 7 | AgentCoreHandle 组装与对外 API | ⏳ |

### 3.2 规模

- 约 5k 行 Rust，21 个单元测试通过
- 3 个提交，已推 `git@github.com:CunZaijiali/QH_Core.git`（`main` = `af9ad85`）

### 3.3 已落地能力

配置加载校验 · 四级事件总线骨架 · 哈希链审计（append-only + verify_chain）· keyring 密钥 · OpenAI 兼容 adapter（HTTPS）· 会话（actor + 多轮 + SQLite 持久化 + 重启恢复）· 插件分类 / 显式列表 / 生命周期状态机

### 3.4 已知问题

| 位置 | 问题 |
|---|---|
| `event.rs` | 四总线未接线，字段全部 `never read` |
| `core.rs` | `AgentCore` 多数字段尚未被 `run()` 消费 |
| `plugin.rs` | `PluginManager` 只到生命周期状态，无 host / disposer |
| `security/apikey.rs` | `ApiKeyManager` 未 `pub`，未接入 adapter 取密钥路径 |
| 全局 | 约 26 个 `dead_code` / `unused` warning |

---

## 4. 已定架构决策（**不要违反**）

| # | 决策 | 内容 |
|---|---|---|
| 1 | **Kernel 界定** | Kernel 不是「插件做完后剩下的」，而是「**任何插件都不允许拥有的东西**」的集合 |
| 2 | **控制硬、能力软** | 权限 / 预算 / 取消 / 超时 / 审计 / 管线顺序**锁在 Kernel**；能力全部可插件替换 |
| 3 | **默认实现也是插件** | adapter / compressor / prompt / loop **全部走插件接口**，随 Core 提供，可被覆盖或禁用 |
| 4 | **管线硬编码** | 对话阶段顺序固定，插件只能在固定位置「选择参与」，不能改顺序 |
| 5 | **Session 归 Kernel** | Kernel 持有内存会话状态（保证定序）；持久化可为插件，但经 Kernel 受控接口 |
| 6 | **物理布局：独立 crate** | `qh_plugin_api`（接口）/ `qh_core`（kernel）/ `qh_plugins`（默认集）/ + user plugins |
| 7 | **依赖方向无环** | 插件**不依赖 Kernel**（物理上杜绝绕过控制） |
| 8 | **插件发现用显式列表** | 配置里 `[[plugins.entries]]` 逐条登记，v1 不扫目录 |
| 9 | **懒加载** | 插件默认懒加载，`auto_start` 可预热 |
| 10 | **v1 不允许插件间调用** | 任何跨插件交互**必须经 Core 中转** |

### 调用管线（Kernel 强制点）

```
locate/create Session → budget check → deadline start
  → context_compressor → prompt_builder → request_transformer
  → permission check
  → adapter.complete
  → response_transformer
  → audit write → persist → publish event
```

`Session / budget / deadline / permission / audit / persist` 是 Kernel 强制点，插件无从插入。

---

## 5. 接下来的安排（建议顺序）

### 5.1 先做：workspace 拆分（为插件铺路）

把 crate 骨架立起来，后续插件代码才有归属：

```
crates/
├── qh_plugin_api/     扩展点 trait + 共享领域类型（最底层，双方共享）
├── qh_core/           Kernel
├── qh_plugins/        默认插件集（adapter-openai / compressor-sliding / prompt-default / loop-budget）
├── qh_macros/         （已有）
└── qh_protocol/       （已有，文档与 Schema）
```

### 5.2 再做：`qh_plugin_api` 的 trait

**由默认插件驱动**——先写 `qh_plugins/*` 的实现，接口从真实需求长出来，**不要先抽象**。

### 5.3 然后：事件传播模型（用户重点）

补齐 `event.rs` 的发布 / 订阅 / 背压 / 持久化，并与管线集成。

### 5.4 然后：Sidecar 宿主 + 协议

- `plugin/protocol.rs`：握手（hello / ready）+ invoke / cancel
- `plugin/sidecar.rs`：子进程宿主 + 懒加载调度
- 第一个参考插件（Python）

### 5.5 最后：MCP 接入

设计 MCP 与自有插件协议的关系，接入为一个 ToolProvider 或独立插件类型。

---

## 6. 命名与品牌

- **QH** 是用户的个人名字，全网未见第二人使用
- crates.io 上 **`qh-*`（带连字符）目前看起来是空的**（`cargo search qh` 里只有 `qhull` / `qhook` / `qht` / `qhyccd-*` / `qhermes-*` 等）
- **但 `cargo search` 不是权威判据**——要确认需 `cargo info <name>` 或访问 `https://crates.io/crates/<name>`（404 = 可用）
- **不要抢注**：crates.io [政策](https://blog.rust-lang.org/2023/09/22/crates-io-usage-policy-rfc/)明确禁止 name squatting
- 正确做法：**发布第一个可用版本时，把同 workspace 的成员一起注册**
- 品牌优先级：**GitHub 组织名 > 域名 > crates.io 前缀**

---

## 7. 参考文档（项目内）

| 文档 | 作用 |
|---|---|
| `README.md` | 门面：定位 + control-vs-capability + 三层架构 |
| `ROADMAP.md` | 进度（Phase 1-7） |
| `crates/qh_protocol/architecture.md` | **架构总纲**：Kernel 界定法 + 三层 + 管线 + 依赖方向 |
| `crates/qh_protocol/core_contract.md` | 最小契约：领域模型 + Session 归属 + 管线 |
| `crates/qh_protocol/plugin_runtime.md` | 插件设计 v0.4：分类 / 生命周期 / 扩展点 / 默认集 |
| `crates/qh_protocol/protocols.md` | 跨语言协议（消息帧 + 握手） |

**外部参考**（DSH 源码，用户提供）：`D:\XiangMu\OT@deepseek-harness\deepseek-harness-master`
重点读：`docs/architecture.zh.md`、`docs/cordis-primer.zh.md`

---

## 8. 环境注意事项（避免重复踩坑）

- **沙箱**：编译 `cargo` 需要写 `qh_core/target`，受限模式下会失败，需完全权限（`danger-full-access`）
- **`web_fetch` 在本机不可用**（返回 non-public IP），要查网页请改用 `web_search`
- **ractor 0.16 的坑**（已踩过）：
  - 必须启用 feature `async-trait`
  - `pre_start` / `handle` 的 `myself` 参数**不带 `&`**
  - 启动是 `Actor::spawn(None, MyActor, args)`，不是 `MyActor::spawn(...)`
- 已写入全局记忆片：`rust-ractor.md`、`pitfalls.md`

---

## 9. 一句话叮嘱

> 这个项目的价值**不来自「独特」**，来自「**它是你的，而且能一直长下去**」。
>
> 不要再为「和前人有何不同」焦虑；把精力放在**事件模型**和**多语言插件**这两件真正想做的事上。
