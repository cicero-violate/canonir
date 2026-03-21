# Implementation Plan: F-Migration — Typed Routing Events

## Current Build Status

```
Phase 1 — 🔴 not started  (add RouteTick + RouteSelected to canon-runtime-events)
Phase 2 — 🔴 not started  (add emit_event() to EventRuntime)
Phase 3 — 🔴 not started  (event_runtime.rs: emit RouteTick typed)
Phase 4 — 🔴 not started  (canon-route: consume RouteTick, emit RouteSelected typed)
Phase 5 — 🔴 not started  (canon-loop: consume RouteSelected typed, remove string guards)
Phase 6 — 🔴 not started  (cargo check + tests)
```

**Prerequisite:** `cargo check --workspace` clean (confirmed post E-migration 2026-03-21).

---

## Goal

Promote the two high-signal `Debug` event kinds that drive control-flow into typed
`CanonEvent` variants. This completes the compiler-enforced semantic collapse begun in
the R/C/D migrations and eliminates the last string-keyed dispatch in the core loop.

```
Before:
  CanonEvent::Debug { source: "supervisor", kind: "route_tick",     payload: { tick: u64 } }
  CanonEvent::Debug { source: "supervisor", kind: "route_selected", payload: { approved_route, ... } }

  Consumers match: d.kind == "route_selected"   ← string comparison, no exhaustiveness
  Routing guards:  d.payload.get("approved_route")  ← untyped JSON field access

After:
  CanonEvent::RouteTick(RouteTick { tick: u64 })
  CanonEvent::RouteSelected(RouteSelected { tick, approved_route, ... })

  Consumers match: CanonEvent::RouteSelected(rs) ← compiler-enforced, typed fields
  Stage dispatch:  rs.approved_route.as_str()    ← no JSON, no strings, no guards
```

**Two events. Zero string comparisons in the core loop afterward.**

---

## What Changes Where

| Crate | File | Change |
|-------|------|--------|
| `canon-runtime-events` | `src/events.rs` | Add `RouteTick`, `RouteSelected` structs + enum variants |
| `canon-runtime` | `src/lib.rs` | Add `pub fn emit_event()` to `EventRuntime` |
| `canon-runtime` | `src/bin/event_runtime.rs` | Replace `emit_debug_event("route_tick")` with `emit_event(CanonEvent::RouteTick(...))` |
| `canon-route` | `src/executor.rs` | Match `CanonEvent::RouteTick`; emit `CanonEvent::RouteSelected` |
| `canon-loop` | `src/stage/mod.rs` | Replace 4 guarded `Debug` arms with 1 `RouteSelected` arm; delete `route_lane()`; update variant types |
| `canon-loop` | `src/stage/plan.rs` | Signature: `DebugEvent` → `RouteSelected`; field: `d.payload.get("tick")` → `rs.tick` |
| `canon-loop` | `src/stage/act.rs` | Signature: `DebugEvent` → `RouteSelected`; field: `d.payload.get("approved_route")` → `rs.approved_route`; remove redundant lane guard |
| `canon-loop` | `src/stage/verify.rs` | Signature: `DebugEvent` → `RouteSelected`; same field upgrades; remove lane guard |
| `canon-loop` | `src/stage/reward.rs` | Signature: `_d: DebugEvent` → `_rs: RouteSelected`; body unchanged |

---

## Phase 1 — Add typed variants to `canon-runtime-events`

**File:** `canon-utils/canon-runtime-events/src/events.rs`

### Step 1a — Define structs

Add after the existing loop event structs (near line 210). Use `canon_event_struct!` macro,
consistent with all other events in this file:

```rust
canon_event_struct!(RouteTick { tick: u64 });

canon_event_struct!(RouteSelected {
    tick: u64,
    approved_route: String,
    suggested_route: String,
    rationale: String,
    #[serde(default)]
    confidence: Option<f32>,
    gate_note: String,
    #[serde(default)]
    gate_rules_fired: Vec<String>,
    gate_changed: bool,
    gate_should_stop: bool,
    prompt: String,
    model_json: String,
});
```

### Step 1b — Add to `CanonEvent` enum

In the `CanonEvent` enum (around line 339), add two new variants adjacent to the other
routing/loop variants:

```rust
RouteTick(RouteTick),
RouteSelected(RouteSelected),
```

### Step 1c — Export from crate root

In `canon-runtime-events/src/lib.rs` (or wherever the public re-exports are), add
`RouteTick` and `RouteSelected` to the public exports so that:

```rust
use canon_event::{RouteTick, RouteSelected};
```

works from any dependent crate.

**Checkpoint:** `cargo check -p canon-runtime-events` exits 0.

---

## Phase 2 — Add `emit_event()` to `EventRuntime`

