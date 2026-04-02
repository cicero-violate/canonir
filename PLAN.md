# Plan: Fix Agent Loop Stuck on LLM Planner Timeout

## A. Context State Tracking
### Status: ACTIVE REPAIR (CANONICALIZATION)

System violates canonical law: routing and control-flow are not fully derived from SemanticStateSummary.

### Canonical Target
state → decision → transition

Where state = SemanticStateSummary is the ONLY authority.

### Root Failures (from diagnostics)
- Canonical pipeline (state → decision → transition) not executing (CRITICAL)
- LoopObserved invariant completely broken (zero emissions)
- Decision → RouteSelected linkage broken (missing 1:1 mapping)
- Dispatch occurring without routing (bypassing canonical flow)
- Observability limited to tooling events (no semantic events emitted)

### Repair Plan (STRICT ORDER)

1. ROOT CAUSE: NON-ATOMIC LOOP + STAGE FRAGMENTATION (PRIMARY)
   - decision, route, dispatch occur in different cycles (counts diverge)
   - observe stage never executes (0 events)
   - pipeline is not bound to a single atomic loop execution
   - REQUIRED: enforce single loop driver per cycle
   - REQUIRED: bind state → decision → route → dispatch → observe to same cycle ID
   - fail-fast if any stage is skipped or executed out-of-cycle
   - CRITICAL: execution currently exits or branches BEFORE observe stage
   - REQUIRED: remove all early exits between dispatch → observe
   - REQUIRED: ensure observe is structurally reachable in the main loop (not conditional)

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
