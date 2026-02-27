# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CAPTURE_REFACTOR_MODEL_EXECUTION_SLICE_23`
- date: `2026-02-27`
- mode: `execution`
- invariant: `No heuristics. Structural invariants only.`

### 1) Investigate the problem
- One remaining specialization remained ad-hoc in `lower.rs`: zero-arg enum constructor suppression depended on a dedicated helper outside `patterns.rs` classification.

### 2) Gather facts
- `ConstUse` already existed in `patterns.rs`, but zero-arg constructor detection was still in `lower.rs` (`is_zero_arg_enum_ctor_use`).
- `mir_assign_stmt` still had an early zero-arg enum suppression check tied to the old helper.

### 3) Break down the facts
- Structural target:
- move zero-arg enum constructor detection into the pattern table,
- make assign flow consume pattern kinds without duplicate detector logic,
- remove redundant helper from `lower.rs`.

### 4) Write it to a state file
- State overwritten for this slice.

### 5) Sort structural and categorical patterns
- Pattern A: pattern-table specialization promotion (`ZeroArgEnumCtor`).
- Pattern B: lowerer simplification by eliminating duplicate classifier checks.

### 6) Write it to state file
- Files changed:
- `canon-capture/src/capture/mir/patterns.rs`
- `canon-capture/src/capture/mir/lower.rs`
- `STRUCTURAL_INVARIANTS_REPORT.md`
- `AGENT_STATE.md`

### 7) Solve the state file
- Added `MirOpKind::ZeroArgEnumCtor` and `is_zero_arg_enum_ctor_pattern(...)` in `capture/mir/patterns.rs`.
- Updated assign dispatch in `lower_assign_statement(...)`:
- `ZeroArgEnumCtor` now emits suppressed binding directly and returns,
- `ConstUse` now falls through to generic assign lowering for non-zero-arg constants.
- Removed redundant zero-arg enum check from `mir_assign_stmt(...)`.
- Deleted now-redundant `is_zero_arg_enum_ctor_use(...)` from `lower.rs`.

### 8) Emit and project the solution incrementally
- Validation:
- `cargo check -p canon-capture`: pass.
- `./run_script.sh repomap`: pass.
- `STRUCTURAL_INVARIANTS_REPORT.md` regenerated.

### 9) Repeat step 3
- Next structural slice:
- continue extracting additional assignment suppression/specialization gates from `mir_assign_stmt` into explicit pattern kinds or guard adapters,
- preserve `lower.rs` as CFG/orchestration surface with thin dispatch.
