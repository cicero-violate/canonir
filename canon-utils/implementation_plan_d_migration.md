# Implementation Plan: D-Migration — Routing + Control into Events (Complete Semantic Collapse)

## Current Build Status

```
Phase 1 — 🔴 not started  (canon-route crate: RouteContext, helpers, RouteDecision, decide())
Phase 2 — 🔴 not started  (RouteExecutor: EventConsumer in canon-route)
Phase 3 — 🔴 not started  (simplify event_runtime.rs)
Phase 4 — 🔴 not started  (cargo check + tests)
Phase 5 — 🔴 not started  (line count confirmed)
```

**Prerequisite:** `cargo check --workspace` clean (confirmed post C-migration 2026-03-21).

---

## Goal — Complete Semantic Collapse

Every prior migration applied the same pattern: replace imperative dispatch with
`TryFrom<CanonEvent> → execute(&mut Ctx)`. The C-migration collapsed 5 consumer structs
into `LoopStageExecutor`. The D-migration completes the collapse by making routing and
control fully event-driven. After this migration, W=1 is never blocked by routing.

```
Before (D-migration target):
  ControlMsg::Tick → handle_control_msg() → 90-second blocking LLM call in W thread
                   ↓
  update_route_runtime_state() — inline state accumulation (free function, not consumer)
                   ↓
  request_route_via_llm_call() — blocks W for up to 90 seconds
                   ↓
  drain_event_queue_with_grace() — post-LLM drain to catch tool results before decision
                   ↓
  route_controller.evaluate_model_output() → emits route_selected debug event
                   ↓
  LoopStageExecutor.on_event(route_selected) → triggers loop stage

After (complete semantic collapse):
  ControlMsg::Tick → W emits CanonEvent::Debug { kind: "route_tick" }  [3 lines]
                   ↓
  RouteExecutor.on_event(route_tick):
    - ctx already up-to-date (observes all events continuously)
    - emits CanonEvent::Llm(LlmCall { role: "router", request_id: "route-UUID" })
    - returns immediately — W is never blocked
                   ↓ [async, via CapabilityExecutor — same worker thread pool as all LLM calls]
  CapabilityExecutor handles LlmCall → worker runs LLM → emits CapabilityCompleted
                   ↓
  RouteExecutor.on_event(CapabilityCompleted { request_id: "route-UUID" }):
    - runs route_controller.evaluate_model_output()
    - emits CanonEvent::Debug { kind: "route_selected", payload: { ... } }
                   ↓
  LoopStageExecutor.on_event(route_selected) → triggers loop stage [unchanged]
```

**What is eliminated entirely:**
- `update_route_runtime_state()` free function (replaced by `RouteContext::update_from_event()` inside `RouteExecutor`)
- `apply_observed_events()` (route state is now maintained continuously by `RouteExecutor`)
- `drain_event_queue_with_grace()` (W is never blocked — no post-LLM drain needed)
- `handle_event_msg()` route_state parameter
- `RouteRuntimeState` struct in event_runtime.rs
- `route_state` and `route_controller` variables in `main()`
- All routing logic from `handle_control_msg()` (147 lines → 5 lines)
- The pre-control grace drain loop in main()

**No schema changes required.** All events used are existing `CanonEvent` variants:
- `Debug { source: "supervisor", kind: "route_tick" }` — already emittable
- `Llm(LlmCall { ... })` — already in schema, handled by CapabilityExecutor
- `CapabilityCompleted { ... }` / `CapabilityFailed { ... }` — already in schema
- `Debug { source: "supervisor", kind: "route_selected", payload }` — already emitted today

---

## Architecture After D-Migration

```
event_runtime.rs — ~750 lines (was 1358)
  W=1 main loop — never blocked
  handle_control_msg: emits route_tick, returns Ok(false) [5 lines]
  handle_event_msg: no route_state parameter [simplified]
  main(): no route_state, no route_controller, no drain calls

canon-route/ — new crate
  context.rs    — RouteContext, update_from_event(), signals(), snapshot_text(), push_journal()
  helpers.rs    — heuristic_route_json, request_route_via_llm_call (used by executor),
                  count_loc, extract_loc_requirement, evaluate_goal_satisfied, DirectEventEmitter
  decision.rs   — RouteDecision, decide() [used inside executor for gatekeeper eval]
  executor.rs   — RouteExecutor: EventConsumer
                  on_event(route_tick)         → emit Llm(LlmCall { role: "router" })
                  on_event(CapabilityCompleted) → run gatekeeper → emit route_selected
                  on_event(CapabilityFailed)   → use heuristic  → emit route_selected
                  on_event(loop/tool events)   → ctx.update_from_event()
```

