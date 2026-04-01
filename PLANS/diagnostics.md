# Diagnostics Report

## Inputs Scanned
- event log segments in state/event_log/event.tlog.d (3 files, ~152MB total)
- python structured scan (latest):
  - loop_acted_no_tool=19
  - act_with_empty_scheduler=9
  - missing_decision_traces=12852
  - missing_route_traces=12850
  - plan_zero_tasks=0
  - missing_plan_error=0
- canon-utils source (invariant, route, loop, runtime-supervisor)

## Ranked Failures

### 1. Impact: HIGH
Signal: LoopActed emitted without tool_result (critical invariant violation)
Evidence:
- event logs: loop_acted_no_tool = 19 (severe and persistent)
Repair Targets:
- canon-loop/src/stage/act.rs
  - enforce invariant: LoopActed ⇒ tool_result_id.is_some()
  - block all emission paths lacking tool_result_id
  - audit all paths (success, error, retry, fallback)

### 2. Impact: HIGH
Signal: Missing DECIDE + ROUTE trace emission (observability failure)
Evidence:
- missing_decision_traces = 12852
- missing_route_traces = 12850
Repair Targets:
- canon-invariant/src/lib.rs
  - emit DECIDE trace with trace_id and structured payload
- canon-route/src/executor.rs
  - emit ROUTE trace with Decision → Route mapping
- global
  - enforce invariant: every route_selected must include DECIDE + ROUTE traces

### 3. Impact: HIGH
Signal: Act executed with empty scheduler
Evidence:
- act_with_empty_scheduler = 9
Repair Targets:
- canon-invariant/src/lib.rs
  - enforce scheduler_len == 0 ⇒ Decision::Observe
- canon-route/src/executor.rs
  - prevent Act mapping when scheduler empty
- canon-loop/src/context.rs
  - validate scheduler_len correctness

### 4. Impact: HIGH
Signal: Decision logic not centralized
Evidence:
- ConstraintRoute still present
- routing logic distributed across modules
Repair Targets:
- canon-invariant/src/lib.rs
  - make decide(...) sole authority
  - eliminate ConstraintRoute from decision path
- canon-route/src/executor.rs
  - remove all routing branches
- canon-route/src/policy.rs
  - reduce to mapping-only or remove

### 5. Impact: MEDIUM
Signal: Plan stage safeguards incomplete
Evidence:
- missing assertions and PLAN_ERROR enforcement
Repair Targets:
- canon-loop/src/stage/plan.rs
  - enforce ≥1 task OR explicit failure
  - emit PLAN_ERROR and assert on invalid outputs

### 6. Impact: MEDIUM
Signal: Dispatch deduplication not strict
Evidence:
- duplicated / weakened dedup logic (from verifier context)
Repair Targets:
- executor / dispatch layer
  - enforce strict deduplication
  - remove duplicated logic paths

### 7. Impact: MEDIUM
Signal: ConstraintState not minimal
Evidence:
- includes non-decision fields
Repair Targets:
- canon-invariant/src/lib.rs
  - reduce to scheduler_len + has_plan only

## Planner Handoff

Highest-value repair targets:
1. Fix LoopActed invariant immediately
2. Implement DECIDE + ROUTE trace emission with trace_id
3. Enforce scheduler_len == 0 ⇒ Observe
4. Centralize decision logic in decide(...)
5. Enforce Plan stage guarantees

Blockers / Gaps:
- Severe invariant violations (LoopActed)
- Observability incomplete (missing traces)
- Decision logic fragmented
- Plan safeguards missing
