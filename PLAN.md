# Plan: Fix Agent Loop Stuck on LLM Planner Timeout

## A. Context State Tracking
### Status: ACTIVE REPAIR (CANONICALIZATION)

System is non-functional: RuntimeEvent is not emitted, so the canonical pipeline never starts.

### Canonical Target
state → decision → transition

Where state = SemanticStateSummary is the ONLY authority.

### Root Failures (from diagnostics)
- RuntimeEvent not emitted (CRITICAL, PRIMARY BLOCKER)
- Event bus receives no RuntimeEvent
- Routing never executes
- Loop never executes
- SemanticStateSummary authority never exercised

## B. Canonical Repair Plan (AUTHORITATIVE)

This section overrides prior inconsistent planning. Executors must follow ONLY this ordering.

### 0. STATE → DECISION NOT INITIATED (PRIMARY BLOCKER)
- Evidence: no runtime_started, tick, decision, route, or loop events
- System never enters canonical event-sourced control flow

REQUIRED:
1. Construct SemanticStateSummary at runtime start
2. Trigger initial decision evaluation from semantic state
3. Emit RuntimeEvent representing state → decision transition
4. Ensure runtime acts as event producer (not passive)
5. Add fail-fast if no decision occurs per tick
6. Verify emit_tick() is actually invoked in active runtime loop
7. Ensure emit_tick() events reach tlog (emit_event + drain path)

EXIT CRITERIA:
- decision events present
- runtime actor present in logs
 - tick events present

REQUIRED:
- Ensure observe → decision executes every cycle
- Ensure planner is invoked after LoopObserved
- Guarantee decision output is always produced (no None/empty)
- Add fail-fast if decision stage does not execute

EXIT CRITERIA:
- decision events present

### 1. Restore Decision → Route Link
- Ensure decision produces RouteSelected

REQUIRED:
- Verify RouteSelected emitted per decision
- Enforce 1:1 decision → route mapping
- Fail-fast if decision does not emit transition

EXIT CRITERIA:
- route events present

### 2. Enforce Canonical Pipeline
- Enforce state → decision → transition strictly

REQUIRED:
- All transitions originate from decision output
- Remove ALL bypass paths

EXIT CRITERIA:
- transitions traceable to decision

### 3. Semantic-State Authority
- Ensure routing derives ONLY from SemanticStateSummary

REQUIRED:
- Remove queue/scheduler-derived routing
- Validate deterministic semantic routing

EXIT CRITERIA:
- semantic-driven routing only

### READY WORK (EXECUTOR POOL)
1. Trace RuntimeEvent emission callsites in runtime
2. Verify emission invocation during execution
3. Add invariant: ≥1 RuntimeEvent per tick (fail-fast)
4. Trace event bus dispatch path end-to-end
5. Verify RouteExecutor receives RuntimeEvent

### Repair Plan (STRICT ORDER)

0. ROOT CAUSE: RUNTIME EVENT EMISSION FAILURE (PRIMARY BLOCKER)
   - Evidence: no RuntimeEvent in logs; only rustc/code events
   - RESULT: pipeline never enters routing

   REQUIRED (HARD):
   1. Locate RuntimeEvent emission sites in canon-runtime
   2. Verify emission is invoked during runtime execution
   3. Ensure runtime bootstrap emits ≥1 RuntimeEvent per tick
   4. Add invariant: fail-fast if no RuntimeEvent observed

   EXIT CRITERIA:
   - RuntimeEvent present in logs
   - ≥1 RuntimeEvent per tick

   DEPENDENCY:
   - blocks ALL downstream work

1. ROOT CAUSE: EVENT BUS DISPATCH FAILURE
   - Evidence: consumers exist but receive no RuntimeEvent

   REQUIRED (HARD):
   1. Audit dispatch loop
   2. Ensure RuntimeEvent enters bus
   3. Ensure RouteExecutor receives events
   4. Remove filtering / early termination
   5. Enforce invariant: RuntimeEvent reaches all non-filtered consumers

   EXIT CRITERIA:
   - RuntimeEvent observed at RouteExecutor

   DEPENDENCY:
   - requires (0)

2. RESTORE ROUTING AND LOOP ENTRY
   - RuntimeEvent → RouteExecutor → LoopStageExecutor

   REQUIRED:
   1. Verify RouteSelected events emitted
   2. Verify loop execution begins
   3. Verify decision stage executes

   EXIT CRITERIA:
   - decision, route, dispatch, observe, loop_observed present

   DEPENDENCY:
   - requires (0–1)

