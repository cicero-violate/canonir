# Violations

## 8. Executor-driven routing still present (CRITICAL)
- Evidence:
  - RouteExecutor::try_dispatch_route directly calls emit_decision()
  - Routing not strictly initiated from SemanticStateSummary → decision → RouteSelected pipeline
- Issue:
  - Violates SPEC semantic authority and "no hidden routing paths"
- Required fix:
  - Ensure all routing originates exclusively from semantic-state-derived decision()
  - Remove any executor-triggered decision shortcuts

## 9. Missing per-cycle validation instrumentation (CRITICAL)
- Evidence:
  - No cycle_id tracking or enforcement of Tick → RouteTick → Decision → RouteSelected
- Issue:
  - Violates Objective 3 and 4
- Required fix:
  - Add explicit cycle tracking and assertions for exactly-one decision and RouteSelected per cycle

## 10. EventBus completeness not enforced (CRITICAL)
- Evidence:
  - No proof that all emitted events reach all consumers
- Issue:
  - Violates Objective 1
- Required fix:
  - Add delivery accounting and hard-fail on missing delivery

## 11. Hook mutation/suppression not prevented (CRITICAL)
- Evidence:
  - No enforcement layer ensuring hooks preserve event identity
- Issue:
  - Violates Objective 2
- Required fix:
  - Add before/after equality checks and reject mutation/suppression

## 12. Determinism not validated (HIGH)
- Evidence:
  - No replay or equivalence testing for identical SemanticStateSummary
- Issue:
  - Violates Objective 5
- Required fix:
  - Add deterministic replay checks

## 13. Async propagation not verified (HIGH)
- Evidence:
  - No trace ensuring async events influence future decisions
- Issue:
  - Violates Objective 6
- Required fix:
  - Add tracing from async emission → decision impact

## Summary
- Queue-local control reductions are partially complete
- Core semantic routing invariants still not fully enforced
- Runtime validation objectives remain largely unimplemented
- System is NOT yet compliant with SPEC.md
## 1. EventBus delivery not enforced (CRITICAL)
- Evidence:
  - Dispatch now records receipts and emits `dispatch_delivery_gap` and `dispatch_consumer_lock_failed`
  - Lock failures and delivery gaps are observable but do not halt or reject execution
- Issue:
  - System detects incomplete delivery but still allows it
  - Violates OBJECTIVES.md Objective 1 (must reach all consumers, not just report failures)
- Required fix:
  - Treat delivery gaps and lock failures as invariant violations
  - Halt or reject event when delivery is incomplete

## 2. Hook mutation/suppression not enforced (CRITICAL)
- Evidence:
  - Hook decisions (Mutate/Deny) emit audit/debug/error events
  - Protected control events trigger error events but are not blocked
- Issue:
  - Hooks can still influence control flow indirectly
  - Violates Objective 2 (must NOT mutate or suppress)
- Required fix:
  - Reject or block mutation/deny for protected control events
  - Enforce equality of event before/after hooks

## 3. Replay suppression still introduces non-semantic control paths (HIGH)
- Evidence:
  - Replay emits audit events for suppression (`replay_suppressed_*`)
- Issue:
  - Suppression still exists as a conditional branch outside SemanticStateSummary
  - Violates SPEC requirement for semantic-state-driven control
- Required fix:
  - Encode replay suppression decisions into semantic state or event log
  - Eliminate hidden conditional replay paths

## 4. Per-cycle control flow guarantees not implemented (HIGH)
- Evidence:
  - No cycle tracking for Tick → RouteTick → Decision → RouteSelected
- Issue:
  - Cannot verify loop correctness
- Required fix:
  - Add cycle_id tracking and assertions

## 5. Exactly-one decision per cycle not enforced (HIGH)
- Evidence:
  - No decision counters or validation
- Issue:
  - Duplicate or missing decisions possible
- Required fix:
  - Enforce 1 decision per cycle invariant

## 6. Deterministic decision behavior not validated (MEDIUM)
- Evidence:
  - No runtime replay validation using identical SemanticStateSummary
- Issue:
  - Determinism is assumed, not proven
- Required fix:
  - Add deterministic replay tests or runtime checks

## 7. Async event propagation not verified (MEDIUM)
- Evidence:
  - No validation that async events affect subsequent decisions
- Issue:
  - Async events may be lost or ignored
- Required fix:
  - Trace async events through loop and decision impact

## Summary
- Structural correctness improvements exist
- Runtime correctness and OBJECTIVES.md validation remain unimplemented
- System is not yet proven compliant with SPEC.md

## 8. Routing not derived from SemanticStateSummary (CRITICAL)
- Evidence:
  - canon-route/src/executor.rs uses `emit_decision("", String::new())` with no semantic input
  - Comment explicitly states: "decision wiring will be added after full integration"
  - No visible construction of DecisionState from SemanticStateSummary in executor
- Issue:
  - Violates SPEC.md semantic authority requirement
  - Routing is not provably derived from SemanticStateSummary
  - Placeholder / incomplete decision path means control truth is not semantic
- Required fix:
  - Construct DecisionState directly from SemanticStateSummary
  - Pass semantic state into canonical decide()
  - Remove placeholder decision calls with empty inputs
  - Add tests proving identical SemanticStateSummary → identical RouteSelected

## 8. pending_act still gates control flow via Noop (CRITICAL)
- Evidence:
  - In act.rs: if pending.request_id != c.request_id → returns LoopStageResult::Noop
- Issue:
  - This is still a control-flow decision based on queue-local state (pending_act)
  - Violates canonical law: SemanticStateSummary must be sole authority
- Required fix:
  - Remove Noop-based gating tied to pending_act
  - Convert to telemetry-only or semantic-state-derived decision

## 8. Queue-local pending_plan still influences control flow (CRITICAL)
- Evidence:
  - plan.rs execute_complete restores pending_plan and returns Noop on request_id mismatch
  - plan.rs returns Noop when action parsing fails (actions.is_empty)
- Issue:
  - Control progression depends on queue-local pending_plan state and parsing outcomes
  - Noop halts semantic progression without emitting canonical events
  - Violates SPEC requirement: SemanticStateSummary must be sole authority for control flow
- Required fix:
  - Eliminate Noop-based control gating tied to pending_plan
  - Always emit canonical events (Debug or PlanningCompleted) to advance semantic pipeline
  - Ensure mismatches and parse failures are represented as semantic events, not silent control stalls

## 9. Queue-local pending_act still influences control flow (CRITICAL)
- Evidence:
  - act.rs execute_complete restores pending_act and returns Noop on request_id mismatch
- Issue:
  - Control flow still branches on queue-local pending_act identity
  - Silent Noop introduces non-semantic control path
  - Violates SPEC requirement for semantic-state-driven control
- Required fix:
  - Replace Noop with explicit semantic events (Debug/Error) and allow loop to progress
  - Remove request_id equality as a control gate; treat mismatch as observable semantic inconsistency
