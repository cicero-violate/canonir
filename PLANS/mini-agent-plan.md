# PLAN: Spec-Derived Execution Index

STOP TOUCH CANON-MINI-AGENT!!!


## Canonical Inputs
- Spec: `PLANS/SPEC.md`
- Diagnostics: `PLANS/diagnostics.md`

## Planner Outputs
- `PLANS/executor-a.md`
- `PLANS/executor-b.md`

## Objective
Convert the canonical requirements in `PLANS/SPEC.md` into a small, current, executor-ready work allocation with minimal drift and no duplicated ownership.

## Rules
- This file is planner-owned and disposable.
- This file does NOT contain canonical truth.
- Canonical truth lives in `PLANS/SPEC.md`.
- Executors do not modify this file.
- Verifier does not verify this file directly; verifier verifies the codebase against `PLANS/SPEC.md`.
- Keep only current decomposition, priority, blockers, and lane assignment here.

## Current Focus
1. Restore runtime freshness / event-log truth
2. Remove queue-driven routing
3. Remove executor-level routing overrides
4. Enforce exact-once observe closure
5. Eliminate duplicate forwarding / fanout
6. Complete DECIDE / ROUTE trace coverage
7. Verify prompt-shell contract boundaries

## Current Blockers
- event log may be stale / runtime freshness not guaranteed
- remaining queue-local routing surfaces may still exist
- executor-side control-flow logic may still bypass policy
- observe fanout / duplicate delivery may still exist
- trace coverage may still have early-return gaps

## Ready-Window Policy
- Executor A: 1-5 ready tasks maximum
- Executor B: 1-5 ready tasks maximum
- No duplicated lane ownership
- Blocked work leaves the ready window
- Root-cause work outranks symptom suppression
- Semantic-state authority outranks queue-local heuristics

## Planner Checklist
- Read `PLANS/SPEC.md`
- Read `PLANS/diagnostics.md`
- Re-scan relevant code and traces
- Refresh `PLANS/executor-a.md`
- Refresh `PLANS/executor-b.md`
- Keep each lane small, concrete, and non-overlapping

## Lane Summary
- Executor A owns one current repair lane
- Executor B owns one non-overlapping repair lane
- Detailed steps live only in:
  - `PLANS/executor-a.md`
  - `PLANS/executor-b.md`

## Completion Model
- Planner assigns work
- Executors execute and report evidence
- Verifier judges the code against `PLANS/SPEC.md`
- Diagnostics updates ranked failures
- Planner re-derives lane plans from the latest evidence

