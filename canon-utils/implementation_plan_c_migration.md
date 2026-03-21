# Implementation Plan: Consumer → Event Collapse (C-Migration)

## Current Build Status

```
Phase 1 — ✅ complete  (canon-loop crate created, LoopStageEvent + TryFrom + LoopContext)
Phase 2 — ✅ complete  (all 5 stage modules: observe, plan, act, verify, reward)
Phase 3 — ✅ complete  (LoopStageExecutor wired in, 5 consumers replaced, cargo check clean)
Phase 4 — ✅ complete  (canon-observe, canon-plan, canon-act, canon-verify, canon-reward deleted)
Phase 5 — ✅ migration clean; pre-existing failures remain (see below)
```

**Build status:** `cargo check --workspace` — ✅ zero errors (confirmed 2026-03-21).

**`cargo test --workspace` status:**
- `cargo test -p canon-loop` — ✅ passes
- `cargo test -p canon-runtime` — ✅ passes (including `async_consumers_preserve_order_per_consumer`)
- All migration-introduced failures — ✅ none
- **Pre-existing failures (unrelated to this migration):**
  - `canon-runtime-supervisor` — `allow(dead_code)` from transitive macro expansion vs `-F dead_code`
  - `canon-storage-eventlog` bins — same pattern
  - `canon-tools-editor/tests/project_editor_tests.rs` — references nonexistent crate `project_editor`
  - `canon-tools-editor` bin `editor_capability_smoke_test` — same `allow(dead_code)` pattern (transitive dep macro, not in source)
  - `algorithms` example `gpu_example` — stale `mut` warning promoted to error

**Zero remaining references** to deleted stage crates confirmed:
```bash
grep -r "canon_observe|canon_plan|canon_act|canon_verify|canon_reward" canon-utils/ → 0 results
```

**Migration verdict:** C-migration is complete — the 5 formal `EventConsumer` loop-stage
consumers are gone and their logic lives in `canon-loop`. However, `event_runtime.rs` is
still 1358 lines. The size is NOT from the consumers (those are removed). It comes from
five distinct concerns that were outside this migration's scope:

| Lines | Block | What it is |
|-------|-------|------------|
| 33–117 | LockGuard, EventMsg, ControlMsg | Process locking, queue message types |
| 128–316 | `RouteRuntimeState` + routing helpers | Routing state machine: `planned_pending`, `context_ready`, `acted_unverified`, journal, `heuristic_route_json`, `request_route_via_llm_call` |
| 317–413 | `update_route_runtime_state()` | **6th implicit consumer** — inline match on loop events that mirrors the consumer pattern but lives in the main loop. Updates `RouteRuntimeState` from every loop event. Was never a formal `dyn EventConsumer`. |
| 415–624 | `handle_event_msg`, `drain_event_queue_with_grace`, `handle_control_msg` | Event dispatch, queue draining, tick handler — calls LLM for routing, emits `route_selected` |
| 630–1107 | `main()` | Process startup, file watching (notify), cursor/session management, tlog replay, full event loop |
| 1107–1270 | Helper utilities | Cursor state, tlog segment reading, tlog equivalence verification |
| 1270–1358 | Tests | Unit tests |

**`update_route_runtime_state()` (lines 317–413) is the next collapse candidate.** It is a
6th implicit consumer: it receives every loop event and accumulates routing signals into
`RouteRuntimeState`. This is a **D-migration** — routing state observer → event. Out of
scope for C-migration. See `implementation_plan_d_migration.md` when ready.

---

## Goal

Replace 5 stateful loop-stage consumers with a single `LoopStageExecutor` that dispatches
via `LoopStageEvent::try_from(event) → execute(&mut LoopContext)`.

```
Before:
  EventRuntime → [ObserveConsumer, PlanConsumer, ActConsumer, VerifyConsumer, RewardConsumer,
                  ErrorLogger, CheckConsumer, CapabilityExecutor]
  (5 separate stateful consumers, each with hidden state, implicit routing)

After:
  EventRuntime → [LoopStageExecutor, ErrorLogger, CheckConsumer, CapabilityExecutor]
  (1 unified executor with LoopContext + LoopStageEvent dispatch, no _ arm)
```

**New crate:** `canon-loop` — holds `LoopContext`, `LoopStageEvent`, stage modules.

**Deleted:** `canon-observe/`, `canon-plan/`, `canon-act/`, `canon-verify/`, `canon-reward/`

**Invariant preserved from R-migration:**
- `LoopStageEvent::execute()` has NO `_` arm.
- Adding a new loop stage requires adding a `LoopStageEvent` variant — compiler error until done.

