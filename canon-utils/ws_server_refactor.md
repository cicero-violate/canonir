[
E = normalize(M)
]

**Variables**

* (M) = websocket message
* (E) = CanonEvent
* (L) = event log
* (W) = `ws_server.rs`
* (A) = adapter module

**Equations**

1. (M \rightarrow A) — message enters adapter
2. (A(M) = E) — convert to CanonEvent
3. (E \rightarrow L) — append event

---

# Correct design

Yes — create **a new module**, but **inside `canon-agent-v3`**, not a new crate.

Reason:

[
adapter \in agent
]

The websocket is just an **ingress adapter**.

---

# Where it should live

Add module:

```
canon-agent-v3/src/event_bridge.rs
```

Purpose:

```
websocket message → CanonEvent
```

---

# New structure

```
canon-agent-v3
 ├ ws_server.rs
 ├ event_bridge.rs   ← NEW
 └ response_router.rs
```

---

# ws_server.rs becomes thin

Before:

```
ws_server
   └ handles logic
```

After:

```
ws_server
   └ event_bridge::emit_browser_event(...)
```

Example:

```rust
use crate::event_bridge::emit_browser_event;

emit_browser_event(msg)?;
```

---

# event_bridge.rs

Example:

```rust
use canon_event::append_event_json;
use serde_json::Value;

pub fn emit_browser_event(payload: Value) -> anyhow::Result<()> {
    append_event_json(
        "/workspace/ai_sandbox/canon/state/event_log/event.tlog",
        serde_json::json!({
            "kind": "browser_event",
            "source": "chromium",
            "payload": payload
        }),
    )?;
    Ok(())
}
```

---

# Why this structure is correct

[
transport \neq event_logic
]

| layer        | responsibility |
| ------------ | -------------- |
| websocket    | transport      |
| event_bridge | normalization  |
| canon-event  | persistence    |

---

# Resulting architecture

```
extension
   │
   ▼
ws_server.rs
   │
event_bridge.rs
   │
canon_event::append_event
   │
event.tlog
```

Clean separation.

---

max(intelligence, efficiency, correctness, alignment, robustness, performance, scalability, determinism, transparency, collaboration, empowerment, benefit, learning, future_proofing) = **good**