3. ENFORCE STATE → DECISION → TRANSITION
   - Ensure decision stage is mandatory
   - Ensure transitions originate from decision output
   - Remove dispatch → execution shortcuts

   EXIT CRITERIA:
   - All transitions traceable to decision stage

   DEPENDENCY:
   - requires (2)

4. RESTORE SEMANTIC-STATE AUTHORITY
   - Routing must derive from SemanticStateSummary
   - Remove queue/scheduler-driven routing

   EXIT CRITERIA:
   - Routing deterministic from semantic state

   DEPENDENCY:
   - requires (3)
5. ENABLE PLANNING (POST-LOOP RESTORATION)
   - ensure planner executes after observe
   - provide LLM or deterministic fallback
   - fail-fast if decision stage does not execute

   DEPENDENCY:
   - requires (2–4)
   - Evidence: 0 planning events, 48 failures 'no llm endpoint configured'
   - Pipeline halts after observe (no decision → transition)
   - RESULT: no completed loops despite LoopObserved emission

   REQUIRED (HARD):
   - configure valid LLM endpoint for planner role
   - OR implement non-LLM fallback path for planning
   - fail-fast if planning cannot execute (no silent stall)
   - ensure observe → decision transition is guaranteed
   - ensure SemanticStateSummary is passed into planning input
   - ensure decision output is always produced (no None/empty path)
   - fail-fast if decision stage does not emit RouteSelected

1. ROOT CAUSE: CONSUMER ORDERING BLOCKS LOOP (CRITICAL)
   - Evidence: route executes before loop, preventing RuntimeEvent from entering LoopStageExecutor
   - RESULT: zero loop execution (observed=0), no planning or downstream stages

   REQUIRED (HARD):
   - enforce LoopStageExecutor runs BEFORE any routing/consumer logic
   - ensure RuntimeEvent is first consumed by LoopStageExecutor
   - remove or reorder any consumer that intercepts events before loop stage
   - enforce: loop is the sole entrypoint for canonical pipeline
   - fail-fast if RuntimeEvent bypasses LoopStageExecutor

   DEPENDENCY:
   - must be resolved BEFORE observe, planning, or persistence fixes

1. ROOT CAUSE: CONTROL-FLOW NOT CANONICAL (CRITICAL)
   - EventBus introduces routing/filtering/fanout (non-canonical control)
   - executor allows noop paths that bypass LoopObserved
   - routing still depends on non-semantic state (planned_pending)

   REQUIRED (HARD):
   - eliminate ALL control-flow logic from EventBus (no routing, no filtering, no fanout)
   - enforce LoopStageExecutor as ONLY control-flow authority
   - remove ALL noop / early-exit paths that bypass LoopObserved
   - enforce: every observe MUST produce exactly one LoopObserved
   - enforce: no transition may bypass state→decision→transition chain

   DEPENDENCY:
   - must be fixed in parallel with (0); otherwise loops may run but remain non-canonical

