# Agent State

## 2026-02-27 — Current Cycle (MIR local/value invariants + projection cleanup + validation)

### 1) Investigate the problem
- Remaining requested work was:
  1. complete body structure invariants for MIR locals/values,
  2. projection cleanup for structured body ops,
  3. final validation sweep.

### 2) Gather facts
- Prior MIR structured extraction could emit unresolved MIR temporaries (`_N`) that projection could not safely bind.
- Projection still had placeholder behavior for `CfgOp::StructLit` and naive re-binding of destinations.

### 3) Break down the facts
- Structural invariant needed:
  - only emit MIR-structured body when all used places/operands can be resolved to stable source identifiers.
  - otherwise fallback to HIR raw body.
- Projection invariant needed:
  - destination writes must choose `let` on first bind and assignment on later writes.

### 4) Write it to a state file
- This file is overwritten for this completed cycle.

### 5) Sort structural and categorical patterns
- Pattern A: unresolved MIR locals are a schema/context gap, not a projection concern.
- Pattern B: strict gating at capture boundary preserves correctness without heuristics.
- Pattern C: structured op rendering should remain direct from CanonIR, no string repair.

### 6) Write it to state file
- Implemented:
  - MIR local-name resolver using param names + `var_debug_info` place bindings.
  - Strict structural gating for MIR body extraction:
    - unresolved place/operand labels cause fallback (`None`) to raw body.
  - Re-enabled `mir_body_structural(...).unwrap_or_else(hir_body_src)` with resolver-based invariants.
  - Projection updates:
    - `emit_body` now tracks declared identifiers (params seeded) and uses bind-vs-assign logic.
    - `CfgOp::StructLit` now renders concrete struct literal syntax instead of placeholder comment.

### 7) Solve the state file
- All requested additions were implemented in this cycle with correctness-preserving fallback behavior.

### 8) Emit and project the solution incrementally
- Validation results:
  - workspace `cargo check`: pass.
  - `repomap`: capture -> orchestration -> emitted `cargo build`: pass.
  - `test_1`: capture -> orchestration -> emitted `cargo build`: pass.

### 9) Repeat step 3
- No remaining pending items from this requested phase set.
