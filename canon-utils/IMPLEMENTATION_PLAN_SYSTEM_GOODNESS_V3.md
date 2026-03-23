# IMPLEMENTATION_PLAN_SYSTEM_GOODNESS_V3.md

**Scope:** Correctness fixes after V2 full integration.
**Status of V2:** All 7 plans are implemented and wired. This plan addresses three remaining bugs discovered by code review.

---

## Current State (post-V2)

| Item                                                  | Status |
|-------------------------------------------------------+--------|
| 13 metrics (T, K removed), exponent 1/13              | ✅     |
| S formula: agent-aware (`completed / agents × ticks`) | ✅     |
| Λ2 denominator capped at 6.0                          | ✅     |
| GoodnessConsumer registered in event_runtime.rs       | ✅     |
| goodness/delta_g in LoopRewarded via LoopContext      | ✅     |
| RouteContext goodness/delta_g + snapshot_text         | ✅     |
| delta_g < 0 → plan gate in canon-judgment             | ✅     |

---

## PLAN-V3-1 — Fix Λ1: clean verification rate (not ambient tick cleanliness)

**File:** `canon-utils/canon-goodness/src/reducers/love.rs`

**Bug:** `total_ticks` counts `RouteTick` events (~1/second). `clean_ticks` counts
`LoopVerified[compiler_clean]` events (~1 per 10+ ticks). These are different timescales —
a perfect agent with 100 RouteTicks and 5 clean verifications scores Λ1 = 5/100 = 0.05.
The metric is perpetually low regardless of code quality.

**Fix:** Replace `total_ticks`/`RouteTick` with `total_verifies`/`LoopVerified` counting.
Λ1 becomes "fraction of verifications that were clean" — directly meaningful, correct scale.

```rust
// BEFORE
pub struct Love {
    clean_ticks: u64,
    total_ticks: u64,           // ← counts RouteTick events (wrong scale)
    ...
}

// update():
RuntimeEvent::RouteTick(RouteTick { .. }) => {
    self.total_ticks = self.total_ticks.saturating_add(1);
}

// value():
let l1 = if self.total_ticks == 0 { 1.0 } else {
    self.clean_ticks as f32 / self.total_ticks as f32
};

// AFTER
pub struct Love {
    clean_verifies: u64,
    total_verifies: u64,        // ← counts LoopVerified events (same scale)
    ...
}

// update(): remove RouteTick arm entirely.
// In LoopVerified arm:
RuntimeEvent::LoopVerified(v) => {
    self.total_verifies = self.total_verifies.saturating_add(1);
    if v.compiler_clean {
        self.clean_verifies = self.clean_verifies.saturating_add(1);
    }
}

// value():
let l1 = if self.total_verifies == 0 { 1.0 } else {
    self.clean_verifies as f32 / self.total_verifies as f32
};
```

Rename the field `clean_ticks` → `clean_verifies` for clarity. Remove the `total_ticks` field entirely.

---

## PLAN-V3-2 — Fix g_prev initialization: 1.0 → 0.0

**File:** `canon-utils/canon-goodness/src/consumer.rs`

**Bug:** `g_prev: 1.0` means the very first `GoodnessSnapshot` will always have
`delta_g = G_1 - 1.0 < 0` (since G ≤ 1.0 and typically starts below 1.0). This triggers
the `delta_g < 0 → plan` gate in canon-judgment on the first tick — an unnecessary replan
before any work has been done.

**Fix:**
```rust
// BEFORE (line 16):
Self { reducers: AllReducers::new(), g_prev: 1.0, ... }

// AFTER:
Self { reducers: AllReducers::new(), g_prev: 0.0, ... }
```

With `g_prev = 0.0`, the first delta is `G_1 - 0.0 = G_1 ≥ 0`, so the gate never fires
spuriously on startup.

---

## PLAN-V3-3 — Fix reward.rs goodness unwrap_or default: 1.0 → 0.0

**File:** `canon-utils/canon-loop/src/stage/reward.rs`

**Bug:** Both call sites use `ctx.goodness.unwrap_or(1.0)`. Before any `GoodnessSnapshot`
has been emitted (before the first `LoopVerified`), the `LoopRewarded` event reports `goodness=1.0`
— an optimistic lie. This is inconsistent with the `g_prev=0.0` fix and could mislead
downstream consumers.

**Fix:** Change both call sites to `unwrap_or(0.0)`:

```rust
// execute_conclude (line 15):
goodness: ctx.goodness.unwrap_or(0.0),

// execute (line 37):
goodness: ctx.goodness.unwrap_or(0.0),
```

When no goodness measurement exists yet, `goodness=0.0` is the honest default: no signal yet.

---

## Implementation order

1. PLAN-V3-2 first (consumer.rs, 1-line change, unblocks correct delta_g semantics)
2. PLAN-V3-3 (reward.rs, 2-line change, keeps goodness default consistent)
3. PLAN-V3-1 last (love.rs, most structural change — field rename + handler removal)

Run `cargo check -p canon-goodness -p canon-loop` after each plan to verify no regressions.

---

## Notes on the delta_g gate (no change needed)

With V3-2 applied (`g_prev=0.0`), the delta_g gate in `canon-judgment/src/lib.rs` (line 307–313)
behaves correctly: it only fires when goodness genuinely decreases from one measurement to the next.
The current single-tick sensitivity (any negative delta triggers replan) is acceptable for now —
it is conservative but not harmful. A follow-up could add a minimum-observation guard (require ≥2
consecutive negative deltas before gating), but that is a tuning concern, not a correctness bug.
