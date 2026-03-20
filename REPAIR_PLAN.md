# REPAIR_PLAN.md — Post-Repair Analysis (Round 7)

## Verification results from latest tlog (82,118 events) + LLM log files (calls #000–#042)

| Check                           | Result                                                                           |
|---------------------------------+----------------------------------------------------------------------------------|
| Error spam                      | **0** ✅                                                                         |
| Verify races                    | **0** ✅                                                                         |
| LTR fix                         | **✅ confirmed working** — LTR appears in router prompts from call #005 onward   |
| Gate blocking                   | **✅ confirmed working** — BLOCKED events show correct pending_tool_call_ids     |
| LTR in tlog route_selected      | **❌ not logged** — route_selected payload has no ltr_present field              |
| Gate override visible in tlog   | **❌ not logged** — gate_note not in route_selected payload                      |
| Gate forcing execute over shape | **❌ BUG** — gate overrides LLM's shape recommendation when has_queued_plan=true |

---

## Bug — Gate forces Execute even when last action failed

**What the LLM log shows (calls #006 and #007):**

```
#005  router  LTR=REAL(success)  → route=execute  ← mkdir succeeded
#006  router  LTR=REAL(failure)  → LLM says shape  ← cargo new failed (status=101)
       ↑ tlog shows approved_route=execute         ← gate OVERRODE to execute!
#007  router  LTR=REAL(failure)  → LLM says shape  ← LLM still says replan
       ↑ tlog shows approved_route=execute         ← gate OVERRODE again!
```

After cargo new fails with "destination already exists", the router LLM correctly diagnoses the problem and returns `route=shape` (to trigger replanning). But the gate overrides this to `execute`, dispatching the NEXT planned action (cargo build), which compiles canon-runtime and restarts the runtime. On restart the same plan is generated and the cycle repeats.

**Root cause in `canon-judgment/src/lib.rs` line 118:**

```rust
// CURRENT — fires unconditionally when any plan actions remain:
if signals.has_queued_plan && lane != RouteKind::Execute {
    lane = RouteKind::Execute;
    changed = true;
    notes.push("queued plan requires execute");
}
```

`has_queued_plan = planned_pending > 0`. After cargo new fails, `planned_pending=3` (cargo build, write_file, done still queued). So the gate forces execute, skipping the LLM's correct shape recommendation.

**Why the gate exists**: Designed to prevent the LLM from routing to scan/shape/validate while queued work remains unexecuted. Correct in the happy path. Wrong when a step fails and the remaining plan is invalid.

---

## Fix

### Part 1 — Add `last_action_failed` signal to canon-judgment

**File: `canon-utils/canon-judgment/src/lib.rs`**

**Add field to `RuntimeSignals`:**

```rust
// BEFORE:
pub struct RuntimeSignals {
    pub context_ready: bool,
    pub has_queued_plan: bool,
    pub workspace_dirty: bool,
    pub performed_recently: bool,
    pub finish_ready: bool,
}

// AFTER:
pub struct RuntimeSignals {
    pub context_ready: bool,
    pub has_queued_plan: bool,
    pub workspace_dirty: bool,
    pub performed_recently: bool,
    pub finish_ready: bool,
    pub last_action_failed: bool,  // ← ADD: true when last LoopActed.success=false
}
```

**Modify gate rule to exempt failures:**

```rust
// BEFORE (line 118):
if signals.has_queued_plan && lane != RouteKind::Execute {
    lane = RouteKind::Execute;
    changed = true;
    notes.push("queued plan requires execute");
}

// AFTER:
if signals.has_queued_plan && lane != RouteKind::Execute && !signals.last_action_failed {
    lane = RouteKind::Execute;
    changed = true;
    notes.push("queued plan requires execute");
}
```

When `last_action_failed=true`, the gate does NOT force execute, allowing the LLM's shape recommendation to pass through.

---

### Part 2 — Track last_action_failed in RouteRuntimeState

**File: `canon-utils/canon-runtime/src/bin/event_runtime.rs`**

**Add field to `RouteRuntimeState` struct (after `last_action_kind`):**

```rust
// BEFORE:
last_action_kind: String,

// AFTER:
last_action_kind: String,
last_action_failed: bool,
```

**Set it in `update_route_runtime_state`, `LoopActed` arm (after the existing `last_action_kind` assignment):**

```rust
// BEFORE:
route_state.last_action_kind = action_kind.clone();

// AFTER:
route_state.last_action_kind = action_kind.clone();
route_state.last_action_failed = !success;
```

**Expose it in `signals()` method:**

```rust
// BEFORE:
fn signals(&self) -> RuntimeSignals {
    RuntimeSignals {
        context_ready: self.context_ready,
        has_queued_plan: self.planned_pending > 0,
        workspace_dirty: self.workspace_dirty,
        performed_recently: self.acted_unverified,
        finish_ready: self.finish_ready,
    }
}

// AFTER:
fn signals(&self) -> RuntimeSignals {
    RuntimeSignals {
        context_ready: self.context_ready,
        has_queued_plan: self.planned_pending > 0,
        workspace_dirty: self.workspace_dirty,
        performed_recently: self.acted_unverified,
        finish_ready: self.finish_ready,
        last_action_failed: self.last_action_failed,
    }
}
```

**Clear it on next successful action (same LoopActed arm, `last_action_failed = !success` handles this already** — when `success=true`, `!success=false`).

---

### Part 3 — Add debug fields to route_selected event (yes, emit debug events)

**File: `canon-utils/canon-runtime/src/bin/event_runtime.rs`**

In `handle_control_msg`, where `route_selected` is emitted (around line 603–616), add `ltr_present`, `gate_changed`, `gate_note`:

```rust
// BEFORE:
runtime.emit_debug_event(
    "supervisor".to_string(),
    "route_selected".to_string(),
    serde_json::json!({
        "tick": route_state.scheduler_tick,
        "suggested_route": selection.route.as_str(),
        "approved_route": lane,
        "rationale": selection.rationale,
        "confidence": selection.confidence,
        "changed": gate.changed,
        "note": gate.note,
        "prompt": prompt,
    }),
)?;

// AFTER:
runtime.emit_debug_event(
    "supervisor".to_string(),
    "route_selected".to_string(),
    serde_json::json!({
        "tick": route_state.scheduler_tick,
        "suggested_route": selection.route.as_str(),
        "approved_route": lane,
        "rationale": selection.rationale,
        "confidence": selection.confidence,
        "changed": gate.changed,
        "note": gate.note,
        "ltr_present": route_state.latest_tool_result.is_some(),
        "last_action_failed": route_state.last_action_failed,
        "prompt": prompt,
    }),
)?;
```

This makes LTR presence and gate overrides visible directly in the tlog without needing to read LLM log files.

---

## File change summary

| File | Change |
|---|---|
| `canon-utils/canon-judgment/src/lib.rs` | (1) Add `last_action_failed: bool` to `RuntimeSignals`. (2) Add `&& !signals.last_action_failed` to the `has_queued_plan → Execute` gate rule. |
| `canon-utils/canon-runtime/src/bin/event_runtime.rs` | (1) Add `last_action_failed: bool` to `RouteRuntimeState`. (2) Set `route_state.last_action_failed = !success` in `LoopActed` arm of `update_route_runtime_state`. (3) Add `last_action_failed: self.last_action_failed` in `signals()`. (4) Add `ltr_present` and `last_action_failed` to `route_selected` event payload. |

---

## Invariants to preserve

- When `last_action_failed=true` and LLM says shape: gate passes shape through (allows replanning).
- When `last_action_failed=true` and LLM says execute: gate still passes execute (LLM may choose to continue despite failure — gate does not block it).
- When `last_action_failed=false` and `has_queued_plan=true`: gate still forces execute as before (existing behavior unchanged in the happy path).
- `last_action_failed` is cleared automatically on the next successful action (`!success = false`), so a recovered plan resumes normal forced-execute behavior.
- Do NOT change `error_logger.rs`, `canon-verify/src/lib.rs`, `dispatch_next_in_active_batch` removal, or the LTR clear-on-ToolCall fix — all confirmed working.

---

---

## Bug — route_selected=execute is silently dropped, causing multi-tick delay before tool dispatch

**Root cause in `canon-runtime/src/bus.rs`:**

`ActConsumer` uses `EventFilter::All` — it receives ALL events including 40,000+ `CanonEvent::Code` (rustc) events per session. Its consumer channel is `bounded(1024)`. The channel fills with rustc events. When `route_selected=execute` (`CanonEvent::Debug`) is dispatched via `try_send`, the channel has no room and the signal is **silently dropped**.

Result: the router fires `route_selected=execute` 2–3 times before ActConsumer ever sees it. When the signal finally gets through, the tool runs and completes quickly. From the LLM's perspective: multiple execute choices → tool result appears "immediately" with no gate-blocking wait in between. This is the "immediate system response" behavior the user observes.

**Evidence from LLM logs (current session):**
```
#004  execute  ltr=(none)   ← router fires, signal dropped
#005  execute  ltr=(none)   ← router fires again, signal dropped
#006  execute  ltr=(none)   ← signal finally delivered → mkdir dispatched
#007  execute  ltr=REAL     ← mkdir done, gate unblocked, LTR=Some
```

**The `is_control_event` check** determines reliable vs try_send delivery:
```rust
// current — missing CanonEvent::Debug:
fn is_control_event(event: &CanonEvent) -> bool {
    matches!(event,
        CanonEvent::Tick(_) | CanonEvent::LoopPlanned(_) | CanonEvent::LoopActed(_) | ...
        // Debug is NOT here → try_send → drops when ActConsumer channel is full
    )
}
```

**Also**: all 43+ `route_selected` events that were missing from the tlog (only 11 visible) are because the **tlog writer's consumer channel** also drops them via the same try_send path.

### Fix — Add `CanonEvent::Debug` to `is_control_event`

**File: `canon-utils/canon-runtime/src/bus.rs`**

```rust
// BEFORE:
fn is_control_event(event: &CanonEvent) -> bool {
    matches!(
        event,
        CanonEvent::Tick(_)
            | CanonEvent::PromptLoaded(_)
            | CanonEvent::CapabilityRequested(_)
            | CanonEvent::CapabilityCompleted(_)
            | CanonEvent::CapabilityFailed(_)
            | CanonEvent::LoopObserved(_)
            | CanonEvent::LoopPlanned(_)
            | CanonEvent::LoopActed(_)
            | CanonEvent::LoopVerified(_)
            | CanonEvent::LoopRewarded(_)
    )
}

// AFTER:
fn is_control_event(event: &CanonEvent) -> bool {
    matches!(
        event,
        CanonEvent::Tick(_)
            | CanonEvent::PromptLoaded(_)
            | CanonEvent::CapabilityRequested(_)
            | CanonEvent::CapabilityCompleted(_)
            | CanonEvent::CapabilityFailed(_)
            | CanonEvent::LoopObserved(_)
            | CanonEvent::LoopPlanned(_)
            | CanonEvent::LoopActed(_)
            | CanonEvent::LoopVerified(_)
            | CanonEvent::LoopRewarded(_)
            | CanonEvent::Debug(_)  // route_selected must reach ActConsumer reliably
    )
}
```

**Why this is safe**: `Debug` events are infrequent (~500/session vs 40,000+ `Code` events). Blocking delivery is fine — all consumers handle Debug events quickly (error_logger ignores them, verify_consumer only inspects `route_selected`, ActConsumer does a simple lane string check). This also fixes the missing `route_selected` events in the tlog.

**Note**: `CanonEvent::ToolCall` and `CanonEvent::ToolResult` do NOT need to be added here — they travel from ActConsumer to event_runtime via the tlog reader (durable, reliable path), not via the event bus consumer channels.

**File change summary addition:**

| File | Change |
|---|---|
| `canon-utils/canon-runtime/src/bus.rs` | Add `CanonEvent::Debug(_)` to `is_control_event` so route_selected events use blocking send instead of try_send. |

---

## What this does NOT fix

- The plan always starts with `cargo new` even though the directory exists. After this fix, the router will route to shape on failure, the planner will be called again, and the planner's prompt includes the LTR (the failure stderr). The planner prompt already says "If a target directory already exists, prefer `cargo init --bin <dir>`" — with the failure LTR now reaching the planner, it should generate the correct plan.
- LLM timeout at calls #041 and #042 — this is a transient LLM bridge connectivity issue, not a routing logic bug. The heuristic fallback handles it by routing to validate or scan.
