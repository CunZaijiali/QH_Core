# qh_core

> A single-modal text chat runtime that runs **without requiring any plugin**.

`qh_core` is a minimal agent runtime written in Rust. Its trade-offs compress into one rule:

> **Only what cannot be removed without breaking basic chat, or without losing a security / lifecycle invariant, stays in the core.**

Tool calling, multimodality, model routing, RAG, memory, context compression, multi-agent — none of these live in the core. They are optional capabilities provided by plugins. The goal was never "fewer features". It is:

- it can start on its own;
- it can complete a basic text turn;
- it handles cancellation, timeouts, errors and shutdown correctly;
- it can load optional extensions, **but never requires a plugin to run**.

---

## Why another agent runtime?

"Everything is a plugin" is no longer a differentiator — most modern frameworks (editors, build
tools, agent harnesses) are pluggable. The interesting question is not *whether* a system is
extensible, but **which parts must be hard-wired, which must stay soft, and where the line is
drawn**.

`qh_core` draws the line in one place:

> **The boundary is not core-vs-plugin. It is control-vs-capability.**

Everything on the *control* side stays in the core — hard-wired and un-overridable. Everything on
the *capability* side is a plugin, **including the ones that ship inside the crate**.

### 1. Control is hard-wired

Permission, budget, cancellation, timeout, audit, loop detection, max depth: these never leave
the core and can never be overridden by a plugin.

> A plugin may **propose**; the core **decides**.
>
> A remote plugin can transform data, but it cannot decide whether to call the model, how many
> times to retry, whether to swallow an error, or whether to exceed the budget.

This is why the core keeps `CancellationToken`, timeouts and audit *inside* rather than handing
them to the extension layer.

### 2. Capabilities are soft — including the built-in ones

The model adapter, the context compressor, the prompt builder, the loop policy, the tool
registry: **all of them are plugins**. The ones shipped inside the crate go through **exactly
the same interface** as an external one. There is no privileged path.

```text
   built-in default ──┐
                      ├──▶  the same plugin interface  ──▶  core
   external native ───┤
   sidecar (any lang) ┘
```

Two consequences that are worth stating explicitly:

- **The interface is validated by the core itself.** If the default adapter can be written
  against the plugin interface, the interface is *provably* sufficient — no abstract
  completeness argument needed.
- **Swapping is uniform.** Replacing the built-in adapter with a third-party one is exactly the
  same operation as replacing a third-party one with another; and disabling it entirely is a
  valid configuration (the core still starts, `complete` just returns `NoAdapterForModel`).

### 3. Process boundary is a deployment choice, not a capability difference

`Native` (in-process Rust) and `Sidecar` (separate process, any language) implement the **same**
extension points and are interchangeable. The only asymmetry is deliberate:

> **Flow-control middleware (retry / cache / rate-limit) can only be in-process**, because
> something that controls the flow must not be a remote process.

Everything else — adapters, compressors, prompt builders, tool providers, event subscribers —
is available to either.

---

## Design

### 1. Three layers

```text
┌──────────────────────────────────────────────────────────┐
│  user plugins          (native / sidecar)                 │  free extension / override
├──────────────────────────────────────────────────────────┤
│  bundled plugins       (shipped with the core)            │  complete by default, trimmable
│  adapter · compressor · prompt · loop · tool · memory     │
├──────────────────────────────────────────────────────────┤
│  KERNEL                (hard-wired control)               │  non-bypassable
│  lifecycle · permission · budget · cancel · timeout ·     │
│  audit · pipeline · registry · comms · host               │
└──────────────────────────────────────────────────────────┘
```

| Layer | Responsibility | Stability |
|---|---|---|
| **Kernel** | Minimal domain model + non-bypassable invariants + invocation pipeline + extension registry + comms + plugin host | Frozen, changed sparingly |
| **Bundled plugins** | The default set (adapter / compressor / prompt / loop / …) shipped with the core | Released with the core |
| **User plugins** | Native / sidecar plugins supplied by the user | Fully free |

> The kernel is not "whatever is left after plugins". It is **the set of things no plugin is allowed to own**.
>
> Details in [architecture.md](crates/qh_protocol/architecture.md).

### 2. Capability boundary

**In Core** — test: removing it breaks the basic chat loop, or loses a security / control boundary.

```text
config loading & validation · lifecycle · minimal Session state
Message / Role (only system|user|assistant) / CompletionRequest / CompletionResponse
the built-in (default-plugin) OpenAI-compatible adapter · timeout · cancellation · layered errors
minimal logging & audit · IPC framing · protocol version & handshake
plugin process isolation · capability check entry point · extension-point registry
```

