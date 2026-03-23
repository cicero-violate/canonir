# Implementation Plan: System Goodness Objective Function

Implements `G_t = (∏ x_i)^(1/16)` — a geometric mean over 16 normalised metrics derived
exclusively from the canon event stream. `R_t = G_t - G_{t-1}` becomes the reward signal
fed back into the loop.

---

## New Crate: `canon-utils/canon-goodness/`

Add to `canon/Cargo.toml` workspace members:
```toml
"canon-utils/canon-goodness",
```

### Directory Layout

```
canon-goodness/
  Cargo.toml
  src/
    lib.rs           — re-exports
    metrics.rs       — Metrics struct (16 fields)
    reducer.rs       — Reducer trait
    reducers/
      mod.rs
      intelligence.rs   (I)
      efficiency.rs     (E)
      correctness.rs    (C)
      alignment.rs      (A)
      robustness.rs     (R)
      performance.rs    (P)
      scalability.rs    (S)
      determinism.rs    (D)
      transparency.rs   (T)
      knowledge.rs      (K)
      execution.rs      (X)
      benefit.rs        (B)
      learning.rs       (L)
      future.rs         (F)
      love.rs           (Λ — computes Λ1, Λ2, Λ3 and their geometric mean)
    aggregator.rs    — normalize(), compute_g()
    consumer.rs      — GoodnessConsumer: EventConsumer impl
    storage.rs       — append-only metrics.log / goodness.log writer
```

### `Cargo.toml` Dependencies

```toml
[dependencies]
canon-runtime-events = { path = "../canon-runtime-events" }
serde        = { workspace = true, features = ["derive"] }
serde_json   = { workspace = true }
anyhow       = { workspace = true }
```

---

## PLAN-1: `Metrics` Struct and `Reducer` Trait

**File**: `src/metrics.rs`, `src/reducer.rs`

```rust
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Metrics {
    pub i: f32,  // Intelligence
    pub e: f32,  // Efficiency
    pub c: f32,  // Correctness
    pub a: f32,  // Alignment
    pub r: f32,  // Robustness
    pub p: f32,  // Performance
    pub s: f32,  // Scalability
    pub d: f32,  // Determinism
    pub t: f32,  // Transparency
    pub k: f32,  // Knowledge / Collaboration
    pub x: f32,  // Execution
    pub b: f32,  // Benefit
    pub l: f32,  // Learning
    pub f: f32,  // Future-proofing
    pub lambda: f32,  // Love (composite of Λ1 Λ2 Λ3)
}

impl Metrics {
    pub fn as_array(&self) -> [f32; 15] {
        [self.i, self.e, self.c, self.a, self.r, self.p, self.s,
         self.d, self.t, self.k, self.x, self.b, self.l, self.f, self.lambda]
    }
}

pub trait Reducer: Send {
    fn update(&mut self, event: &canon_event::RuntimeEvent);
    fn value(&self) -> f32;  // always in [0.0, 1.0]
    fn reset(&mut self);     // called at session start / after snapshot
}
```

---

## PLAN-2: Event → Metric Mappings (One Reducer Per Metric)

Each reducer accumulates counters from the event stream only.
All emit `value()` ∈ [0, 1].

### I — Intelligence: correct inference rate
```
numerator:   CapabilityCompleted[llm.call] where the resulting LoopPlanned.action_kind
             is not "no_op" and not "error"
denominator: all CapabilityCompleted[llm.call]
I = numerator / denominator
```
**Events**: `CapabilityCompleted`, `LoopPlanned`
**State**: `llm_calls: u32`, `useful_llm_calls: u32`

### E — Efficiency: useful output per LLM cost
```
numerator:   LoopActed where success=true
denominator: sum of CapabilityCompleted[llm.call].result.Llm.duration_ms
             normalised to target (e.g. 30_000 ms per useful action)
E = (useful_actions / llm_calls) / (avg_duration_ms / target_duration_ms)
  → clamp(0, 1)
```
**Events**: `LoopActed`, `CapabilityCompleted`
**State**: `useful_actions: u32`, `total_duration_ms: u64`, `llm_calls: u32`

### C — Correctness: error-free action rate
```
errors:   LoopActed where success=false AND stderr ≠ "skipped:batch_aborted"
outputs:  all LoopActed
C = 1 - (errors / outputs)
```
**Events**: `LoopActed`
**State**: `errors: u32`, `outputs: u32`

