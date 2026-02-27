# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CANON_BODY_STRUCTURAL_PRIMARY_V1`
- date: `2026-02-27`
- mode: `execution`
- invariant: `No heuristics. Structural invariants only.`

### 1) Investigate the problem
- Goal: make MIR-structured body ops primary and eliminate active raw body/op emission dependency.
- Required: preserve emitted crate compilability for fixture matrix.

### 2) Gather facts
- Prior flow still used raw fallback surfaces for function/method bodies.
- Projection still had permissive raw-op rendering and incomplete structured body emission behavior.
- `AGENT_STATE_SECTION_DELTAS.sh` requires stable `### N)` section headers.

### 3) Break down the facts
- Structural gap A: MIR locals/places need canonical resolvable names before structured op emission.
- Structural gap B: projection must track declaration state for destination writes.
- Structural gap C: raw-op rendering in projection hides invariant violations.

### 4) Write it to a state file
- This file is recreated/overwritten in canonical numbered-section form.

### 5) Sort structural and categorical patterns
- Pattern A: unresolved local/value identity must block/skip structured op emission.
- Pattern B: active raw text emission is an invariant leak.
- Pattern C: validation must be compile-based across known fixtures.

### 6) Write it to state file
- Implemented structural changes:
  - MIR local-name resolver (parameter + var_debug_info based).
  - Fn/assoc fn capture switched to MIR structured body as primary source.
  - `Body::Raw`/`Stmt::Raw` no longer emit `CfgOp::Raw` in active capture flow.
  - Projection `CfgOp::Raw` path changed to panic invariant.
  - Structured `StructLit` render + bind-vs-assign destination tracking.

### 7) Solve the state file
- Objective slice completed: active body emission path is structural-first and raw-op projection dependence is removed.

### 8) Emit and project the solution incrementally
- Validation results:
  - workspace `cargo check`: pass.
  - `repomap`: capture -> orchestration -> emitted `cargo build`: pass.
  - `test_1`: capture -> orchestration -> emitted `cargo build`: pass.

### 9) Repeat step 3
- No pending items remain in `PLAN.md` for this phase set.
- Next execution should start from a new plan id if further invariants are introduced.
