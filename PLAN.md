# PLAN: Restore Canonical Event-Sourced Control Through Semantic-State Authority

## A. Authoritative Context

### Current State
- Spec authority: `SemanticStateSummary` is the single source of truth for routing and control-flow correctness.
- Observed failure: runtime is not participating in the canonical event system.
- Log evidence: only `rustc` actor activity is present; no `runtime_started`, `tick`, `decision`, `route`, `observe`, or `LoopObserved` events are recorded.

### Canonical Control Law
semantic state -> judgment/decision -> lawful transition -> event log

### Planning Rule
- Prioritize root-cause restoration of canonical runtime participation and semantic-state-driven decision entry.
- Keep queue-truth cleanup behind semantic-state authority work.
- Do not spend ready-window capacity on downstream loop symptoms before upstream event emission and decision routing exist.

## B. Ranked Root Failures

### 0. Runtime not participating in the event system (PRIMARY BLOCKER)
Evidence:
- diagnostics: actors = {"rustc": ...} only
- violations: no runtime actor, no runtime_started, no tick events
- result: canonical event-sourced control never starts

Required repair:
1. Audit runtime bootstrap entrypoint and active runtime loop startup.
2. Ensure runtime actor registration occurs before control work begins.
3. Emit `runtime_started` exactly once per process.
4. Restore or implement the tick driver so runtime emits recurring tick events.
5. Verify emitter wiring from runtime into event bus and tlog append path.
6. Add fail-fast when process boots without runtime actor participation or canonical startup events.

Exit criteria:
- runtime actor appears in log
- `runtime_started` appears once
- recurring tick events appear

### 1. Semantic state is not being turned into decisions
Evidence:
- diagnostics: no decision events
- diagnostics: `SemanticStateSummary` never evaluated
- violations: state -> decision chain absent

Required repair:
1. Construct `SemanticStateSummary` at startup and on each tick.
2. Invoke decision evaluation from semantic state every tick.
3. Emit canonical decision/control event from semantic-state evaluation.
4. Add fail-fast if a tick completes without a decision event.
5. Ensure decision output is never replaced by queue-local truth such as `scheduler_len` or `planned_pending`.

Exit criteria:
- decision events present
- each decision is traceable to semantic-state evaluation
- no tick completes without decision output

### 2. Decision -> route transition not established under semantic-state authority
Evidence:
- violations: no route events
- diagnostics: routing blocked upstream by absent decision stage
- spec: routing must derive from semantic truth, not scheduler-first orchestration

Required repair:
1. Restore decision -> `RouteSelected` emission.
2. Enforce one lawful route transition per decision when required by policy.
3. Make route derivation depend on `SemanticStateSummary` and policy/invariants only.
4. Move `scheduler_len`, `planned_pending`, and similar counters out of root-truth routing roles.
5. Add fail-fast when decision emits no lawful transition.

Exit criteria:
- route events present
- route selection is semantic-state-derived
- queue-local counters are not route authority

### 3. Route -> loop entry is blocked behind missing upstream control
Evidence:
- diagnostics: no observe or `LoopObserved`
- violations: canonical loop fully inactive

Required repair:
1. Verify route consumer/subscription path after decision -> route is restored.
2. Restore lawful `RouteSelected(observe)` -> `LoopObserved` entry.
3. Add fail-fast when routed control is not consumed by the loop stage.

Exit criteria:
- observe events present
- `LoopObserved` present

### 4. Downstream stages are blocked, not primary
Evidence:
- no plan/act/verify because loop never begins

Required repair:
1. Only after loop entry exists, restore plan/act/verify/reward sequencing.
2. Keep this work blocked until Sections 0-3 exit.

Exit criteria:
- downstream canonical stages appear in order

### 5. Queue-truth cleanup is follow-on work, not the current root-cause fix
Evidence:
- spec explicitly rejects scheduler-first orchestration and executor-local routing
- diagnostics show upstream absence of runtime and decision events is the current blocker

Required repair:
1. After semantic-state routing exists, remove residual queue/counter truth from control decisions.
2. Demote local suppression patches that preserve queue-truth.

Exit criteria:
- control truth is semantic-state/policy/invariant driven only

## C. Dependency Order

1. Runtime bootstrap into event system
2. Semantic-state construction and per-tick decision emission
3. Decision -> route under semantic-state authority
4. Route -> loop entry
5. Downstream plan/act/verify recovery
6. Queue-truth cleanup and residual bypass removal

## D. READY NOW

### Executor: executor_pool
1. Audit and repair runtime bootstrap so the runtime becomes an event producer, registers a runtime actor, emits `runtime_started`, and drives recurring tick events.
2. Trace and repair runtime emitter wiring end-to-end so runtime events reach both the event bus and the tlog, with fail-fast when startup events are absent.
3. Wire `SemanticStateSummary` construction plus per-tick decision evaluation so each tick emits canonical decision output from semantic state.
4. Restore decision -> `RouteSelected` emission under semantic-state authority and demote `scheduler_len` / `planned_pending` / similar counters out of route-truth roles.
5. Only after route events exist, verify route consumer delivery into observe / `LoopObserved`; keep downstream loop-stage work blocked until this precondition is met.

## E. Blocked Until Upstream Exit Criteria Hold

- Any loop-stage patching that assumes observe is already firing
- Any planner timeout tuning that assumes the canonical loop is active
- Any local scheduler or queue suppression patch that preserves queue-truth as routing authority
- Any downstream plan/act/verify sequencing work before runtime, decision, and route events exist
