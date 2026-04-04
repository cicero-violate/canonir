# Violations

## ❌ Duplicate RouteSelected Emission (CRITICAL)

### Evidence
- Failing test: `duplicate_route_selected_in_same_tick_panics`
- Runtime panic: "duplicate RouteSelected within the same tick"
- Logs show multiple RouteSelected events emitted for a single tick

### Spec Violations
- Deterministic control (SPEC: deterministic at control layer)
- Invariant 6: Event Uniqueness
- Exactly-once transition requirement per decision

### Impact
- Breaks canonical control determinism
- Causes runtime panic and invalid log state

### Required Fix
- Enforce exactly one RouteSelected per RouteTick
- Prevent re-entrant or duplicate dispatch within same tick

---

## ❌ Successor Obligation Violation (CRITICAL)

### Evidence
- Log shows: `route_selected -> route_selected`
- Invariant failure: expected `planning_completed`, got `route_selected`

### Spec Violations
- Invariant 8: Successor Obligation (FSM)
- SPEC: RouteSelected(plan) → PlanningCompleted

### Impact
- Invalid control-flow graph
- Event log append failure

### Required Fix
- Enforce FSM boundary at emission time
- Block RouteSelected if required successor not satisfied

---

## ❌ Non-Semantic Routing (HIGH)

### Evidence
- Repeated RouteSelected triggered by event flow, not single decision
- Warning: "non-canonical RouteSelected received"

### Spec Violations
- SemanticStateSummary must be sole routing authority
- "No hidden branches" (H = 0)

### Impact
- Routing depends on runtime flow instead of semantic truth
- Violates determinism and replayability

### Required Fix
- Centralize routing strictly in decision() pipeline
- Ensure RouteTick → decision → RouteSelected occurs exactly once

---

## ❌ Hidden Control Flow / Re-Entrancy (HIGH)

### Evidence
- Duplicate routing triggered during same tick execution
- Dispatch occurs multiple times within same control cycle

### Spec Violations
- No hidden branches (SPEC constraint)
- Executors must not introduce control flow

### Impact
- Non-transparent behavior
- Breaks replay determinism

### Required Fix
- Eliminate re-entrant dispatch paths
- Introduce strict per-tick routing guard

---

## Summary

System builds, but fails SPEC compliance:
- Duplicate RouteSelected ❌
- FSM successor violation ❌
- Non-semantic routing ❌
- Hidden control flow ❌

Overall: NOT compliant with PLANS/SPEC.md
