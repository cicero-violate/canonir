# Diagnostics Report

## Inputs Scanned
- state/event_log/event.tlog.d (548 segment files)
- latest stage-signal analysis
- VIOLATIONS.md
- PLANS/SPEC.md

## Ranked Failures

### 1. Impact: CRITICAL
Signal: Canonical loop is non-atomic and never completes
Evidence:
- decision=30, route=107, dispatch=87, observe=0
- No LoopObserved events
- Increasing divergence (route >> decision, dispatch >> decision)

Root Cause:
- Runtime executes partial pipeline fragments without enforcing full-cycle completion
- Execution exits or diverges before observe stage

Repair Targets:
- canon-runtime: enforce atomic loop execution (state → decision → route → dispatch → observe)
- remove all early exits prior to observe
- enforce single loop driver controlling all stages
- add invariant: every cycle must end with LoopObserved

---

### 2. Impact: CRITICAL
Signal: Observe stage never executes
Evidence:
- observe=0 across all logs

Root Cause:
- observe stage unreachable in current execution path

Repair Targets:
- canon-loop::observe: guarantee execution after dispatch
- emit exactly-once LoopObserved per cycle
- fail-fast if observe not executed

---

### 3. Impact: HIGH
Signal: Pipeline fragmentation and cross-cycle leakage
Evidence:
- route (107) >> decision (30)
- dispatch (87) >> decision (30)

Root Cause:
- stages invoked independently or reuse prior-cycle artifacts

Repair Targets:
- bind decision → route → dispatch to same cycle ID
- enforce strict 1:1:1 mapping per cycle
- prevent reuse of stale outputs

---

### 4. Impact: HIGH
Signal: Dispatch not strictly gated by routing
Evidence:
- dispatch significantly exceeds decision count

Root Cause:
- dispatch bypasses canonical decision→route chain

Repair Targets:
- require dispatch to reference same-cycle RouteSelected
- block dispatch without fresh routing

---

### 5. Impact: HIGH
Signal: Invariant violations accelerating
Evidence:
- invariant_errors = 2275 (increasing)

Root Cause:
- invariants are logged but not enforced

Repair Targets:
- convert invariants to fail-fast assertions
- abort execution immediately on violation

---

## Planner Handoff
1. Enforce atomic canonical loop execution (no partial execution)
2. Guarantee observe stage execution (LoopObserved exactly-once)
3. Bind decision→route→dispatch to same cycle
4. Eliminate dispatch_without_route paths
5. Convert invariants to fail-fast

## Notes
- scheduler_len removal confirmed complete
- primary failure remains loop fragmentation and non-completion
- system degradation continues as invariant violations accumulate

