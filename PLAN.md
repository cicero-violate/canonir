## Pending Plan

Only pending work remains in this file. Implemented phases were deleted.

### P1 — Capture Body Structure Completion (Phase 3.5 continuation)

Goal: eliminate remaining body-level `CfgOp::Raw` reliance for function/method bodies.

Pending tasks:

1. Complete structural MIR lowering coverage beyond current `FieldAccess` / `MethodCall` / `StructLit` extraction.
2. Reduce `Body::Raw` fallback usage in item projection for fn/assoc fn where MIR is available.
3. Keep `PathRef` body projection structural and aligned with MIR-derived body operations.

### P2 — Projection Coverage For Structured Body Ops

Goal: keep projection pure and structural for newly emitted body ops.

Pending tasks:

1. Validate emitted source for `FieldAccess` / `MethodCall` / `StructLit` paths across fixtures.
2. Remove or shrink placeholder output surfaces where structured data already exists.

### P3 — Final Validation Sweep

Goal: close this plan with structural confidence checks.

Pending tasks:

1. Run workspace `cargo check`.
2. Re-run fixture pipeline (`capture -> orchestration -> emitted cargo build`) for:
   - `test_projects/test_rust_projects/capture/repomap`
   - `test_projects/test_rust_projects/capture/test_1`
3. Record final pending/none state in `AGENT_STATE.md` (overwrite only).