**File:** `canon-utils/canon-runtime/src/lib.rs`

`EventRuntime` currently has `emit_tick()` (line 225) and `emit_debug_event()` (line 232).
Both call `handle_runtime_event(event)?` then `drain_emitted_events()`. Add a generic form:

```rust
pub fn emit_event(&mut self, event: CanonEvent) -> Result<()> {
    self.handle_runtime_event(event)?;
    self.drain_emitted_events()?;
    Ok(())
}
```

This follows the exact same pattern as `emit_debug_event`. Place it immediately after
`emit_debug_event` for readability.

**Checkpoint:** `cargo check -p canon-runtime` exits 0.

---

## Phase 3 — Update `event_runtime.rs`: emit `RouteTick`

**File:** `canon-utils/canon-runtime/src/bin/event_runtime.rs`

### Step 3a — Update import

Add `RouteTick` to the `canon_event` import at the top of the file.

### Step 3b — Replace `emit_debug_event` call in `handle_control_msg()`

Current (`handle_control_msg`, lines ~156–164):
```rust
*scheduler_tick = scheduler_tick.saturating_add(1);
runtime.emit_debug_event(
    "supervisor".to_string(),
    "route_tick".to_string(),
    serde_json::json!({ "tick": *scheduler_tick }),
)?;
```

Replace with:
```rust
*scheduler_tick = scheduler_tick.saturating_add(1);
runtime.emit_event(CanonEvent::RouteTick(RouteTick { tick: *scheduler_tick }))?;
```

No other changes to `handle_control_msg`.

**Checkpoint:** `cargo check -p canon-runtime` exits 0.

---

## Phase 4 — Update `canon-route/src/executor.rs`

### Step 4a — Update imports

```rust
// Add:
use canon_event::{RouteTick, RouteSelected};
// RouteTick replaces the Debug(d) pattern for route_tick.
// RouteSelected is the new emit type replacing the Debug emission.
```

Remove any import of `DebugEvent` if it is no longer needed after this phase.

### Step 4b — Replace `CanonEvent::Debug` route_tick arm

Current `on_event()` structure:
```rust
if let CanonEvent::Debug(d) = event {
    if d.source == "supervisor" && d.kind == "route_tick" {
        // ... routing logic
    }
    return;
}
```

Replace with:
```rust
if let CanonEvent::RouteTick(_rt) = event {
    // ... routing logic (body unchanged — uses self.ctx, self.controller, self.emitter)
    return;
}
```

The `return` placement is critical — it must still short-circuit so the
`CapabilityCompleted`/`CapabilityFailed` arms below are not reached on a `RouteTick` event.

### Step 4c — Replace `emit_decision()` emission

Current `emit_decision()`:
```rust
canon_meta::canon_emit_meta!(emitter; "supervisor", "route_selected", payload);
```

Where `payload` is a `serde_json::json!({ ... })` with fields:
`approved_route, suggested_route, rationale, confidence, gate_note, gate_rules_fired,
gate_changed, gate_should_stop, prompt, model_json`.

Replace with typed emission:
```rust
canon_meta::canon_emit_meta!(emitter; RouteSelected(RouteSelected {
    tick: self.ctx.scheduler_tick,
    approved_route: decision.lane.as_str().to_string(),
    suggested_route: decision.suggested_route.as_str().to_string(),
    rationale: decision.rationale.clone(),
    confidence: decision.confidence,
    gate_note: decision.note.clone(),
    gate_rules_fired: decision.gate_rules_fired.clone(),
    gate_changed: decision.changed,
    gate_should_stop: decision.should_stop,
    prompt: decision.prompt.clone(),
    model_json: model_json.to_string(),
}));
```

**Verify field names match `RouteDecision` struct** in `canon-route/src/decision.rs`.
`RouteDecision.lane` maps to `approved_route` (the gatekeeper-approved lane).
`RouteDecision.suggested_route` maps to `suggested_route`.

**Checkpoint:** `cargo check -p canon-route` exits 0.

---

## Phase 5 — Update `canon-loop`

### Step 5a — `src/stage/mod.rs`

**Update variant types** in `LoopStageEvent`:
```rust
// Before:
pub enum LoopStageEvent {
    Observe(Tick),
    PlanTrigger(DebugEvent),
    ActDispatch(DebugEvent),
    VerifyTrigger(DebugEvent),
    Conclude(DebugEvent),
    CapabilityDone(CapabilityCompleted),
    CapabilityFail(CapabilityFailed),
    Reward(LoopVerified),
}

// After:
pub enum LoopStageEvent {
    Observe(Tick),
    PlanTrigger(RouteSelected),
    ActDispatch(RouteSelected),
    VerifyTrigger(RouteSelected),
    Conclude(RouteSelected),
    CapabilityDone(CapabilityCompleted),
    CapabilityFail(CapabilityFailed),
    Reward(LoopVerified),
}
```

