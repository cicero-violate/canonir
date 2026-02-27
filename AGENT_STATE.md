# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CANON_CAPTURE_LOC_REDUCTION_V1_PHASE_5_EDGE_TEMPLATE_SLICE4`
- date: `2026-02-27`
- mode: `execution`
- invariant: `No heuristics. Structural invariants only.`

### 1) Investigate the problem
- Continue consolidating project-level edge construction into one structural emission backend.

### 2) Gather facts
- Shared `project/edge_emit.rs` was added and already adopted by:
- `engine.rs` (`use_item` edge path)
- `relations.rs` (relation-template dispatch helpers)
- This slice extends adoption to `body.rs`:
- CFG (`CfgEdge`), const dependency (`ConstDep`), call (`Calls`), and pathref containment (`Contains`) now emit through `edge_emit`.

### 3) Break down the facts
- Project-level edge constructors are now centralized in one module.
- Rule/template logic still controls edge selection; only constructor backend changed.
- Behavior and invariants remain unchanged.

### 4) Write it to a state file
- State overwritten to current checkpoint.

### 5) Sort structural and categorical patterns
- Pattern A: selection layer (`RuleEdge`, `RelationTemplate`, MIR traversal) is separated from construction layer (`edge_emit`).
- Pattern B: additional edge kinds can be standardized by adding wrappers without touching selection logic.

### 6) Write it to state file
- Files changed this slice:
- `canon-capture/src/project/body.rs`
- `canon-capture/src/project/edge_emit.rs`
- `canon-capture/src/project/engine.rs`
- `canon-capture/src/project/relations.rs`
- `canon-capture/src/project/mod.rs`
- `PLAN.md`
- `AGENT_STATE.md`
- `PROJECT_STATUS.md`

### 7) Solve the state file
- Completed body-edge migration to shared primitives; project-level edge emission path is now structurally unified.

### 8) Emit and project the solution incrementally
- Validation performed:
- `cargo check -p canon-capture`: pass
- `cargo check` workspace: pass
- `repomap` capture -> orchestration -> emitted `cargo build`: pass
- `test_1` capture -> orchestration -> emitted `cargo build`: pass
- LOC snapshot:
- `body.rs`: `144`
- `edge_emit.rs`: `36`
- `engine.rs`: `449`
- `relations.rs`: `129`
- `canon-capture/src` total: `4810`

### 9) Repeat step 3
- Next slice:
- audit remaining non-project edge constructors (e.g. assemble path) and decide Phase 5 boundary
- if out-of-scope for capture/project refactor, mark Phase 5 project-side completion explicitly
