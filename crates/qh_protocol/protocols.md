# Protocols

## Basic Message Frame

```rust
Request {
    id: String,
    method: String,
    payload: serde_json::Value,
},
Response {
    id: String,
    result: serde_json::Value,
},
Error {
    id: Option<String>,
    code: i64,
    message: String,
    data: Option<serde_json::Value>,
},
Event {
    topic: String,
    payload: serde_json::Value,
},
Ping {
    nonce: u64,
},
Pong {
    nonce: u64,
},
```

## Handshake Protocol

- core declare capabilities
- plugins should declare capabilities

plugin:

```json
{
    "kind": "hello",
    "protocol": "qh.plugin",
    "version": 1,
    "plugin_id": "example.write_hello_world",
    "runtime": "python",
    "capabilities":[
        {
            "id": "cap_write_hello_world",
            "action": ["write"],
            "resource":{ "type": "file", "path": "./tmp/hello_world.py", "max_size": 1024 },
            "constraints":[
                 { "type": "session", "id": "sess-42" },
                { "type": "audit_level", "level": "verbose" }
            ],
            "valid_from": 10000,
            "valid_until": 19000,
            "delegatable": false,
            "signature": "abc123",
            "sesitivity": "critical"
        }
    ]
}
```

core:

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