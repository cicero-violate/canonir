# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CANON_CAPTURE_LOC_REDUCTION_V1_KICKOFF`
- date: `2026-02-27`
- mode: `execution`
- invariant: `No heuristics. Structural invariants only.`

### 1) Investigate the problem
- `item.rs` is oversized and mixes analysis, rule selection, emission templates, hooks, and MIR integration.

### 2) Gather facts
- Current validated fixture scope (`repomap`, `test_1`) is green.
- Existing implementation has many repeated DefKind branches and repeated edge/node boilerplate.
- Current request is architectural LOC reduction, not bugfix-only iteration.

### 3) Break down the facts
- Structural split target:
- `A` (engine algorithm)
- `R` (rule table data)
- `E` (backend adapters/hooks)
- This enables replacing repeated branch code with compact table-driven lowering.

### 4) Write it to a state file
- State transitioned from bug-closure phase to refactor phase kickoff.

### 5) Sort structural and categorical patterns
- Pattern A: rule matching and emission ordering logic is global and should be centralized.
- Pattern B: DefKind-specific emission templates are data, not control flow.
- Pattern C: MIR lowering remains isolated as procedural backend and should not be mixed into rule matching.

### 6) Write it to state file
- Next file targets:
- `canon-capture/src/project/rules.rs`
- `canon-capture/src/project/engine.rs`
- `canon-capture/src/project/item.rs` (shrink wrapper)

### 7) Solve the state file
- Active plan loaded from `PLAN.md` (`CANON_CAPTURE_LOC_REDUCTION_V1`).

### 8) Emit and project the solution incrementally
- Next execution chunk:
- establish baseline metrics
- scaffold rules/engine modules
- keep behavior unchanged initially

### 9) Repeat step 3
- iterate per phase with compile + fixture matrix after each slice.
