# Implementation Plan 01 — Hooks / Middleware Pipeline

## Status

**Partially implemented.** `hooks.rs` was created and compiles with 4 errors.
This plan documents the original design and the exact fixes for each error.

---

## Compile errors to fix (in `canon-utils/canon-runtime/src/hooks.rs`)

### Error 1 — Unused import: `Arc` (line 2)

`Arc` is in the import but `HookChain` is not Arc-wrapped in the current
implementation. Remove it.

**Change line 2** from:
```rust
use std::sync::{Arc, Mutex};
```
To:
```rust
use std::sync::Mutex;
```

---

### Error 2 — `CapabilityConfig` has no `hooks` field

`CapabilityRateLimitHook::from_config` and `CostCapHook::from_config` both call
`cfg.hooks.as_ref()...` — but `CapabilityConfig` (defined in
`canon-utils/canon-llm-runtime/src/config.rs`) has no `hooks` field.

**`CapabilityRateLimitHook::from_config`** — replace the body with a hardcoded
default. The method signature stays identical so call sites don't change:

```rust
pub fn from_config(_cfg: &canon_llm::config::CapabilityConfig) -> Self {
    Self { buckets: Mutex::new(HashMap::new()), max_per_sec: 100 }
}
```

**`CostCapHook::from_config`** — same pattern:

```rust
pub fn from_config(_cfg: &canon_llm::config::CapabilityConfig) -> Self {
    Self { max_turns: 500, used: std::sync::atomic::AtomicU64::new(0) }
}
```

The `_cfg` prefix silences any unused-variable warning since the argument is no
longer read.

---

### Error 3 — Type mismatch on `guard.entry(cap.capability.clone())`

`CapabilityInvoked.capability` is `&'static str` (see `events.rs`:
`canon_event_struct!(CapabilityInvoked { ..., capability: &'static str, ... })`).
Calling `.clone()` on `&'static str` returns another `&'static str`, but the
`HashMap` key type is `String`. The `entry()` call therefore fails to compile.

**In `CapabilityRateLimitHook::on_pre`**, change:

```rust
let bucket = guard.entry(cap.capability.clone()).or_insert_with(|| TokenBucket::new(self.max_per_sec));
```

To:

```rust
let bucket = guard.entry(cap.capability.to_string()).or_insert_with(|| TokenBucket::new(self.max_per_sec));
```

---

### Error 4 — Unused `mut` in `WatchdogPreHook::on_pre` (Tick arm)

In the `Tick` arm, `guard` is locked and used only for `.get()` (read-only).
The `mut` binding is unnecessary and triggers `-Dunused` as an error.

**Change** (in the stalled-collection block inside the `Tick` arm):

```rust
let mut guard = self.last_stage.lock().unwrap();
```

To:

```rust
let guard = self.last_stage.lock().unwrap();
```

---

## Original design (unchanged — for reference)

### Goal

Add a deterministic middleware layer to `EventBus` that intercepts every capability
invocation before dispatch and every outcome after. Hooks fire regardless of LLM
reasoning — hard enforcement points for rate limiting, cost caps, audit logging,
and policy.

### `hooks.rs` interface

```
pub enum HookDecision { Allow, Deny { reason: String }, Mutate { replacement: RuntimeEvent } }
pub trait PreHook: Send + Sync { fn name(&self) -> &'static str; fn on_pre(&self, event: &RuntimeEvent) -> HookDecision; }
pub trait PostHook: Send + Sync { fn name(&self) -> &'static str; fn on_post(&self, event: &RuntimeEvent, outcome: &EventOutcome); }
pub struct HookChain { pre: Vec<Box<dyn PreHook>>, post: Vec<Box<dyn PostHook>> }
```

### `bus.rs` changes

1. Add `hooks: Arc<HookChain>` field to `EventBus`.
2. Add `pub fn set_hooks(&mut self, hooks: Arc<HookChain>)`.
3. In `dispatch()`, call `self.hooks.run_pre(&event)` before sending to consumers.
   - `Deny` → emit `hook_denied_event(&reason)` via emitter; skip dispatch.
   - `Mutate` → dispatch `replacement` instead of original event.
4. In `register()` consumer thread loop, call `hooks.run_post(&msg.event, &outcome)`
   after `consumer.on_event` and before acting on the outcome.

### `event_runtime.rs` wiring

```rust
let mut hooks = HookChain::new();
hooks.add_pre(Box::new(CapabilityRateLimitHook::from_config(&config)));
hooks.add_pre(Box::new(CostCapHook::from_config(&config)));
hooks.add_post(Box::new(AuditLogHook::new()));
bus.set_hooks(Arc::new(hooks));
```

---

## Verification

```
cargo check -p canon-runtime
cargo test -p canon-runtime
```

All four errors above must be gone. Then confirm:
- `state/audit.log` grows with entries when the runtime runs.
- A `CostCapHook` with `max_turns: 1` blocks the second `Llm` event with
  `ErrorOccurred("hook_denied", reason="llm_cost_cap_reached")`.
