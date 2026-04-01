# Delta-Based Dedup & Invariant Enforcement

**Objective:** Eliminate redundant event writes by enforcing delta-based state transitions,
hash-based deduplication, structural reuse, and compile-time invariants.

---

## Invariant Equations

```
valid(e) = shape(e) ∧ transition(e) ∧ Δ(e)
Δ(e)     = S_after − S_before ≠ 0
dedup(e) = H(e) == H(prev_kind) → drop
```

---

## Layer Map

```
proc_macro  → shape + delta slot correctness (compile-time)
writer      → hash dedup gate + prev_event_id chain (CRITICAL PATH)
runtime     → skip propagation on identical consecutive event
planner     → no re-emission of identical action batch
executor    → dep resolution via action_kind (already fixed)
state/graph → structural reuse via Arc (future)
log         → append-only, immutable (invariant)
```

---

## Layer 1 — Proc Macro (`canon-proc-macros/src/lib.rs`)

### 1a. `must_delta` struct-level attribute in `canon_event_struct!`

Extend `canon_event_struct!` to recognise a `#[must_delta]` struct-level attribute.
When present, emit a **compile error** if no fields are tagged `#[delta]`.

```rust
// Usage
canon_event_struct! {
    #[must_delta]
    LoopObserved {
        #[input]  error_count: u32,
        #[output] goal_text: Option<String>,
        #[delta]  delta_g: Option<f32>,   // required by #[must_delta]
    }
}
```

**Implementation in `canon_event_struct` arm of the macro:**
```rust
// After parsing fields, if must_delta flag is set:
if must_delta && delta_pairs.is_empty() {
    return syn::Error::new(
        name.span(),
        format!("#[must_delta]: struct `{}` has no #[delta] fields", name),
    ).to_compile_error().into();
}
```

### 1b. `content_hash()` method generation

`canon_event_struct!` generates `fn content_hash(&self) -> u64` on every struct,
hashing all `#[delta]` fields via `DefaultHasher`. Callers can use this for
O(1) dedup checks without re-serialising to JSON.

```rust
// Generated code (example):
impl LoopObserved {
    pub fn content_hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.delta_g.hash(&mut h);
        h.finish()
    }
}
```

### 1c. `CanonEvent` runtime invariant — `prev_event_id` non-null for non-root

In `CanonEvent::new()` (wire.rs), add a debug-mode assertion:
```rust
#[cfg(debug_assertions)]
if !root && prev_event_id.is_none() {
    eprintln!("[canon-event] WARN: non-root event kind={kind} has no prev_event_id");
}
```

---

## Layer 2 — `CanonEvent` Wire Format (`canon-runtime-events/src/wire.rs`)

### 2a. Add `prev_event_id` field

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonEvent {
    pub id: EventId,
    pub parent_ids: Vec<EventId>,
    pub actor: String,
    pub kind: EventKind,
    pub ts: u64,
    pub payload: CanonPayload,
    /// Previous event of the same kind — forms a per-kind causal chain.
    /// None for the first event of a kind in a session.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub prev_event_id: Option<EventId>,
}
```

`CanonEvent::new()` initialises `prev_event_id: None`.
`EventRuntime::append_runtime_event` sets it from `last_event_id_per_kind` before writing.

---

## Layer 3 — Writer Dedup Gate (`canon-runtime/src/lib.rs`)

### 3a. New fields on `EventRuntime`

```rust
pub struct EventRuntime {
    // ... existing fields ...
    /// Per-kind hash of last written event's `payload.data`.
    /// Consecutive identical events (same kind + same data hash) are dropped.
    last_kind_hash: HashMap<EventKind, u64>,
    /// Per-kind id of last written event, used to set `prev_event_id` on the next.
    last_event_id_per_kind: HashMap<EventKind, EventId>,
}
```

### 3b. Gate logic in `append_runtime_event`

```rust
fn append_runtime_event(&mut self, event, file, line, parent_ids, event_id) {
    let Some(path) = self.tlog_path.clone() else { return; };
    let Some(mut wire) = runtime_event_to_wire(event, parent_ids, event_id, file, line) else {
        return;
    };

    // --- DEDUP GATE ---
    let content_hash = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        wire.kind.hash(&mut h);
        wire.payload.data.to_string().hash(&mut h);
        h.finish()
    };
    if self.last_kind_hash.get(&wire.kind) == Some(&content_hash) {
        return; // identical consecutive event for this kind — drop write
    }
    self.last_kind_hash.insert(wire.kind, content_hash);

    // --- PREV_EVENT_ID CHAIN ---
    wire.prev_event_id = self.last_event_id_per_kind.get(&wire.kind).cloned();
    self.last_event_id_per_kind.insert(wire.kind, wire.id.clone());

    // ... existing write logic ...
}
```

### 3c. Clear on reset

```rust
pub fn reset(&mut self) {
    // ... existing ...
    self.last_kind_hash.clear();
    self.last_event_id_per_kind.clear();
}
```

---

## Layer 4 — Planner Action-Batch Dedup (`canon-loop`)

### 4a. New field on `LoopContext`

```rust
// In context.rs, Plan section:
pub last_emitted_plan_hash: Option<u64>,
```

### 4b. Dedup check in `plan::execute_complete`

After building the `out: Vec<LoopPlanned>` action batch (before emitting):

```rust
let action_batch_hash = {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for p in &out {
        p.action_kind.hash(&mut h);
        p.action_payload.to_string().hash(&mut h);
    }
    h.finish()
};
if ctx.last_emitted_plan_hash == Some(action_batch_hash) {
    // LLM returned identical actions — emit Noop and allow re-plan on next observation.
    ctx.last_planned_observed_tick = None;
    return Ok(LoopStageResult::Noop);
}
ctx.last_emitted_plan_hash = Some(action_batch_hash);
```

### 4c. Clear on `LoopActed` and `LoopVerified` in `executor.rs`

```rust
// In LoopActed handler:
self.ctx.last_emitted_plan_hash = None;

// In LoopVerified handler:
self.ctx.last_emitted_plan_hash = None;
```

---

## Layer 5 — Executor (already done)

Dependency resolution by `action_kind` name (LLM-friendly) was added in the previous
session. No further changes needed here.

---

## Layer 6 — State/Graph (future)

Convert `recent_compiler_errors: Vec<serde_json::Value>` in `LoopContext` to use
`Arc<[serde_json::Value]>` to allow structural sharing without copying.
Track immutable snapshots via `Arc` so `LoopObserved` clones are O(1) pointer copies.

This is deferred — current bottleneck is write dedup, not allocation.

---

## Success Criteria

| Condition | Mechanism |
|-----------|-----------|
| Identical consecutive events → 0 writes | Layer 3 hash gate |
| Log growth ∝ actual change | Layer 3 + Layer 4 |
| No repeated plan without state change | Layer 4 action-batch hash |
| Every event has prev_event_id chain | Layer 2 + Layer 3 |
| Compile error if no delta field | Layer 1 `#[must_delta]` |
| Fully replayable from log | Append-only, no mutation |

## Fail Conditions

- Full snapshot write (no delta) — caught by `#[must_delta]` at compile time
- Repeated identical event — caught by Layer 3 hash gate at runtime
- Event that does not change state — currently only caught by Layer 4; Layer 3 drops
  write but bus dispatch still fires (bus-level dedup is deferred to Layer 6 state model)
