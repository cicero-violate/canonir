# Implementation Plan: System Goodness V2

Picks up from V1. The `canon-goodness` crate is fully built (15 reducers, aggregator,
consumer, storage). This plan covers: metric corrections, wiring the consumer into the
runtime, and propagating G/ΔG through the router and gate.

---

## What Is Already Done

| Component | Status |
|-----------|--------|
| `canon-goodness` crate — all 15 reducers, aggregator, consumer, storage | ✅ |
| `GoodnessSnapshot` event defined in `events.rs` | ✅ |
| `LoopRewarded.goodness` and `LoopRewarded.delta_g` fields (serde default) | ✅ |
| `canon-goodness` in workspace `Cargo.toml` | ✅ |

## What Is Not Yet Done

| Component | Plan |
|-----------|------|
| `T` (Transparency) and `K` (Knowledge reuse) reducers still present | PLAN-1 |
| `S` (Scalability) formula is single-agent only | PLAN-2 |
| `Λ2` (Cooperation) formula is action diversity, not cooperation | PLAN-3 |
| Exponent still `1/15` — needs update to `1/13` | PLAN-1 |
| `GoodnessConsumer` not registered in `event_runtime.rs` | PLAN-4 |
| `reward.rs` does not populate `goodness`/`delta_g` in `LoopRewarded` | PLAN-5 |
| `RouteContext` has no `goodness`/`delta_g` fields | PLAN-6 |
| `RuntimeSignals` has no `goodness`/`delta_g` fields | PLAN-7 |
| ΔG gate rule not in `canon-judgment` | PLAN-7 |

---

## PLAN-1: Remove `T` and `K` — Update Exponent to `1/13`

**Files**:
- `canon-utils/canon-goodness/src/metrics.rs`
- `canon-utils/canon-goodness/src/aggregator.rs`
- `canon-utils/canon-goodness/src/reducers/mod.rs`
- `canon-utils/canon-goodness/src/consumer.rs` (AllReducers)
- Delete: `src/reducers/transparency.rs`, `src/reducers/knowledge.rs`

### `metrics.rs`

Remove `t` and `k` fields from `Metrics`. Update `as_array` to return `[f32; 13]`:

```rust
pub struct Metrics {
    pub i: f32,
    pub e: f32,
    pub c: f32,
    pub a: f32,
    pub r: f32,
    pub p: f32,
    pub s: f32,
    pub d: f32,
    pub x: f32,
    pub b: f32,
    pub l: f32,
    pub f: f32,
    pub lambda: f32,
}

impl Metrics {
    pub fn as_array(&self) -> [f32; 13] {
        [self.i, self.e, self.c, self.a, self.r, self.p, self.s,
         self.d, self.x, self.b, self.l, self.f, self.lambda]
    }
}
```

### `aggregator.rs`

Update exponent:

```rust
pub fn compute_g(m: &Metrics) -> f32 {
    let arr = m.as_array();  // now [f32; 13]
    let product: f32 = arr.iter().map(|&x| x.max(0.01)).product();
    product.powf(1.0 / 13.0)
}
```

### `reducers/mod.rs` and `AllReducers`

Remove `transparency` and `knowledge` fields, remove their `update_all` calls and
`snapshot` assignments.

---

## PLAN-2: Fix `S` (Scalability) — Multi-Agent Ready Formula

**File**: `canon-utils/canon-goodness/src/reducers/scalability.rs`

### Current (broken)
Compares throughput at first 10 ticks vs last 10 ticks — duplicates `L` (learning).

### New Formula

```
S = completed_tasks / (active_agents × tick_window)
    normalised to target: 1 successful action per agent per 5 ticks = S=1.0
```

With one agent: `S = completed / (1 × ticks_elapsed)` clamped to target.
With N agents: `S = completed / (N × ticks_elapsed)` — measures whether agents
are contributing proportionally.

### Implementation

