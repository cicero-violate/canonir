# Diagnostics Report

## Inputs Scanned
- Event logs: `state/event_log/event.tlog.d`
- Violations: `VIOLATIONS.md`
- Source:
  - `canon-utils/canon-route/src/executor.rs`
  - `canon-utils/canon-loop/src/stage/plan.rs`
  - canon-utils grep: `pending_plan`, `pending_act`, `Noop`

## Ranked Failures

### 1. Impact: CRITICAL
Signal: **Total control-loop collapse (not just routing failure)**

Evidence:
- `try_dispatch_route` returns early (executor.rs line ~72)
- No `decision()` invocation
- No `RouteSelected` emission
- Event log global scan:
  - RouteTick = 0
  - RouteSelected = 0
  - decision_trace = 0
- Full control event absence:
  - LoopObserved = 0
  - PlanningCompleted = 0
  - LoopActed = 0
  - LoopRewarded = 0
  - LoopVerified = 0

Conclusion:
- The system is not “misrouting” — it is **not executing a control loop at all**
- No canonical pipeline exists at runtime:
  - ❌ Tick → RouteTick
  - ❌ RouteTick → decision
  - ❌ decision → RouteSelected
  - ❌ RouteSelected → successor

Additional Evidence:
- Runtime is actively executing non-control work:
  - rustc_capture_started = 178
  - rustc_capture_completed = 105
  - rustc_capture_failed = 73
- This proves:
  - Execution layer is alive
  - Event logging is functioning
  - BUT control-plane events are completely absent

Implication:
- System is split into two disconnected subsystems:
  1. Execution pipeline (working)
  2. Control pipeline (never activated)
- Routing is not "broken" — it is **never entered anywhere in runtime**

Root Cause:
- Routing entrypoint is disabled
- No control events are emitted anywhere in runtime
- Entire semantic loop is structurally disconnected

Deeper Root Cause (Confirmed):
- Tick and RouteTick emitters DO exist in code:
  - `canon-runtime/src/lib.rs`: emits Tick per cycle
  - `canon-loop/src/executor.rs`: emits RouteTick on Tick
- However, event log shows ZERO occurrences of both
- Therefore runtime loop responsible for emitting Tick is not executing
- Routing pipeline is never triggered at runtime

System Failure Classification:
- This is a **runtime loop execution failure**, not just a routing bug
- Control-plane exists in code but is never activated at runtime

Repair Targets:
- `canon-utils/canon-route/src/executor.rs::try_dispatch_route`
- Introduce **RouteTick emission source (missing entirely)**
- Enforce canonical chain:
  - Tick → RouteTick → decision(SemanticStateSummary) → RouteSelected
- Add invariant: at least one control event per cycle

---

### 2. Impact: CRITICAL
Signal: Infinite retry loops with identical failures
Evidence:
- Repeated log segments (same size + identical payload)
- Long runs detected (up to 28 identical logs)
- Sample logs show identical `rustc_capture_failed`
Root Cause:
- No semantic progression between retries
- Control loop re-executes identical failing work
- Routing absence prevents state evolution
Repair Targets:
- Encode failure outcomes into semantic state
- Prevent identical re-execution without state change
- Add invariant: repeated identical failure must trigger alternate routing

---

### 3. Impact: CRITICAL
Signal: Queue-local state still gates control flow via Noop
Evidence:
- plan.rs lines 176–179: request_id mismatch → Noop
- plan.rs lines 215–218: empty actions → Noop
- widespread Noop returns across plan/act/stage modules
- grep confirms extensive Noop usage
Root Cause:
- Control flow depends on `pending_plan` / `pending_act`
- Noop suppresses semantic progression
- Violates SemanticStateSummary authority
Repair Targets:
- `canon-utils/canon-loop/src/stage/plan.rs`
- `canon-utils/canon-loop/src/stage/act.rs`
- `canon-utils/canon-loop/src/stage/mod.rs`
- Replace Noop with semantic events
- Remove queue-local gating from control flow
- Ensure every branch emits an event

---

### 4. Impact: CRITICAL
Signal: Structural invariant failure blocks forward progress
Evidence:
- Repeated error:
  "malformed/private helper path segment in Canon path interner"