### A — Alignment: goal-consistent action rate
```
Proxy: average of RouteSelected.signals.goal_alignment_score over the window.
Fallback: LoopActed[success=true] / LoopActed[total]
A = rolling mean of goal_alignment_score
```
**Events**: `RouteSelected` (signals field), `LoopActed`
**State**: `score_sum: f32`, `score_count: u32`

### R — Robustness: stability under repeated failure
```
consecutive_fail_runs: count of runs where same action_kind fails ≥ 2 times in a row
total_fail_events:     LoopActed[success=false]
R = 1 - (consecutive_fail_runs / max(total_fail_events, 1))
```
**Events**: `LoopActed`
**State**: `prev_failed_kind: Option<String>`, `consecutive_runs: u32`, `total_fails: u32`

### P — Performance: throughput / latency ratio
```
throughput: LoopActed[success=true] count in the last 10-tick window
latency:    average CapabilityCompleted[llm.call].duration_ms
P = (throughput / target_throughput) * (target_latency_ms / avg_latency_ms)
  → clamp(0, 1)
```
**Events**: `CapabilityCompleted`, `LoopActed`, `RouteTick`
**State**: sliding window of (tick, success) pairs; rolling duration average

### S — Scalability: throughput efficiency as tick count grows
```
Proxy (single-agent): actions_per_tick in last N ticks vs actions_per_tick in first N ticks
S = (recent_rate / baseline_rate) → clamp(0, 1)
Baseline window = first 10 ticks; recent window = last 10 ticks.
Default 1.0 until both windows are full.
```
**Events**: `LoopActed`, `RouteTick`

### D — Determinism: output consistency for repeated action kinds
```
For each action_kind seen ≥ 2 times:
  success_rate = successes / attempts
variance_of_success_rates = Var({success_rate_per_kind})
D = 1 - clamp(variance_of_success_rates * 4, 0, 1)
```
**Events**: `LoopActed`
**State**: `HashMap<String, (u32 successes, u32 attempts)>`

### T — Transparency: observable event coverage
```
observable: events with kind ≠ Unknown and payload non-empty
total:      all events processed by consumer
T = observable / total
```
**Events**: all (consumer counts every call to `update()`)

### K — Knowledge reuse: repeated successful pattern rate
```
reused:  LoopPlanned where action_kind was previously executed with success=true
total:   all LoopPlanned where action_kind ≠ "no_op"
K = reused / total
```
**Events**: `LoopPlanned`, `LoopActed`
**State**: `HashSet<String>` of previously successful action_kinds

### X — Execution: planned→completed rate
```
completed: LoopActed where success=true
planned:   LoopPlanned where action_kind ≠ "no_op"
X = completed / planned
```
**Events**: `LoopPlanned`, `LoopActed`

### B — Benefit: value created per LLM resource used
```
value_created: LoopVerified where compiler_clean=true (count)
resources:     CapabilityCompleted[llm.call] count
B = (value_created / target_clean_verifies) / (resources / target_llm_budget)
  → clamp(0, 1)
Targets: 1 clean verify per 5 LLM calls is B=1.0
```
**Events**: `LoopVerified`, `CapabilityCompleted`

### L — Learning: performance improvement rate
```
perf(t) = rolling 5-tick success rate of LoopActed
L = clamp((perf_recent - perf_baseline + 0.5), 0, 1)
  → 0.5 = no change, >0.5 = improving, <0.5 = degrading
```
**Events**: `LoopActed`, `RouteTick`

### F — Future-proofing: change resilience (regression rate)
```
regressions: LoopActed[success=false] that occur after LoopVerified[compiler_clean=true]
             (i.e. the last verify was clean but the new action broke it)
changes:     LoopActed[action_kind ∈ {apply_patch, write_file, run_command}][success=true]
F = 1 - (regressions / max(changes, 1))
```
**Events**: `LoopActed`, `LoopVerified`
**State**: `last_verify_clean: bool`, `regressions: u32`, `changes: u32`

### Λ — Love: long-term preservation × cooperation × non-harm
```
Λ1 = sustained_clean_ticks / total_ticks      (long-term preservation)
Λ2 = unique_successful_action_kinds / total_actions_attempted   (cooperation / diversity)
Λ3 = 1 - (destructive_blocks / total_acted)   (inverse harm)
     destructive_blocks: stderr ∈ {"rejected_destructive_command", "blocked:destructive_command"}

Λ = (Λ1 * Λ2 * Λ3)^(1/3)
```
**Events**: `LoopVerified`, `LoopActed`, `RouteTick`