**Replace the `TryFrom` implementation:**

Delete the `route_lane()` helper function entirely.

Replace the four guarded `Debug` arms with a single `RouteSelected` arm:

```rust
// Before (4 arms):
CanonEvent::Debug(d) if route_lane(&d) == "shape"    => Ok(LoopStageEvent::PlanTrigger(d)),
CanonEvent::Debug(d) if route_lane(&d) == "execute"  => Ok(LoopStageEvent::ActDispatch(d)),
CanonEvent::Debug(d) if route_lane(&d) == "validate" => Ok(LoopStageEvent::VerifyTrigger(d)),
CanonEvent::Debug(d) if route_lane(&d) == "conclude" => Ok(LoopStageEvent::Conclude(d)),

// After (1 arm):
CanonEvent::RouteSelected(rs) => match rs.approved_route.as_str() {
    "shape"    => Ok(LoopStageEvent::PlanTrigger(rs)),
    "execute"  => Ok(LoopStageEvent::ActDispatch(rs)),
    "validate" => Ok(LoopStageEvent::VerifyTrigger(rs)),
    "conclude" => Ok(LoopStageEvent::Conclude(rs)),
    _          => Err(CanonEvent::RouteSelected(rs)),
},
```

**Update execute dispatch** — the 4 delegate calls now pass `RouteSelected`:
```rust
// Before:
LoopStageEvent::PlanTrigger(d) => plan::execute_trigger(d, ctx),
LoopStageEvent::ActDispatch(d) => act::execute_dispatch(d, ctx),
LoopStageEvent::VerifyTrigger(d) => verify::execute(d, ctx),
LoopStageEvent::Conclude(d)    => reward::execute_conclude(d, ctx),

// After: identical structure, types inferred from enum variant
LoopStageEvent::PlanTrigger(rs) => plan::execute_trigger(rs, ctx),
LoopStageEvent::ActDispatch(rs) => act::execute_dispatch(rs, ctx),
LoopStageEvent::VerifyTrigger(rs) => verify::execute(rs, ctx),
LoopStageEvent::Conclude(rs)    => reward::execute_conclude(rs, ctx),
```

**Update imports**: add `RouteSelected`, remove `DebugEvent` (no longer used here).

### Step 5b — `src/stage/plan.rs`

**Signature change:**
```rust
// Before:
pub fn execute_trigger(d: DebugEvent, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult>

// After:
pub fn execute_trigger(rs: RouteSelected, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult>
```

**Field access change** (line ~11):
```rust
// Before:
let tick = d.payload.get("tick").and_then(|v| v.as_u64()).unwrap_or(0);

// After:
let tick = rs.tick;
```

**Update imports**: add `RouteSelected`, remove `DebugEvent`.

### Step 5c — `src/stage/act.rs`

**Signature change:**
```rust
// Before:
pub fn execute_dispatch(d: DebugEvent, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult>

// After:
pub fn execute_dispatch(rs: RouteSelected, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult>
```

**Field access change + remove redundant lane guard** (lines ~15–21):
```rust
// Before:
let lane = d.payload.get("approved_route")
    .or_else(|| d.payload.get("lane"))
    .and_then(|v| v.as_str())
    .unwrap_or("");
if lane != "execute" {
    return Err(anyhow::anyhow!("not an execute route"));
}

// After: delete the lane variable and the guard entirely.
// The TryFrom already guarantees approved_route == "execute" for ActDispatch.
// No code needed here — rs is available if any remaining body needs rs.approved_route.
```

**Update imports**: add `RouteSelected`, remove `DebugEvent`.

### Step 5d — `src/stage/verify.rs`

**Signature change:**
```rust
// Before:
pub fn execute(d: DebugEvent, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult>

// After:
pub fn execute(rs: RouteSelected, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult>
```

**Field access changes** (lines ~9–15 and ~45):
```rust
// Before:
let lane = d.payload.get("approved_route")
    .or_else(|| d.payload.get("lane"))
    .and_then(|v| v.as_str())
    .unwrap_or("");
if lane != "validate" {
    return Err(anyhow::anyhow!("not a validate route"));
}
// ...
tick: d.payload.get("tick").and_then(|v| v.as_u64()).unwrap_or(0),

// After: delete lane variable and guard (same reason as act.rs).
// Replace tick access:
tick: rs.tick,
```

**Update imports**: add `RouteSelected`, remove `DebugEvent`.

### Step 5e — `src/stage/reward.rs`

**Signature change only** — body is unchanged (`_d` is not used):
```rust
// Before:
pub fn execute_conclude(_d: DebugEvent, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult>

// After:
pub fn execute_conclude(_rs: RouteSelected, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult>
```

