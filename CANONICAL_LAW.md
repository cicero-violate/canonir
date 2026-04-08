# Canonical Law

Canon is an event-sourced control system governed by the principle:

```
state → decision → transition → event log
```

All routing and control-flow decisions must be derived from semantic state, not from queue-local counters or scheduler heuristics.

## Routing Authority

- `SemanticStateSummary` is the single source of truth for routing and control-flow correctness.
- `scheduler_len`, `planned_pending`, and similar counters are derived telemetry — they are not authoritative unless the code explicitly proves they are derived mirrors of semantic state.
- Any routing path that depends on queue-local state when semantic-state facts are available is an invariant violation.

## Decision Discipline

- Prefer changes that make code follow: `state → decision → transition`
- Do not introduce or preserve routing logic that depends on local mirrors when semantic-state facts exist.
- Prioritize work that migrates decision logic to semantic-state authority before local edge patches that preserve queue-truth.
- A task is NOT complete if it leaves `scheduler_len` or local queue mirrors acting as routing authority where `SemanticStateSummary` is available.

## Diagnostics

- A high-impact failure exists whenever queue-local state still drives routing in places that should derive from semantic state.
- State-authority drift (queue-truth routing surviving a semantic-state migration) must be ranked as a critical issue.
- Explicitly check whether routing/control-flow still depends on `scheduler_len`, `planned_pending`, or other local queue mirrors instead of `SemanticStateSummary`.

## Event Log Authority

- The append-only event log under `state/event_log/event.tlog.d` is the authoritative record of all transitions.
- Trust the event log over ad-hoc temp traces (`/tmp/runtime.trace`, etc.) when they disagree.
- Before trusting a trace file, confirm it was updated in the current cycle (mtime, size change, or fresh producer command).