---

## Architecture

### Stage event flow (current)

```
Tick
  → ObserveConsumer → LoopObserved
  → PlanConsumer (route="shape") → LlmCall → CapabilityCompleted
  → PlanConsumer → LoopPlanned × n
  → ActConsumer (route="execute") → File/Bash/Cargo → CapabilityCompleted
  → ActConsumer → LoopActed
  → VerifyConsumer (route="validate") → LoopVerified
  → RewardConsumer → LoopRewarded
```

### Route-selected pattern (verified across all 5 consumers)

Every consumer that responds to routing uses:
```rust
CanonEvent::Debug(debug) if debug.kind == "route_selected" => {
    let lane = debug.payload
        .get("approved_route")
        .or_else(|| debug.payload.get("lane"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if lane == "<lane_name>" { ... }
}
```
Lanes: `"shape"` (plan trigger), `"execute"` (act dispatch), `"validate"` (verify trigger),
`"conclude"` (forced halt).

### CapabilityCompleted disambiguation

Both PlanConsumer and ActConsumer receive every `CapabilityCompleted`. Each silently ignores
completions that don't match their pending `request_id`. In the collapsed form, a single
`CapabilityDone(CapabilityCompleted)` variant's `execute()` checks `ctx.pending_plan.request_id`
first, then `ctx.pending_act.request_id`. If neither matches → `LoopStageResult::Noop`.

---

## LoopStageEvent — 8 Variants (No `_` Arm)

| Variant                               | Trigger                   | Primary Output                                         |
|---------------------------------------+---------------------------+--------------------------------------------------------|
| `Observe(Tick)`                       | Every tick                | `LoopObserved`                                         |
| `PlanTrigger(DebugEvent)`             | `lane == "shape"`         | `CanonEvent::Llm(LlmCall)` or `LoopPlanned(no_op)`     |
| `ActDispatch(DebugEvent)`             | `lane == "execute"`       | `File/Bash/Cargo` capability events                    |
| `VerifyTrigger(DebugEvent)`           | `lane == "validate"`      | `LoopVerified`                                         |
| `Conclude(DebugEvent)`                | `lane == "conclude"`      | `LoopRewarded(halt=true)`                              |
| `CapabilityDone(CapabilityCompleted)` | Any capability completion | `LoopPlanned × n` (plan) OR `LoopActed` (act)          |
| `CapabilityFail(CapabilityFailed)`    | Any capability failure    | `LoopPlanned(no_op)` (plan) OR `LoopActed(fail)` (act) |
| `Reward(LoopVerified)`                | Every `LoopVerified`      | `LoopRewarded`                                         |

---

## LoopContext — Unified State

All 5 consumer struct fields merge into one struct. Field prefixes indicate origin.

```rust
// canon-loop/src/context.rs
use canon_event::{CanonEvent, EventEmitterHandle, LoopActed, LoopObserved, LoopPlanned, ToolResult};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::time::Instant;

pub struct LoopContext {
    // — Infrastructure (shared) —
    pub workspace: PathBuf,
    pub tlog_path: PathBuf,
    pub emitter: Option<EventEmitterHandle>,

    // — ObserveConsumer state —
    pub goal_text: Option<String>,
    pub recent_compiler_errors: Vec<serde_json::Value>,  // capped at 16 entries
    pub error_count: usize,
    pub warning_count: usize,

    // — PlanConsumer state —
    pub pending_plan: Option<PendingPlan>,
    pub last_observed: Option<LoopObserved>,
    pub last_planned_observed_tick: Option<u64>,
    pub last_done_goal: Option<String>,
    pub batch_acted: Vec<LoopActed>,
    pub batch_tool_results: Vec<ToolResult>,
    pub last_prompted_goal: Option<String>,

    // — ActConsumer state —
    pub act_queue: VecDeque<LoopPlanned>,
    pub pending_act: Option<PendingAct>,
    pub artifact_dir: PathBuf,
    pub artifact_counter: u32,
    pub active_batch_llm_request_id: Option<String>,
    pub queued_artifact_index: HashMap<String, u32>,
    pub act_batch_tracker: HashMap<String, BatchStatus>,
    pub last_act_reconcile: Option<Instant>,
    pub destructive_cmd_policy: DestructiveCmdPolicy,

    // — VerifyConsumer state —
    pub last_verify_trace_id: Option<String>,
    pub last_verify_execution_id: Option<String>,
    pub last_act_span_id: Option<String>,
    pub last_acted: Option<LoopActed>,
    pub last_verified_action_key: Option<String>,

    // — RewardConsumer state —
    pub errors_before: usize,
    pub stagnant_ticks: u32,
    pub last_action_kind: String,
    pub last_action_success: bool,
    pub halted: bool,
    pub last_reward_trace_id: Option<String>,
    pub last_reward_execution_id: Option<String>,
    pub last_reward_verify_span_id: Option<String>,
}
```

