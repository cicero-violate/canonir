# Repair Plan: canon-analyst Build Errors

Three errors, three fixes.

---

## Error 1 — `tempfile` not in Cargo.toml

**File:** `canon-utils/canon-analyst/Cargo.toml`

`tempfile` is in the workspace deps (`Cargo.toml` root) but was not declared in the
analyst crate. Add it:

```toml
[dependencies]
anyhow = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
canon_llm = { package = "canon-llm-runtime", path = "../canon-llm-runtime" }
canon_event = { package = "canon-runtime-events", path = "../canon-runtime-events" }
tokio = { workspace = true, features = ["full"] }
uuid = { workspace = true }
tempfile = { workspace = true }    # ← add this line
```

---

## Error 2 — `WsBridge::new()` does not exist

**File:** `canon-utils/canon-analyst/src/agent.rs`, line 56

`WsBridge` has no `new()` constructor. It is created only via `ws_server::spawn(addr,
timeout_secs, emitter)` which starts the WebSocket server that the browser extension
connects to.

The analyst must use the same bridge creation pattern as `canon-exec/src/exec/llm.rs`
lines 57–62:
- Read `CANON_LLM_BRIDGE_ADDR` env var, defaulting to `"127.0.0.1:9100"`
- Call `ws_server::spawn(addr, config.response_timeout_secs, Arc::new(OnceLock::new()))`
- The `OnceLock` emitter can be empty — the analyst does not emit events to the bus.

Replace line 56:
```rust
// BEFORE:
let bridge = WsBridge::new();

// AFTER:
let bridge_addr = std::env::var("CANON_LLM_BRIDGE_ADDR")
    .unwrap_or_else(|_| "127.0.0.1:9100".to_string());
let addr: std::net::SocketAddr = bridge_addr.parse()
    .unwrap_or_else(|_| "127.0.0.1:9100".parse().unwrap());
let ws_emitter: std::sync::Arc<std::sync::OnceLock<canon_event::EventEmitterHandle>> =
    std::sync::Arc::new(std::sync::OnceLock::new());
let bridge = canon_llm::ws_server::spawn(addr, config.response_timeout_secs, ws_emitter);
```

Also add the missing import at the top of `agent.rs`:
```rust
// BEFORE:
use canon_llm::ws_server::WsBridge;

// AFTER (replace):
// WsBridge is obtained via ws_server::spawn — no direct import needed.
// Remove the WsBridge import line entirely.
```

---

## Error 3 — `llm_worker_send_request` wrong argument count and order

**File:** `canon-utils/canon-analyst/src/agent.rs`, lines 78–95

The correct 14-argument signature (from `endpoint_worker.rs` line 163):
```
bridge, endpoint_id, url, stateful, prompt, role_schema,
node_id: Option<&str>, cache_key: Option<u64>, bust_cache: bool,
allow_req_id_mismatch: bool, phase: &str, tabs, max_tabs, tab_cooldown_ms
```

The current call passes 15 args and has two `"analyst"` strings where `node_id`
(Option<&str>) and `phase` (&str) should be, with booleans and None in wrong positions.

Replace the entire `llm_worker_send_request(...)` call block:

```rust
// BEFORE (lines 78–95):
let raw = llm_worker_send_request(
    &bridge,
    &endpoint.id,
    &endpoint.url,
    endpoint.stateful,
    &prompt,
    "",   // role_schema already embedded in first prompt
    "analyst",
    None,
    None,
    false,
    true,
    "analyst",
    &tabs,
    endpoint.max_tabs,
    config.tab_cooldown_ms,
)
.await?;

// AFTER:
let raw = llm_worker_send_request(
    &bridge,
    &endpoint.id,
    &endpoint.url,
    endpoint.stateful,
    &prompt,
    "",              // role_schema (embedded in prompt)
    None,            // node_id
    None,            // cache_key
    false,           // bust_cache
    true,            // allow_req_id_mismatch
    "analyst",       // phase
    &tabs,
    endpoint.max_tabs,
    config.tab_cooldown_ms,
)
.await?;
```

---

## Summary

| Error | File | Fix |
|---|---|---|
| E0433 `tempfile` unresolved | `canon-analyst/Cargo.toml` | Add `tempfile = { workspace = true }` |
| E0599 `WsBridge::new` not found | `agent.rs` line 56 | Replace with `ws_server::spawn(addr, timeout, emitter)` |
| E0061 wrong arg count/types | `agent.rs` lines 78–95 | Remove extra arg, fix `node_id`/`phase`/bool order |
