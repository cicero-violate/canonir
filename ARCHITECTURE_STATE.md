## Current State

The system is now organized around a policy-first control model.

- The event log remains the source of truth.
- The writer still enforces transition invariants rather than correcting them.
- Route and loop executors have been reduced toward coordination/emission roles.
- Most control decisions that were previously implicit executor branches are now explicit policy evaluations.

## Control Shape

Primary control progression is still:

- `observe -> plan -> act -> verify -> conclude`

The important shift is that control selection is no longer mainly encoded as scattered executor conditionals. It is increasingly encoded as:

- policy classification
- policy transition evaluation
- executor emission of the selected result

## Policy Surfaces

### Route Policy

`canon-utils/canon-route/src/policy.rs` now owns explicit decision surfaces for:

- deterministic fast paths
  - bootstrap refresh -> observe
  - done -> verify
  - continue act
  - planned to act
  - missing observed context -> observe
- route rewrites
  - repeated observe -> plan
  - missing target -> plan
  - cycle-cap fallback -> plan or observe
- dispatch suppression
  - halted
  - context not ready
  - pending request in flight
  - awaiting successor
  - duplicate route for current control event
- cached-route behavior
  - replay cached route
  - invalidate cached observe route
  - suppress duplicate prompt
- route emit legality
  - duplicate emit before successor
  - illegal control reentry
  - illegal control emit against the wrong expected successor
- route event dispatch
  - batch settled
  - idle dispatch
  - recoverable empty plan redispatch
- failure fallback
  - capability failure -> heuristic reroute
- emit side effects
  - clear deterministic observe sentinel
  - halt on conclude
- recovery emission
  - emit expected-successor recovery event
- successor consumption
  - clear `awaiting_control_successor` when the matching successor arrives

### Loop Policy

`canon-utils/canon-loop/src/policy.rs` now owns explicit decision surfaces for:

- invalid-plan classification
  - mixed batch
  - patch format
  - path/cwd
  - missing context
  - unknown
- retry policy
  - discovery only
  - single patch only
  - corrective retry
- action classification
  - discovery
  - edit
  - validation
  - bootstrap
  - completion
- action outcome classification
  - bootstrap success
  - validation compiler failure
  - validation success
  - semantic failure
  - patch missing target
  - patch apply failure
  - edit success
  - other
- loop transition recovery
  - invalid-plan suppression clearing
  - act-stall observe recovery
  - reward recovery
- loop runtime execution mode
  - forced observe
  - triggered observe
  - suppress observe on invariant
  - suppress observe on pending successor
  - halt blocking
  - halted route-selected warning
- recovery-event classification
  - force observe
  - reward execute
  - reward skip already satisfied
  - reward missing context
- recovery execution normalization
  - reward noop / error
  - observe deferred / noop / error
- error-to-observe routing
  - explicit recovery
  - generic error observe
  - no observe
- bootstrap invalidation
  - invalidate queued plan work after successful bootstrap

## Executor Role

### Route Executor

`canon-utils/canon-route/src/executor.rs` now mostly does:

- update route context from events
- record control state
- call policy evaluators
- emit selected route / suppression / recovery events
- manage request bookkeeping for router LLM calls

### Loop Executor

`canon-utils/canon-loop/src/executor.rs` now mostly does:

- update loop context from events
- record control state
- call policy evaluators
- execute selected observe / reward recovery operations
- emit normalized debug and error events
- manage scheduler, dependency tracker, and stage execution plumbing

## Matrix Coverage

`canon-utils/canon-policy-matrix` now provides a shared scenario harness.

It covers:

- route transition families
- route dispatch families
- route emit legality families
- route cache families
- route failure fallback
- route emit effects
- route recovery emission
- route successor consumption
- loop transition families
- loop runtime families
- recovery-event families
- recovery-execution families
- bootstrap-effect families
- run-command outcome families
- apply-patch outcome families
- verify outcome families
- invalid-plan retry families

This is family coverage, not exhaustive full-state cross-product coverage.

## What Is Still Operational

The remaining executor logic is mostly operational, not policy-bearing:

- event/context accumulation
- scheduler and dependency tracker mutation
- pending request bookkeeping
- stage execution plumbing
- event emission mechanics

This is the intended steady-state boundary.

## What Is Not Fully Closed Yet

The system is substantially more explicit than before, but not mathematically complete.

Open limits:

- matrix coverage is family-complete, not full state-space complete
- some runtime behavior still depends on operational sequencing, not just policy rows
- generic stage execution is still opaque compared with the policy layer
- there is not yet an automatic proof that every new executor branch is either:
  - policy-backed
  - or explicitly marked operational-only

## Practical Summary

The architecture currently is:

- event-sourced
- invariant-enforced
- policy-first
- increasingly deterministic
- significantly less dependent on hidden executor branching than earlier versions

The main architectural shift already achieved is this:

- policy decides
- executors coordinate and emit

That is the current state of the system.