```rust
pub struct ScalabilityReducer {
    completed: u32,
    active_agents: u32,   // incremented on AgentRegistered
    ticks: u64,
    target_rate: f32,     // default: 1.0 / 5.0 (one action per 5 ticks per agent)
}

// update():
RuntimeEvent::AgentRegistered(_) => self.active_agents += 1,
RuntimeEvent::LoopActed(a) if a.success => self.completed += 1,
RuntimeEvent::RouteTick(t) => self.ticks = t.tick,

// value():
let agents = self.active_agents.max(1) as f32;
let rate = self.completed as f32 / (agents * self.ticks.max(1) as f32);
(rate / self.target_rate).clamp(0.0, 1.0)
```

---

## PLAN-3: Fix `Λ2` (Cooperation) — Capability Breadth Formula

**File**: `canon-utils/canon-goodness/src/reducers/love.rs`

### Current (broken)
`Λ2 = unique_successful_action_kinds / total_actions` — measures action diversity,
not cooperation.

### New Formula

```
Λ2 = unique_capability_types_used / (active_agents × total_capability_types_available)
     clamped [0, 1]
```

`total_capability_types_available` = known capability set size (constant: 6 —
`llm.call`, `run_command`, `apply_patch`, `write_file`, `read_file`, `list_dir`).

With one agent: measures whether the agent is using the full capability surface.
With N agents: a capability used by any agent counts once — measures whether agents
divide work across capability types rather than all doing the same thing.

### Implementation

```rust
// In LoveReducer, replace Λ2 state:
seen_capabilities: HashSet<String>,  // distinct capability names used
active_agents: u32,

const TOTAL_CAPABILITIES: f32 = 6.0;

// update():
RuntimeEvent::CapabilityCompleted(c) => {
    self.seen_capabilities.insert(c.capability.clone());
}
RuntimeEvent::AgentRegistered(_) => self.active_agents += 1,

// lambda2():
let agents = self.active_agents.max(1) as f32;
let breadth = self.seen_capabilities.len() as f32;
(breadth / (agents * TOTAL_CAPABILITIES).min(TOTAL_CAPABILITIES)).clamp(0.0, 1.0)
```

Note: the denominator is capped at `TOTAL_CAPABILITIES` so with many agents this
doesn't collapse — it rewards coverage, not repetition.

---

## PLAN-4: Register `GoodnessConsumer` in `event_runtime.rs`

**File**: `canon-utils/canon-runtime/src/bin/event_runtime.rs`

Add import and instantiation alongside existing consumers:

```rust
use canon_goodness::consumer::GoodnessConsumer;

// In the consumers vec:
let storage_path = tlog_path.parent()
    .unwrap_or(&tlog_path)
    .to_path_buf();

consumers.push(Box::new(
    GoodnessConsumer::new(Some(storage_path))
));
```

`GoodnessConsumer::new(storage_path)` — add this constructor if not present:

```rust
impl GoodnessConsumer {
    pub fn new(storage_path: Option<PathBuf>) -> Self {
        Self {
            reducers: AllReducers::default(),
            g_prev: 0.0,   // start at 0 not 1 — G must be earned
            storage: storage_path.map(MetricsStorage::new),
            emitter: None,
        }
    }
}
```

Note: initialise `g_prev = 0.0` not `1.0`. Starting at 1.0 means the first real
measurement always produces `delta_g < 0`, which poisons the gate on the first tick.

---

## PLAN-5: Populate `goodness`/`delta_g` in `reward.rs`

**File**: `canon-utils/canon-loop/src/stage/reward.rs`

The `Arc<Mutex<f32>>` coupling from V1 is the wrong approach. Instead, read from
the `GoodnessSnapshot` event that already lands on the shared event stream.

### Approach: `LoopContext` carries last goodness snapshot

**Step 1** — Add to `LoopContext` (`canon-loop/src/context.rs`):
```rust
pub last_goodness: f32,
pub last_delta_g: f32,
```

**Step 2** — In `LoopStageExecutor::on_event` state accumulation block:
```rust
RuntimeEvent::GoodnessSnapshot(snap) => {
    self.ctx.last_goodness = snap.g;
    self.ctx.last_delta_g = snap.delta_g;
}
```

**Step 3** — In `reward::execute` and `reward::execute_conclude`:
```rust
let rewarded = LoopRewarded {
    // ... existing fields ...
    goodness: ctx.last_goodness,
    delta_g: ctx.last_delta_g,
    // ...
};
```