**Update imports**: add `RouteSelected`, remove `DebugEvent`.

### Step 5f — `src/executor.rs`

No logic changes. Verify imports: `DebugEvent` was imported at line 1 for the stage module
dispatch. After this phase it may no longer be needed in `executor.rs` directly — check
and remove if unused.

**Checkpoint:** `cargo check -p canon-loop` exits 0.

---

## Phase 6 — Verify

```bash
cargo check --workspace
cargo test -p canon-route
cargo test -p canon-loop
cargo test -p canon-runtime
```

Expected:
- `cargo check --workspace` — zero errors
- `cargo test -p canon-route` — 2 tests pass
- `cargo test -p canon-loop` — all tests pass
- `cargo test -p canon-runtime` — `async_consumers_preserve_order_per_consumer` passes

---

## Execution Order

```
Phase 1 — 🔴 next  (schema; cargo check -p canon-runtime-events exits 0)
Phase 2 — 🔴       (EventRuntime::emit_event; cargo check -p canon-runtime exits 0)
Phase 3 — 🔴       (event_runtime.rs; cargo check -p canon-runtime exits 0)
Phase 4 — 🔴       (canon-route; cargo check -p canon-route exits 0)
Phase 5 — 🔴       (canon-loop; cargo check -p canon-loop exits 0)
Phase 6 — 🔴 last  (cargo check --workspace exits 0; all tests pass)
```

**Phases 1 and 2 are independent** — they can be done in either order.
**Phases 3, 4, 5 each depend on Phase 1** (need the new types).
**Phases 3, 4, 5 are otherwise independent** and can be done in parallel.

---

## Files Created / Modified

| Phase | Status | File | Change |
|-------|--------|------|--------|
| 1 | 🔴 | `canon-runtime-events/src/events.rs` | Add `RouteTick`, `RouteSelected` structs + enum variants |
| 1 | 🔴 | `canon-runtime-events/src/lib.rs` | Export `RouteTick`, `RouteSelected` |
| 2 | 🔴 | `canon-runtime/src/lib.rs` | Add `pub fn emit_event(&mut self, event: CanonEvent) -> Result<()>` |
| 3 | 🔴 | `canon-runtime/src/bin/event_runtime.rs` | Replace `emit_debug_event("route_tick")` → `emit_event(RouteTick { tick })` |
| 4 | 🔴 | `canon-route/src/executor.rs` | Match `RouteTick`; emit `RouteSelected`; update imports |
| 5 | 🔴 | `canon-loop/src/stage/mod.rs` | Delete `route_lane()`; replace 4 guarded Debug arms with 1 RouteSelected arm; update variant types |
| 5 | 🔴 | `canon-loop/src/stage/plan.rs` | `DebugEvent` → `RouteSelected`; `d.payload.get("tick")` → `rs.tick` |
| 5 | 🔴 | `canon-loop/src/stage/act.rs` | `DebugEvent` → `RouteSelected`; remove lane guard |
| 5 | 🔴 | `canon-loop/src/stage/verify.rs` | `DebugEvent` → `RouteSelected`; remove lane guard; `rs.tick` |
| 5 | 🔴 | `canon-loop/src/stage/reward.rs` | `_d: DebugEvent` → `_rs: RouteSelected` |

---

## What This Achieves

**Before F-migration** — the core dispatch chain contains:
```
route_lane(&d) == "shape"     ← string comparison
d.payload.get("approved_route")  ← JSON field access, no type guarantee
d.payload.get("tick")            ← untyped
if lane != "execute" { ... }     ← redundant runtime guard, dead letter
```

**After F-migration** — the core dispatch chain contains:
```
CanonEvent::RouteSelected(rs) => match rs.approved_route.as_str() { ... }
                                     ← typed, exhaustive, compiler-checked
rs.tick                            ← u64, not Option<u64>, no unwrap_or
```

**The `route_lane()` helper is deleted.** It existed only to paper over the lack of a
typed event. With `RouteSelected`, the lane is a first-class field — no helper needed.

**The redundant lane guards in `act.rs` and `verify.rs` are deleted.** They checked
`lane != "execute"` and `lane != "validate"` inside the stage functions as a secondary
filter — but the `TryFrom` already enforces the correct lane for each variant.
The guards were defensive code that could never fire. Removing them tightens the logic.

**After all migrations (R → C → D → E → F):**
```
S = min(E, C, J, R)  where:
  E — event schema:   all control signals are typed CanonEvent variants ✅
  C — consumers:      LoopStageExecutor + RouteExecutor — no string dispatch ✅
  J — judgment:       RouteController inside RouteExecutor ✅
  R — routing:        compiler-enforced lane dispatch ✅

Core loop string comparisons: 0
```
