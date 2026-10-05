# 架构总纲

> 状态：**决策已定** · 关联：[core_contract.md](core_contract.md)、[plugin_runtime.md](plugin_runtime.md)
> 本文定义 Kernel 的边界与三层架构；plugin 细节见 `plugin_runtime.md`。

---

## 0. 一句话定位

> **Kernel 不是「插件做完后剩下的那些」，而是「任何插件都不允许拥有的东西」的集合。**

判定方法：只要问「把这件事交给插件，它会不会反过来控制系统」——会，就是 Kernel 的职责。

---

## 1. 三层架构

```
┌──────────────────────────────────────────────────────────┐
│  user plugins          (native / sidecar)                 │  用户扩展 / 覆盖
├──────────────────────────────────────────────────────────┤
│  bundled plugins       (default set, shipped with core)   │  默认携带，可裁剪
│  adapter · compressor · prompt · loop · tool · memory     │
├──────────────────────────────────────────────────────────┤
│  KERNEL                (hard-wired control)               │  不可绕过
│  lifecycle · permission · budget · cancel · timeout       │
│  audit · pipeline · registry · comms · host               │
└──────────────────────────────────────────────────────────┘
```

**默认值主张**：bundled 层**默认全集携带**（开箱即完整 agent），但每一件都可被 user 层覆盖或禁用。这与「最小安装 + 按需装」相反——我们选择「默认完整 + 可裁剪」。

---

## 2. Kernel 的 6 块职责

### ① 生命周期与装配（Assembly）
- 读配置 → 校验 → 装配
- 解析每个扩展点：**默认实现 / 配置覆盖 / 显式禁用**
- 启动与优雅关闭（顺序：先停插件，再停 Kernel）

### ② 不变量执行（Control Plane）
Kernel 存在的第一理由。在**每次调用**上强制，插件无法跳过：

```
permission.check() → budget.charge() → deadline.start() → ... → audit.write()
```

### ③ 调用管线（Orchestration）
编排一次对话经过哪些阶段、每阶段**谁实现**。Kernel 拥有**顺序**，插件只填**内容**（见 §3）。

### ④ 扩展点注册表（Registry）
- 「扩展点 → 实现」的唯一映射
- 内置默认、外部覆盖、显式禁用
- **唯一的插件入口**——插件只能从这里进来

### ⑤ 通信中枢（Comms）
- `ipc.rs`：跨语言 framing
- 协议层：握手 / invoke / cancel
- 四总线事件：Notification / Command / Stream / Broadcast
- **所有跨插件交互经此中转**（v1 不允许插件直连）

### ⑥ 插件宿主（Host）
- native 注册表的装配
- sidecar 子进程的拉起 / 监控 / 重启 / 停止
- 懒加载调度 + 健康检查

> 外加一份**最小领域模型**：`Message / Role / CompletionRequest / CompletionResponse / ID`。注意是**最小**——tool、vision、reasoning 都不在这里。

---

## 3. 调用管线（硬编码）

**决策：管线顺序硬编码在 Kernel，不可配置**（顺序一旦可配，就违背「插件不能改管线」）。

```
 user message
      │
 ┌────▼─────────────────── KERNEL ───────────────────┐
 │  1. locate / create Session      (Kernel 拥有)     │
 │  2. budget check                 (Kernel 强制)     │
 │  3. deadline start               (Kernel 强制)     │
 │                                                    │
 │  4. context_compressor   ◀── plugin                │
 │  5. prompt_builder       ◀── plugin                │
 │  6. request_transformer  ◀── plugin                │
 │                                                    │
 │  7. permission check             (Kernel 强制)     │
 │                                                    │
 │  8. adapter.complete     ◀── plugin                │
 │                                                    │
 │  9. response_transformer ◀── plugin                │
 │                                                    │
 │ 10. audit write                  (Kernel 强制)     │
 │ 11. persist                      (Kernel 拥有)     │
 │ 12. publish event        ◀── plugin subscribers    │
 └────────────────────────────────────────────────────┘
      │
 user reply
```

**规则**：

- 插件只能出现在 `◀──` 的位置；
- 第 1 / 2 / 3 / 7 / 10 / 11 是 Kernel 强制点，插件无从插入；
- 任何一步超时、超预算或被取消，Kernel 都能中止整条链；
- 插件可以「选择不参与」（如未注册 compressor→跳过第 4 步），但**不能改变顺序**。

---

## 4. 三个已定决策

| # | 决策点 | 结论 |
|---|---|---|
| 1 | Session 状态归属 | **Kernel 持有内存会话状态**（保证定序）；持久化可以是插件，但只能经 Kernel 的受控接口读写 |
| 2 | 管线表达 | **硬编码**；插件只能在固定位置选择「用 / 不用」 |
| 3 | Kernel 与默认插件的物理边界 | **独立 crate**（workspace 内） |

---

## 5. 物理布局（workspace）

```
qh_core/                          Cargo workspace
├── crates/
│   ├── qh_plugin_api/            插件接口层（最底层）
│   │   ├── extension points: ContextCompressor / PromptBuilder /
│   │   │   RequestTransformer / ResponseTransformer / ModelSelector /
│   │   │   LoopDecision / CompletionAdapter / ToolProvider / EventSubscriber
│   │   └── shared domain types: Message / Role / CompletionRequest / ...
│   │
│   ├── qh_core/                  KERNEL
│   │   ├── kernel/
│   │   │   ├── lifecycle.rs      bootstrap / shutdown / assembly
│   │   │   ├── control/          permission · budget · deadline · audit
│   │   │   ├── pipeline.rs       调用管线（顺序 + 强制点）
│   │   │   ├── registry.rs       扩展点注册表
│   │   │   ├── host/             native registry · sidecar supervisor · lazy load
│   │   │   └── comms/            ipc · protocol · four-bus events
│   │   └── ...
│   │
│   ├── qh_plugins/               默认插件集（bundled）
│   │   ├── adapter-openai/       OpenAI-compatible adapter
│   │   ├── compressor-sliding/   滑动窗口压缩
│   │   ├── prompt-default/       默认 prompt 构造
│   │   └── loop-budget/          预算 + 完成判定
│   │
│   ├── qh_macros/                过程宏（define_id）
│   └── qh_protocol/              协议文档与 Schema
```

**依赖方向**（必须无环）：

```
qh_plugin_api  ◀── qh_core (kernel)
       ▲        ◀── qh_plugins (default set)
       │        ◀── user plugins
       └── 只有这一层被双方共享
```

- `qh_plugin_api` 是**唯一的共享底层**：扩展点 trait + 共享领域类型；
- Kernel 依赖它，默认插件依赖它，用户插件也依赖它；
- **插件不依赖 Kernel**（拿不到 Kernel 内部句柄，物理上就杜绝了绕过控制）。

---

## 6. Kernel 明确不做

- 不实现任何**能力**（模型调用、压缩算法、prompt 模板）
- 不做**业务决策**（选哪个模型、要不要重试）
- 不持有**插件内部状态**
- 不解析**跨语言的数据内容**（只做信封）
- 不提供**插件直连插件**的通路

---

## 7. 待办与遗留

- `qh_plugin_api` 的 trait 签名需要逐个定稿（P5.7），建议**由默认插件驱动**：先写 `qh_plugins/*` 的实现，接口从真实需求长出来；
- 「完整 agent」的默认集边界（放哪些插件）待定：共识是 `adapter` / `compressor` / `prompt` / `loop` 必进；`tool` / `memory` 待评估；
- `ToolProvider` 与最小消息模型的关系见 `plugin_runtime.md` §6.1。