---

## What Moves to `canon-route`

### Block 1: `RouteRuntimeState` → `RouteContext`
**Source:** `event_runtime.rs` lines 127–187

All fields move verbatim. Rename only.

```rust
// canon-route/src/context.rs
use canon_decision::JournalLine;
use canon_goal::GoalSpec;
use std::collections::HashSet;

#[derive(Default)]
pub struct RouteContext {
    pub scheduler_tick: u64,
    pub mission_raw: String,
    pub mission_summary: String,
    pub mission_goal_spec: Option<GoalSpec>,
    pub context_ready: bool,
    pub workspace_dirty: bool,
    pub planned_pending: usize,
    pub acted_unverified: bool,
    pub last_action_failed: bool,
    pub pending_tool_result_ids: HashSet<String>,
    pub latest_tool_result: Option<serde_json::Value>,
    pub finish_ready: bool,
    pub last_action_kind: String,
    pub journal: Vec<JournalLine>,
}
```

Port the three `impl RouteRuntimeState` methods verbatim as `impl RouteContext`:
- `signals(&self) -> RuntimeSignals` — no change
- `snapshot_text(&self) -> String` — no change
- `push_journal(&mut self, lane, summary)` — no change

Add `pub fn new() -> Self { Self::default() }`.

### Block 2: `update_route_runtime_state()` → `RouteContext::update_from_event()`
**Source:** `event_runtime.rs` lines 317–413

Port verbatim as a method. Signature change only:

```rust
// Before (free function):
fn update_route_runtime_state(route_state: &mut RouteRuntimeState, event: &CanonEvent, workspace: &Path)

// After (method):
impl RouteContext {
    pub fn update_from_event(&mut self, event: &CanonEvent, workspace: &Path)
}
```

Body identical: `route_state.push_journal(...)` → `self.push_journal(...)`,
`route_state.X = ...` → `self.X = ...`.

Requires `canon_goal::{parse_agent_goal_markdown, summarize_goal}` and
`evaluate_goal_satisfied()` (also in this crate).

### Block 3: Helper functions
**Source:** `event_runtime.rs` lines 189–315

Move verbatim into `canon-route/src/helpers.rs`:

```rust
pub fn heuristic_route_json(ctx: &RouteContext) -> String
    // param rename: state: &RouteRuntimeState → ctx: &RouteContext — no other change

pub fn request_route_via_llm_call(
    workspace: &Path, prompt: String, timeout: Duration,
    _last_tool_result: Option<serde_json::Value>,
) -> Result<String>
    // unchanged — kept for use by RouteExecutor when a synchronous call is needed
    // (fallback path only — normal path is fully async via event dispatch)

fn count_loc(dir: &Path) -> usize                           // private
fn extract_loc_requirement(spec: &GoalSpec) -> usize        // private
pub fn evaluate_goal_satisfied(spec: Option<&GoalSpec>, workspace: &Path) -> bool

struct DirectEventEmitter { tx: crossbeam_channel::Sender<CanonEvent> }
impl canon_event::EventEmitter for DirectEventEmitter { ... }
```

### Block 4: `RouteDecision` + `decide()` — internal to executor
**Source:** routing logic inside `handle_control_msg()` lines 556–607

```rust
// canon-route/src/decision.rs
use canon_decision::RouteKind;
use canon_runtime_supervisor::judgment_loop::RouteController;
use crate::{RouteContext, helpers::{heuristic_route_json}};

pub struct RouteDecision {
    pub lane: RouteKind,
    pub suggested_route: RouteKind,
    pub rationale: String,
    pub confidence: Option<f32>,
    pub changed: bool,
    pub note: String,
    pub gate_rules_fired: Vec<String>,
    pub should_stop: bool,
    pub prompt: String,
}

/// Evaluate model JSON output through gatekeeper. Pure decision logic —
/// does NOT do any I/O, does NOT block, does NOT emit events.
/// Called by RouteExecutor after the async LLM result arrives.
pub fn decide_from_json(
    ctx: &RouteContext,
    model_json: &str,
    prompt: String,
    controller: &mut RouteController,
) -> Result<RouteDecision>
```

