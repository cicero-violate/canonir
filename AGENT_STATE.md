# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CAPTURE_REFACTOR_MODEL_EXECUTION_SLICE_2`
- date: `2026-02-27`
- mode: `execution`
- invariant: `No heuristics. Structural invariants only.`

### 1) Investigate the problem
- Slice 1 introduced `capture/*` architecture but `engine` and `rules` were still wrappers around `project/*`.
- This kept duplicated ownership and blocked clean separation.

### 2) Gather facts
- `project/engine.rs` and `project/rules.rs` contained full implementations.
- `capture/engine.rs` and `capture/rules.rs` were pass-through re-exports.
- Pipeline already routes through `capture::pipeline::capture`.

### 3) Break down the facts
- Structural migration target for this slice:
- make `capture/engine.rs` authoritative,
- make `capture/rules.rs` authoritative,
- reduce `project/*` to compatibility facades.

### 4) Write it to a state file
- State overwritten for this slice.

### 5) Sort structural and categorical patterns
- Pattern A: ownership inversion (new layer depended on old layer content location).
- Pattern B: compatibility retention required to avoid broad-callsite churn in one turn.

### 6) Write it to state file
- Files changed:
- `canon-capture/src/capture/engine.rs`
- `canon-capture/src/capture/rules.rs`
- `canon-capture/src/project/engine.rs`
- `canon-capture/src/project/rules.rs`
- `AGENT_STATE.md`

### 7) Solve the state file
- Copied full implementations from `project/*` into `capture/engine.rs` and `capture/rules.rs`.
- Updated `capture/engine.rs` imports to use compatibility dependencies (`project::edge_emit`, `project::helpers`, `project::item`) while keeping engine ownership in `capture`.
- Replaced `project/engine.rs` and `project/rules.rs` with single-line compatibility re-exports to `capture`.

### 8) Emit and project the solution incrementally
- Validation:
- `cargo check -p canon-capture`: pass.
- `./run_script.sh repomap`: pass.

### 9) Repeat step 3
- Next structural slice:
- extract `LocalNameResolver` and MIR-local naming surfaces from `project/item.rs` into `capture/mir/resolver.rs`,
- move MIR CFG lowering entry from `project/item.rs::mir_body_structural` into `capture/mir/lower.rs` authoritative implementation,
- keep `project/item.rs` as compatibility facade where possible.