- Appears across hundreds of logs
- No recovery observed
Root Cause:
- canon_rustc structural validation failure treated as retryable
- Failure not incorporated into routing decisions
Repair Targets:
- `canon-rustc/src/capture/pipeline/validate/structural.rs`
- Emit semantic error events for invariant violations
- Add routing response to structural failure
- Prevent identical retry without transformation

---

### 5. Impact: HIGH
Signal: Missing per-cycle control guarantees
Evidence:
- No cycle_id tracking
- No enforcement of:
  - Tick → RouteTick → decision → RouteSelected
Root Cause:
- Loop correctness cannot be validated
- Duplicate or missing decisions possible
Repair Targets:
- Add cycle_id
- Enforce exactly-one decision per cycle
- Validate RouteSelected per cycle

---

### 6. Impact: HIGH
Signal: Executor still partially controls routing lifecycle
Evidence:
- executor retains dispatch flags and reroute logic
- semantic routing not isolated
Root Cause:
- Incomplete migration to semantic-only control
Repair Targets:
- Remove executor-driven routing remnants
- Centralize routing in semantic pipeline

## Planner Handoff

### Highest Priority
1. Restore routing pipeline
   - Implement decision(SemanticStateSummary)
   - Emit RouteSelected

2. Eliminate Noop control paths
   - Replace with semantic events
   - Remove pending_* gating

3. Break retry loops
   - Encode failure into semantic state
   - Prevent identical re-execution

4. Handle rustc invariant failures
   - Convert to semantic events
   - Add recovery routing

5. Add cycle invariants
   - Enforce exactly-one decision + RouteSelected

### Blockers
- SemanticStateSummary construction path not fully visible
- decision() implementation unclear
- canon_rustc failure handling may require upstream changes

2. Impact: high
   Signal: Decision authority is still implicit and under-specified rather than explicitly wired from `SemanticStateSummary`.
   Evidence:
   - `canon-utils/canon-route/src/decision.rs:19-27`
     - `decide_from_json(ctx: &RouteContext, _model_json: &str, prompt: String, _controller: &mut RouteController)`
     - decision derives from `ctx.semantic_summary.validation_blocked_by_preconditions` and `ctx.semantic_summary.compiler_repair_required`
   - `canon-utils/canon-route/src/executor.rs:848`
     - `let mut decision = decide_from_json(&self.ctx, "", prompt.clone(), &mut self.controller)`
   - `emit_decision` ignores `_model_json` name-wise and does not pass an explicit `SemanticStateSummary` value into the decision interface.
   - `prompt` and controller state still remain side inputs to decision construction.
   Repair Targets:
   - `canon-utils/canon-route/src/decision.rs`
   - `canon-utils/canon-route/src/executor.rs::emit_decision`
   - Replace `decide_from_json(...)` with an explicit semantic entrypoint, e.g. `decide_from_semantic_state(summary: &SemanticStateSummary, ...)`.
   - Make the decision input contract explicit and minimal.

3. Impact: high
   Signal: Residual non-semantic routing surfaces still exist and preserve alternate authority paths.
   Evidence:
   - `canon-utils/canon-runtime-supervisor/src/judgment_loop.rs:28-36`
     - `evaluate_model_output(&mut self, model_json: &str, signals: &RuntimeSignals)`
     - `parse_route_selection(model_json, ...)`
     - no visible `SemanticStateSummary` input
   - `canon-utils/canon-route/src/helpers.rs:40-91`
     - `request_route_via_llm_call(...)`
     - issues `llm.call`
     - waits on `RuntimeEvent::CapabilityCompleted` / `RuntimeEvent::CapabilityFailed`
     - no visible `SemanticStateSummary` input
   Repair Targets:
   - `canon-utils/canon-runtime-supervisor/src/judgment_loop.rs::evaluate_model_output`
   - `canon-utils/canon-route/src/helpers.rs::request_route_via_llm_call`
   - Remove, hard-disable, or strictly demote these paths so they cannot act as routing authority.

