# Roadmap

> `qh_core` is the kernel of the **QH series**: the shared runtime layer that QH applications are built
> on. This file tracks what is done, what is next, and what is still in flight.
>
> Delivery proceeds in **Phases 1-7**. Architecture (kernel boundary + three layers) is defined in
> [crates/qh_protocol/architecture.md](crates/qh_protocol/architecture.md).

---

## 🎯 Current focus

Three areas are the declared focus of the next stretch of work, in this order:

1. **Event propagation model.** `event.rs` today defines the four buses (Notification / Command /
   Stream / Broadcast) as a skeleton. Still to add: publish / subscribe, backpressure, persistence
   so offline subscribers can catch up, and integration with the invocation pipeline.
2. **Multi-language plugin system.** `Native` (in-process Rust, static registration) and `Sidecar`
   (separate process, framed IPC) must implement the *same* extension points with equal capability.
   The only deliberate asymmetry is that flow-control middleware stays in-process. Reference plugin
   languages: Python → Node (TS) → Rust.
3. **MCP support.** MCP should be admitted as a plugin / tool source. Open question: whether it
   reuses the MCP protocol directly, bridges to the native plugin protocol, or is admitted only as
   one more `ToolProvider`.

Planned order of work: workspace split for plugins → `qh_plugin_api` traits (driven by the default
plugins, not abstracted up front) → event model → sidecar host + protocol → MCP.

---

## ✅ Phase 1 · Infrastructure

- [x] `Logger`: tracing, stdout + daily rolling file (`WorkerGuard` held by `AgentCore`)
- [x] `SqliteStore`: sqlx + WAL, `session_records` / `message_records` tables
- [x] `HttpClient`: reqwest + native-tls (HTTPS works)

## 🚧 Phase 2 · Event system

- [x] Four-bus skeleton: `NotificationBus` / `CommandBus` / `StreamHub` / `BroadcastHub`
- [x] `EventService::new(&EventsConfig)` assembly
- [x] Event-domain IDs: `EventId` / `TraceId` / `RequestId` / `ConsumerId` / `StreamId` / `SubscriptionId`
- [ ] `publish` / `subscribe` / backpressure per bus
- [ ] `NotificationStore` / `BroadcastStore` persistence (offline catch-up)

## ✅ Phase 3 · Core services

- [x] `AuditStore`: SHA-256 hash chain, append-only triggers, `verify_chain()`
- [x] `AuditService` wrapper (`security.rs`)
- [x] `ApiKeyManager`: OS keyring + `secrecy` / `zeroize` (`security/apikey.rs`)
- [ ] `PermissionService`: wire in `capability::evaluate` as the plugin gate
- [ ] `SandboxManager`: process sandbox policy (`SandboxPolicy` already in config)
- [ ] `FileService`: workspace path validation + I/O (`fs.rs` is an empty shell)

## ✅ Phase 4 · Model adapter

- [x] `CompletionAdapter` trait: `model()` + `complete(request, cancel)`
- [x] `OpenAiCompatibleAdapter`: HTTP `chat/completions`, `finish_reason` / `usage` mapping, cancellation
- [x] `AgentCore::complete()` vertical slice (config → adapter → HTTP → response)
- [ ] Adapter health check & retry (`max_retries` is in config but unused)
- [ ] Multiple adapter registration & switching (`LlmAdapterHandler`)

## 🚧 Phase 5 · Plugin runtime

- [x] Plugin classification: `PluginKind = Native | Sidecar { runtime }` — Rust native (in-process) vs other languages (separate process)
- [x] Explicit plugin list in config (`[[plugins.entries]]`; no directory scanning in v1)
- [x] `PluginState` lifecycle machine (Discovered → Loading → Handshaking → Ready → Stopping → Stopped / Failed)
- [x] Workspace split, step 1: `qh_plugins` crate added to the workspace (still a skeleton)
- [ ] Workspace split, step 2: `qh_plugin_api` (extension traits + shared domain types) as the
      bottom layer shared by kernel and plugins
