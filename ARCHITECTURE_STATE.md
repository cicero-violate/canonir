## Current State

The system is materially closer to the objective function in `ARCHITECURAL_OBJECTIVE_FUNCTION.md`, but it is not at the target state yet.

The current architecture is best described as:

- event-sourced
- policy-first for control behavior
- typed-semantic for planner/route judgment
- still partially redundant in test/matrix coverage and legacy helper surfaces

The important shift is no longer just "policy decides, executors emit". It is now also:

- observe derives semantic state once
- plan consumes typed semantic state
- route consumes typed semantic state

That is the main judgment-layer improvement over the earlier control-only refactor.

## Alignment To Objective Function

Using the project objective:

- `C` has improved at the policy-family level, but is not yet `1`
- `K` has improved because control and judgment are more explicit
- `D_det` has improved because more routing and recovery behavior is policy-owned
- `L` increased during refactor and migration
- `D_dup` decreased in executors, but still exists in some policy/test/helper layers
- `B_hidden` is much lower, but not provably `0`

So the system is in this state:

- control architecture: largely aligned
- judgment architecture: meaningfully improved
- mathematical closure: not yet achieved

## Source Of Truth

Current source-of-truth layers are:

- event log for runtime state evolution
- typed `LoopObserved.semantic_summary` for semantic workspace/judgment state
- policy evaluators for transition choice

This is a real improvement over the previous arrangement where semantic meaning was tunneled through `workspace_facts` strings and re-derived in multiple places.

## Semantic State

`canon-utils/canon-semantic-state` is now the central semantic transport object.

`SemanticStateSummary` currently carries:

- target root
- path existence
- repo / cargo project state
- crate name
- entrypoint kind
- source file graph summary
- module gap summary
- planning preconditions
- repair intents
- compiler hints
- blocked-validation / compiler-repair booleans

The active path is now:

- `observe` derives semantic state
- `LoopObserved` carries semantic state directly
- `plan` validates against semantic state directly
- `route` reasons from semantic state directly

This is the current semantic contract.

## Control Architecture

### Route

`canon-utils/canon-route/src/policy.rs` owns the major route decisions:

- deterministic route selection
- route rewrite policy
- dispatch suppression
- emit legality
- cached-route behavior
- event-trigger redispatch
- recovery emission
- successor consumption
- cycle-cap downgrade behavior
- actionable-failure detection

`canon-utils/canon-route/src/executor.rs` is now mostly:

- context accumulation
- policy invocation
- request bookkeeping
- event emission

### Loop

`canon-utils/canon-loop/src/policy.rs` owns the major loop decisions:

- invalid-plan classification
- retry policy
- action/outcome classification
- observe/reward recovery selection
- runtime observe suppression
- bootstrap invalidation
- recovery-event classification
- recovery-execution normalization

`canon-utils/canon-loop/src/executor.rs` is now mostly:

- context accumulation
- policy invocation
- stage scheduling/plumbing
- normalized emit helpers
- recovery execution

## Judgment Architecture

This is the most important improvement after the control migration.

The system now has explicit judgment inputs:

- workspace model
- compiler hints
- planning preconditions
- repair intents
- semantic summary

And it now uses them in three places:

- planner prompt construction
- plan-batch validation
- route actionable-failure reasoning

This means the planner is less blind than before. It is still not fully intelligent, but it is no longer operating mainly on raw logs plus generic instructions.

## Compiler-Driven Repair

The compiler hint layer now recognizes more actionable classes.

Current typed compiler hint coverage includes:

- missing module
- dead-code / forbid conflict
- missing entrypoint
- unresolved import
- missing symbol
- duplicate definition
- trait bound failure
- generic compiler failure

These now feed:

- semantic summary rendering
- planning preconditions
- repair intents
- route actionable-failure detection

The new system property here is:

- compiler failures are increasingly interpreted as structured repair work, not just raw stderr

## Repair-Intent Enforcement

Repair validation is no longer only class-level.

It now checks whether the first batch targets the relevant object/path for the highest-priority intent.

Examples:

- missing modules -> must target expected module paths
- missing entrypoint -> must target `src/main.rs` or `src/lib.rs`
- dead-code conflict -> must target relevant source files
- unresolved import / missing symbol / duplicate definition / trait-bound failure -> can now be tied to hinted target files

This is a meaningful step toward better `J` in the user's judgment model.

## Matrix And Coverage

`canon-utils/canon-policy-matrix` provides shared coverage harnessing for:

- route transition families
- route dispatch / emit / cache / recovery families
- loop transition / runtime / recovery families
- command outcome families
- invalid-plan retry families

This is still family coverage, not full cross-product closure.

So:

- `C` has improved
- but `C != 1`

That matters because the objective function explicitly requires full state-space coverage.

## What Is Strong Now

The strongest parts of the current architecture are:

- typed semantic observation
- policy-owned control branching
- structured compiler-hint interpretation
- repair-intent-aware plan validation
- reduced executor hidden branching

These are the parts that are most aligned with the stated objective function.

## What Is Still Weak

The main remaining weaknesses are:

- no full state-space proof / exhaustive valid-state enumeration
- no proof that all remaining executor branches are operational-only
- semantic summary still mixes compact derived state with string-encoded subfields
- compiler-hint targeting is heuristic, not yet normalized into richer typed objects
- route and planner still rely on some summary-string parsing conventions instead of deeper typed substructures
- matrix/test completeness is below the objective target

In objective-function terms:

- `C` is still below the required target
- `B_hidden` is probably not zero, only much lower
- `D_dup` is reduced, but not minimized
- `L` is currently inflated by the migration phase

## What Is No Longer True

These earlier statements are no longer accurate:

- semantic meaning is primarily carried in `workspace_facts`
- planner validation can proceed without complete semantic state
- route mirrors semantic state in its own duplicated fields
- missing-target state needs a separate error channel outside observation

Those have been removed or materially reduced.

## Practical Stop Condition

It is reasonable to stop when the following are true:

- `cargo build` is green
- `cargo test` is green
- typed semantic observation is stable
- policy-owned control remains stable
- compiler-hint-driven repair behavior is sufficient for current project goals

It is not necessary to reach full mathematical closure before stopping, but it should be explicit that the system is stopping short of the objective target, not claiming completion.

## Practical Summary

Right now the system is:

- architecturally coherent
- much more explicit than before
- significantly closer to the objective function
- not yet globally closed

The correct concise description is:

- control is mostly policy-owned
- judgment is now semantically structured
- execution is mostly coordination/emission
- full closure remains future work
