# Violations

## 1. Fail-fast enforcement still not globally consistent (CRITICAL)
- Evidence:
  - emit_tick()? exists but `let _ = runtime.emit_tick()` still present in runtime loop
  - Mixed error-handling semantics remain
- Issue:
  - Runtime can still swallow critical failures
  - Violates end-to-end fail-fast requirement
- Required fix:
  - Remove all ignored Results for critical control operations
  - Enforce global fail-fast policy across entire runtime

## 2. Zero-consumer dispatch fail-fast not proven universal (CRITICAL)
- Evidence:
  - Executor claims patch, but no verification across all dispatch entry points
- Issue:
  - Some paths may still allow silent dispatch with no consumers
- Required fix:
  - Audit all dispatch paths to ensure zero-consumer condition always returns Err
  - Add invariant assertion at dispatch boundary

## 3. End-to-end pipeline fail-fast not enforced (CRITICAL)
- Evidence:
  - Fail-fast added at emission and dispatch, but no proof for decision, routing, or loop stages
- Issue:
  - Partial failures may still propagate silently
- Required fix:
  - Ensure all stages (emit → validate → append → dispatch → route → loop) propagate Result
  - Any failure must halt pipeline execution

## 4. SemanticStateSummary routing not proven authoritative (CRITICAL)
- Evidence:
  - Executor acknowledges remaining gap
- Issue:
  - Routing may still depend on queue state or heuristics
- Required fix:
  - Enforce routing = f(SemanticStateSummary)
  - Reject any routing decisions derived from scheduler_len or queue mirrors

## 5. No guarantee of successful control progression per tick (HIGH)
- Evidence:
  - emit_tick ensures attempt, dispatch enforces consumers, but no guarantee of full pipeline completion
- Issue:
  - System may emit events without reaching decision/route/loop stages
- Required fix:
  - Add invariant: each tick must produce a completed control transition (observe→decision→route→act/verify)
  - Fail-fast if pipeline does not advance

## 6. System not fully spec-compliant
- Evidence:
  - Partial fixes applied (ordering, dispatch fail-fast)
  - Core guarantees still incomplete
- Issue:
  - System still allows inconsistent or partial execution
- Required fix:
  - Enforce invariant and fail-fast guarantees across entire control-flow
  - Ensure no escape paths exist outside canonical pipeline