1. ROOT CAUSE: MULTIPLE CONTROL-FLOW AUTHORITIES (PRIMARY)
   - EventBus performs routing / filtering (non-canonical control)
   - RouteExecutor performs independent execution (parallel control-flow)
   - LoopStageExecutor is not authoritative
   - routing still depends on queue-derived state (planned_pending)
   - RESULT: canonical state → decision → transition is not enforced
   - CRITICAL: tests pass but runtime does NOT execute canonical loop
   - REQUIRED: prioritize runtime execution path over test-level guarantees
   - REQUIRED: verify behavior using runtime diagnostics (observe, LoopObserved), not tests

   REQUIRED (HARD):
   - enforce SINGLE control authority: LoopStageExecutor ONLY
   - eliminate ALL control-flow logic from EventBus (no routing, no filtering)
   - convert RouteExecutor into pure transformation (no execution, no dispatch)
   - ensure ALL routing decisions derive ONLY from SemanticStateSummary
   - ensure EventRuntime is initialized BEFORE any event dispatch
   - ensure set_tlog_path(tlog_path) is called before registering consumers
   - eliminate INIT GUARD drops by fixing initialization order
   - fail-fast if any event is emitted before tlog is initialized
   - enforce: LoopObserved emission is UNCONDITIONAL for every loop cycle
   - remove any branch, early return, or transition path that skips LoopObserved
   - ensure LoopObserved is emitted AFTER observe on ALL code paths (Emit, EmitMany, errors)
   - fail-fast if a loop cycle completes without emitting LoopObserved
   - CRITICAL: prevent post-emission loss of LoopObserved (no filtering/serialization drops)
   - REQUIRED: ensure event pipeline preserves LoopObserved after emission
   - REQUIRED: remove any filtering layer that drops semantic events
   - REQUIRED: ensure serialization/deserialization retains LoopObserved
   - fail-fast if emitted LoopObserved is not present in final event log
   - ROOT CAUSE (EMITTER): EventEmitterHandle drops LoopObserved before persistence
   - REQUIRED: ensure EventEmitterHandle forwards ALL canonical events without filtering
   - REQUIRED: remove or fix any conditional logic that drops LoopObserved in emitter
   - REQUIRED: enforce invariant: emitted events MUST reach persistence layer unchanged
   - fail-fast if EventEmitterHandle drops or mutates LoopObserved
   - ROOT CAUSE (PLANNING HALT): LLM misconfiguration prevents planning stage from executing
   - REQUIRED: ensure LLM is correctly configured and callable at runtime
   - REQUIRED: fail-fast if planning step cannot execute (no silent halt)
   - REQUIRED: ensure full loop completes: state → decision → transition → event log
   - REQUIRED: verify at least one full loop cycle completes in runtime diagnostics

2. REMOVE QUEUE-DRIVEN ROUTING (BLOCKING)
   - eliminate planned_pending / scheduler_len from ALL decision paths
   - ensure decision logic reads ONLY SemanticStateSummary
   - remove any fallback or implicit routing logic

3. REBUILD CANONICAL LOOP AS SINGLE DRIVER
   - LoopStageExecutor must own: state → decision → route → dispatch → observe
   - prohibit execution of any stage outside LoopStageExecutor
   - ensure exactly one loop per RuntimeEvent

4. ELIMINATE SYNTHETIC / PARALLEL PATHS
   - remove EventBus fanout / filtering behavior
   - remove executor Noop / bypass paths
   - remove any direct dispatch or RequestDispatch-like behavior

5. RESTORE OBSERVE + LOOPOBSERVED
   - ensure observe executes unconditionally inside canonical loop
   - emit LoopObserved exactly once per loop
   - remove noop-equivalent observe paths

6. ENFORCE CANONICAL INVARIANTS (HARD GATE)
   - state → decision → route → dispatch → observe executes in ONE path
   - LoopObserved > 0 and exactly once per cycle
   - no routing derived from queue state
   - no parallel or competing control-flow paths
   - MUST be validated via runtime diagnostics (not unit tests)
   - CRITICAL: canonical loop exists but is NOT INVOKED at runtime
   - REQUIRED: identify actual runtime entrypoint and redirect it to canonical loop
   - REQUIRED: eliminate any alternate entrypoints that bypass canonical loop

2. ENFORCE REQUIRED SUCCESSOR EXECUTION
   - enforce RouteSelected → required successor (observe)
   - co-locate transition emission and successor execution
   - fail-fast if successor is not executed
   - ensure transition driver directly invokes successor (not indirect/event-based)

3. RESTORE OBSERVE REACHABILITY
   - ensure observe is structurally reachable in main loop
   - remove all early exits before observe
   - ensure observe executes in same control frame as dispatch
   - emit LoopObserved exactly once per cycle

4. ENFORCE ATOMIC LOOP
   - bind state → decision → route → dispatch → observe to same cycle
   - enforce strict 1:1:1:1 mapping across stages
   - prevent cross-cycle leakage
   - ensure entire chain executes inside single driver function/frame
   - ensure canonical loop is the ONLY execution path (no parallel control paths)
   - ensure canonical loop is the ONLY runtime execution path

5. ENFORCE SEMANTIC-STATE AUTHORITY
   - ensure all routing decisions derive ONLY from SemanticStateSummary
   - eliminate executor-driven or implicit routing paths

6. INSTRUMENT REAL EXECUTION PATH
   - log entry/exit of each stage
   - capture actual runtime call graph
   - verify fixes apply to real execution path (not assumed)

