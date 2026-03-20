# PLAN_judgment: Dynamic Step Routing With Guarded Control

## Objective

Replace rigid phase ordering with adaptive step routing while keeping deterministic and safe runtime control.

This plan introduces two new crates:

- `canon-decision`: constructs the routing prompt contract and parses LLM route selections.
- `canon-judgment`: validates/overrides route selections using deterministic policy.

The runtime keeps phase executors (`observe`, `plan`, `act`, `verify`) unchanged and executable on demand.

---

## Design Split

### `canon-decision` (model-facing)

Responsibilities:

- Build structured LLM routing prompt from:
  - mission/goal
  - current snapshot
  - recent execution log
  - allowed routes
- Parse model JSON into a typed route-selection object.
- Normalize fenced JSON and reject malformed outputs.

Output contract (semantic, not fixed symbol names):

- chosen route
- reason
- optional confidence
- optional runtime signals

### `canon-judgment` (system-facing)

Responsibilities:

- Enforce policy constraints before route execution.
- Bound loops with step caps and repetition caps.
- Apply deterministic fallback route when selection is invalid.
- Track progression markers (context readiness, recent action, completion eligibility).

---

## Runtime Placement

Execution chain becomes:

1. runtime emits tick/context
2. supervisor requests route selection from `canon-decision`
3. supervisor validates route with `canon-judgment`
4. approved route dispatches one phase executor
5. outcome is appended to history
6. repeat until routed completion or guard stop

---

## Guardrails

Policy rules required in `canon-judgment`:

- hard cycle cap (`max_cycles`)
- repetition ceiling on same route
- `execute` requires context readiness
- `validate` requires recent execution result
- `finish` requires completion eligibility (or explicit override policy)
- confidence floor (optional)

Fallback behavior:

- invalid or risky route -> deterministic safe route (default: `scan`)

---

## Initial Deliverables

1. Add `canon-decision` crate:
   - route enum
   - route selection structs
   - prompt renderer
   - JSON parser + validator

2. Add `canon-judgment` crate:
   - policy config
   - mutable gate state
   - route review function
   - deterministic fallback and notes

3. Add supervisor integration surface:
   - utility module in `canon-runtime-supervisor` that applies decision+judgment pipeline
   - no phase behavior changes in this step

4. Keep compatibility:
   - existing event flow remains intact
   - dynamic router can be adopted incrementally by runtime wiring

---

## Next Implementation Step (after scaffolding)

Wire `EventRuntime::emit_tick` loop to call supervisor route pipeline and dispatch a single selected phase per cycle, replacing fixed order execution.