The private structs `PendingPlan`, `PendingAct`, `BatchStatus`, `DestructiveCmdPolicy` are
ported verbatim from their origin crates into `canon-loop/src/context.rs`. Do NOT change
their fields.

- `PendingPlan` — currently at `canon-plan/src/lib.rs:27`
- `PendingAct` — currently at `canon-act/src/lib.rs:26`
- `BatchStatus` — currently at `canon-act/src/lib.rs:45`
- `DestructiveCmdPolicy` — currently at `canon-act/src/lib.rs:53`

`LoopContext::new(workspace, tlog_path)` initializes all fields to their zero/empty state,
mirroring the `new()` constructors of the 5 consumer structs. `artifact_dir` is derived as
`workspace.join("artifacts")`. `destructive_cmd_policy` reads from `CANON_DESTRUCTIVE_CMD_POLICY`
env var, same as the current `ActConsumer`.

---

## State Accumulation — Inline in `LoopStageExecutor::on_event()`

These events update `LoopContext` fields without producing output `CanonEvent`s. They are
handled with an inline `match` BEFORE the `LoopStageEvent::try_from()` dispatch call.

| Event                                   | Mutation                                                                                                                                                                                                               |
|-----------------------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `Tick(t)`                               | Call `ctx.check_plan_timeout(t.tick)` (port from `PlanConsumer::check_llm_timeout`) and `ctx.check_act_timeout()` + `ctx.reconcile_stale_pending_artifacts()` (port from `ActConsumer`)                                |
| `LoopObserved(o)`                       | `ctx.last_observed = Some(o.clone()); ctx.errors_before = o.error_count;`                                                                                                                                              |
| `LoopActed(a)`                          | `ctx.last_acted = Some(a.clone()); ctx.last_action_kind = a.action_kind.clone(); ctx.last_action_success = a.success; ctx.batch_acted.push(a.clone()); if !a.success { ctx.last_planned_observed_tick = None; }`       |
| `LoopPlanned(p)`                        | `ctx.act_queue.push_back(p.clone()); ctx.artifact_index_for_plan(p); ctx.write_tool_call_queued_artifact(artifact_n, p); ctx.mark_batch_planned(p, artifact_n);` — port enqueue logic from `ActConsumer::enqueue_plan` |
| `LoopVerified(v)`                       | `ctx.last_verify_trace_id = v.trace_id.clone(); ctx.last_verify_execution_id = v.execution_id.clone(); if v.passed { ctx.error_count = 0; ctx.warning_count = 0; }`                                                    |
| `LoopRewarded(r)`                       | `if r.halt { ctx.halted = true; }`                                                                                                                                                                                     |
| `PromptLoaded(p)`                       | Port `ObserveConsumer` goal extraction and `PlanConsumer::handle_prompt_loaded` — both update `ctx.goal_text` and `ctx.last_prompted_goal`                                                                             |
| `ErrorOccurred(e)`                      | Port `ObserveConsumer` error accumulation: increment `ctx.error_count` or `ctx.warning_count` based on severity; push to `ctx.recent_compiler_errors` (cap at 16)                                                      |
| `ToolResult(r) if r.kind != "llm.plan"` | `ctx.batch_tool_results.push(r.clone());`                                                                                                                                                                              |

---

## ~~Phase 1~~ — ✅ Complete — Create `canon-loop` crate

### Step 1a — Add to workspace

In `/workspace/ai_sandbox/canon/Cargo.toml`, add `"canon-utils/canon-loop"` to the
`members` array.

### Step 1b — `canon-utils/canon-loop/Cargo.toml`

```toml
[package]
name = "canon-loop"
version = "0.1.0"
edition = "2021"

[lib]
name = "canon_loop"
path = "src/lib.rs"

[dependencies]
anyhow.workspace = true
serde_json.workspace = true
uuid.workspace = true
canon_event  = { package = "canon-runtime-events", path = "../canon-runtime-events" }
canon-meta   = { path = "../canon-meta" }
```

No deps on the 5 stage crates — their logic is ported directly.

### Step 1c — `canon-loop/src/lib.rs`

