# Implementation Plan: H1b — Fix canon-storage-eventlog Projectors

## Status

```
Phase H1b — 🔴 not started  (BLOCKING: cargo build --workspace fails, 36 errors)
```

**Must be completed before any other H-phase.**

---

## Root Cause

`canon-storage-eventlog` was not updated when `CanonEvent` changed from flat `TlogEvent`-style
(with top-level `kind: String`, `ts: u64`, `payload: serde_json::Value`) to the new wire format
(`meta: EventMeta`, `payload: CanonPayload` enum). Two projectors still use the old access pattern.

**Error classes (all in 2 files):**

| Error | Old code | Fix |
|-------|----------|-----|
| E0609 `no field 'kind'` | `canon.kind.as_str()` | `canon.payload.kind_str()` |
| E0609 `no field 'ts'` | `canon.ts` | `canon.meta.ts` |
| E0599 `no method 'get' on &CanonPayload` | `payload.get("field")` | `payload.as_value().and_then(\|v\| v.get("field"))` |

---

## Scope: One Turn, Three Files

1. `canon-runtime-events/src/wire.rs` — add two methods to `CanonPayload`
2. `canon-storage-eventlog/src/capability_graph_projector.rs` — fix access pattern
3. `canon-storage-eventlog/src/goal_graph_projector.rs` — fix access pattern

Also add missing `CanonPayload` variants needed by projectors.

---

## Step 1 — Add missing variants to `CanonPayload` in `wire.rs`

The projectors reference these event kinds that have no `CanonPayload` variant:

| Kind | Used in | Missing variant |
|------|---------|----------------|
| `capability_requested` | `capability_graph_projector` | Add `CapabilityRequested(serde_json::Value)` |
| `node_started` | `goal_graph_projector` | Add `NodeStarted(serde_json::Value)` |
| `node_completed` | `goal_graph_projector` | Add `NodeCompleted(serde_json::Value)` |
| `node_failed` | `goal_graph_projector` | Add `NodeFailed(serde_json::Value)` |
| `agent_registered` | bootstrap writes | Add `AgentRegistered(serde_json::Value)` |

Also add corresponding arms to `CanonPayload::from_kind()`:
```rust
"capability_requested" => CanonPayload::CapabilityRequested(data),
"node_started" => CanonPayload::NodeStarted(data),
"node_completed" => CanonPayload::NodeCompleted(data),
"node_failed" => CanonPayload::NodeFailed(data),
"agent_registered" => CanonPayload::AgentRegistered(data),
```

Also add corresponding arms to `runtime_event_to_wire` in `canon-runtime/src/lib.rs`
so these events are not silently dropped. Currently `NodeStarted`, `NodeCompleted`,
`NodeFailed` fall into `_ => return None`:

```rust
RuntimeEvent::NodeStarted(p) => CanonPayload::NodeStarted(serde_json::to_value(p).ok()?),
RuntimeEvent::NodeCompleted(p) => CanonPayload::NodeCompleted(serde_json::to_value(p).ok()?),
RuntimeEvent::NodeFailed(p) => CanonPayload::NodeFailed(serde_json::to_value(p).ok()?),
RuntimeEvent::AgentRegistered(p) => CanonPayload::AgentRegistered(p.payload.clone()),
```

And in the `source` match in `runtime_event_to_wire`:
```rust
RuntimeEvent::NodeStarted(_) | RuntimeEvent::NodeCompleted(_) | RuntimeEvent::NodeFailed(_) => "agent-consumer",
RuntimeEvent::AgentRegistered(_) => "bootstrap",
```

---

## Step 2 — Add helper methods to `CanonPayload` in `wire.rs`

Add these two `impl CanonPayload` methods. They allow projectors to use the old
string-dispatch + `.get()` pattern with minimal rewriting.