No shared mutable state. `GoodnessSnapshot` arrives on the event stream before
`LoopVerified` triggers `reward::execute` (same broadcast tick), so the value is fresh.

---

## PLAN-6: Add `goodness`/`delta_g` to `RouteContext`

**File**: `canon-utils/canon-route/src/context.rs`

**Step 1** — Add fields:
```rust
pub goodness: f32,
pub delta_g: f32,
```

**Step 2** — Handle `GoodnessSnapshot` in `update_from_event`:
```rust
RuntimeEvent::GoodnessSnapshot(snap) => {
    self.goodness = snap.g;
    self.delta_g = snap.delta_g;
}
```

**Step 3** — Add to `snapshot_text()`:
```rust
format!("...goodness={g:.3}\ndelta_g={delta_g:+.3}",
    // ...
    g = self.goodness,
    delta_g = self.delta_g,
)
```

The router LLM now sees system health in every prompt.

---

## PLAN-7: Add ΔG Gate Rule to `canon-judgment`

**File**: `canon-utils/canon-judgment/src/lib.rs`

**Step 1** — Add to `RuntimeSignals`:
```rust
pub goodness: f32,
pub delta_g: f32,
```

**Step 2** — Populate in `RouteContext::signals()`:
```rust
RuntimeSignals {
    // ... existing fields ...
    goodness: self.goodness,
    delta_g: self.delta_g,
}
```

**Step 3** — Add gate rule in `apply_gate` (insert before the final note assembly):
```rust
// G declining AND action failed → force replan immediately.
if signals.delta_g < -0.05 && signals.last_action_failed
    && lane != RouteKind::Plan
{
    lane = RouteKind::Plan;
    changed = true;
    notes.push("delta_g declining with failure — force replan");
}

// G healthy AND finish_ready → allow conclude even if gate would override.
if signals.goodness > 0.7 && signals.finish_ready
    && lane != RouteKind::Conclude
{
    lane = RouteKind::Conclude;
    changed = true;
    notes.push("goodness>0.7 and finish_ready — conclude approved");
}
```

Threshold `-0.05` rather than `< 0.0` avoids noise from tiny fluctuations.

---

## File Change Summary

| File | Change |
|------|--------|
| `canon-goodness/src/metrics.rs` | Remove `t`, `k` fields; `as_array` → `[f32; 13]` |
| `canon-goodness/src/aggregator.rs` | Exponent `1/15` → `1/13` |
| `canon-goodness/src/reducers/mod.rs` | Remove transparency, knowledge |
| `canon-goodness/src/reducers/scalability.rs` | New formula (agent-aware) |
| `canon-goodness/src/reducers/love.rs` | Λ2 new formula (capability breadth) |
| `canon-goodness/src/consumer.rs` | Remove T/K from AllReducers; `g_prev = 0.0` |
| `canon-goodness/src/reducers/transparency.rs` | Delete |
| `canon-goodness/src/reducers/knowledge.rs` | Delete |
| `canon-runtime/src/bin/event_runtime.rs` | Register GoodnessConsumer |
| `canon-loop/src/context.rs` | Add `last_goodness`, `last_delta_g` |
| `canon-loop/src/executor.rs` | Handle GoodnessSnapshot → update context |
| `canon-loop/src/stage/reward.rs` | Populate `goodness`/`delta_g` in LoopRewarded |
| `canon-route/src/context.rs` | Add `goodness`, `delta_g`; handle GoodnessSnapshot |
| `canon-judgment/src/lib.rs` | Add fields to RuntimeSignals; two new gate rules |

---

## Priority Order

| Plan | Description | Effort |
|------|-------------|--------|
| PLAN-1 | Remove T, K; fix exponent to 1/13 | XS |
| PLAN-2 | Fix S formula (agent-aware) | XS |
| PLAN-3 | Fix Λ2 formula (capability breadth) | XS |
| PLAN-4 | Register GoodnessConsumer in event_runtime | XS |
| PLAN-5 | Populate goodness/delta_g in LoopRewarded via LoopContext | S |
| PLAN-6 | Add goodness/delta_g to RouteContext + snapshot_text | XS |
| PLAN-7 | Add gate rules in canon-judgment | XS |
