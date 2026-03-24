# Implementation Plan: Apply #[must_emit] + Fix Test + Heartbeat

## Status

All prior plans have been applied. Three items remain before the exhaustiveness
work is complete:

| # | Item | File(s) | Status |
|---|---|---|---|
| 1 | `async_bus.rs` test broken — old `on_event` signature | `canon-runtime/tests/async_bus.rs` | ❌ compile error |
| 2 | Heartbeat thread missing — watchdog blind if main loop stalls | `canon-runtime/src/bin/event_runtime.rs` | ❌ missing |
| 3 | `#[must_emit]` not applied — wildcards everywhere | 10 files | ❌ not started |

Run order: 1 → 2 → 3 → `cargo test --workspace` to verify.

---

## Task 1 — Fix `async_bus.rs` test

**File:** `canon-utils/canon-runtime/tests/async_bus.rs`

`RecordingConsumer::on_event` was written before `EventOutcome` was added to the
trait. It currently returns `()`, causing a trait signature mismatch compile error.

Replace:
```rust
fn on_event(&mut self, event: &RuntimeEvent) {
    let RuntimeEvent::Code(canon_event::Code { delta, .. }) = event else {
        return;
    };
    let mut guard = self.seen.lock().unwrap();
    guard.push(delta.id);
    if guard.len() >= self.expected {
        let _ = self.done.send(());
    }
}
```

With:
```rust
fn on_event(&mut self, event: &RuntimeEvent) -> EventOutcome {
    let RuntimeEvent::Code(canon_event::Code { delta, .. }) = event else {
        return EventOutcome::NoOp("recording_consumer_not_a_code_event");
    };
    let mut guard = self.seen.lock().unwrap();
    guard.push(delta.id);
    if guard.len() >= self.expected {
        let _ = self.done.send(());
    }
    EventOutcome::NoOp("recording_consumer_recorded")
}
```

Also add `EventOutcome` to the import at the top:
```rust
use canon_event::{EventConsumer, EventFilter, EventMask, EventOutcome, RuntimeEvent, RustcEvent};
```

---

## Task 2 — Heartbeat thread in `event_runtime.rs`

**File:** `canon-utils/canon-runtime/src/bin/event_runtime.rs`

The `WatchdogConsumer` fires only when `Tick` events arrive. If the event loop
stalls (filesystem watcher hangs, channel blocks), no Ticks flow and the watchdog
is silent. A heartbeat thread injects Ticks on a wall-clock schedule independently.

Add this block **just before** the `consumers` vec is assembled (around line 257,
after `tlog_path` is set up). The `tlog_path` variable is already in scope at that
point.

```rust
// Heartbeat thread: inject a Tick into the tlog every 5 s so the WatchdogConsumer
// can detect stalls even if the main event loop is blocked.
{
    let heartbeat_tlog = tlog_path.clone();
    std::thread::Builder::new()
        .name("canon_heartbeat".to_string())
        .spawn(move || {
            use std::time::Duration;
            let mut tick: u64 = u64::MAX / 2; // high base to not collide with loop ticks
            loop {
                std::thread::sleep(Duration::from_secs(5));
                tick = tick.wrapping_add(1);
                let _ = canon_meta::canon_emit_meta!(
                    "heartbeat", "Tick",
                    serde_json::json!({ "tick": tick }),
                    &heartbeat_tlog
                );
            }
        })
        .expect("heartbeat thread");
}
```

If `canon_emit_meta!` macro does not support the tlog-path form, use the lower-level
write directly:

```rust
let _ = canon_event::write_canon_event_auto(
    &heartbeat_tlog,
    &canon_event::CanonEvent {
        event_id: None,
        meta: canon_event::EventMeta {
            ts: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            source: "heartbeat".to_string(),
            file: String::new(),
            line: 0,
        },
        payload: canon_event::CanonPayload::Tick(
            serde_json::json!({ "tick": tick })
        ),
    },
);
```

Choose whichever compiles. Run `cargo check -p canon-runtime` after adding.

---

## Task 3 — Apply `#[must_emit]` to all `on_event` implementations

The `#[must_emit]` proc-macro exists and is depended on by all relevant crates, but
has not been applied anywhere yet. Every consumer still uses `_ =>` wildcards.

### Step A — Add import to every file listed below

Add this use statement to each file:
```rust
use canon_proc_macros::must_emit;
```

### Step B — Add `#[must_emit]` before each `fn on_event`

```rust
#[must_emit]
fn on_event(&mut self, event: &RuntimeEvent) -> EventOutcome {
```

