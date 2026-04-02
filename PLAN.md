# Plan: Fix Agent Loop Stuck on LLM Planner Timeout

## A. Context State Tracking
### Status: ACTIVE REPAIR (CANONICALIZATION)

System violates canonical law: routing and control-flow are not fully derived from SemanticStateSummary.

### Canonical Target
state → decision → transition

Where state = SemanticStateSummary is the ONLY authority.

### Root Failures (from diagnostics)
- LoopObserved missing (severe under-emission)
- Decision → RouteSelected linkage broken
- Synthetic dispatch paths present
- EventBus mutates control-flow
- Residual queue-driven routing exists

### Repair Plan (STRICT ORDER)

1. Fix LoopObserved emission + propagation (PRIMARY BLOCKER)
   - Remove noop / fallback observe paths
   - Guarantee exactly one LoopObserved per observe execution
   - Enforce exactly one propagation into decision()
   - Fail-fast on missing or duplicate delivery

2. Enforce decision → RouteSelected invariant (STRICT)
   - Require decision_trace before ANY RouteSelected
   - Guarantee exactly one RouteSelected per decision
   - Fail-fast on missing or duplicate RouteSelected

3. Eliminate synthetic dispatch paths
   - Remove RequestDispatch and implicit dispatch triggers
   - Ensure RouteSelected is sole dispatch entrypoint
   - Fail-fast on any non-canonical dispatch path

4. Neutralize EventBus (transport-only)
   - Remove fanout, filtering, replay, or conditional dispatch
   - Ensure single linear delivery path only
   - Fail-fast on multi-consumer control behavior

5. Establish SemanticStateSummary as sole routing authority (AFTER stabilization)
   - Remove ALL routing inputs derived from scheduler_len / planned_pending
   - Ensure policy decisions read ONLY SemanticStateSummary
   - Fail-fast on any non-semantic routing signal

6. Prove linear control-flow end-to-end
   - Trace: SemanticStateSummary → decision → RouteSelected → dispatch → successor
   - Validate exact-once observe lifecycle
   - Fail-fast on duplication, re-entry, or alternate paths

## Success Criteria
- no infinite plan loop
- deterministic routing after threshold
- no noop_spam
- event log consistency
- deterministic replay

---

## CANONICAL REWRITE (PIPELINE FIRST)

### PRIORITY ORDER (ROOT-CAUSAL)
1. Enforce single canonical control-flow pipeline (PRIMARY)
   - Enforce: SemanticStateSummary → decision → RouteSelected → dispatch
   - Eliminate ALL alternative dispatch entrypoints
   - Fail-fast on any non-canonical pipeline execution

2. Restore LoopObserved exact-once emission + propagation
   - Guarantee exactly one emission per observe cycle
   - Enforce exactly one propagation into decision()
   - Fail-fast on missing or duplicate delivery

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
