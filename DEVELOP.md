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
// removed awaiting_control_successor constraint (handled by invariants)
- Required successors override dedupe, but only when the emission is otherwise legal.
- Cached replay is valid only if it satisfies the current required successor without re-entering the same control state.

## Planning Invariants

- Discovery and mutation must not be mixed in the same batch.
  - Discovery: `list_dir`, `read_file`
  - Mutation/execution: `apply_patch`, `patch_file`, `write_file`, `run_command`, `done`
- `apply_patch` payloads must parse before they are emitted as plan steps.
- `run_command` must carry an absolute `cwd`.
- Unknown `action_kind` is invalid.

## Scenario Matrix

Use this matrix as the source of truth for retry and transition policy. The key rule is that recovery is reason-specific, not generic.

| Command class | Typical actions                           | Failure / outcome class          | Required recovery                                                                                        |
| ---           | ---                                       | ---                              | ---                                                                                                      |
| discovery     | `list_dir`, `read_file`, `search_files`   | target missing / path invalid    | route to `plan` for bootstrap or corrective retry; do not loop discovery forever                         |
| discovery     | `list_dir`, `read_file`, `search_files`   | success, no new context          | preserve prior retry memory; do not clear invalid-plan state just because discovery succeeded            |
| edit          | `apply_patch`, `write_file`, `patch_file` | patch-format / parse failure     | next retry is `single_patch_only`; one `apply_patch`, one file, no `run_command`                         |
| edit          | `apply_patch`, `write_file`, `patch_file` | path/context mismatch            | corrective retry; discovery only if file contents are actually missing                                   |
| validation    | `run_command` (`cargo check`, tests)      | compiler / semantic failure      | route to `plan`; carry stderr/stdout as planner hint; never force `conclude` only because of cycle count |
| bootstrap     | `run_command` (`cargo init`, `cargo new`) | success                          | clear stale bootstrap state, clear queued pre-bootstrap work, force fresh `observe`                      |
| completion    | `done`                                    | verify failed / goal unsatisfied | clear premature completion and replan                                                                    |

### Retry policy mapping

- `mixed discovery + execution` -> `discovery_only`
- `apply_patch` parse / invalid hunk / malformed patch -> `single_patch_only`
- invalid `cwd` / invalid path / missing context -> `corrective_retry`
- successful read-only discovery does not clear retry policy by itself

### Cycle-cap policy

- cycle cap must never force `conclude` when recent actionable failure evidence exists
  - fallback: `plan`
- cycle cap must not force `conclude` when there is no terminal success signal and `finish_ready=false`
  - fallback: `observe`
- `conclude` is reserved for explicit success / explicit stop, not generic stagnation

### Command-state closure checklist

Every command path should define:

- command class
- success/failure/progress outcome class
- which memory is preserved vs cleared
- next required route
- whether an observe refresh is forced
- whether stale queued work must be dropped

If any of those fields are implicit, the system is not fully closed.

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