### Step C — Replace `_ =>` wildcards in RuntimeEvent matches with the exhaustive ignore block

The `#[must_emit]` macro will reject any `_ =>` arm inside a match that has
`RuntimeEvent::` pattern arms. Replace every such wildcard with the full explicit
list:

```rust
// EXHAUSTIVE IGNORE BLOCK — copy verbatim into every consumer
// that doesn't handle these variants. Update if RuntimeEvent gains new variants.
RuntimeEvent::Code(_)
| RuntimeEvent::Debug(_)
| RuntimeEvent::Edit(_)
| RuntimeEvent::ErrorOccurred(_)
| RuntimeEvent::Tick(_)
| RuntimeEvent::LoopObserved(_)
| RuntimeEvent::LoopPlanned(_)
| RuntimeEvent::LoopActed(_)
| RuntimeEvent::LoopVerified(_)
| RuntimeEvent::LoopRewarded(_)
| RuntimeEvent::GoodnessSnapshot(_)
| RuntimeEvent::RouteTick(_)
| RuntimeEvent::RouteSelected(_)
| RuntimeEvent::Cargo(_)
| RuntimeEvent::File(_)
| RuntimeEvent::Bash(_)
| RuntimeEvent::Llm(_)
| RuntimeEvent::RequestDispatch(_)
| RuntimeEvent::SubTaskResult(_)
| RuntimeEvent::Analysis(_)
| RuntimeEvent::RuntimeStateUpdated(_)
| RuntimeEvent::NodeReady(_)
| RuntimeEvent::NodeStarted(_)
| RuntimeEvent::NodeCompleted(_)
| RuntimeEvent::NodeFailed(_)
| RuntimeEvent::CapabilityCompleted(_)
| RuntimeEvent::CapabilityFailed(_)
| RuntimeEvent::PolicyBaselineUpdated(_)
| RuntimeEvent::GoalSelected(_)
| RuntimeEvent::SystemConfigLoaded(_)
| RuntimeEvent::AgentRegistered(_)
| RuntimeEvent::PromptLoaded(_)
| RuntimeEvent::ToolCall(_)
| RuntimeEvent::ToolResult(_)
| RuntimeEvent::ToolBatchSettled(_)
| RuntimeEvent::GoalNodeCreated(_)
| RuntimeEvent::GoalNodeRetracted(_)
| RuntimeEvent::GoalNodeRewritten(_)
| RuntimeEvent::GoalEdgeDefined(_)
| RuntimeEvent::GoalGraphCheckpointed(_)
| RuntimeEvent::CapabilityInvoked(_)
| RuntimeEvent::CapabilityResolved(_)
    => EventOutcome::NoOp("REASON_STRING"),
```

Replace `"REASON_STRING"` with a descriptive string per consumer (see table below).

**NOTE:** Wildcards in NON-RuntimeEvent inner matches (e.g., `RustcEvent`, `CapabilityResult`,
state enums) must be left alone. `#[must_emit]` only fires on matches that contain
`RuntimeEvent::` arms — inner matches on other types are unaffected.

### Files and their wildcards

Apply `#[must_emit]` + exhaustive block to each `on_event` listed:

---

