# Agent State

## 2026-02-27 — Current Cycle (Phase 3.1 structural type lowering)

### 1) Investigate the problem
- `canon-capture` still relied on string-based type parsing in `canon_assemble` (`str_to_type_kind` and related text split/normalize helpers).
- This violated the structural-only capture boundary and created unresolved/projection failures under async/opaque forms.

### 2) Gather facts
- `project/item.rs` already had `TyCtxt` access and could lower from `rustc_middle::ty::Ty` directly.
- Model layer (`canon-capture/src/types.rs`) represented types as `String`, forcing assemble-time text parsing.
- Orchestration panic surfaces were caused by non-structural fallback paths (`Unresolved` reaching projection).

### 3) Break down the facts
- Introduce structural model type expression in capture model.
- Lower `rustc` `Ty` into structural model at project phase.
- Convert structural model to Canon `TypeKind` in assemble phase.
- Remove string parser dependency for item/type lowering.

### 4) Write it to a state file
- This file is the overwritten cycle snapshot.

### 5) Sort structural and categorical patterns
- Structural pattern A: types are lowered from compiler `Ty` into capture `TypeExpr`.
- Structural pattern B: assemble maps `TypeExpr` -> `TypeKind` directly.
- Structural pattern C: async/coroutine type path is resolved through `CoroutineArgsExt::return_ty` rather than text unwrap.
- Categorical pattern A: remove parsing heuristics from capture type boundary.

### 6) Write it to state file
- Acceptance criteria for this cycle:
  - no `str_to_type_kind` path for item/type lowering,
  - structural lowering compiles,
  - orchestration succeeds for baseline projects.

### 7) Solve the state file
- `canon-capture/src/types.rs`
  - added structural `TypeExpr` and `PrimType`.
  - migrated model type-bearing fields from `String` to `TypeExpr`.
- `canon-capture/src/project/item.rs`
  - added structural `lower_ty` from `rustc_middle::ty::Ty` to `TypeExpr`.
  - handled `FnPtr`, `FnDef`, `Dynamic`, `Alias(Opaque)` and `Coroutine` structurally.
  - removed string `fmt_ty` return-path dependency for node type fields.
- `canon-capture/src/canon_assemble.rs`
  - replaced string parser use with `intern_ty_expr` (`TypeExpr` -> `TypeKind`).
  - retained local type relink pass and updated call sites to structural type model.
  - removed obsolete parser tests tied to `str_to_type_kind`.

### 8) Emit and project the solution incrementally
- Validation:
  - `cargo check` passed.
  - capture + orchestration passed for:
    - `test_projects/test_rust_projects/capture/test_1`
    - `test_projects/test_rust_projects/capture/repomap`

### 9) Repeat step 3
- Post-change fact breakdown:
  - item/type lowering no longer depends on the old string parser path.
  - remaining heuristics are outside this specific type-lowering slice (notably body/path text scan paths).
- Next pending slice:
  - continue Phase 3 structuralization for body/path reference extraction (remove raw text scan path in assemble and feed structural `PathRef` directly from projection).
