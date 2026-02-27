# Agent State

## 2026-02-27 — Current Cycle (Structural invariants for declaration types + body operand path refs)

### 1) Investigate the problem
- `emit/repomap` compile errors exposed structural drift between captured declaration types and emitted source types.
- Failures included:
  - missing explicit lifetime ties in function signatures,
  - unstable `Vec<T, Global>` allocator parameter leakage,
  - unresolved external crate reference (`tree_sitter_rust`) not present in emitted dependencies.

### 2) Gather facts
- Declaration-facing types were sourced primarily from normalized semantic `Ty`, which can erase source-level lifetime naming and materialize defaulted generic args.
- `PathRef` extraction in body projection did not visit all MIR constant operands; it missed some external defs (e.g., constant references used in call arguments).
- These are invariant gaps, not formatting errors.

### 3) Break down the facts
- Enforce source declaration invariant for item signatures/fields: prefer HIR-declared type forms for locally-defined declarations.
- Enforce body reference coverage invariant: collect external DefIds from all MIR operands.

### 4) Write it to a state file
- This file is the overwritten cycle snapshot.

### 5) Sort structural and categorical patterns
- Structural pattern A: declaration types must preserve source-level shape (lifetimes, explicit generic args).
- Structural pattern B: dependency path refs must cover all MIR operand constant references.
- Categorical pattern A: semantic-normalization-only type lowering is insufficient for source-faithful emission.

### 6) Write it to state file
- Acceptance criteria:
  - emitted repomap compiles without lifetime/allocator/path errors,
  - dependency for `tree_sitter_rust` is emitted structurally,
  - no heuristic fallback introduced.

### 7) Solve the state file
- `canon-capture/src/project/item.rs`
  - added HIR-declared type extraction for local declaration surfaces:
    - function/method/trait-method params and return types,
    - item/field const/static/type-alias/assoc-type/assoc-const declaration types.
  - these are sourced by HIR spans and used as declaration-facing `TypeExpr::Path` forms.
  - retained `Ty` structural lowering as fallback where HIR-declared form is unavailable.
- `canon-capture/src/project/body.rs`
  - added MIR visitor over operands to collect external `DefId` paths from all constant operands.
  - this closes missing `PathRef` coverage for dependency extraction.

### 8) Emit and project the solution incrementally
- Validation:
  - `cargo check` passed.
  - `run_capture + orchestration` for `capture/repomap` passed.
  - `cargo build` in `test_projects/test_rust_projects/emit/repomap` passed (the previously reported errors are resolved).

### 9) Repeat step 3
- Post-change fact breakdown:
  - declaration type shape and dependency path coverage invariants are now enforced for this failure class.
- Next pending slice:
  - continue Phase 3.5 body structuralization to reduce remaining `Body::Raw`/`Stmt::Raw` surfaces.
