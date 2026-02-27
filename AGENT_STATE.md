# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CANON_CAPTURE_LOC_REDUCTION_V1_PHASE_5_EDGE_TEMPLATE_SLICE2`
- date: `2026-02-27`
- mode: `execution`
- invariant: `No heuristics. Structural invariants only.`

### 1) Investigate the problem
- Continue Phase 5 by removing remaining inline edge boilerplate in relation projection.

### 2) Gather facts
- `project/relations.rs` previously had inline conditional edge pushes for:
- parent `Contains`
- assoc-item `AssocItem`
- impl `ImplFor`
- impl `ImplRef`
- A new structural relation-template dispatcher is now implemented:
- `RelationTemplate` enum
- `relation_templates(def_kind)` mapping
- per-template emission helpers (`push_parent_contains`, `maybe_push_parent_assoc_item`, `maybe_push_impl_for`, `maybe_push_impl_ref`)

### 3) Break down the facts
- Relation edge emission is now table-driven in shape rather than ad-hoc branching.
- Emitted edge semantics are preserved: same edge kinds and same gating conditions.
- Phase 5 now has two active migrated surfaces:
- `engine/use_item` via `RuleEdge`
- `relations.rs` via `RelationTemplate`

### 4) Write it to a state file
- State overwritten to current checkpoint.

### 5) Sort structural and categorical patterns
- Pattern A: edge category declarations now drive execution paths.
- Pattern B: concrete edge push behavior is centralized in narrow helpers.
- Pattern C: remaining edge work should target cross-module harmonization (shared template helpers) while preserving current invariants.

### 6) Write it to state file
- Files changed this slice:
- `canon-capture/src/project/relations.rs`
- `PLAN.md`
- `AGENT_STATE.md`
- `PROJECT_STATUS.md`

### 7) Solve the state file
- Completed relation-template migration slice with no fallback or heuristic behavior.

### 8) Emit and project the solution incrementally
- Validation performed:
- `cargo check -p canon-capture`: pass
- `cargo check` workspace: pass
- `repomap` capture -> orchestration -> emitted `cargo build`: pass
- `test_1` capture -> orchestration -> emitted `cargo build`: pass
- LOC snapshot:
- `item.rs`: `1391`
- `engine.rs`: `460`
- `rules.rs`: `279`
- `relations.rs`: `144`
- `canon-capture/src` total: `4802`

### 9) Repeat step 3
- Next slice:
- converge `RuleEdge` and relation-template paths into shared edge-emission primitives
- continue Phase 5 until remaining inline edge boilerplate is removed