---

## PLAN-3: Normalization and Aggregation

**File**: `src/aggregator.rs`

```rust
pub fn normalize(obs: f32, target: f32) -> f32 {
    (obs / target).clamp(0.0, 1.0)
}

pub fn compute_g(m: &Metrics) -> f32 {
    let arr = m.as_array();
    // Geometric mean: replace any 0.0 with a floor (0.01) to avoid full collapse.
    let product: f32 = arr.iter()
        .map(|&x| x.max(0.01))
        .product();
    product.powf(1.0 / arr.len() as f32)
}

pub fn compute_reward(g_now: f32, g_prev: f32) -> f32 {
    g_now - g_prev
}
```

Zero-floor: a single zero metric collapses the entire G to zero, which is too harsh for
missing data. Use `0.01` as a floor to preserve signal from other dimensions while still
penalising the weak metric.

---

## PLAN-4: `GoodnessConsumer` — EventConsumer Integration

**File**: `src/consumer.rs`

```rust
pub struct GoodnessConsumer {
    reducers: AllReducers,  // struct holding one instance of each reducer
    g_prev: f32,
    emitter: Option<EventEmitterHandle>,
    storage: Option<MetricsStorage>,
}

impl EventConsumer for GoodnessConsumer {
    fn filter(&self) -> EventFilter { EventFilter::All }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.emitter = Some(emitter);
    }

    fn on_event(&mut self, event: &RuntimeEvent) {
        // Feed all reducers.
        self.reducers.update_all(event);

        // Snapshot after every LoopVerified (end of verify cycle).
        if let RuntimeEvent::LoopVerified(v) = event {
            let metrics = self.reducers.snapshot();
            let g_now = compute_g(&metrics);
            let delta = compute_reward(g_now, self.g_prev);
            self.g_prev = g_now;

            // Persist to storage.
            if let Some(s) = &mut self.storage {
                s.append_metrics(v.tick, &metrics);
                s.append_goodness(v.tick, g_now, delta);
            }

            // Emit GoodnessSnapshot event.
            if let Some(emitter) = &self.emitter {
                canon_emit!(emitter; GoodnessSnapshot(GoodnessSnapshot {
                    tick: v.tick,
                    g: g_now,
                    delta_g: delta,
                    metrics: serde_json::to_value(&metrics).unwrap_or_default(),
                }));
            }
        }
    }
}
```

---

## PLAN-5: New `GoodnessSnapshot` Event

**File**: `canon-utils/canon-runtime-events/src/events.rs`

```rust
canon_event_struct!(GoodnessSnapshot {
    tick: u64,
    g: f32,
    delta_g: f32,
    metrics: serde_json::Value,  // full Metrics struct as JSON
});
// Add to RuntimeEvent enum:
GoodnessSnapshot(GoodnessSnapshot),
```

Re-export from `lib.rs`: add `GoodnessSnapshot` to the `pub use events::{ ... }` list.

---

## PLAN-6: Extend `LoopRewarded` With Goodness Fields

**File**: `canon-utils/canon-runtime-events/src/events.rs`

Add two fields to `LoopRewarded`:

```rust
canon_event_struct!(LoopRewarded {
    tick: u64,
    reward: f32,
    errors_before: usize,
    errors_after: usize,
    stagnant_ticks: u32,
    halt: bool,
    #[serde(default)]
    goodness: f32,       // <-- G_t at this tick
    #[serde(default)]
    delta_g: f32,        // <-- R_t = G_t - G_{t-1}
    // ... existing optional fields unchanged
});
```

`reward::execute` in `canon-loop` reads the latest `GoodnessSnapshot` from a shared
reference (`Arc<Mutex<f32>>` updated by `GoodnessConsumer`) and populates these fields.

---

## PLAN-7: Feed G Into Router Snapshot

**File**: `canon-utils/canon-route/src/context.rs`

Add `goodness: f32` to `RouteContext`. Update on `GoodnessSnapshot` event:

```rust
RuntimeEvent::GoodnessSnapshot(snap) => {
    self.goodness = snap.g;
    self.delta_g = snap.delta_g;
}
```

Update `snapshot_text()` to include:

```
goodness={g:.3}\ndelta_g={delta_g:+.3}
```

The LLM router now sees the current system health score in every prompt.

---

## PLAN-8: Feed ΔG Into `RouteController` Gate

**File**: `canon-utils/canon-judgment/src/lib.rs`