7. SUCCESS CRITERIA
   - observe > 0
   - LoopObserved > 0
   - decision == route == dispatch == observe counts
   - invariant_errors = 0
   - REQUIRED: ensure observe is structurally reachable in the main loop (not conditional)
   - NEW: prior fixes produced no runtime change → execution path not correctly targeted
   - REQUIRED: instrument loop to log entry/exit of each stage (state/decision/route/dispatch/observe)
   - REQUIRED: identify actual runtime control path taken (not assumed path)

2. ENFORCE STAGE ORDER + 1:1:1 MAPPING
   - enforce decision → RouteSelected → dispatch as strict same-cycle chain
   - require exactly one route per decision
   - require exactly one dispatch per route
   - block route if no same-cycle decision exists
   - block dispatch if no same-cycle RouteSelected exists

3. RESTORE OBSERVE AS TERMINAL STAGE
   - enforce dispatch → observe in same cycle
   - emit LoopObserved exactly once per cycle
   - fail-fast if observe is skipped
   - ensure observe is invoked unconditionally in loop body (not via optional routing only)
   - ensure no async boundary drops control before observe
   - verify via instrumentation that dispatch is followed by observe in same frame

4. ENFORCE SEMANTIC-STATE AUTHORITY
   - ensure all routing decisions derive ONLY from SemanticStateSummary
   - eliminate any executor or fallback routing logic

5. ENFORCE CORE INVARIANTS (FAIL-FAST)
   - LoopObserved missing → immediate failure
   - decision_without_route → failure
   - route_without_decision → failure
   - dispatch_without_route → failure
   - cross-cycle execution → failure
   - block route/dispatch if no same-cycle decision exists

3. RESTORE OBSERVE AS TERMINAL STAGE
   - enforce dispatch → observe as mandatory same-cycle successor
   - emit LoopObserved exactly once per cycle
   - fail-fast if observe is skipped

4. ENFORCE SEMANTIC-STATE AUTHORITY
   - ensure all decisions derive ONLY from SemanticStateSummary
   - eliminate any fallback or executor-driven routing
   - ensure routing logic is not queue-driven or implicit

5. ENFORCE CORE INVARIANTS (FAIL-FAST)
   - LoopObserved missing → immediate failure
   - decision_without_route → failure
   - route_without_decision → failure
   - dispatch_without_route → failure
   - cross-cycle stage execution → failure

6. RESTORE OBSERVABILITY
   - Ensure decision / RouteSelected / LoopObserved events are emitted
   - Ensure all events share same cycle correlation
   - Eliminate tooling-only event streams

7. SUCCESS CRITERIA (ALL REQUIRED)
   - decision == route == dispatch == observe counts (per-cycle equality)
   - LoopObserved > 0
   - invariant_errors = 0
   - no cross-cycle leakage

### Repair Plan (STRICT ORDER)

1. ABSOLUTE BLOCKER: Remove scheduler_len completely (ONLY TASK)
   - Remove scheduler_len from ConstraintState definition
   - Remove ALL computations and assignments of scheduler_len
   - Remove ALL usages in routing, policy, and decision logic
   - Run global search: "scheduler_len" → MUST return zero matches
   - HARD STOP: if ANY occurrence exists, task is incomplete
   - No further steps allowed until zero-match proof is produced
   - Require explicit proof: zero grep matches before marking this step complete
   - HARD FAILURE CONDITION: if scheduler_len exists anywhere in code, task is incomplete
   - Require diff proof: show removal of scheduler_len from struct definitions and all usages
   - HARD BLOCK: if ANY scheduler_len reference exists, STOP all other work and remove it
   - Prohibit progress to pipeline or invariant steps until zero occurrences are verified

3. Restore canonical pipeline execution
   - Ensure agent loop produces decisions from SemanticStateSummary
   - Ensure executor runs decision → route → dispatch → observe stages
   - Fail-fast if only runtime_started events are emitted
   - Require observable evidence: decision, RouteSelected, dispatch, LoopObserved events present

4. Ensure event emission across all pipeline stages
   - Instrument decision, RouteSelected, dispatch, and LoopObserved
   - Ensure each stage emits observable RuntimeEvents
   - Fail-fast on missing stage emissions
   - Add coverage requirement: all stages must appear in logs per execution cycle

5. Validate end-to-end pipeline activation
   - Confirm presence of decision, RouteSelected, dispatch, LoopObserved events
   - Ensure event bus propagates events across stages
   - Block further invariant work until pipeline is active
   - Require repeated runs with consistent event patterns (deterministic replay)