```rust
pub mod context;
pub mod result;
pub mod stage;
pub mod executor;

pub use context::LoopContext;
pub use result::LoopStageResult;
pub use stage::LoopStageEvent;
pub use executor::LoopStageExecutor;
```

### Step 1d — `canon-loop/src/result.rs`

```rust
use canon_event::CanonEvent;

#[derive(Debug)]
pub enum LoopStageResult {
    Emit(CanonEvent),
    EmitMany(Vec<CanonEvent>),
    Noop,
    Deferred,  // used by PlanTrigger: LlmCall sent, waiting for CapabilityCompleted
}
```

### Step 1e — `canon-loop/src/context.rs`

Declare `LoopContext` as the struct listed above. Port `PendingPlan`, `PendingAct`,
`BatchStatus`, `DestructiveCmdPolicy` verbatim from their source crates (listed above).

Implement `LoopContext::new(workspace: PathBuf, tlog_path: PathBuf) -> Self` initializing
all fields to empty/zero, mirroring each consumer's `new()`.

Also port all helper methods from the consumers that operate on fields now in LoopContext:
- From `ObserveConsumer`: `scan_tlog_for_goal()` (called on init to recover goal from tlog history)
- From `PlanConsumer`: `check_llm_timeout()`, `handle_prompt_loaded()`, `build_prompt()`,
  `parse_llm_actions()`, `count_loc_in_workspace()`
- From `ActConsumer`: `artifact_index_for_plan()`, `write_tool_call_queued_artifact()`,
  `mark_batch_planned()`, `check_act_timeout()`, `reconcile_stale_pending_artifacts()`,
  `is_destructive_cmd()`
- From `VerifyConsumer`: `run_cargo_check()`, `parse_compiler_messages()`,
  `check_tlog_clean()`, `check_file_written()`, `acted_action_key()`
- From `RewardConsumer`: reward formula helpers

These helpers become `pub(crate)` methods on `LoopContext` or free functions in `context.rs`.

### Step 1f — `canon-loop/src/stage/mod.rs` — LoopStageEvent skeleton

```rust
use canon_event::{CanonEvent, CapabilityCompleted, CapabilityFailed, DebugEvent,
                  LoopVerified, Tick};
use super::{LoopContext, LoopStageResult};

pub mod observe;
pub mod plan;
pub mod act;
pub mod verify;
pub mod reward;

pub enum LoopStageEvent {
    Observe(Tick),
    PlanTrigger(DebugEvent),
    ActDispatch(DebugEvent),
    VerifyTrigger(DebugEvent),
    Conclude(DebugEvent),
    CapabilityDone(CapabilityCompleted),
    CapabilityFail(CapabilityFailed),
    Reward(LoopVerified),
}

impl LoopStageEvent {
    pub fn execute(self, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
        match self {
            LoopStageEvent::Observe(t)          => observe::execute(t, ctx),
            LoopStageEvent::PlanTrigger(d)      => plan::execute_trigger(d, ctx),
            LoopStageEvent::ActDispatch(d)      => act::execute_dispatch(d, ctx),
            LoopStageEvent::VerifyTrigger(d)    => verify::execute(d, ctx),
            LoopStageEvent::Conclude(d)         => reward::execute_conclude(d, ctx),
            LoopStageEvent::CapabilityDone(c)   => dispatch_capability_done(c, ctx),
            LoopStageEvent::CapabilityFail(f)   => dispatch_capability_fail(f, ctx),
            LoopStageEvent::Reward(v)           => reward::execute(v, ctx),
            // NO _ arm
        }
    }
}

/// CapabilityCompleted can be for a plan (LLM) or an act (file/bash/cargo).
/// Check pending_plan.request_id first; if no match, check pending_act.request_id.
fn dispatch_capability_done(
    c: CapabilityCompleted,
    ctx: &mut LoopContext,
) -> anyhow::Result<LoopStageResult> {
    if ctx.pending_plan.as_ref().map(|p| &p.request_id) == Some(&c.request_id) {
        return plan::execute_complete(c, ctx);
    }
    if ctx.pending_act.as_ref().map(|p| &p.request_id) == Some(&c.request_id) {
        return act::execute_complete(c, ctx);
    }
    Ok(LoopStageResult::Noop)
}

fn dispatch_capability_fail(
    f: CapabilityFailed,
    ctx: &mut LoopContext,
) -> anyhow::Result<LoopStageResult> {
    if ctx.pending_plan.as_ref().map(|p| &p.request_id) == Some(&f.request_id) {
        return plan::execute_failed(f, ctx);
    }
    if ctx.pending_act.as_ref().map(|p| &p.request_id) == Some(&f.request_id) {
        return act::execute_failed(f, ctx);
    }
    Ok(LoopStageResult::Noop)
}

impl TryFrom<CanonEvent> for LoopStageEvent {
    type Error = CanonEvent;
    fn try_from(e: CanonEvent) -> Result<Self, CanonEvent> {
        fn route_lane(d: &DebugEvent) -> &str {
            if d.kind != "route_selected" { return ""; }
            d.payload
                .get("approved_route")
                .or_else(|| d.payload.get("lane"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
        }
        match e {
            CanonEvent::Tick(t) => Ok(LoopStageEvent::Observe(t)),
            CanonEvent::Debug(d) if route_lane(&d) == "shape"    => Ok(LoopStageEvent::PlanTrigger(d)),
            CanonEvent::Debug(d) if route_lane(&d) == "execute"  => Ok(LoopStageEvent::ActDispatch(d)),
            CanonEvent::Debug(d) if route_lane(&d) == "validate" => Ok(LoopStageEvent::VerifyTrigger(d)),
            CanonEvent::Debug(d) if route_lane(&d) == "conclude" => Ok(LoopStageEvent::Conclude(d)),
            CanonEvent::CapabilityCompleted(c) => Ok(LoopStageEvent::CapabilityDone(c)),
            CanonEvent::CapabilityFailed(f)    => Ok(LoopStageEvent::CapabilityFail(f)),
            CanonEvent::LoopVerified(v)        => Ok(LoopStageEvent::Reward(v)),
            other => Err(other),
        }
    }
}
```