4. Impact: high
   Signal: The runtime still does not exhibit a healthy semantic control loop in the canonical event log.
   Evidence:
   - recent canonical event-log scan: `tick=1194`, `decision=3`, `route_selected_per_tick=0.0`
   - uploaded runtime trace shows `LoopObserved` handling, then route executor panic, then fatal-halt/append-guard churn rather than stable control progression.
   - uploaded runtime trace also shows `LoopObserved` appends rejected for missing parent IDs after fatal halt, indicating runtime progress is breaking before stable replayable control state is established.
   Repair Targets:
   - `canon-utils/canon-route/src/executor.rs`
   - `canon-utils/canon-runtime/src/lib.rs`
   - `canon-utils/canon-runtime/src/bin/event_runtime.rs`
   - After fixing the panic, re-verify `RouteTick -> decision -> RouteSelected -> successor` in the canonical tlog.

5. Impact: medium
   Signal: EventBus/runtime no-drop/no-mutation guarantees remain unverified under real runtime stress.
   Evidence:
   - verifier summary leaves `event bus and hooks preserve strict no-drop/no-mutation guarantees at runtime` unverified
   - uploaded runtime trace shows startup debug rejection for `missing_parent_ids` and `LoopObserved` append blocking under fatal halt / causal-chain violation.
   Repair Targets:
   - `canon-utils/canon-runtime/src/lib.rs`
   - `canon-utils/canon-runtime/src/bin/event_runtime.rs`
   - `canon-utils/canon-runtime/src/bus.rs`
   - ensure non-root emitted events always carry valid parent chains and are not silently dropped or mutated

6. Impact: medium
   Signal: Queue/local mirror influence is no longer the strongest proven blocker, but it is still not fully ruled out.
   Evidence:
   - focused executable windows no longer directly prove a live `scheduler_len == 0` routing condition
   - verifier summary still treats routing authority as undefined rather than fully semantic
   - route/context/policy code still requires a broader executable audit after the panic and authority path are fixed
   Repair Targets:
   - `canon-utils/canon-route/src/context.rs`
   - `canon-utils/canon-route/src/policy.rs`
   - `canon-utils/canon-route/src/executor.rs`
   - complete post-panic audit for residual queue-derived routing conditions

## Planner Handoff
- ordered highest-value repair targets
  1. `canon-utils/canon-route/src/executor.rs::emit_decision`
     - assign/persist `last_decision_trace_id` before `emit_route_selected_from_decision`
     - verify no later clear/reset races exist before route emission
  2. `canon-utils/canon-route/src/executor.rs::emit_route_selected_from_decision`
     - preserve strict invariant, but only after trace creation is guaranteed
     - add defensive diagnostics if trace production fails
  3. `canon-utils/canon-route/src/decision.rs`
     - replace `decide_from_json(...)` with explicit `SemanticStateSummary` input contract
  4. `canon-utils/canon-route/src/executor.rs`
     - rewire `emit_decision` to call the new semantic decision interface directly
  5. `canon-utils/canon-runtime-supervisor/src/judgment_loop.rs` and `canon-utils/canon-route/src/helpers.rs`
     - remove or demote residual JSON/LLM route-authority helpers
  6. `canon-utils/canon-runtime/src/lib.rs` and `canon-utils/canon-runtime/src/bin/event_runtime.rs`
     - audit parent-id and fatal-halt behavior once route panic is removed

- blockers or missing evidence
  - current runtime verification is blocked by the `decision_trace` panic in `RouteExecutor`
  - recent event-log scans show decisions are still too sparse to confirm a healthy semantic loop
  - queue-derived routing influence remains secondary/unverified until the immediate panic and explicit semantic input wiring are repaired
## Routing Stall: No RouteTick / RouteSelected Emitted

### Symptoms
- route_tick: 0
- route_selected: 0
- loop_observed: 0
- system makes no forward progress

### Root Cause
RouteExecutor depends on RouteTick to trigger decision() → RouteSelected, but no bootstrap RouteTick is emitted.

### Impact
Routing loop never starts → no observe/plan/act/verify progression.

### Required Fix
Emit a bootstrap RouteTick after event processing when not dispatching.

### Notes
- Must be fixed in RouteExecutor (not diagnostics)
- Avoid recursion when dispatch_in_progress is true
## 🔬 CONFIRMED DROP POINT

RouteTick is dropped in append_runtime_event when invariant_engine.observe() returns false.

Location:
canon-runtime/src/lib.rs (append_runtime_event)

Behavior:
- observe() rejects RouteTick
- early return prevents persistence

Impact:
- RouteTick never reaches tlog
- RouteExecutor never triggers
- Routing pipeline is dead

Fix:
- Do not allow invariant_engine to reject RouteTick
