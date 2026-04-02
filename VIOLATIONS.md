# Violations

## 0. scheduler_len removal — RESOLVED
- Evidence:
  - Diagnostics confirm removal complete
  - ConstraintState contains no scheduler_len field
- Outcome:
  - Queue-driven routing eliminated

## 1. Canonical loop not executing atomically (CRITICAL)
- Evidence:
  - decision=30, route=107, dispatch=74, observe=0
  - route >> decision, dispatch >> decision
- Issue:
  - state → decision → route → dispatch → observe not executed as a single atomic cycle
- Required fix:
  - enforce single loop driver
  - guarantee full ordered execution per cycle

## 2. Observe stage not executing (CRITICAL)
- Evidence:
  - observe=0 across all logs
  - no LoopObserved events
  - executor claim contradicted by diagnostics
- Issue:
  - observe stage unreachable in runtime
- Required fix:
  - guarantee observe executes after dispatch
  - emit exactly-once LoopObserved per cycle
  - fail-fast if skipped

## 3. Pipeline fragmentation / cross-cycle leakage
- Evidence:
  - route (107) >> decision (30)
  - dispatch (74) >> decision (30)
- Issue:
  - stages executed independently or reuse stale outputs
- Required fix:
  - bind decision → route → dispatch to single cycle ID
  - enforce strict 1:1:1 mapping

## 4. Dispatch not gated by routing
- Evidence:
  - dispatch significantly exceeds decision count
- Issue:
  - dispatch bypasses canonical routing chain
- Required fix:
  - require same-cycle RouteSelected before dispatch
  - block dispatch without fresh route

## 5. Invariants not enforced
- Evidence:
  - invariant_errors = 2123 and increasing
- Issue:
  - violations logged but not enforced
- Required fix:
  - convert invariants to fail-fast
  - abort execution on violation

## 6. System not spec-compliant
- Evidence:
  - missing observe stage
  - non-atomic pipeline
  - LoopObserved invariant not satisfied
- Issue:
  - canonical control-flow (state → decision → transition) not fulfilled
- Required fix:
  - achieve full atomic loop execution with exact invariants