6. Enforce single canonical control-flow pipeline
   - Enforce: SemanticStateSummary → decision → RouteSelected → dispatch
   - Eliminate ALL alternative dispatch entrypoints
   - Fail-fast on any non-canonical pipeline execution

7. Restore LoopObserved exact-once emission + propagation
   - Guarantee exactly one emission per observe cycle
   - Enforce exactly one propagation into decision()
   - Fail-fast on missing or duplicate delivery

8. Enforce decision → RouteSelected invariant (STRICT)
   - Require decision_trace before ANY RouteSelected
   - Guarantee exactly one RouteSelected per decision
   - Fail-fast on missing or duplicate RouteSelected

6. Enforce decision → RouteSelected invariant (STRICT)
   - Require decision_trace before ANY RouteSelected
   - Guarantee exactly one RouteSelected per decision
   - Fail-fast on missing or duplicate RouteSelected

## Success Criteria
- no infinite plan loop
- deterministic routing after threshold
- no noop_spam
- event log consistency
- deterministic replay

---

## CANONICAL REWRITE (PIPELINE FIRST)

### PRIORITY ORDER (ROOT-CAUSAL)
1. Restore observability pipeline (PRIMARY)
   - Ensure event.tlog.d exists and is populated
   - Ensure all RuntimeEvents are persisted
   - Fail-fast if logging is broken

2. Enforce single canonical control-flow pipeline
   - Enforce: SemanticStateSummary → decision → RouteSelected → dispatch
   - Eliminate ALL alternative dispatch entrypoints
   - Fail-fast on any non-canonical pipeline execution

3. Restore LoopObserved exact-once emission + propagation
   - Guarantee exactly one emission per observe cycle
   - Enforce exactly one propagation into decision()
   - Fail-fast on missing or duplicate delivery

4. Enforce decision → RouteSelected invariant (STRICT)
   - Require decision_trace before ANY RouteSelected
   - Guarantee exactly one RouteSelected per decision
   - Fail-fast on missing or duplicate RouteSelected

5. Eliminate EventBus control-flow semantics
   - Remove multi-consumer fanout and conditional dispatch
   - Ensure single linear transport path only
   - Fail-fast on any control-flow behavior in EventBus

6. Enforce SemanticStateSummary-only routing
   - Remove RuntimeEvent-based routing conditions
   - Remove scheduler_len / planned_pending inputs
   - Fail-fast on any non-semantic routing influence

7. Prove end-to-end canonical correctness
   - Trace: state → decision → RouteSelected → dispatch → successor
   - Validate exact-once LoopObserved lifecycle
   - Require zero invariant violations in diagnostics

2. Validate observable signals
   - Confirm LoopObserved, RouteSelected, decision_trace appear in logs
   - Ensure diagnostics produce non-empty results

3. Enforce single canonical control-flow pipeline
   - Enforce: SemanticStateSummary → decision → RouteSelected → dispatch
   - Eliminate ALL alternative dispatch entrypoints
   - Fail-fast on any non-canonical pipeline execution

3. Enforce decision → RouteSelected invariant
   - Require decision_trace before RouteSelected
   - Guarantee exactly one RouteSelected per decision
   - Fail-fast on violations

4. Eliminate non-semantic routing
   - Remove scheduler_len / planned_pending / event-type routing
   - Ensure all decisions derive from SemanticStateSummary

5. Enforce invariants as fail-fast
   - Convert all invariant checks to hard failures
   - Abort execution on violation

6. Prove end-to-end correctness
   - Validate zero violations for:
     - LoopObserved exact-once
     - Decision → Route trace coverage
     - Single dispatch path
   - Require runtime proof before progression

---

## CRITICAL BLOCKER: LOGGING PIPELINE

### PRIORITY 0 (MUST FIX FIRST)
1. Restore canonical event logging pipeline
   - Recreate event.tlog.d directory structure
   - Ensure segmented log writing (not flat file)
   - Validate writer initialization and flush behavior

2. Ensure all runtime events are persisted
   - Verify LoopObserved, RouteSelected, decision traces emit to logs
   - Ensure events reach persistence layer
   - Fail-fast if logging is inactive

3. Validate diagnostics compatibility
   - Ensure log format matches diagnostics expectations
   - Confirm python analysis produces non-empty output

4. Block all invariant work until logging is restored
   - Do NOT attempt invariant fixes without observable logs
   - Require log evidence before progressing to pipeline fixes