```rust
impl CanonPayload {
    /// Returns the kind string for this payload variant.
    /// Matches the kind tag used in serde serialization.
    pub fn kind_str(&self) -> &'static str {
        match self {
            CanonPayload::LoopObserved(_) => "loop_observed",
            CanonPayload::LoopPlanned(_) => "loop_planned",
            CanonPayload::LoopActed(_) => "loop_acted",
            CanonPayload::LoopVerified(_) => "loop_verified",
            CanonPayload::LoopRewarded(_) => "loop_rewarded",
            CanonPayload::RouteTick(_) => "route_tick",
            CanonPayload::RouteSelected(_) => "route_selected",
            CanonPayload::CapabilityCompleted(_) => "capability_completed",
            CanonPayload::CapabilityFailed(_) => "capability_failed",
            CanonPayload::CapabilityRequested(_) => "capability_requested",
            CanonPayload::CapabilityInvoked(_) => "capability_invoked",
            CanonPayload::CapabilityResolved(_) => "capability_resolved",
            CanonPayload::NodeStarted(_) => "node_started",
            CanonPayload::NodeCompleted(_) => "node_completed",
            CanonPayload::NodeFailed(_) => "node_failed",
            CanonPayload::AgentRegistered(_) => "agent_registered",
            CanonPayload::ErrorOccurred(_) => "error_occurred",
            CanonPayload::Debug(_) => "debug",
            CanonPayload::PromptLoaded(_) => "prompt_loaded",
            CanonPayload::RuntimeStateUpdated(_) => "runtime_state.updated",
            CanonPayload::ToolCall(_) => "tool_call",
            CanonPayload::ToolResult(_) => "tool_result",
            CanonPayload::RustcEvent(_) => "rustc_event",
            CanonPayload::EditEvent(_) => "edit_event",
            CanonPayload::SupervisorEvent(_) => "supervisor_event",
            CanonPayload::GoalNodeCreated(_) => "goal_node_created",
            CanonPayload::GoalNodeRetracted(_) => "goal_node_retracted",
            CanonPayload::GoalNodeRewritten(_) => "goal_node_rewritten",
            CanonPayload::GoalEdgeDefined(_) => "goal_edge_defined",
            CanonPayload::GoalGraphCheckpointed(_) => "goal_graph_checkpointed",
            CanonPayload::Unknown => "unknown",
        }
    }

    /// Returns a reference to the inner JSON Value for variants that carry one.
    /// Returns None for typed variants (LoopObserved, RouteTick, RouteSelected) and Unknown.
    pub fn as_value(&self) -> Option<&serde_json::Value> {
        match self {
            CanonPayload::LoopPlanned(v) | CanonPayload::LoopActed(v)
            | CanonPayload::LoopVerified(v) | CanonPayload::LoopRewarded(v)
            | CanonPayload::CapabilityCompleted(v) | CanonPayload::CapabilityFailed(v)
            | CanonPayload::CapabilityRequested(v) | CanonPayload::CapabilityInvoked(v)
            | CanonPayload::CapabilityResolved(v) | CanonPayload::NodeStarted(v)
            | CanonPayload::NodeCompleted(v) | CanonPayload::NodeFailed(v)
            | CanonPayload::AgentRegistered(v) | CanonPayload::ErrorOccurred(v)
            | CanonPayload::Debug(v) | CanonPayload::PromptLoaded(v)
            | CanonPayload::RuntimeStateUpdated(v) | CanonPayload::ToolCall(v)
            | CanonPayload::ToolResult(v) | CanonPayload::RustcEvent(v)
            | CanonPayload::EditEvent(v) | CanonPayload::SupervisorEvent(v)
            | CanonPayload::GoalNodeCreated(v) | CanonPayload::GoalNodeRetracted(v)
            | CanonPayload::GoalNodeRewritten(v) | CanonPayload::GoalEdgeDefined(v)
            | CanonPayload::GoalGraphCheckpointed(v) => Some(v),
            // Typed variants — no inner Value
            CanonPayload::LoopObserved(_) | CanonPayload::RouteTick(_)
            | CanonPayload::RouteSelected(_) | CanonPayload::Unknown => None,
        }
    }
}
```

---

## Step 3 — Fix `capability_graph_projector.rs`

**File:** `canon-storage-eventlog/src/capability_graph_projector.rs`

Three mechanical changes — do NOT restructure logic:

### 3a — Extract payload value at top of loop body

Add one line after `let payload = &canon.payload;`:

```rust
// Before:
let payload = &canon.payload;
match canon.kind.as_str() {

// After:
let payload_value = canon.payload.as_value().unwrap_or(&serde_json::Value::Null);
match canon.payload.kind_str() {
```

Then rename all `payload.get(` inside the match arms to `payload_value.get(`.

### 3b — Fix `canon.ts` (3 occurrences)

```rust
// Before:
start_times.insert(id.clone(), canon.ts);
node.duration_ms = Some(canon.ts.saturating_sub(*start));  // two of these

// After:
start_times.insert(id.clone(), canon.meta.ts);
node.duration_ms = Some(canon.meta.ts.saturating_sub(*start));
```

---

## Step 4 — Fix `goal_graph_projector.rs`

**File:** `canon-storage-eventlog/src/goal_graph_projector.rs`

Same three mechanical changes:

### 4a — Extract payload value at top of function

```rust
// Before:
let payload = &canon.payload;
match canon.kind.as_str() {

// After:
let payload_value = canon.payload.as_value().unwrap_or(&serde_json::Value::Null);
match canon.payload.kind_str() {
```

Rename all `payload.get(` → `payload_value.get(`.

### 4b — Fix `canon.ts` (2 occurrences at lines 104-105)

```rust
// Before:
if canon.ts > state.seq_processed {
    state.seq_processed = canon.ts;

// After:
if canon.meta.ts > state.seq_processed {
    state.seq_processed = canon.meta.ts;
```

---

## Checkpoint

```bash
cargo build --workspace
```

Must exit 0. Zero errors, zero warnings about unused imports from these files.

```bash
cargo test -p canon-storage-eventlog 2>/dev/null || echo "no tests"
```

---

## What This Does NOT Do

- Does NOT restructure projector logic — only mechanical field access fixes
- Does NOT add a full typed match (that would be H-later)
- Does NOT touch any other crate beyond `canon-runtime-events`, `canon-runtime`, and
  `canon-storage-eventlog`