Body extracted from `handle_control_msg()` lines 556–607:
```rust
pub fn decide_from_json(ctx: &RouteContext, model_json: &str, prompt: String, controller: &mut RouteController) -> Result<RouteDecision> {
    let signals = ctx.signals();
    let (selection, gate) = match controller.evaluate_model_output(model_json, &signals) {
        Ok(v) => v,
        Err(_) => {
            let fallback_json = heuristic_route_json(ctx);
            controller.evaluate_model_output(&fallback_json, &signals)
                .map_err(|e| anyhow::anyhow!("routing gatekeeper failed: {e}"))?
        }
    };
    let gate_rules_fired = gate.note
        .split("; ")
        .filter(|s| !s.is_empty() && *s != "accepted")
        .map(String::from)
        .collect();
    Ok(RouteDecision {
        lane: gate.lane,
        suggested_route: selection.route,
        rationale: selection.rationale,
        confidence: selection.confidence,
        changed: gate.changed,
        note: gate.note.clone(),
        gate_rules_fired,
        should_stop: gate.should_stop,
        prompt,
    })
}
```

### Block 5: Unit tests
**Source:** `event_runtime.rs` lines 1265–1358

Move into `canon-route/src/context.rs` tests module:
- `route_state_transitions_after_loop_events` → update `RouteRuntimeState` → `RouteContext`,
  `update_route_runtime_state(...)` → `ctx.update_from_event(...)`
- `journal_is_bounded_to_32_lines` → update type name only

---

## Phase 1 — Create `canon-route` crate

### Step 1a — Add to workspace

In `/workspace/ai_sandbox/canon/Cargo.toml`, add `"canon-utils/canon-route"` to `members`.

### Step 1b — `canon-utils/canon-route/Cargo.toml`

```toml
[package]
name = "canon-route"
version = "0.1.0"
edition = "2021"

[lib]
name = "canon_route"
path = "src/lib.rs"

[dependencies]
anyhow.workspace = true
serde_json.workspace = true
uuid.workspace = true
crossbeam-channel.workspace = true
canon_event      = { package = "canon-runtime-events",   path = "../canon-runtime-events" }
canon-meta       = { path = "../canon-meta" }
canon_decision   = { package = "canon-decision",         path = "../canon-decision" }
canon_judgment   = { package = "canon-judgment",         path = "../canon-judgment" }
canon_goal       = { package = "canon-goal",             path = "../canon-goal" }
canon_exec       = { package = "canon-exec",             path = "../canon-exec" }
canon_runtime_supervisor = { package = "canon-runtime-supervisor", path = "../canon-runtime-supervisor" }
```

### Step 1c — `canon-route/src/lib.rs`

```rust
pub mod context;
pub mod decision;
pub mod executor;
pub mod helpers;

pub use context::RouteContext;
pub use decision::{decide_from_json, RouteDecision};
pub use executor::RouteExecutor;
pub use helpers::{evaluate_goal_satisfied, heuristic_route_json};
```

### Step 1d — `canon-route/src/context.rs`

Port Block 1 and Block 2:
- `RouteContext` struct (renamed from `RouteRuntimeState`)
- `impl RouteContext`: `new()`, `signals()`, `snapshot_text()`, `push_journal()`, `update_from_event()`
- Unit tests (Block 5) in `#[cfg(test)] mod tests { ... }` at bottom

### Step 1e — `canon-route/src/helpers.rs`

Port Block 3:
- `heuristic_route_json(ctx: &RouteContext) -> String`
- `request_route_via_llm_call(workspace, prompt, timeout, _last_tool_result) -> Result<String>`
- `count_loc`, `extract_loc_requirement` (private)
- `evaluate_goal_satisfied` (pub)
- `DirectEventEmitter` struct + `EventEmitter` impl

### Step 1f — `canon-route/src/decision.rs`