**IMPORTANT:** `CapabilityFailed.request_id` — verify the exact field name by reading
`CapabilityFailed` struct definition in `canon-runtime-events/src/events.rs` before
implementing `dispatch_capability_fail`. Use the same field name used in `ActConsumer::handle_failed`.

**Verify:** `cargo check -p canon-loop` — zero errors before proceeding to Phase 2.

---

## ~~Phase 2~~ — ✅ Complete — Port each stage module

Create `canon-loop/src/stage/observe.rs`, `plan.rs`, `act.rs`, `verify.rs`, `reward.rs`.

Each file is a **direct port** of the corresponding consumer's private methods, with the
following substitutions:
- `self.field` → `ctx.field`
- `self.emitter.as_ref()` → `ctx.emitter.as_ref()`
- `self.emit_debug(kind, payload)` → inline `canon_meta::canon_emit_meta!(emitter; source, kind, payload)`
- Return type: `anyhow::Result<LoopStageResult>` instead of `()`

### `observe.rs` — port from `canon-observe/src/lib.rs`

Port the `Tick` handler logic (lines 47–61 of observe/src/lib.rs).

```rust
pub fn execute(t: Tick, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    // Port the body of ObserveConsumer::on_event for Tick, which builds and emits LoopObserved.
    // Returns LoopStageResult::Emit(CanonEvent::LoopObserved { tick, error_count,
    //   warning_count, compiler_errors, goal_text }) instead of calling emitter directly.
    // ...
}
```

### `plan.rs` — port from `canon-plan/src/lib.rs`

Three entry points:

```rust
/// route="shape" trigger — port handle_observed() (lines 164–279 of plan/src/lib.rs)
pub fn execute_trigger(d: DebugEvent, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult>

/// CapabilityCompleted (LLM response) — port handle_capability_completed() (lines 282–439)
pub fn execute_complete(c: CapabilityCompleted, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult>

/// CapabilityFailed (LLM failure) — port handle_capability_failed()
pub fn execute_failed(f: CapabilityFailed, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult>
```

