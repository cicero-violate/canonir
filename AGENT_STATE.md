# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CANON_CAPTURE_LOC_REDUCTION_V1_PHASE_4_FN_ASSOC_SLICE1`
- date: `2026-02-27`
- mode: `execution`
- invariant: `No heuristics. Structural invariants only.`

### 1) Investigate the problem
- Migrate `Fn`/`AssocFn` metadata lowering to engine templates while preserving MIR body delegation.

### 2) Gather facts
- Rule table updates:
- `fn_item` -> `Template("fn_item")`
- `assoc_fn_item` -> `Template("assoc_fn_item")`
- Engine template emitters added:
- `lower_fn_item(...)`
- `lower_assoc_fn_item(...)`
- Shared helper visibility widened for engine reuse:
- `map_params` -> `pub(crate)`
- `declared_fn_return_type_expr` -> `pub(crate)`
- `mir_body_structural` -> `pub(crate)`
- Legacy branches deleted from `project_item_legacy`:
- removed `DefKind::Fn`
- removed `DefKind::AssocFn`

### 3) Break down the facts
- Function metadata path is now engine-owned.
- MIR body lowering call boundary remained unchanged and isolated in item module.
- Fallback still covers remaining higher-complexity kinds (`Trait`, `Impl`, assoc const/type).

### 4) Write it to a state file
- State overwritten to current checkpoint.

### 5) Sort structural and categorical patterns
- Pattern A: function path migration is complete without touching MIR lowering internals.
- Pattern B: engine now owns both low-risk item kinds and function metadata forms.
- Pattern C: remaining legacy weight is concentrated in trait/impl/assoc metadata and helper internals.

### 6) Write it to state file
- Files changed this slice:
- `canon-capture/src/project/rules.rs`
- `canon-capture/src/project/engine.rs`
- `canon-capture/src/project/item.rs`

### 7) Solve the state file
- Phase 4 is in progress; function/assoc-fn slice complete.

### 8) Emit and project the solution incrementally
- Validation performed:
- `cargo check -p canon-capture`: pass
- `cargo check` workspace: pass
- `repomap` full pipeline/build: pass
- `test_1` full pipeline/build: pass
- LOC:
- `item.rs`: `2134 -> 1995`

### 9) Repeat step 3
- Next slice:
- migrate `Trait`, `Impl`, `AssocTy`, `AssocConst` into engine templates.