Port Block 4:
- `RouteDecision` struct (pub fields)
- `pub fn decide_from_json(ctx, model_json, prompt, controller) -> Result<RouteDecision>`

**Checkpoint:** `cargo check -p canon-route` exits 0 before Phase 2.

---

## Phase 2 — `RouteExecutor` EventConsumer

Create `canon-route/src/executor.rs`.

`RouteExecutor` implements `EventConsumer`. It is the routing analogue of `LoopStageExecutor`:
- Continuously accumulates routing signals from loop/tool events into `RouteContext`
- Responds to `Debug { kind: "route_tick" }` by dispatching an async LLM call
- Responds to `CapabilityCompleted` / `CapabilityFailed` for its pending routing request by evaluating the gatekeeper and emitting `route_selected`
- Never blocks — returns from `on_event()` immediately

```rust
// canon-route/src/executor.rs
use canon_decision::RouteKind;
use canon_event::{CanonEvent, CapabilityResult, EventConsumer, EventEmitterHandle, EventFilter, LlmCall};
use canon_judgment::GuardConfig;
use canon_runtime_supervisor::judgment_loop::RouteController;
use std::path::PathBuf;
use uuid::Uuid;
use crate::{
    context::RouteContext,
    decision::{decide_from_json, RouteDecision},
    helpers::heuristic_route_json,
};

pub struct RouteExecutor {
    ctx: RouteContext,
    workspace: PathBuf,
    controller: RouteController,
    emitter: Option<EventEmitterHandle>,
    /// request_id of the in-flight routing LLM call; None when idle.
    pending_request_id: Option<String>,
    /// Prompt used to start the in-flight call (stored to include in route_selected payload).
    pending_prompt: Option<String>,
}

impl RouteExecutor {
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            ctx: RouteContext::new(),
            workspace,
            controller: RouteController::new(GuardConfig::default()),
            emitter: None,
            pending_request_id: None,
            pending_prompt: None,
        }
    }
}
```

### `set_emitter()`:
```rust
fn set_emitter(&mut self, emitter: EventEmitterHandle) {
    self.emitter = Some(emitter);
}
```

### `filter()`:
```rust
fn filter(&self) -> EventFilter {
    EventFilter::All
}
```

### `on_event()` — three arms:

**Arm 1 — state accumulation (all loop/tool events):**
```rust
fn on_event(&mut self, event: &CanonEvent) {
    // Always update routing context from any loop/tool event.
    self.ctx.update_from_event(event, &self.workspace);

    // --- Routing tick: start async LLM call ---
    if let CanonEvent::Debug(d) = event {
        if d.source == "supervisor" && d.kind == "route_tick" {
```

**Arm 2 — route_tick: start async LLM call:**

When `route_tick` arrives:
1. If `pending_request_id.is_some()` → already routing, skip (emit `route_busy` debug for visibility)
2. If `ctx.pending_tool_result_ids` is non-empty → emit `route_blocked_waiting_tool_result`, skip
3. Increment `ctx.scheduler_tick`
4. Emit `Debug { kind: "signals_snapshot", payload: { tick, context_ready, ... } }` (same fields as today)
5. Build prompt: `controller.build_prompt(&ctx.mission_summary, &ctx.snapshot_text(), ctx.latest_tool_result.as_ref(), &ctx.journal)`
6. Generate request_id: `format!("route-{}", Uuid::new_v4())`
7. Emit `CanonEvent::Llm(LlmCall { request_id: request_id.clone(), prompt: prompt.clone(), role: Some("router".to_string()) })`
8. Set `self.pending_request_id = Some(request_id)`, `self.pending_prompt = Some(prompt)`
9. Return — W is unblocked