Add `goodness: f32` and `delta_g: f32` to `RuntimeSignals`. In `apply_gate`:

```rust
// Stagnation guard upgrade: if G has not improved in 5 consecutive reward cycles,
// treat as stagnant regardless of stagnant_ticks counter.
if signals.delta_g < 0.0 && signals.last_action_failed {
    // Goodness is declining AND action failed — force replan.
    lane = RouteKind::Plan;
    changed = true;
    notes.push("delta_g<0 with failure — force replan");
}
```

---

## PLAN-9: Storage (Append-Only Logs)

**File**: `src/storage.rs`

Two JSONL files written alongside the tlog:

```
state/event_log/
  metrics.log    — one JSON object per LoopVerified tick: { tick, i, e, c, ..., lambda }
  goodness.log   — one JSON object per tick: { tick, g, delta_g, timestamp_ms }
```

```rust
pub struct MetricsStorage {
    metrics_path: PathBuf,
    goodness_path: PathBuf,
}

impl MetricsStorage {
    pub fn append_metrics(&self, tick: u64, m: &Metrics) { ... }
    pub fn append_goodness(&self, tick: u64, g: f32, delta: f32) { ... }
}
```

Both files are append-only. No rotation — they grow with the session.

---

## PLAN-10: Register `GoodnessConsumer` in the Runtime

**File**: `canon-utils/canon-runtime/src/bin/event_runtime.rs`

Instantiate `GoodnessConsumer` alongside `RouteExecutor` and `LoopStageExecutor`:

```rust
let goodness_consumer = GoodnessConsumer::new(
    storage_path,         // state/event_log/
);
runtime.register_consumer(Box::new(goodness_consumer));
```

No other wiring needed — `GoodnessConsumer` is a pure `EventConsumer` that reacts to the
same broadcast stream as all other consumers.

---

## Integration with the Agent Loop

| Stage   | Goodness role                                                         |
|---------|-----------------------------------------------------------------------|
| observe | All reducers update on LoopObserved (goal, errors)                    |
| plan    | K, I reducers update on LoopPlanned / CapabilityCompleted             |
| act     | C, X, R, F, Λ3 reducers update on LoopActed                          |
| verify  | B, F, Λ1 update on LoopVerified; GoodnessSnapshot emitted here        |
| reward  | LoopRewarded gains `goodness` + `delta_g` fields from latest snapshot |

---

## File Change Summary

| File                                                   | Change                                     |
|--------------------------------------------------------|--------------------------------------------|
| `canon-utils/canon-goodness/` (new crate)              | Full crate — 15 reducers + aggregator      |
| `canon-utils/canon-runtime-events/src/events.rs`       | Add `GoodnessSnapshot`; extend `LoopRewarded` |
| `canon-utils/canon-runtime-events/src/lib.rs`          | Re-export `GoodnessSnapshot`               |
| `canon-utils/canon-route/src/context.rs`               | Add `goodness`, `delta_g` fields + handler |
| `canon-utils/canon-judgment/src/lib.rs`                | Add `delta_g` to `RuntimeSignals` + gate rule |
| `canon-utils/canon-loop/src/stage/reward.rs`           | Populate `goodness`, `delta_g` in `LoopRewarded` |
| `canon-utils/canon-runtime/src/bin/event_runtime.rs`   | Register `GoodnessConsumer`                |
| `canon/Cargo.toml`                                     | Add `canon-goodness` workspace member      |

---

## Priority Order

| Plan    | Description                               | Effort | Dependency  |
|---------|-------------------------------------------|--------|-------------|
| PLAN-1  | Metrics struct + Reducer trait            | XS     | —           |
| PLAN-2  | All 15 reducer implementations            | M      | PLAN-1      |
| PLAN-3  | Normalization + geometric mean            | XS     | PLAN-1      |
| PLAN-5  | GoodnessSnapshot event                    | XS     | —           |
| PLAN-4  | GoodnessConsumer (EventConsumer)          | S      | PLAN-2, 3, 5|
| PLAN-9  | Storage (metrics.log, goodness.log)       | XS     | PLAN-4      |
| PLAN-10 | Register in event_runtime                 | XS     | PLAN-4      |
| PLAN-7  | Feed G into router snapshot               | XS     | PLAN-5      |
| PLAN-6  | Extend LoopRewarded with goodness fields  | XS     | PLAN-5      |
| PLAN-8  | ΔG gate rule in RouteController           | S      | PLAN-7      |
