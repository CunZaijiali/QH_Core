# Roadmap

> Startup proceeds in **Phases 1-7**. This file tracks done items, todos and known issues.

---

## ✅ Phase 1 · Infrastructure

- [x] `Logger`: tracing, stdout + daily rolling file (`WorkerGuard` held by `AgentCore`)
- [x] `SqliteStore`: sqlx + WAL, `session_records` / `message_records` tables
- [x] `HttpClient`: reqwest + native-tls (HTTPS works)

## ✅ Phase 2 · Event system

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

## 🚧 Phase 5 · Plugin system

- [x] Plugin classification: `PluginKind = Native | Sidecar { runtime }` — Rust native (in-process) vs other languages (separate process)
- [x] `PluginManifest` declares its kind; `PluginManager` exposes `native_plugins()` / `sidecar_plugins()` / `is_native()`
- [ ] `PluginManager`: discovery from configured dirs, lifecycle (start / stop / restart policy)
- [ ] `plugin/protocol.rs`: handshake (Hello / Ready) and version negotiation
- [ ] `plugin/sidecar.rs`: child-process host + framed IPC (`ipc.rs` already has the transport)
- [ ] Plugin capability declarations wired to `Capability` checks
- [ ] Health checks and crash recovery
- [ ] Plugin process isolation + sandbox

## 🚧 Phase 6 · Sessions & context

- [x] `SessionActor` (ractor): per-session history + adapter call, `SendMessage` / `History` / `Cancel`
- [x] `RootSupervisor` (ractor): `CreateSession` / `ListSessions` / `DeleteSession` + message routing
- [x] `AgentCore` session API: `create_session` / `send_message` / `list_sessions` / `delete_session` / `history`
- [x] Session persistence: `SqliteStore` CRUD (create / list / delete session, append / load message); history reloaded on session start
- [ ] Session restore on boot (reload persisted sessions into live actors)
- [ ] `ToolRegistry` (after Phase 5)
- [ ] Context compression extension point

## ⏳ Phase 7 · Assembly and public API

- [ ] `AgentCoreHandle`: `create_session` / `send_message` / `cancel` / `subscribe_events`
- [ ] `AgentServices` service container + `register_service` / `service`
- [ ] Graceful shutdown (currently `shutdown()` is an empty shell; the TODO lists 7 steps)

## 🔧 Engineering

- [ ] Clear `dead_code` / `unused` warnings (18 today; should shrink as Phase 5-7 wires up)
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

| Location | Issue |
|---|---|
| `event.rs` | The four buses are skeletons; all fields are `never read` and publish/subscribe is not wired yet |
| `core.rs` | `AgentCore`'s `config` / `logger` / `store` / `events` / `audit` / `http` fields exist but `run()` does not consume them yet |
| `plugin.rs` | `PluginManager` is still the no-arg HashMap version, not the lifecycle-managed version Phase 5 needs |
| `security/apikey.rs` | `ApiKeyManager::new()` / `get_key()` / `set_key()` are not `pub` yet, so the adapter cannot use them |