```rust
            if self.pending_request_id.is_some() {
                if let Some(emitter) = &self.emitter {
                    emitter.emit(CanonEvent::debug("supervisor", "route_busy",
                        serde_json::json!({ "tick": self.ctx.scheduler_tick, "reason": "llm_call_in_flight" })));
                }
                return;
            }
            if !self.ctx.pending_tool_result_ids.is_empty() {
                if let Some(emitter) = &self.emitter {
                    emitter.emit(CanonEvent::debug("supervisor", "route_blocked_waiting_tool_result",
                        serde_json::json!({
                            "tick": self.ctx.scheduler_tick,
                            "pending_tool_call_ids": self.ctx.pending_tool_result_ids.iter().cloned().collect::<Vec<_>>(),
                            "planned_pending": self.ctx.planned_pending,
                            "acted_unverified": self.ctx.acted_unverified,
                            "last_action_kind": self.ctx.last_action_kind,
                        })));
                }
                return;
            }
            self.ctx.scheduler_tick = self.ctx.scheduler_tick.saturating_add(1);
            if let Some(emitter) = &self.emitter {
                emitter.emit(CanonEvent::debug("supervisor", "signals_snapshot",
                    serde_json::json!({
                        "tick": self.ctx.scheduler_tick,
                        "context_ready": self.ctx.context_ready,
                        "planned_pending": self.ctx.planned_pending,
                        "has_queued_plan": self.ctx.planned_pending > 0,
                        "acted_unverified": self.ctx.acted_unverified,
                        "last_action_kind": self.ctx.last_action_kind,
                        "last_action_failed": self.ctx.last_action_failed,
                        "workspace_dirty": self.ctx.workspace_dirty,
                        "finish_ready": self.ctx.finish_ready,
                        "ltr_present": self.ctx.latest_tool_result.is_some(),
                        "pending_tool_count": self.ctx.pending_tool_result_ids.len(),
                    })));
            }
            let snapshot = self.ctx.snapshot_text();
            let prompt = self.controller.build_prompt(
                &self.ctx.mission_summary, &snapshot,
                self.ctx.latest_tool_result.as_ref(), &self.ctx.journal);
            let request_id = format!("route-{}", Uuid::new_v4());
            if let Some(emitter) = &self.emitter {
                emitter.emit(CanonEvent::Llm(LlmCall {
                    request_id: request_id.clone(),
                    prompt: prompt.clone(),
                    role: Some("router".to_string()),
                }));
            }
            self.pending_request_id = Some(request_id);
            self.pending_prompt = Some(prompt);
        }
        return;
    }
```

**Arm 3 — CapabilityCompleted for routing LLM: run gatekeeper → emit route_selected:**
```rust
    if let CanonEvent::CapabilityCompleted(done) = event {
        let pending = match &self.pending_request_id {
            Some(id) if *id == done.request_id && done.capability == "llm.call" => id.clone(),
            _ => return,
        };
        let prompt = self.pending_prompt.take().unwrap_or_default();
        self.pending_request_id = None;

        let model_json = match &done.result {
            CapabilityResult::Llm(res) => {
                res.response.get("text").and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| res.response.to_string())
            }
            CapabilityResult::Process(p) => p.stdout.clone(),
            CapabilityResult::Empty => String::new(),
        };

        let decision = match decide_from_json(&self.ctx, &model_json, prompt, &mut self.controller) {
            Ok(d) => d,
            Err(err) => {
                if let Some(emitter) = &self.emitter {
                    emitter.emit(CanonEvent::debug("supervisor", "route_error",
                        serde_json::json!({ "tick": self.ctx.scheduler_tick, "error": err.to_string(), "fallback": "failed" })));
                }
                return;
            }
        };

        self.emit_route_selected(&decision);
    }

    if let CanonEvent::CapabilityFailed(failed) = event {
        if self.pending_request_id.as_deref() != Some(&failed.request_id) || failed.capability != "llm.call" {
            return;
        }
        let prompt = self.pending_prompt.take().unwrap_or_default();
        self.pending_request_id = None;
        // Use heuristic fallback
        let fallback_json = heuristic_route_json(&self.ctx);
        let decision = match decide_from_json(&self.ctx, &fallback_json, prompt, &mut self.controller) {
            Ok(d) => d,
            Err(_) => return,
        };
        self.emit_route_selected(&decision);
    }
}
```