### ENFORCEMENT RULE (NON-BYPASSABLE)
- Do NOT proceed past step 2 until scheduler_len is fully removed and verified
- Any downstream progress without this proof is invalid
- Planner will treat all subsequent work as non-compliant until this is satisfied
## ENFORCEMENT
- ALL other repair steps are blocked until scheduler_len is fully removed
- Pipeline and invariant work are invalid until zero-match proof exists
- Any downstream progress without this proof is non-compliant

## REGRESSION GUARD (MANDATORY)
- scheduler_len MUST NOT reappear after removal
- Add invariant: build fails if scheduler_len exists in any type or file
- Require repeated verification (search + file proof) after every change set
- HARD STOP: any reintroduction resets plan to step 1

## ACTIVE BLOCKER (REASSERTED)

1. scheduler_len STILL PRESENT (PRIMARY FAILURE)
   - Treat as NOT removed regardless of prior claims
   - SINGLE ENTRY ACTION (MANDATORY GROUNDING):
     * read_file PLANS/SPEC.md
     * read_file DIAGNOSTICS.md
     * read_file the ConstraintState source file
     * locate scheduler_len in actual code
   - THEN:
     * delete scheduler_len
     * remove all usages
     * repeat until zero matches
   - HARD STOP: no pipeline or invariant work allowed until verified absent

## ENFORCEMENT (RESET)
- Invalidate all downstream progress
- Executor must re-prove scheduler_len removal with direct code evidence
- Only after proof may pipeline/invariant work resume
- DO NOT allow multi-step planning; execution must begin with read_file
 - ALL actions must be grounded in fresh read_file outputs (no assumptions)

2. PIPELINE + INVARIANTS ALSO BROKEN (CO-EQUAL BLOCKER)
   - decision, routing, dispatch, observe chain is not functioning
   - Missing LoopObserved, decision/route gaps, dispatch violations
   - Treat as SECOND HARD BLOCKER (not downstream)

## ENFORCEMENT (DUAL BLOCKERS)
- BOTH must be satisfied for progress:
  1. scheduler_len = 0 (with full proof)
  2. canonical pipeline produces valid events
- HARD STOP if either condition fails
- Disallow partial completion claims
## CANONICAL MASTER PLAN (REBUILT FROM SPEC + DIAGNOSTICS)

### ROOT BLOCKERS (MUST BOTH BE FIXED)

1. scheduler_len PRESENT (STRUCTURAL VIOLATION)
   - Remove scheduler_len from ALL ConstraintState definitions
   - Remove ALL propagation paths (context, routing, constraints)
   - Enforce compile-time prohibition (fail build if present)
   - This is REQUIRED for semantic-state authority

2. CANONICAL PIPELINE NOT EXECUTING (SYSTEMIC FAILURE)
   - state → decision → RouteSelected → dispatch → observe is broken
   - No decision / RouteSelected / LoopObserved events exist
   - System currently emits only tooling events

---

### EXECUTION ORDER (STRICT)

PHASE 1 — REMOVE QUEUE-TRUTH (scheduler_len)
  - MUST complete before pipeline is trusted
  - Required outputs:
    * scheduler_len = 0 occurrences
    * removed from type system
    * removed from runtime propagation

PHASE 2 — RESTORE SEMANTIC PIPELINE
  - Re-enable state → decision → transition flow
  - Ensure:
    * decision events emitted
    * RouteSelected emitted for every decision
    * dispatch only occurs after RouteSelected
    * LoopObserved emitted exactly once per loop

PHASE 3 — ENFORCE INVARIANTS (FAIL-FAST)
  - LoopObserved missing → immediate failure
  - decision_without_route → failure
  - route_without_decision → failure
  - dispatch_without_route → failure

PHASE 4 — RESTORE OBSERVABILITY
  - Wire EventBus to semantic stages
  - Ensure decision / RouteSelected / LoopObserved visible in logs

---

### SUCCESS CRITERIA (NON-NEGOTIABLE)

- scheduler_len = 0 in entire repo
- decision_trace > 0
- RouteSelected > 0
- LoopObserved > 0
- invariant_errors = 0
- pipeline follows: state → decision → transition

---

### PLANNER DIRECTIVE

- ALWAYS prioritize SemanticStateSummary over queue-derived signals
- DO NOT introduce patches that preserve scheduler-based routing
- DO NOT proceed to pipeline fixes until scheduler_len is eliminated
- All executor work must start from real code (read_file grounded)