`execute_trigger` returns `LoopStageResult::Deferred` when an `LlmCall` is emitted
(because the plan isn't ready yet — it arrives as a future `CapabilityCompleted`).
It returns `LoopStageResult::EmitMany(vec![LoopPlanned(no_op)])` when skipping.

`execute_complete` returns `LoopStageResult::EmitMany(planned_events)` with one
`LoopPlanned` per parsed action.

**Key constant to preserve:** `LLM_TIMEOUT_TICKS: u64 = 60` — move to `context.rs` as a
module-level constant.

### `act.rs` — port from `canon-act/src/lib.rs`

```rust
/// route="execute" trigger — port dispatch_batch_on_execute()
pub fn execute_dispatch(d: DebugEvent, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult>

/// CapabilityCompleted for capability (file/bash/cargo) — port handle_completed()
pub fn execute_complete(c: CapabilityCompleted, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult>

/// CapabilityFailed for capability — port handle_failed()
pub fn execute_failed(f: CapabilityFailed, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult>
```

`execute_dispatch` dequeues plans from `ctx.act_queue` (FIFO), calls the dispatching logic
for each, emits `File/Bash/Cargo` capability events. Returns `EmitMany` with capability events.

`execute_complete` emits `LoopActed` with success=true, stdout/stderr, duration_ms.

`execute_failed` emits `LoopActed` with success=false.

**Preserve all destructive command blocking logic** from the current `dispatch_plan()`.
`ctx.destructive_cmd_policy` is read from env during `LoopContext::new()` — do not re-read it
on every dispatch.

### `verify.rs` — port from `canon-verify/src/lib.rs`

```rust
/// route="validate" trigger — port verify_acted() (lines 96–184 of verify/src/lib.rs)
pub fn execute(d: DebugEvent, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult>
```

Returns `LoopStageResult::Emit(CanonEvent::LoopVerified { ... })`.

Port helper functions into `context.rs` as `LoopContext` methods (or free functions in
`context.rs`):
- `run_cargo_check(workspace: &Path) -> anyhow::Result<std::process::Output>` with 30s timeout
- `parse_compiler_messages(output: &std::process::Output) -> (usize, Vec<String>)`
- `check_tlog_clean(tlog_path: &Path, acted: &LoopActed) -> bool`
- `check_file_written(path: &Path) -> Option<String>` (returns diagnostic if file missing/bad)

**Preserve the deduplication check** via `ctx.last_verified_action_key`.

### `reward.rs` — port from `canon-reward/src/lib.rs`

```rust
/// LoopVerified trigger — port handle_verified() (lines 88–177 of reward/src/lib.rs)
pub fn execute(v: LoopVerified, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult>

/// route="conclude" trigger — port emit_forced_halt()
pub fn execute_conclude(d: DebugEvent, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult>
```

`execute` returns `LoopStageResult::Emit(CanonEvent::LoopRewarded { ... })`.

Preserve the full reward formula verbatim:
```
reward = (errors_before - errors_after) as f32
reward += 0.5 if verified.passed && last_action_kind != "no_op"
reward -= 1.0 if !verified.passed
halt = true if last_action_kind == "done" && last_action_success
halt = true if stagnant_ticks > 5
```

**Verify:** `cargo check -p canon-loop` — zero errors before Phase 3.

---

## ~~Phase 3~~ — ✅ Complete — Create `LoopStageExecutor` in `canon-runtime`

### Step 3a — `canon-runtime/src/consumers/loop_executor.rs` — new file

```rust
use canon_event::{CanonEvent, EventConsumer, EventEmitterHandle, EventFilter,
                  new_error_occurred};
use canon_loop::{LoopContext, LoopStageEvent, LoopStageResult};
use std::path::PathBuf;
use serde_json::json;

pub struct LoopStageExecutor {
    ctx: LoopContext,
}

impl LoopStageExecutor {
    pub fn new(workspace: PathBuf, tlog_path: PathBuf) -> Self {
        Self { ctx: LoopContext::new(workspace, tlog_path) }
    }
}

impl EventConsumer for LoopStageExecutor {
    fn filter(&self) -> EventFilter { EventFilter::All }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.ctx.emitter = Some(emitter);
        // Port any set_emitter startup logic from the 5 consumers (debug emissions, etc.)
    }

    fn on_event(&mut self, event: &CanonEvent) {
        // Phase 1: State accumulation (inline match — updates ctx, no output events)
        //   Copy the table from "State Accumulation" section above verbatim.
        match event {
            CanonEvent::ErrorOccurred(e)          => { /* accumulate errors */ }
            CanonEvent::LoopVerified(v) if v.passed => { /* reset error counts */ }
            CanonEvent::PromptLoaded(p)            => { /* update goal_text */ }
            CanonEvent::LoopObserved(o)            => { /* update last_observed */ }
            CanonEvent::LoopActed(a)               => { /* update last_acted, batch_acted */ }
            CanonEvent::LoopPlanned(p)             => { /* enqueue to act_queue + artifact */ }
            CanonEvent::LoopVerified(v)            => { /* update verify trace ids */ }
            CanonEvent::LoopRewarded(r) if r.halt  => { ctx.halted = true; }
            CanonEvent::Tick(t)                    => { /* check_plan_timeout, check_act_timeout */ }
            CanonEvent::ToolResult(r) if r.kind != "llm.plan" => { /* accumulate */ }
            _ => {}
        }

        // Phase 2: Stage dispatch
        let Ok(stage) = LoopStageEvent::try_from(event.clone()) else { return; };
        let Some(emitter) = self.ctx.emitter.clone() else { return; };
        match stage.execute(&mut self.ctx) {
            Ok(LoopStageResult::Emit(e))      => emitter.emit(e),
            Ok(LoopStageResult::EmitMany(es)) => es.into_iter().for_each(|e| emitter.emit(e)),
            Ok(LoopStageResult::Noop | LoopStageResult::Deferred) => {}
            Err(err) => emitter.emit(CanonEvent::ErrorOccurred(new_error_occurred(
                "loop_stage", "loop_executor",
                err.to_string(), "error",
                json!({ "event": format!("{:?}", event) }),
                None,
            ))),
        }
    }
}
```

**NOTE:** The `match event` state-accumulation block has overlapping arms for `LoopVerified`
(one for `v.passed` reset and one for trace id update). In Rust, write two separate arms:
```rust
CanonEvent::LoopVerified(v) => {
    ctx.last_verify_trace_id = v.trace_id.clone();
    ctx.last_verify_execution_id = v.execution_id.clone();
    ctx.last_reward_verify_span_id = v.span_id.clone();
    if v.passed {
        ctx.error_count = 0;
        ctx.warning_count = 0;
    }
}
```

### Step 3b — `canon-runtime/src/consumers/mod.rs`

Add `pub mod loop_executor;` to the module list.

### Step 3c — `canon-runtime/src/bin/event_runtime.rs`

Replace the 5 consumer instantiations with a single `LoopStageExecutor`:

```rust
// Before:
Box::new(ObserveConsumer::new(workspace.clone(), tlog_path.clone())),
Box::new(PlanConsumer::new(workspace.clone())),
Box::new(ActConsumer::new(workspace.clone())),
Box::new(VerifyConsumer::new(workspace.clone(), tlog_path.clone())),
Box::new(RewardConsumer::new()),

// After:
Box::new(LoopStageExecutor::new(workspace.clone(), tlog_path.clone())),
```

Remove imports of `ObserveConsumer`, `PlanConsumer`, `ActConsumer`, `VerifyConsumer`,
`RewardConsumer` from `event_runtime.rs`. Add import of `LoopStageExecutor`.

### Step 3d — `canon-runtime/Cargo.toml`

Add:
```toml
canon_loop = { package = "canon-loop", path = "../canon-loop" }
```

Do NOT remove `canon_observe`, `canon_plan`, `canon_act`, `canon_verify`, `canon_reward`
deps yet — that is Phase 4.

**Verify:** `cargo check -p canon-runtime` — zero errors before Phase 4.

---

## ~~Phase 4~~ — ✅ Complete — Delete the 5 stage crates

**Order: clean all references first (4a), verify zero grep hits (4b), then delete (4c).**

### Step 4a — `canon-runtime/Cargo.toml`

Remove these 5 lines:
```toml
canon_observe = { package = "canon-observe", path = "../canon-observe" }
canon_plan    = { package = "canon-plan",    path = "../canon-plan" }
canon_act     = { package = "canon-act",     path = "../canon-act" }
canon_verify  = { package = "canon-verify",  path = "../canon-verify" }
canon_reward  = { package = "canon-reward",  path = "../canon-reward" }
```

These 5 packages are **only imported in `canon-runtime`** (confirmed by search at plan-writing
time). No other crate in the workspace depends on them.

### Step 4b — Zero-reference gate

Run:
```bash
grep -r "canon_observe\|canon_plan\b\|canon_act\b\|canon_verify\|canon_reward" \
     canon-utils/ --include="*.rs" --include="*.toml"
```
Must return zero results (excluding the crates' own directories) before Step 4c.

### Step 4c — Delete crates and workspace members

1. Remove from `/workspace/ai_sandbox/canon/Cargo.toml` members array:
   - `"canon-utils/canon-observe"`
   - `"canon-utils/canon-plan"`
   - `"canon-utils/canon-act"`
   - `"canon-utils/canon-verify"`
   - `"canon-utils/canon-reward"`

2. Delete directories:
   - `canon-utils/canon-observe/`
   - `canon-utils/canon-plan/`
   - `canon-utils/canon-act/`
   - `canon-utils/canon-verify/`
   - `canon-utils/canon-reward/`

**Verify:** `cargo check --workspace` — zero errors.

---

## ~~Phase 5~~ — ✅ Complete — Verify and smoke test

Run:
```bash
cargo check --workspace && cargo test -p canon-loop && cargo test -p canon-runtime
```

Expected: zero errors. The 4 pre-existing `cargo test --workspace` failures
(`canon-runtime-supervisor`, `canon-storage-eventlog` bins, `project_editor_tests`,
`algorithms`) are out of scope — they pre-date this migration.

---

## Execution Order

```
Phase 1 — ✅ done  (cargo check -p canon-loop)
Phase 2 — ✅ done  (cargo check -p canon-loop)
Phase 3 — ✅ done  (cargo check -p canon-runtime)
Phase 4 — ✅ done  (cargo check --workspace)
Phase 5 — ✅ migration clean (canon-loop + canon-runtime tests pass; pre-existing failures out of scope)
```

**Migration is complete. Remaining `cargo test --workspace` failures are pre-existing and
unrelated to the C-migration. They require separate investigation.**

---

## Files Created / Modified / Deleted

| Phase | Status | File                                           | Action                                                                                                |
|-------+--------+------------------------------------------------+-------------------------------------------------------------------------------------------------------|
|     1 | ✅     | `Cargo.toml` (workspace)                       | `"canon-utils/canon-loop"` member added                                                               |
|     1 | ✅     | `canon-loop/Cargo.toml`                        | Created                                                                                               |
|     1 | ✅     | `canon-loop/src/lib.rs`                        | Created                                                                                               |
|     1 | ✅     | `canon-loop/src/result.rs`                     | Created: `LoopStageResult`                                                                            |
|     1 | ✅     | `canon-loop/src/context.rs`                    | Created: `LoopContext`, ported private structs, `LoopContext::new()`                                  |
|     1 | ✅     | `canon-loop/src/stage/mod.rs`                  | Created: `LoopStageEvent` enum, `execute()`, `TryFrom<CanonEvent>`                                    |
|     2 | ✅     | `canon-loop/src/stage/observe.rs`              | Created: `execute(Tick, ctx)` — ported from canon-observe                                             |
|     2 | ✅     | `canon-loop/src/stage/plan.rs`                 | Created: `execute_trigger`, `execute_complete`, `execute_failed` — ported from canon-plan             |
|     2 | ✅     | `canon-loop/src/stage/act.rs`                  | Created: `execute_dispatch`, `execute_complete`, `execute_failed` — ported from canon-act             |
|     2 | ✅     | `canon-loop/src/stage/verify.rs`               | Created: `execute(DebugEvent, ctx)` — ported from canon-verify                                        |
|     2 | ✅     | `canon-loop/src/stage/reward.rs`               | Created: `execute(LoopVerified, ctx)`, `execute_conclude(DebugEvent, ctx)` — ported from canon-reward |
|     3 | ✅     | `canon-runtime/src/consumers/loop_executor.rs` | Created: `LoopStageExecutor`                                                                          |
|     3 | ✅     | `canon-runtime/src/consumers/mod.rs`           | `pub mod loop_executor;` added                                                                        |
|     3 | ✅     | `canon-runtime/src/bin/event_runtime.rs`       | 5 consumer instantiations replaced with `LoopStageExecutor::new`                                      |
|     3 | ✅     | `canon-runtime/Cargo.toml`                     | `canon_loop` dep added                                                                                |
|    4a | ✅     | `canon-runtime/Cargo.toml`                     | 5 stage crate deps removed                                                                            |
|    4c | ✅     | `canon-observe/`                               | Entire crate deleted                                                                                  |
|    4c | ✅     | `canon-plan/`                                  | Entire crate deleted                                                                                  |
|    4c | ✅     | `canon-act/`                                   | Entire crate deleted                                                                                  |
|    4c | ✅     | `canon-verify/`                                | Entire crate deleted                                                                                  |
|    4c | ✅     | `canon-reward/`                                | Entire crate deleted                                                                                  |
|    4c | ✅     | `Cargo.toml` (workspace)                       | 5 stage crate members removed                                                                         |

---

## Final Score

```
R_structure = 1.0  (no consumer array for loop stages, single LoopStageExecutor)
R_coverage  = 1.0  (LoopStageEvent covers all 8 trigger patterns exhaustively)
R_binding   = 1.0  (execute() has no _ arm — missing variant = compile error)

Cycles     = 0     (canon-loop new crate, no inversions in dep graph)
CoreBloat  = 0     (canon-runtime-events unchanged)
StateLeak  = bounded (LoopContext is explicit, single owner, explicit init)

System: Event → Observe → Event → Plan → Event → Act → Event → Verify → Event → Reward → Event
        Closed loop. Fully self-describing.
```