**Helper `emit_route_selected()`:**
```rust
fn emit_route_selected(&self, decision: &RouteDecision) {
    let Some(emitter) = &self.emitter else { return; };
    emitter.emit(CanonEvent::debug("supervisor", "route_selected",
        serde_json::json!({
            "tick": self.ctx.scheduler_tick,
            "suggested_route": decision.suggested_route.as_str(),
            "approved_route": decision.lane.as_str(),
            "rationale": decision.rationale,
            "confidence": decision.confidence,
            "changed": decision.changed,
            "note": decision.note,
            "gate_rules_fired": decision.gate_rules_fired,
            "ltr_present": self.ctx.latest_tool_result.is_some(),
            "last_action_kind": self.ctx.last_action_kind,
            "last_action_failed": self.ctx.last_action_failed,
            "last_action_success": !self.ctx.last_action_failed,
            "prompt": decision.prompt,
            "should_stop": decision.should_stop,
        })));
    // Note: should_stop is now handled by LoopStageExecutor reading the route_selected event.
    // The loop concludes naturally when the loop stage sends a stop signal.
    // If RouteKind::Scan: the existing LoopStageExecutor conclude stage handles halt.
}
```

**NOTE on `should_stop`:** In the current design, `handle_control_msg()` returns `Ok(true)` (exit signal to `main()`) when `gate.should_stop`. After D-migration, `route_selected` carries `should_stop: true` in its payload. `LoopStageExecutor` already handles `route_selected` with `lane == "conclude"` — the halt path flows through `LoopRewarded { halt: true }` naturally. Verify this in `stage/reward.rs` and `stage/verify.rs`. If a hard stop signal from the supervisor is still needed, `RouteExecutor` can emit a `CanonEvent::LoopRewarded { halt: true }` when `should_stop == true`.

**NOTE on `CanonEvent::debug()`:** Verify whether this constructor exists in canon-runtime-events. If not, use the existing construction pattern:
```rust
CanonEvent::Debug(canon_event::Debug {
    source: "supervisor".to_string(),
    kind: "route_selected".to_string(),
    payload: serde_json::json!({ ... }),
})
```
Check the event schema and use whatever constructor is available.

**NOTE on mission init:** `event_runtime.rs` lines 920–924 currently initialize `route_state.mission_raw/summary/goal_spec` from the AGENT_GOAL.md file at startup. `RouteExecutor::new()` should do the same:
```rust
pub fn new(workspace: PathBuf) -> Self {
    let mut ctx = RouteContext::new();
    ctx.mission_raw = std::fs::read_to_string("/workspace/ai_sandbox/canon/canon-agent-prompts/AGENT_GOAL.md")
        .unwrap_or_default();
    let initial_spec = canon_goal::parse_agent_goal_markdown(&ctx.mission_raw);
    ctx.mission_summary = canon_goal::summarize_goal(&initial_spec);
    ctx.mission_goal_spec = Some(initial_spec);
    Self { ctx, workspace, controller: RouteController::new(GuardConfig::default()),
           emitter: None, pending_request_id: None, pending_prompt: None }
}
```

**Checkpoint:** `cargo check -p canon-route` exits 0 before Phase 3.

---

## Phase 3 — Simplify `event_runtime.rs`

### Step 3a — Add canon-route dependency

In `canon-utils/canon-runtime/Cargo.toml`, add:
```toml
canon_route = { package = "canon-route", path = "../canon-route" }
```

### Step 3b — Add import

At top of `event_runtime.rs`, add:
```rust
use canon_route::RouteExecutor;
```

### Step 3c — Add `RouteExecutor` to consumers list

In `main()`, where consumers are built (lines 693–700), add `RouteExecutor`:
```rust
let mut consumers: Vec<Box<dyn canon_event::EventConsumer>> = vec![
    Box::new(LoopStageExecutor::new(workspace.clone(), tlog_path.clone())),
    Box::new(RouteExecutor::new(workspace.clone())),   // <-- ADD
    Box::new(ErrorLogger::new(None)),
    Box::new(CheckConsumer::new()),
];
```

### Step 3d — Remove moved definitions from `event_runtime.rs`

Delete these blocks entirely:
- `struct RouteRuntimeState { ... }` (lines 127–143)
- `impl RouteRuntimeState { signals, snapshot_text, push_journal }` (lines 155–187)
- `fn heuristic_route_json(...)` (lines 189–216)
- `fn request_route_via_llm_call(...)` (lines 218–258)
- `fn count_loc(...)` (lines 260–276)
- `fn extract_loc_requirement(...)` (lines 278–289)
- `fn evaluate_goal_satisfied(...)` (lines 291–315)
- `fn update_route_runtime_state(...)` (lines 317–413)
- `fn apply_observed_events(...)` (lines 415–424)
- Both test functions in `#[cfg(test)] mod tests { ... }` (lines 1265–1358)

