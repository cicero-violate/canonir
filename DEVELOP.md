# Development Patterns

This repo is converging on a specific pattern: detect contradictions early, emit structured diagnostics, and recover with a valid next transition instead of letting the writer discover the problem late.

## Core Rules

- Validate before execution.
  - Plans are checked before `act`.
  - Invalid batches emit diagnostics and close with a non-actionable status such as `invalid_plan`.
- Validate before emission.
  - Executors should check control legality before emitting control events.
  - Writer invariants remain the hard backstop, not the first line of defense.
- Recovery must execute, not just describe intent.
  - A recovery event is only useful if it leads to a real successor or a fresh generator.
- Fatal state must stay observable.
  - Runtime mode changes must be appended as events with causal parents.

## Event-Driven Self-Correction Pattern

Use this sequence when adding or upgrading behavior:

1. Detect
   - Identify the contradiction or invalid batch at the source.
2. Diagnose
   - Emit structured `debug` with:
     - attempted action/event
     - last accepted control event
     - expected successor
     - recovery strategy
3. Guard
   - Refuse to emit the invalid event or execute the invalid batch.
4. Recover
   - Trigger a valid next step:
     - fresh LLM request
     - deterministic fallback
     - explicit successor recovery
5. Close the loop
   - Ensure the recovery path produces a real successor, not only metadata.

## Control-Flow Invariants

- A control event may not be emitted twice while its required successor is still outstanding.
- If `awaiting_control_successor` is set, no new `route_selected` may be emitted.
- Required successors override dedupe, but only when the emission is otherwise legal.
- Cached replay is valid only if it satisfies the current required successor without re-entering the same control state.

## Planning Invariants

- Discovery and mutation must not be mixed in the same batch.
  - Discovery: `list_dir`, `read_file`
  - Mutation/execution: `apply_patch`, `patch_file`, `write_file`, `run_command`, `done`
- `apply_patch` payloads must parse before they are emitted as plan steps.
- `run_command` must carry an absolute `cwd`.
- Unknown `action_kind` is invalid.

## Runtime Robustness Rules

- No panic in the writer path for recoverable invalid events.
- Non-root events must have causal parents.
- Fatal invariant transitions must emit `runtime_state_updated` in-band with the failure.

## Design Standard

When changing the runtime, prefer:

- source-side legality checks
- structured diagnostics
- deterministic fallbacks where possible
- writer invariants as the final safety net

Avoid:

- silent suppression with no active recovery
- retrying the same illegal action
- treating metadata-only recovery as successful correction