#### `canon-runtime/src/consumers/watchdog_consumer.rs`
- Wildcard: line 63 `_ => EventOutcome::NoOp("watchdog_not_a_stage_event")`
- Already handles: Tick, LoopObserved/Planned/Acted/Verified/Rewarded (6 variants)
- Replace `_ =>` with exhaustive block using reason `"watchdog_not_a_stage_event"`
  (remove Tick and the 5 Loop variants from the block since they're already handled)

---

#### `canon-runtime/src/consumers/agent_registry.rs`
- `on_event` at line 101
- Wildcard: line 120 `_ => EventOutcome::NoOp("agent_registry_ignored")`
- Replace with exhaustive block, reason `"agent_registry_ignored"`

---

#### `canon-runtime/src/consumers/analyst_consumer.rs`
- `on_event` at line 169
- Wildcard at line 230: `_ => EventOutcome::NoOp("analyst_ignored_event")`
  (top-level RuntimeEvent match arm — this one needs replacing)
- The other 3 wildcards (lines 181, 198, 203) are in inner state/result matches —
  leave them unchanged
- Replace line 230 wildcard with exhaustive block, reason `"analyst_ignored_event"`
  (omit LoopRewarded, Tick, CapabilityCompleted, CapabilityFailed since they're handled)

---

#### `canon-runtime/src/consumers/goal_gen_consumer.rs`
- `on_event` at line 79
- Wildcard at line 202: `_ => EventOutcome::NoOp("goal_gen_noop")` (RuntimeEvent match)
- Wildcard at line 139: `_ => String::new()` (inner CapabilityResult match — leave alone)
- Replace line 202 wildcard with exhaustive block, reason `"goal_gen_noop"`
  (omit Tick and CapabilityCompleted/CapabilityFailed since they're handled)

---

#### `canon-runtime/src/consumers/failure_store.rs`
- `on_event` at line 39
- Wildcard at line 103: outer RuntimeEvent match (after inner RustcEvent match)
- Wildcard at line 101: inner RustcEvent match inside `RuntimeEvent::Code` arm — leave alone
- Replace line 103 wildcard with exhaustive block, reason `"failure_store_ignored"`
  (omit `RuntimeEvent::Code` and `RuntimeEvent::ErrorOccurred` if they're handled)

---

#### `canon-runtime/src/consumers/dispatch_consumer.rs`
- Three `on_event` impls at lines 44, 68, 216
- Wildcard at line 99: in one of the on_event impls; identify which struct it belongs to
- Add `#[must_emit]` to all three on_event impls
- Replace RuntimeEvent-level wildcards with exhaustive blocks

---

#### `canon-runtime/src/consumers/goal_graph_consumer.rs`
- `on_event` at line 78
- Wildcards at lines 51 and 90 — line 51 is BEFORE on_event (in a different method,
  leave alone); line 90 is inside on_event
- Replace line 90 wildcard with exhaustive block, reason `"goal_graph_ignored"`
  (omit GoalNodeCreated/Retracted/Rewritten/GoalEdgeDefined/GoalGraphCheckpointed
  since they're handled)

---

#### `canon-route/src/executor.rs`
- `on_event` at line 97
- Wildcards: line 139 `_ => false` (inner match inside `should_try` block — leave alone);
  line 196 `_ => EventOutcome::NoOp("route_executor_noop")` (RuntimeEvent match — replace)
- Replace line 196 with exhaustive block, reason `"route_executor_noop"`

---

#### `canon-loop/src/executor.rs`
- `on_event` at line 31
- Wildcard at line 157: `_ => {}` — check if this is in the RuntimeEvent match or an
  inner match. If inner (e.g., inside a guard block), leave alone. If outer (RuntimeEvent
  match arm), replace with exhaustive block returning `EventOutcome::NoOp("loop_executor_noop")`.

---

#### `canon-runtime/src/consumers/error_logger.rs`
- `on_event` at line 40
- Wildcards at lines 191 and 193: likely in inner matches (on event subfields).
  Apply `#[must_emit]` and verify build; if macro fires, replace the RuntimeEvent-level
  wildcard. Inner `_ => None` arms on non-RuntimeEvent types are safe to leave.

---

#### Files already exhaustive — add `#[must_emit]` only (no wildcard replacement needed)

These files have 0 wildcards in their `on_event` and will compile immediately after
adding `#[must_emit]`:

| File | Note |
|---|---|
| `canon-runtime/src/consumers/capability_executor.rs` | Returns NoOp for all |
| `canon-runtime/src/consumers/check_consumer.rs` | Already exhaustive |
| `canon-goodness/src/consumer.rs` | Already exhaustive |
| `canon-tools-editor/src/consumer.rs` | Already exhaustive |

For these, just add:
```rust
use canon_proc_macros::must_emit;
// ...
#[must_emit]
fn on_event(&mut self, event: &RuntimeEvent) -> EventOutcome {
```

---

### Step D — Build verification

After applying to all files:

```
cargo check --workspace
cargo test --workspace
```

Both must pass with zero errors. A compile error after adding `#[must_emit]` to a file
means there is a RuntimeEvent-level wildcard in that file that was not caught in the
survey above — replace it with the exhaustive block.

---

## Summary

| Task | Files changed | Key change |
|---|---|---|
| 1 — Fix test | `tests/async_bus.rs` | `-> EventOutcome` + `NoOp` returns |
| 2 — Heartbeat | `bin/event_runtime.rs` | 5-second Tick-injecting thread |
| 3 — must_emit | 14 files | `#[must_emit]` + exhaustive RuntimeEvent arms |

After all three tasks: `cargo test --workspace` passes, and adding any new
`RuntimeEvent` variant causes a compile error in every consumer that hasn't
acknowledged it.