### Step 3e — Remove `route_state` from `handle_event_msg()`

Before:
```rust
fn handle_event_msg(
    msg: EventMsg, runtime: &mut EventRuntime, route_state: &mut RouteRuntimeState, workspace: &Path,
    processed: &mut usize, cursor_path: &Path, tlog_path: &Path, start_seq: u64, session_id: &str,
    last_saved: &mut Instant, last_saved_processed: &mut usize,
) -> Result<()>
```

After — remove `route_state: &mut RouteRuntimeState` and `workspace: &Path` from signature.
Remove the `apply_observed_events(runtime, route_state, workspace)?;` calls inside.

### Step 3f — Replace `handle_control_msg()` (147 lines → 5 lines)

Delete the entire function body. New implementation:
```rust
fn handle_control_msg(msg: ControlMsg, runtime: &mut EventRuntime) -> Result<bool> {
    match msg {
        ControlMsg::Tick => {
            runtime.emit_debug_event(
                "supervisor".to_string(),
                "route_tick".to_string(),
                serde_json::json!({ "ts": now_ms() }),
            )?;
            runtime.flush_emitted_events()?;
        }
    }
    Ok(false)
}
```

**NOTE on return value `Ok(true)` (exit signal):** The current `handle_control_msg()` returns
`Ok(true)` when `gate.should_stop`. After D-migration, `RouteExecutor` emits a `route_selected`
with `should_stop: true`. The main loop must observe this and exit.

Two options:
1. `RouteExecutor` also emits `CanonEvent::LoopRewarded { halt: true }` when `should_stop == true` — `LoopStageExecutor` already handles halt (sets `ctx.halted = true`) and propagates naturally
2. Add a new `CanonEvent::RuntimeStop {}` variant (requires schema change)

**Use Option 1**: when `decide_from_json` returns `should_stop == true`, `RouteExecutor` emits both `route_selected` AND `CanonEvent::LoopRewarded { halt: true, ... }`. The existing halt path in `LoopStageExecutor` handles the rest. No schema change needed.

Verify in `canon-loop/src/stage/reward.rs` that `LoopRewarded { halt: true }` triggers process exit or sends the final cursor save. If not, verify the halt propagation path and document the actual mechanism.

### Step 3g — Delete `drain_event_queue_with_grace()`

Delete the entire function (lines 453–476). W is never blocked — no post-LLM drain is needed.

### Step 3h — Simplify the W=1 main loop

The main loop currently has three sites that call `drain_event_queue_with_grace()` before
each `handle_control_msg()` call, and one additional call inside `handle_control_msg()` itself.

After removing `drain_event_queue_with_grace()`, the three call sites in the main loop
become direct calls to `handle_control_msg(control_msg, &mut runtime)` with no drain.

Also remove the `let pre_control_grace_ms` / `let pre_control_grace` lines.

Update all `handle_control_msg(...)` call sites to use the new 2-parameter signature.
Update all `handle_event_msg(...)` call sites to remove `route_state` and `workspace` params.

### Step 3i — Remove `route_state` and `route_controller` from `main()`

Remove these lines (lines 919–925):
```rust
let mut route_controller = RouteController::new(GuardConfig::default());
let mut route_state = RouteRuntimeState::default();
route_state.mission_raw = std::fs::read_to_string(...).unwrap_or_default();
let initial_spec = parse_agent_goal_markdown(&route_state.mission_raw);
route_state.mission_summary = summarize_goal(&initial_spec);
route_state.mission_goal_spec = Some(initial_spec);
apply_observed_events(&mut runtime, &mut route_state, &workspace)?;
```

`RouteExecutor::new()` handles mission init internally.

### Step 3j — Remove now-unused imports

After the removals, grep for unused imports in event_runtime.rs and remove them:
- `use canon_decision::{JournalLine, RouteKind}` — if no longer referenced
- `use canon_goal::{parse_agent_goal_markdown, summarize_goal, GoalSpec}` — if no longer referenced
- `use canon_judgment::{GuardConfig, RuntimeSignals}` — if no longer referenced
- `use canon_runtime_supervisor::judgment_loop::RouteController` — moved to canon-route
- Any struct destructuring imports for `LoopObserved, LoopActed, LoopPlanned, LoopRewarded, LoopVerified, ToolCall, ToolResult` — now only needed in canon-route/context.rs

