# qh_core

> A single-modal text chat runtime that runs **without requiring any plugin**.

`qh_core` is a minimal agent runtime written in Rust. All of its trade-offs compress into one rule:

> **Only what cannot be removed without breaking basic chat, or without losing a security / lifecycle invariant, stays in the core.**

Tool calling, multimodality, model routing, RAG, memory, context compression, multi-agent — none of these live in the core. They are optional capabilities provided by plugins and extensions. The goal was never "fewer features". It is:

- it can start on its own;
- it can complete a basic text turn;
- it handles cancellation, timeouts, errors and shutdown correctly;
- it can load optional extensions, **but never requires a plugin to run**.

---

## Design

### 1. Three layers

| Layer | Responsibility | Stability |
|---|---|---|
| **Core Kernel** | Minimal domain model + non-bypassable invariants (lifecycle / permissions / budget / cancellation / timeout / audit) | Frozen, changed sparingly |
| **Core Protocol** | Stable wire protocol between Core ↔ Provider / Plugin / Extension | Versioned, backward compatible |
| **Optional Capabilities** | Every replaceable capability from Plugin / Extension / Sidecar | Add/remove freely, never enters Core |

### 2. Capability boundary

**In Core** — test: removing it breaks the basic chat loop, or loses a security / control boundary.

```text
config loading & validation · lifecycle · minimal Session state
Message / Role (only system|user|assistant) / CompletionRequest / CompletionResponse
one built-in OpenAI-compatible adapter · timeout · cancellation · layered errors
minimal logging & audit · IPC framing · protocol version & handshake
plugin process isolation · Capability check entry point
```

**Not in Core** — always delegated to Plugin / Extension / Sidecar.

```text
Tool Calling and Tool Role · Vision / Audio / Video · Reasoning fields
multiple Provider enums · parallel tool calls · JSON Schema tool args
context compression / prompt templates / tool selection / model routing / fallback / retry / cache
RAG / memory / browser / code sandbox / sub-agent / MCP / A2A
```

Decision rules — run every new field / trait / module through these:

| Question | Yes | No |
|---|---|---|
| Can basic text completion still work without it? | Not in Core (Plugin) | Keep judging |
| Does it guard a non-bypassable invariant? | Core | Keep judging |
| Is it just a policy? | Extension | Keep judging |
| Does it depend on a language runtime / system library? | Sidecar Plugin | Keep judging |
| Are there multiple reasonable implementations? | Extension / Plugin | Core |
| Does it need control over the flow? | Core / WASM | Remote plugin: no |

### 3. Role does not chase maximum compatibility

Core knows exactly three roles:

```rust
pub enum Role { System, User, Assistant }
```

`tool` / `developer` / `function` / `reasoning` are converted **inside** the provider adapter; if the conversion is impossible it returns `AdapterError::UnsupportedMessageRole`. A provider's private semantics never gets promoted into the Core.

### 4. Cross-language plugins: protocol before SDK

The cross-language boundary is not a Rust trait; it is a **versioned wire protocol**:

```text
Rust Core  ──framed JSON──▶  Python / Node / Go / Rust / Java / C#  Plugin
```

- **Transport**: length-prefixed JSON (`ipc.rs`)
- **Protocol layer**: `Hello / Ready / Describe / Invoke / Cancel / Shutdown / Health`
- **Capability names**: strings (`completion.text`, `extension.context_compressor`), not Rust enums — adding a capability never requires upgrading every SDK
- **Fixed build order**: protocol doc → JSON Schema → golden fixtures → Rust host → language SDKs. Never the other way around.

### 5. Four-bus event model

Four unified event primitives, in-process and cross-process (`event.rs`):

| Bus | Semantics |
|---|---|
| **Notification** | point-to-point / multicast notification |
| **Command** | request-reply with deadline and cancellation |
| **Stream** | ordered stream with multiple subscribers |
| **Broadcast** | persisted broadcast (offline subscribers can catch up) |

### 6. Capability authorization + verifiable audit

- **Capability model** (`capability.rs`): `subject / action / resource / constraint`, **explicit deny wins**; delegation cannot escalate (a child capability must be a subset of its parent).
- **Audit** (`audit.rs`): SQLite + SHA-256 hash chain, `append-only` enforced by database triggers, `verify_chain()` checks chain integrity.
- **Secrets** (`security/apikey.rs`): stored via the OS keyring, combined with `secrecy` / `zeroize` to avoid plaintext in memory.

---

## Status

Startup is built in **Phases 1-7**. Currently **Phases 1-4 are done** — it compiles and runs:

| Phase | Content | Status |
|---|---|---|
| 1 | Infrastructure: logging / SQLite storage / HTTP client | ✅ |
| 2 | Event system: four-bus skeleton | ✅ |
| 3 | Core services: hash-chain audit / key management | ✅ |
| 4 | Model adapter: OpenAI-compatible adapter + text completion | ✅ |
| 5 | Plugin system | ⏳ |
| 6 | Sessions & context (ractor supervision tree) | ⏳ |
| 7 | AgentCoreHandle assembly and public API | ⏳ |

See [ROADMAP.md](ROADMAP.md) for details.

---

## Quick start

Requires Rust 1.85+ (edition 2024).

```bash
# 1. Starts without any model configured (verifies lifecycle & schema creation)
cargo run -p qh_core

# 2. Configure an OpenAI-compatible model and send one message
#    Windows PowerShell:
$env:DEEPSEEK_API_KEY = "sk-..."
cargo run -p qh_core
```

TOML configuration example:

```toml
[[adapters]]
id = "deepseek"
provider = "open_ai_compatible"
enabled = true
models = ["deepseek-chat"]
base_url = "https://api.deepseek.com"
api_key_source = { environment = "DEEPSEEK_API_KEY" }
```

Without an adapter configured the core still starts; `complete` simply returns `NoAdapterForModel`.

---

## Layout

```text
qh_core/                        # Cargo workspace
├── crates/
│   ├── qh_core/                # main runtime crate
│   │   └── src/
│   │       ├── core.rs         # AgentCore + the Phase 1-7 startup
│   │       ├── config.rs       # full TOML config & validation
│   │       ├── capability.rs   # capability authorization model
│   │       ├── audit.rs        # hash-chain audit
│   │       ├── event.rs        # four-bus events
│   │       ├── ipc.rs          # cross-language IPC framing
│   │       ├── plugin.rs       # plugin manifests
│   │       ├── http.rs         # HTTP client
│   │       ├── security/       # audit service + keyring secrets
│   │       ├── storage/        # SQLite storage
│   │       ├── domain/         # domain model (IDs / messages)
│   │       ├── llm/            # adapter contract + OpenAI-compatible impl
│   │       └── main.rs         # executable entry point
│   ├── qh_macros/              # proc macros (define_id)
│   └── qh_protocol/            # protocol docs & schemas
└── ROADMAP.md
```

---

## Reference

- [Core v1 Contract](crates/qh_protocol/core_contract.md) — the frozen minimal boundary
- [Cross-language protocol](crates/qh_protocol/protocols.md) — plugin handshake and message frames
- [Capability schema](crates/qh_protocol/capability_schema.json)

## Roadmap

See [ROADMAP.md](ROADMAP.md).
