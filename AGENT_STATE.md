# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CANON_CAPTURE_COMPRESSION_V2_MIR_DISPATCHER_SLICE_2`
- date: `2026-02-27`
- mode: `execution`
- invariant: `No heuristics. Structural invariants only.`

### 1) Investigate the problem
- `item.rs` still carried duplicated MIR structural-gating and sentinel-emission logic after the first pattern-dispatch extraction.

### 2) Gather facts
- `mir_patterns.rs` already owned statement-kind classification.
- `mir_engine.rs` already owned structural input gate (`structural_guard`/`value_known`).
- `item.rs` still duplicated:
- legacy guard helpers (`stmt_inputs_known`, `value_known`, structural-expression helpers),
- legacy candidate helpers (`is_field_access_candidate`, `is_struct_lit_candidate`, `is_opaque_aggregate_candidate`),
- repeated inline `__canon_suppressed__` emission blocks.

### 3) Break down the facts
- Structural ownership split was incomplete:
- pattern ownership should live in `mir_patterns`,
- guard and suppression invariants should live in `mir_engine`,
- `item.rs` should orchestrate CFG traversal and invoke shared primitives.

### 4) Write it to a state file
- State overwritten for current slice.

### 5) Sort structural and categorical patterns
- Pattern A: duplicated suppression emission across stmt/call branches.
- Pattern B: mixed invariant ownership between `item.rs` and `mir_engine.rs`.
- Pattern C: non-unit return binding must remain explicit (`__ret` binding + terminal return completeness).

### 6) Write it to state file
- Files changed:
- `canon-capture/src/project/mir_engine.rs`
- `canon-capture/src/project/item.rs`
- `canon-capture/src/project/mir_patterns.rs` (kept active as dispatcher source)
- `canon-capture/src/project/mod.rs` (module wiring already active)
- `AGENT_STATE.md`
- `PROJECT_STATUS.md`

### 7) Solve the state file
- Added shared suppression primitive:
- `mir_engine::emit_suppressed_binding`.
- Replaced repeated inline sentinel assignment blocks in `item.rs` with the shared helper.
- Deleted legacy duplicated guard/candidate helpers from `item.rs`.
- Added explicit `emit_suppressed_ret_binding` for `__ret` suppression paths to preserve return structural completeness without relying on fallback behavior.

### 8) Emit and project the solution incrementally
- Validation:
- `cargo check -p canon-capture`: pass.
- `cargo check` workspace: pass.
- `./run_script.sh repomap`: capture + orchestration + emitted `cargo build` pass for fixture.

### 9) Repeat step 3
- Next structural slice:
- move call-terminator classification into a call-pattern dispatcher table (method/plain/filtered/fallthrough),
- consolidate operand/path labeling surfaces toward a single MIR operand-label API,
- continue reducing `item.rs` to CFG traversal + dispatcher orchestration only.
