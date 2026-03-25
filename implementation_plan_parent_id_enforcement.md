# Implementation Plan: Parent ID Enforcement

## Status
Parent ID threading is implemented and compiling cleanly. This plan covers the remaining enforcement and validation layers.

## What's Already Done
- `EventMessage.event_id` pre-generated in `EventRuntime` and threaded through bus → consumer thread-local → `LoopStageExecutor::on_event`
- `emit_with_parents` called for loop stage emissions when `trigger_id` is present
- `LocatedEvent.parent_ids` field added, populated by `RuntimeEmitterImpl::emit_with_parents`
- Synthetic error events (CapabilityFailed, NodeFailed, PanicCaptured) parent to their triggering event

---

## Task 1: Write-Gate Enforcement in `runtime_event_to_wire`

**File:** `canon-utils/canon-runtime/src/lib.rs`

**Location:** `fn runtime_event_to_wire(event: &RuntimeEvent, parent_ids: Vec<EventId>, event_id: EventId) -> Option<CanonEvent>`

**Change:** After the `let kind = ...` / payload construction, before returning `Some(CanonEvent {...})`, add a soft-warn (log + continue) for non-root events with empty `parent_ids`.

Define root kinds — these are legitimately parentless:
```rust
const ROOT_KINDS: &[EventKind] = &[
    EventKind::Tick,
    EventKind::PromptLoaded,
    EventKind::SystemConfigLoaded,
    EventKind::AgentRegistered,
];
```

After payload construction, insert:
```rust
if parent_ids.is_empty() && !ROOT_KINDS.contains(&kind) {
    eprintln!(
        "[canon-runtime] WARN: event {:?} kind={:?} has no parent_ids — causal chain broken",
        event_id, kind
    );
}
```

This is soft (warn only) for now. Escalate to hard-reject later once the tlog is verified clean.

---

## Task 2: Verify Live Tlog Has Populated `parent_ids`

**No code change** — this is a manual verification step.

Run an agent session and inspect the tlog with:
```sh
# Find the latest tlog segment
ls -lt /workspace/ai_sandbox/canon/tlogs/ | head -5

# Decode and check loop_observed events
cat <latest>.bin | canon-tlog-reader 2>/dev/null | \
  jq 'select(.kind == "loop_observed") | {id, parent_ids}'
```

Expected: `loop_observed` events show `parent_ids: ["<tick_event_id>"]`, not `[]`.

If still empty, check:
1. Is `LoopStageExecutor` actually receiving the `CURRENT_DISPATCH_ID` thread-local? Add a `dbg!(current_dispatch_id())` trace in `on_event`.
2. Is `emit_with_parents` properly populating `LocatedEvent.parent_ids`? Verify the `RuntimeEmitterImpl::emit_with_parents` implementation is NOT being overridden to a no-op.

---

## Task 3: Other Consumer Emitters

**Problem:** `LoopStageExecutor` is one consumer, but other consumers (analyst, goal_gen, dispatch) also call `emitter.emit_located(...)` and emit without parents.

**Files to update:**
- `canon-loop/src/stage/plan.rs` — `LlmCall`, `RequestDispatch` emissions
- `canon-analyst/src/analyst_consumer.rs` — `LlmCall` emissions
- `canon-analyst/src/goal_gen_consumer.rs` — `LlmCall` emissions
- `canon-dispatch/src/dispatch_consumer.rs` — `GoalNodeRetracted`, `ToolCall` emissions

**Pattern for each:** Same as `executor.rs`:
```rust
fn on_event(&mut self, event: &RuntimeEvent) -> EventOutcome {
    let trigger_id = current_dispatch_id();
    // ... existing logic ...
    // Replace: emitter.emit_located(e, file!(), line!())
    // With:
    if let Some(pid) = trigger_id.clone() {
        emitter.emit_with_parents(e, vec![pid], file!(), line!());
    } else {
        emitter.emit_located(e, file!(), line!());
    }
}
```

Add `use canon_event::current_dispatch_id;` to each file's imports.

---

## Task 4: Tighten Writer Validation — Non-Empty Payload Fields

**File:** `canon-utils/canon-runtime/src/lib.rs`, `runtime_event_to_wire`

**Goal:** Warn if `input`, `output`, or `delta` are serialized as `{}` (empty object) for event kinds that should always carry data.

Define a check after payload serialization:
```rust
// For debug purposes: warn on completely empty payloads
if payload.input == serde_json::Value::Object(Default::default())
    && payload.output == serde_json::Value::Object(Default::default())
    && payload.delta == serde_json::Value::Object(Default::default())
    && payload.data == serde_json::Value::Null
{
    eprintln!("[canon-runtime] WARN: event {:?} kind={:?} has empty payload", event_id, kind);
}
```

This is diagnostic only — do not hard-reject yet.

---

## Task 5: Watchdog — Every Event Must Be Consumed or Terminal

**This is a separate subsystem, not an inline change.**

**Concept:** After the event bus dispatches an event, track whether any consumer processed it. Events that nobody consumed (no consumer had a matching filter) AND are not known terminal kinds should be flagged.

**Approach:**
- Add `consumed_count: usize` tracking to `EventBus::dispatch` return value
- In `EventRuntime`, after dispatch, if `consumed_count == 0` for a non-terminal event: log a warning

**File:** `canon-utils/canon-runtime/src/bus.rs`

Change `dispatch` signature:
```rust
pub fn dispatch(&self, event: RuntimeEvent, event_id: EventId) -> usize // returns consumer count
```

Count consumers that passed the filter check (not those that skipped via `continue`). Return the count.

In `lib.rs`, after `self.bus.dispatch(...)`, check:
```rust
let consumer_count = self.bus.dispatch(event.clone(), event_id.clone());
if consumer_count == 0 {
    // Only warn for events that are not purely informational
    const INFORMATIONAL_KINDS: &[&str] = &["debug", "runtime_state_updated"];
    if !INFORMATIONAL_KINDS.contains(&kind_str) {
        eprintln!("[canon-runtime] WARN: event {:?} had 0 consumers", kind_str);
    }
}
```

**Note:** This is lower priority — implement after Task 1-3 are verified working.

---

## Priority Order

1. **Task 2** — Verify the tlog has parent_ids now (confirms existing code works)
2. **Task 1** — Add write-gate soft warn (catch regressions)
3. **Task 3** — Propagate `current_dispatch_id` pattern to other consumers
4. **Task 4** — Empty payload warnings
5. **Task 5** — Watchdog consumer tracking
