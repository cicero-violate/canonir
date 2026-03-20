# REPAIR PLAN v3 — routing deadlock under active batch (2026-03-20)

## Root cause summary

The runtime terminates mid-batch with queued planned actions orphaned. Two bugs in
`canon-utils/canon-judgment/src/lib.rs` combine to cause this:

1. **Cycle cap fires unconditionally** — ignores `has_queued_plan`, halts with pending work.
2. **`performed_recently` blocks `execute`** — any action (including an inline rejection)
   sets `acted_unverified=true`, which permanently routes away from `execute` until verify
   clears it. The remaining batch items can never dispatch because `execute` is never
   selected again.

Observed event sequence that triggers both bugs:

```
rm-rf rejected (inline, no capability dispatched)
  → LoopActed(success=false)
  → route_state.acted_unverified = true
  → route_state.planned_pending = 4 (5 planned − 1 acted)

cargo new dispatched → fails (exit 101)
  → LoopActed at event_id=169 (not yet applied to route_state when tick fires)

Tick 13 fires:
  flush_emitted_events → applies rm-rf LoopActed → planned_pending=4
  snapshot: planned_pending=4, acted_unverified=true

  LLM route call → "no llm endpoints configured" → heuristic fallback
  heuristic: acted_unverified=true → Validate   (planned_pending check never reached)
  Gatekeeper.review: cycle_count=13 > max_cycles=12 → early-return Conclude, should_stop=true

Runtime exits. 3 queued actions orphaned: cargo build, write README, done.
```

---

## Fix 1 — Cycle cap must not fire while a batch is in progress

**File:** `canon-utils/canon-judgment/src/lib.rs`

**Problem (lines 78–85):**
```rust
if self.state.cycle_count > self.cfg.max_cycles {
    return GateResult {
        lane: RouteKind::Conclude,
        changed: true,
        note: "cycle cap reached; forcing conclude".to_string(),
        should_stop: true,
    };
}
```
This is an unconditional early return that runs before any signal is examined.
`signals.has_queued_plan` (which is `planned_pending > 0`) is never checked.

**Fix:** When the cap fires and `has_queued_plan` is true, reset the cycle counter and
fall through to normal routing instead of halting. The cap still fires when there is
nothing left to do.

Replace the block at lines 78–85 with:

```rust
if self.state.cycle_count > self.cfg.max_cycles {
    if signals.has_queued_plan {
        // Mid-batch: the planner has already issued work that has not been
        // executed yet. Resetting the counter lets the batch drain before
        // the cap is reconsidered. This prevents early termination when
        // slow actions (e.g. cargo build) push the tick count past the limit.
        self.state.cycle_count = 0;
        // Fall through to normal routing below.
    } else {
        return GateResult {
            lane: RouteKind::Conclude,
            changed: true,
            note: "cycle cap reached; forcing conclude".to_string(),
            should_stop: true,
        };
    }
}
```

---

## Fix 2 — `performed_recently` must not override `execute` mid-batch

**File:** `canon-utils/canon-judgment/src/lib.rs`

**Problem (lines 124–128):**
```rust
if signals.performed_recently && lane != RouteKind::Validate {
    lane = RouteKind::Validate;
    changed = true;
    notes.push("acted_unverified=true requires validate");
}
```
`performed_recently` is `acted_unverified`. It is set by every `LoopActed`, including
inline rejections (`no_op`, `done`, rejected destructive commands). Once set, this guard
overrides any `execute` selection for every subsequent tick — even when there are still
planned actions in the queue waiting for `dispatch_batch_on_execute` to be called.

The remaining batch items (cargo build, write README, done) can only be dispatched when
the route is `execute`. With this guard always firing, the route is locked on `validate`
and those items sit in the queue forever.

**Fix:** Skip the `validate` override when a queued plan is waiting. A batch that is
mid-execution should continue executing; verify can run after the batch completes.

Replace lines 124–128 with:

```rust
if signals.performed_recently && !signals.has_queued_plan && lane != RouteKind::Validate {
    lane = RouteKind::Validate;
    changed = true;
    notes.push("acted_unverified=true requires validate");
}
```

The single change is adding `&& !signals.has_queued_plan`. When `has_queued_plan=true`
the guard is skipped, allowing the route to stay on `execute`. When the queue is empty
(batch done) the original behaviour is restored: `validate` is forced until verify runs.

---

## Fix 3 — Heuristic has the same priority inversion

**File:** `canon-utils/canon-runtime/src/bin/event_runtime.rs`

**Problem (lines 203–216):**
```rust
let route = if state.finish_ready {
    RouteKind::Conclude
} else if state.acted_unverified {
    RouteKind::Validate          // ← fires before planned_pending check
} else if state.planned_pending > 0 {
    RouteKind::Execute
```
`acted_unverified` is checked before `planned_pending`. When the LLM route call fails
and the heuristic is used as fallback, it also returns `Validate` mid-batch. Fix 2 in
the gatekeeper would catch this, but the heuristic should be consistent:

Replace the `else if` chain inside `heuristic_route_json`:

```rust
let route = if state.finish_ready {
    RouteKind::Conclude
} else if state.planned_pending > 0 {
    RouteKind::Execute           // drain the active batch first
} else if state.acted_unverified {
    RouteKind::Validate          // verify only after batch is empty
} else if state.workspace_dirty {
    RouteKind::Validate
} else if state.context_ready {
    RouteKind::Shape
} else {
    RouteKind::Scan
};
```

`planned_pending > 0` is now checked before `acted_unverified`.

---

## Fix 4 — `GuardConfig::default` max_cycles is too low for slow builds

**File:** `canon-utils/canon-judgment/src/lib.rs`

`max_cycles: 12` means 12 seconds of ticks (P3 fires every 1 s). A single `cargo build`
can take 15–20 s, consuming more than 12 ticks on its own. Even with Fix 1, the cap
would fire repeatedly and reset during long commands.

Raise the default:

```rust
impl Default for GuardConfig {
    fn default() -> Self {
        Self {
            max_cycles: 64,      // was 12
            max_repeat_lane: 3,
            minimum_confidence: Some(0.20),
            fallback_lane: RouteKind::Scan,
        }
    }
}
```

64 ticks gives ~64 s per planning cycle before the cap fires, which is sufficient for
most build operations. The cap still exists as a liveness guard against truly stuck
states.

---

## Files to modify

| File | Fixes |
|------|-------|
| `canon-utils/canon-judgment/src/lib.rs` | 1, 2, 4 |
| `canon-utils/canon-runtime/src/bin/event_runtime.rs` | 3 |

---

## Acceptance criteria

| Check | Pass condition |
|-------|----------------|
| Remaining batch items execute after an inline rejection (rm-rf blocked) | cargo build, write README, done all appear in `_tool_results.json` in the same run |
| Runtime does not exit while `planned_pending > 0` | `_batch_status.json` shows `status=completed` or `status=failed_partial` before process exits |
| Cycle cap still fires when the queue is empty and goal is incomplete | After N idle ticks with `planned_pending=0` the runtime concludes normally |
| LLM heuristic fallback returns `execute` when `planned_pending > 0` | `route_selected` debug event shows `approved_route=execute` during active batch even when LLM is unavailable |

## Implementation order

1. Fix 4 (`max_cycles: 64`) — one-line, no logic change, immediate safety margin
2. Fix 1 (cycle cap skips when `has_queued_plan`) — guards the hard stop
3. Fix 2 (gatekeeper `execute` override) — core routing fix
4. Fix 3 (heuristic priority order) — consistency fix, low risk
