# Plan: Fix idle_plan Spin (Event Spam While Waiting for Plan LLM)

## Problem

The runtime emits thousands of `route_selected` events while waiting for the plan LLM to
respond. Analysis of the event log shows 4852 `route_selected` vs 2 LLM calls — a ratio of
~2426:1. Events [1193–1207] in the log are all `route_selected` immediately after the LLM
is dispatched at event [1192].

## Root Cause

`try_dispatch_route` in `canon-utils/canon-route/src/executor.rs` has three deterministic
paths before falling through to LLM-based routing:

1. `finish_ready` → conclude
2. `planned_pending > 0` → act  (sets `pending_request_id = "deterministic"`)
3. **`idle_plan`** → plan        (does NOT set `pending_request_id`)

Because path 3 never sets `pending_request_id`, the guard at line 37–39:
```rust
if self.pending_request_id.is_some() { return; }
```
never fires for the idle-plan case. As a result, every `LoopObserved` event (emitted on
every tick by the loop executor, `executor.rs:42-56`) re-enters `try_dispatch_route` and
emits another `RouteSelected(plan)` before the previous plan LLM's `ToolCall` has been
processed by the route context.

The plan stage correctly short-circuits via `pending_plan.is_some()` in `handle_observed`,
so no duplicate LLM calls are made — but the `RouteSelected(plan)` spam still floods the
event log and wastes CPU.

## Fix

### File: `canon-utils/canon-route/src/executor.rs`

**Change 1 — `idle_plan` path: set `pending_request_id` sentinel**

In `try_dispatch_route`, the idle-plan block (lines 59–63):

```rust
// BEFORE
if self.ctx.planned_pending == 0 && !self.ctx.acted_unverified
    && !self.ctx.workspace_dirty_tracker.any_dirty()
    && !self.ctx.finish_ready && self.ctx.context_ready
{
    let json = heuristic_route_json(&self.ctx);
    self.emit_decision(&json, "deterministic:idle_plan".to_string());
    return;
}
```

Change to:

```rust
// AFTER
if self.ctx.planned_pending == 0 && !self.ctx.acted_unverified
    && !self.ctx.workspace_dirty_tracker.any_dirty()
    && !self.ctx.finish_ready && self.ctx.context_ready
{
    let json = heuristic_route_json(&self.ctx);
    self.pending_request_id = Some("deterministic".to_string());
    self.emit_decision(&json, "deterministic:idle_plan".to_string());
    return;
}
```

Setting `pending_request_id = Some("deterministic")` means any subsequent call to
`try_dispatch_route` (from the next tick's `LoopObserved`) hits the early-return guard
at line 37-39 and suppresses the duplicate `RouteSelected(plan)`.

**Change 2 — `LoopPlanned` handler: clear sentinel before routing to act**

After a plan LLM response arrives, the route executor sees `LoopPlanned` events. The
existing handler at lines 150–155:

```rust
// BEFORE
if let RuntimeEvent::LoopPlanned(_) = event {
    if self.ctx.planned_pending > 0 && self.ctx.pending_tool_result_ids.is_empty() {
        self.try_dispatch_route();
        return EventOutcome::NoOp("route_executor_plan_dispatch");
    }
}
```

Change to:

```rust
// AFTER
if let RuntimeEvent::LoopPlanned(_) = event {
    if self.ctx.planned_pending > 0 && self.ctx.pending_tool_result_ids.is_empty() {
        if self.pending_request_id.as_deref() == Some("deterministic") {
            self.pending_request_id = None;
        }
        self.try_dispatch_route();
        return EventOutcome::NoOp("route_executor_plan_dispatch");
    }
}
```

Without this, `pending_request_id = "deterministic"` (set by the idle_plan path) would
block `try_dispatch_route` when it is called from the `LoopPlanned` handler, preventing
routing to "act" after planning completes. Clearing the sentinel here is safe: it mirrors
the identical pattern already applied in the `should_try + idle` path (lines 139–142).

## Why This Works

After Change 1 fires for a given tick's `LoopObserved`:
- `pending_request_id = "deterministic"` is set
- `RouteSelected(plan)` is emitted once
- Plan stage runs → `ToolCall { kind: "llm.plan" }` is queued

On the next tick's `LoopObserved`:
- `try_dispatch_route` is called
- Line 37-39 guard fires: `pending_request_id.is_some()` → early return — no spam ✓

Safety valve (lines 98–101) handles the timeout/lost-LLM case:
- `pending_request_id.is_some() && planned_pending == 0 && pending_tool_result_ids.is_empty()`
- When the `ToolCall` is in the channel, `pending_tool_result_ids` is non-empty → NOT cleared
- If the LLM response is never received and the ToolCall somehow resolves (timeout path in
  plan stage), `pending_tool_result_ids` empties → safety valve clears `pending_request_id`
  → routing resumes ✓

After the LLM responds and `LoopPlanned(action)` arrives (Change 2):
- Sentinel cleared → `try_dispatch_route` → `planned_pending > 0` path → `RouteSelected(act)` ✓

## No-op plan case

When the plan stage emits `LoopPlanned { action_kind: "no_op" }` (placeholder goal, already
done), `planned_pending` stays 0. The safety valve fires immediately:
- `pending_request_id = "deterministic"` AND `planned_pending == 0` AND `pending_tool_result_ids.is_empty()` → cleared ✓
- The `LoopPlanned` handler condition `planned_pending > 0` is false → Change 2 is not reached
- System is quiet until the next tick's `LoopObserved`, which re-enters via Change 1

This means no-op plans still fire at tick rate (100ms), not in a tight spin. That is
acceptable behavior — the goal-complete spin is a separate issue not addressed here.

## Files Changed

- `canon-utils/canon-route/src/executor.rs` — two hunks as described above
