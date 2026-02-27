# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CANON_CAPTURE_LOC_REDUCTION_V1_PHASE_5_EDGE_TEMPLATE_SLICE3`
- date: `2026-02-27`
- mode: `execution`
- invariant: `No heuristics. Structural invariants only.`

### 1) Investigate the problem
- Unify edge emission across engine and relations so both flow through one structural primitive layer.

### 2) Gather facts
- Added shared module: `canon-capture/src/project/edge_emit.rs`.
- `edge_emit` now owns common `EdgeHint` construction:
- `push`, `push_contains`, `push_resolves`, `push_reexports`,
- `push_assoc_item`, `push_impl_for`, `push_impl_ref`.
- Migrated `engine.rs` (`use_item` edge path) to `edge_emit`.
- Migrated `relations.rs` template dispatch helpers to `edge_emit`.

### 3) Break down the facts
- Shared primitive layer removes duplicate `EdgeHint` constructors in multiple modules.
- Relation-template dispatch remains active; only emission backend changed.
- Rule-edge template dispatch remains active for `use_item`; only emission backend changed.

### 4) Write it to a state file
- State overwritten to current checkpoint.

### 5) Sort structural and categorical patterns
- Pattern A: edge declaration and edge emission are now separated.
- Pattern B: rule/template systems decide *what* edges to emit.
- Pattern C: `edge_emit` decides *how* edges are constructed.

### 6) Write it to state file
- Files changed this slice:
- `canon-capture/src/project/edge_emit.rs` (new)
- `canon-capture/src/project/mod.rs`
- `canon-capture/src/project/engine.rs`
- `canon-capture/src/project/relations.rs`
- `PLAN.md`
- `AGENT_STATE.md`
- `PROJECT_STATUS.md`

### 7) Solve the state file
- Completed shared edge primitive unification without fallback or heuristic logic.

### 8) Emit and project the solution incrementally
- Validation performed:
- `cargo check -p canon-capture`: pass
- `cargo check` workspace: pass
- `repomap` capture -> orchestration -> emitted `cargo build`: pass
- `test_1` capture -> orchestration -> emitted `cargo build`: pass
- LOC snapshot:
- `item.rs`: `1391`
- `engine.rs`: `449`
- `rules.rs`: `279`
- `relations.rs`: `129`
- `edge_emit.rs`: `36`
- `canon-capture/src` total: `4813`

### 9) Repeat step 3
- Next slice:
- migrate remaining inline edge constructors in `project/body.rs` to `edge_emit`
- continue Phase 5 until project-level edge construction sites are consolidated