- [ ] `qh_plugin_api`: extension-point traits, **driven by the default plugins** (the interface grows out of real needs, not up-front abstraction)
- [ ] `qh_plugins`: the default set — `adapter-openai` / `compressor-sliding` / `prompt-default` / `loop-budget`
- [ ] `plugin/protocol.rs`: handshake + invoke / cancel on top of `ipc.rs`
- [ ] `plugin/sidecar.rs`: child-process host + lazy-load scheduling
- [ ] Reference plugins: Python → Node(TS) → Rust
- [ ] Capability declarations wired to `Capability` checks
- [ ] Health checks + restart policy

## ✅ Phase 6 · Sessions & context

- [x] `SessionActor` (ractor): per-session history + adapter call, `SendMessage` / `History` / `Cancel`
- [x] `RootSupervisor` (ractor): `CreateSession` / `ListSessions` / `DeleteSession` + message routing
- [x] `AgentCore` session API: `create_session` / `send_message` / `list_sessions` / `delete_session` / `history`
- [x] Session persistence: `SqliteStore` CRUD (create / list / delete session, append / load message); history reloaded on session start
- [x] Session restore on boot (`RootSupervisor` reloads persisted sessions from `SqliteStore`)
- [ ] `ToolRegistry` (after Phase 5)
- [ ] Context compression extension point

## ⏳ Phase 7 · Assembly and public API

- [ ] `AgentCoreHandle`: `create_session` / `send_message` / `cancel` / `subscribe_events`
- [ ] `AgentServices` service container + `register_service` / `service`
- [ ] Graceful shutdown (currently `shutdown()` is an empty shell; the TODO lists 7 steps)

## 🔧 Engineering

- [ ] Clear `dead_code` / `unused` warnings (10 today, from `cargo check`; should shrink as Phase 5-7 wires up)
- [ ] Finish the `BootstrapError` variant consolidation and update the remaining call sites (see Known issues)
- [ ] Fill in unit tests (`capability` / `audit` / `ipc` / `qh_macros` have tests; the rest don't)
- [ ] Integration tests: protocol golden fixtures (`tests/protocol/*.json`)
- [ ] CI: `cargo fmt` / `cargo clippy` / `cargo test`
- [ ] Repo hygiene: `.gitignore` covers `target/`, `.idea/`

## 📄 Protocol docs

- [x] [Core v1 Contract](crates/qh_protocol/core_contract.md) — the frozen minimal boundary
- [x] [protocols.md](crates/qh_protocol/protocols.md) — message frames + handshake
- [x] [capability_schema.json](crates/qh_protocol/capability_schema.json) — capability authorization schema
- [ ] Protocol golden fixtures (shared by language SDKs)

---

## Known issues

The error layer is being consolidated: `BootstrapError` is being reduced to a smaller set of variants,
and a few construction sites still use variants from the older enum. Those are expected to be updated
as the consolidation lands.

| Location | Issue |
|---|---|
| `error.rs` → `core.rs:79`, `http.rs:15` | A few `BootstrapError::*` construction sites still reference variants from the older, larger enum, so a clean build waits on that consolidation |
| `event.rs` | The four buses are skeletons; publish / subscribe is not wired yet and most fields are unused |
| `core.rs` | `AgentCore`'s `config` / `events` / `audit` / `http` fields exist but `run()` does not consume them yet |
| `plugin.rs` | `PluginManager` only reaches the lifecycle state machine; there is no host / disposer yet |
| `core/handle.rs`, `core/shutdown.rs`, `plugin/{manager,manifest,protocol,sidecar}.rs`, `cli.rs`, `context.rs`, `domain/context.rs`, `token.rs` | Empty placeholder modules |
| `security/apikey.rs` | `ApiKeyManager::new()` / `get_key()` / `set_key()` are not `pub` yet, so the adapter cannot use them |
| `qh_plugins/` | The default plugin set is a `Hello, world!` skeleton; the real plugins land with `qh_plugin_api` |