**Not in Core** — always delegated to a plugin.

```text
Tool Calling and Tool Role · Vision / Audio / Video · Reasoning fields
multiple provider enums · parallel tool calls · JSON Schema tool args
context compression / prompt templates / tool selection / model routing / fallback / retry / cache
RAG / memory / browser / code sandbox / sub-agent / MCP / A2A
```

Decision rules — run every new field / trait / module through these:

| Question | Yes | No |
|---|---|---|
| Can basic text completion still work without it? | Not in Core (Plugin) | Keep judging |
| Does it guard a non-bypassable invariant? | **Core** | Keep judging |
| Is it just a policy? | Plugin (extension point) | Keep judging |
| Does it depend on a language runtime / system library? | Sidecar plugin | Keep judging |
| Are there multiple reasonable implementations? | Plugin | **Core** |
| Does it need control over the flow? | **Core / Native only** | Remote plugin: no |

### 3. Role does not chase maximum compatibility

Core knows exactly three roles:

```rust
pub enum Role { System, User, Assistant }
```

`tool` / `developer` / `function` / `reasoning` are converted **inside** the provider adapter; if
the conversion is impossible it returns `AdapterError::UnsupportedMessageRole`. A provider's
private semantics never gets promoted into the core.

### 4. Cross-language plugins: protocol before SDK

The cross-language boundary is not a Rust trait; it is a **versioned wire protocol**:

```text
Rust Core  ──framed JSON──▶  Python / Node / Go / Rust / Java / C#  plugin
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

- **Capability model** (`capability.rs`): `subject / action / resource / constraint`, **explicit
  deny wins**; delegation cannot escalate (a child capability must be a subset of its parent).
- **Audit** (`audit.rs`): SQLite + SHA-256 hash chain, `append-only` enforced by database
  triggers, `verify_chain()` checks chain integrity.
- **Secrets** (`security/apikey.rs`): stored via the OS keyring, combined with `secrecy` /
  `zeroize` to avoid plaintext in memory.

---

## Status

Startup is built in **Phases 1-7**. Currently **Phases 1-4 are done and 5-6 are in progress** —
it compiles, runs, and holds a multi-turn conversation with persistence:

| Phase | Content | Status |
|---|---|---|
| 1 | Infrastructure: logging / SQLite storage / HTTP client | ✅ |
| 2 | Event system: four-bus skeleton | ✅ |
| 3 | Core services: hash-chain audit / key management | ✅ |
| 4 | Model adapter: OpenAI-compatible adapter + text completion | ✅ |
| 5 | Plugin runtime: classification / explicit list / lifecycle state machine | 🚧 |
| 6 | Sessions: ractor actors, multi-turn, persistence, restore on boot | ✅ |
| 7 | AgentCoreHandle assembly and public API | ⏳ |

See [ROADMAP.md](ROADMAP.md) and the [plugin runtime design](crates/qh_protocol/plugin_runtime.md).

---

## Quick start

Requires Rust 1.85+ (edition 2024).

```bash
# 1. Starts without any model configured (verifies lifecycle & schema creation)
cargo run -p qh_core

# 2. Configure an OpenAI-compatible model and send a message
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

Without an adapter configured the core still starts; `complete` simply returns
`NoAdapterForModel`. The built-in adapter is itself a plugin — it can be replaced or disabled
by configuration.

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
│   │       ├── plugin.rs       # plugin manifest / kind / lifecycle state
│   │       ├── http.rs         # HTTP client
│   │       ├── security/       # audit service + keyring secrets
│   │       ├── storage/        # SQLite storage
│   │       ├── domain/         # domain model (IDs / messages)
│   │       ├── runtime/        # session actors (SessionActor / RootSupervisor)
│   │       ├── llm/            # adapter contract + OpenAI-compatible impl
│   │       └── main.rs         # executable entry point
│   ├── qh_macros/              # proc macros (define_id)
│   └── qh_protocol/            # protocol docs & schemas
└── ROADMAP.md
```

---

## Reference

- [Architecture](crates/qh_protocol/architecture.md) — kernel boundary and the three-layer layout
- [Core v1 Contract](crates/qh_protocol/core_contract.md) — the frozen minimal boundary
- [Plugin runtime design](crates/qh_protocol/plugin_runtime.md) — kinds, lifecycle, extension points, defaults & override
- [Cross-language protocol](crates/qh_protocol/protocols.md) — plugin handshake and message frames
- [Capability schema](crates/qh_protocol/capability_schema.json)

## Roadmap

See [ROADMAP.md](ROADMAP.md).