**Verify:** `cargo check -p canon-runtime` exits 0 before Phase 4.

---

## Phase 4 — Verify and test

```bash
cargo check --workspace
cargo test -p canon-route
cargo test -p canon-runtime
```

Expected:
- `cargo check --workspace` — zero errors
- `cargo test -p canon-route` — 2 ported tests pass:
  - `route_state_transitions_after_loop_events`
  - `journal_is_bounded_to_32_lines`
- `cargo test -p canon-runtime` — passes (including `async_consumers_preserve_order_per_consumer`)

---

## Phase 5 — Confirm line reduction

```bash
wc -l canon-utils/canon-runtime/src/bin/event_runtime.rs
```

Expected: ≤ 750 lines (down from 1358). Reduction of ~600 lines.

Breakdown of what remains:
| Block | Lines (approx) |
|-------|----------------|
| LockGuard + pid helpers | ~65 |
| EventMsg, ControlMsg, is_kernel_canon_event | ~30 |
| handle_event_msg (simplified, no route_state) | ~25 |
| handle_control_msg (emit route_tick, return) | ~10 |
| main() — startup, queue setup, producers P1–P4 | ~300 |
| Cursor/session helpers | ~130 |
| tlog equivalence verification | ~30 |
| **Total** | **~590** |

---

## Execution Order

```
Phase 1 — 🔴 next   (cargo check -p canon-route exits 0)
Phase 2 — 🔴        (RouteExecutor compiles; cargo check -p canon-route exits 0)
Phase 3 — 🔴        (cargo check -p canon-runtime exits 0)
Phase 4 — 🔴        (cargo check --workspace exits 0; 2 tests pass in canon-route)
Phase 5 — 🔴 last   (line count ≤ 750 confirmed)
```

---

## Files Created / Modified / Deleted

| Phase | Status | File | Action |
|-------|--------|------|--------|
| 1 | 🔴 | `Cargo.toml` (workspace) | Add `"canon-utils/canon-route"` member |
| 1 | 🔴 | `canon-route/Cargo.toml` | Create |
| 1 | 🔴 | `canon-route/src/lib.rs` | Create |
| 1 | 🔴 | `canon-route/src/context.rs` | Create: `RouteContext`, `update_from_event()`, 2 unit tests |
| 1 | 🔴 | `canon-route/src/helpers.rs` | Create: helpers + `DirectEventEmitter` |
| 1 | 🔴 | `canon-route/src/decision.rs` | Create: `RouteDecision`, `decide_from_json()` |
| 2 | 🔴 | `canon-route/src/executor.rs` | Create: `RouteExecutor` EventConsumer |
| 3 | 🔴 | `canon-runtime/Cargo.toml` | Add `canon_route` dep |
| 3 | 🔴 | `canon-runtime/src/bin/event_runtime.rs` | Remove ~600 lines; add `RouteExecutor` to consumers |

---

## Semantic Collapse Completeness

After D-migration, the system is fully event-driven end-to-end:

```
S = min(E, C, J, R)
  E — event schema:        all signals in CanonEvent variants ✅
  C — consumer collapse:   5 loop stages → LoopStageExecutor ✅ (C-migration)
                           routing supervisor → RouteExecutor ✅ (D-migration)
  J — judgment:            RouteController inside RouteExecutor (no inline gating in W) ✅
  R — routing quality:     R_structure · R_coverage · R_binding all compiler-enforced ✅

W=1 main loop responsibilities after D-migration:
  - Receive events from Q_e, append to L, dispatch to consumers
  - Receive ControlMsg::Tick, emit route_tick into event bus
  - Manage producers P1–P4
  - Cursor save (crash recovery)
  NO routing logic. NO state accumulation. NO blocking I/O.
```

Every decision, every capability, every loop stage transition flows through
`TryFrom<CanonEvent> → execute(&mut Ctx)`. The compiler enforces exhaustiveness.
The tlog is the single source of truth for all decisions — routing decisions are now
events in the log, not transient state in W.
