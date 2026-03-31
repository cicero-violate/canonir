# Diagnostics Report

## Inputs Scanned
- event log segments in state/event_log/event.tlog.d (3 files, ~145MB total)
- python structured scan (latest): loop_acted_no_tool=3, act_with_empty_scheduler=0
- canon-utils source (invariant, route, loop, runtime-supervisor)
- commands: python log scan, rg ConstraintRoute usage

## Ranked Failures

### 1. Impact: HIGH
Signal: Decision logic not centralized (architectural violation)
Evidence:
- canon-invariant/src/lib.rs defines both ConstraintRoute and Decision
- ConstraintRoute still widely used across modules
- executor and policy layers still contain routing logic
- Decision is not the sole authority for routing
Repair Targets:
- canon-invariant/src/lib.rs
  - make decide(...) return Decision ONLY
  - remove or isolate ConstraintRoute behind mapping layer
- canon-route/src/executor.rs
  - remove ALL local routing logic
  - consume Decision exclusively
- canon-route/src/policy.rs
  - eliminate all branching
  - reduce to mapping or delete
- global
  - eliminate all ConstraintRoute-based decisions

### 2. Impact: HIGH
Signal: LoopActed emitted without tool_result
Evidence:
- event logs: loop_acted_no_tool = 3
Repair Targets:
- canon-loop/src/stage/act.rs
  - enforce invariant: LoopActed ⇒ tool_result_id.is_some()
  - block emission in ALL paths
  - audit all emit sites

### 3. Impact: MEDIUM
Signal: Historical Act-with-empty-scheduler issue appears resolved but not guaranteed
Evidence:
- latest logs: act_with_empty_scheduler = 0
- earlier runs showed violations → indicates fragility
Repair Targets:
- canon-invariant/src/lib.rs
  - enforce scheduler_len == 0 ⇒ Decision::Observe
- canon-route/src/executor.rs
  - ensure no fallback paths produce Act
- validation
  - add regression checks

### 4. Impact: MEDIUM
Signal: ConstraintState not minimal for decision
Evidence:
- ConstraintState includes many unrelated fields
- decision should depend only on scheduler_len and has_plan
Repair Targets:
- canon-invariant/src/lib.rs
  - reduce ConstraintState to minimal decision fields
  - split diagnostic fields elsewhere

### 5. Impact: MEDIUM
Signal: Distributed decision logic persists across system
Evidence:
- supervisor and tests still interact with ConstraintRoute
- partial Decision usage only (not universal)
Repair Targets:
- canon-runtime-supervisor/src/*
  - ensure no override of Decision
- canon-loop/src/stage/*
  - ensure all stages consume Decision only

## Planner Handoff

Highest-value repair targets:
1. Make Decision the single source of truth for routing
2. Remove ConstraintRoute as a decision mechanism
3. Eliminate all distributed routing logic (executor, policy, supervisor)
4. Enforce LoopActed ⇒ tool_result_id invariant strictly
5. Minimize ConstraintState to decision-relevant fields

Blockers / Gaps:
- ConstraintRoute still deeply embedded
- Decision not universally enforced
- No proof from logs that all decisions originate from decide(...)
