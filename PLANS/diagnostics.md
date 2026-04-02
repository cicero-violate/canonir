# Diagnostics Report

## Inputs Scanned
- state/event_log/event.tlog.d (latest)
- canon-loop (executor.rs, observe.rs)
- canon-runtime (bus.rs)
- canon-route (policy.rs)
- verifier summaries

## Ranked Failures

### 1. Impact: HIGH
Signal: LoopObserved exact-once invariant still violated (unproven and contradicted by logs)
Evidence:
- 24,258 missing LoopObserved
- only 4 LoopObserved emitted
- explicit duplicate detected (1)
- no improvement across cycles

Interpretation:
- System fails both completeness (missing) and uniqueness (duplicate)
- No confirmed single authoritative emission or propagation path
- EventBus changes insufficient to guarantee end-to-end invariant

Repair Targets:
- canon-loop/src/observe.rs: define single authoritative emission point
- canon-loop/src/executor.rs: eliminate all alternate or retry emission paths
- introduce loop_id-scoped invariant: exactly one LoopObserved per loop
- enforce idempotency at emission boundary (not downstream)
- add fail-fast: duplicate OR missing LoopObserved aborts execution

---

### 2. Impact: HIGH
Signal: Decision → Route invariant still broken
Evidence:
- 5,167 missing decision traces
- 5,163 missing route traces
- no reduction across runs

Repair Targets:
- canon-route: enforce decision_trace as prerequisite for RouteSelected
- require 1:1 mapping between decision and route
- block RouteSelected without decision linkage

---

### 3. Impact: HIGH
Signal: EventBus / dispatch layer still non-canonical
Evidence:
- 3,879 synthetic dispatch signals
- dispatch occurs outside RouteSelected path

Repair Targets:
- canon-runtime/src/bus.rs: verify removal of all fanout/filter paths
- enforce strict linear flow: RouteSelected → dispatch only
- add invariant: dispatch without RouteSelected is illegal

---

### 4. Impact: HIGH
Signal: Invariants not enforced (system continues under violation)
Evidence:
- 7,418 invariant errors

Repair Targets:
- canon-invariant: convert all invariants to fail-fast
- abort execution immediately on violation

---

### 5. Impact: MEDIUM
Signal: Residual queue-driven routing signals persist
Evidence:
- 16 queue-driven routing signals still present in logs

Repair Targets:
- audit for indirect routing dependencies (scheduler_len, derived mirrors)
- confirm SemanticStateSummary is sole runtime authority

---

### 6. Impact: MEDIUM
Signal: Successor lifecycle gaps
Evidence:
- 2,188 successor discharge gaps

Repair Targets:
- enforce exactly-once successor discharge
- validate lifecycle at transition boundary

---

## Planner Handoff
1. Enforce LoopObserved exact-once invariant at emission source (primary blocker)
2. Enforce decision → route invariant strictly
3. Eliminate all non-canonical dispatch paths
4. Convert invariants to fail-fast
5. Remove residual queue-driven routing signals
6. Fix successor lifecycle

## Notes
- No evidence that recent changes improved system correctness
- Exact-once LoopObserved remains unproven and contradicted by logs
- Core issue: lack of single authoritative control-flow path from observe → route → dispatch

