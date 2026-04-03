# Violations

## 1. RouteTick is not handled → decision not loop-driven (CRITICAL)
- Evidence:
  - RouteTick is not present in the event match arms
  - Large group of events (including LoopObserved, LoopPlanned, etc.) are treated as NoOp
  - No explicit RouteTick → emit_decision path exists
- Issue:
  - Decision is not executed per loop cycle
  - Violates SPEC: control must be loop-driven via semantic state
- Required fix:
  - Add explicit RuntimeEvent::RouteTick handler
  - Invoke emit_decision unconditionally from RouteTick
  - Fail-fast if decision is not produced

## 2. Decision stage remains event-gated (CRITICAL)
- Evidence:
  - emit_decision is only invoked in CapabilityCompleted/Failed paths (previously observed)
  - No loop-driven trigger present in current match
- Issue:
  - Decision depends on external capability events
  - Violates invariant: one decision per cycle
- Required fix:
  - Remove event-gated decision triggers as primary path
  - Ensure decision is executed every cycle via RouteTick

## 3. Routing authority not proven SemanticStateSummary-derived (CRITICAL)
- Evidence:
  - No explicit SemanticStateSummary passed into decision path
  - Routing still mediated through decide_from_json interface
- Issue:
  - No proof routing is derived from semantic state
- Required fix:
  - Replace decision interface with SemanticStateSummary input
  - Enforce routing = f(SemanticStateSummary)

## 4. Multiple control successors imply non-canonical flow (HIGH)
- Evidence:
  - control_successor_for_event maps multiple events (PlanningCompleted, LoopActed, etc.) directly to RouteSelected
- Issue:
  - Multiple implicit routing entry points exist
  - Violates single decision authority invariant
- Required fix:
  - Ensure only decision stage produces RouteSelected
  - Remove implicit routing transitions from other events

## 5. System not spec-compliant
- Evidence:
  - RouteTick intended but not implemented as decision driver
  - Decision still event-gated and not semantic-state-driven
- Issue:
  - Core control loop invariant is broken
- Required fix:
  - Implement RouteTick-driven decision execution
  - Ensure semantic-state-only routing authority
