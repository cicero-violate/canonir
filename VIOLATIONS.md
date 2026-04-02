# Violations

- Routing not fully derived from SemanticStateSummary: residual event-driven conditions (e.g., event-type checks in policy/executor paths) still influence dispatch decisions.
- Decision → Route invariant not strictly enforced: evidence of RouteSelected emission paths without guaranteed decision_trace linkage.
- EventBus / consumer pipeline still encodes control-flow semantics indirectly (multi-consumer handling, conditional dispatch), violating linear state→decision→transition model.
- Exact-once LoopObserved invariant relies on guards/panics across layers rather than a single canonical emission+propagation path.
- End-to-end proof of single dispatch path (RouteSelected → dispatch) not established; multiple entrypoints (event-triggered dispatch evaluation) still present.

## Additional Violations (from latest evidence)

- Multi-consumer fanout still present:
  - Multiple consumers independently process LoopObserved, creating parallel control-flow effects.
  - Violates requirement for single linear control-flow path.

- EventBus has multiple emission paths:
  - Multiple `emit_with_parents` call sites indicate non-singleton transition emission.
  - Violates canonical single transition requirement.

- Control-flow remains event-driven in parts of the system:
  - Routing and downstream behavior triggered by RuntimeEvent matching instead of semantic-state evaluation.
  - Violates state → decision → transition principle.

- End-to-end invariants not proven:
  - No conclusive runtime evidence showing zero violations for:
    - LoopObserved exact-once
    - Decision → Route trace coverage
    - Successor discharge
  - System still lacks proof of canonical correctness.
