# Agent State

## 2026-02-27 — Current Cycle (next-plan continuation and blocker identification)

### 1) Investigate the problem
- Next unresolved plan items are primarily:
  - Phase 4.6 (`Instantiates` derivation without heuristics),
  - Phase 5.3 (remove dependency underscore→hyphen heuristic in projection).

### 2) Gather facts
- `canon-analyzer/src/solver/type_solver.rs` still derives `Instantiates` via string parsing helpers (`split_generic_path`, `split_top_level`, text normalization).
- `canon-projection/src/layout/mod.rs` still uses `render_dependency_entry` underscore→hyphen conversion heuristic for Cargo deps.
- Current IR shape for dependency roots is path-root only (`Crate.dependencies: Vec<PathId>`), with no explicit structural package-name field per dependency.
- Current IR type representation for external paths does not always preserve structural generic-arg decomposition for all declaration-origin types.

### 3) Break down the facts
- Cannot remove these heuristics cleanly without introducing missing structural invariants:
  1. Dependency invariant missing: explicit per-dependency Cargo package name (when Rust crate name != package name).
  2. Type instantiation invariant missing: explicit generic argument structure available at Canon layer for all relevant external/local type references.

### 4) Write it to a state file
- This file is the overwritten cycle snapshot.

### 5) Sort structural and categorical patterns
- Structural pattern A: heuristic logic remains exactly where schema does not yet carry required structural data.
- Structural pattern B: removing heuristics now without adding invariants would regress emission correctness.
- Categorical pattern A: this is a schema/invariant gap, not an implementation bug.

### 6) Write it to state file
- Acceptance criteria for next slice:
  - add explicit structural representation for dependency package mapping,
  - add explicit structural representation for instantiation arg links,
  - then remove projection/type-solver heuristics.

### 7) Solve the state file
- No heuristic-removal patch was applied in this cycle because it would violate structural correctness without new invariants.

### 8) Emit and project the solution incrementally
- Previous cycle validations remain green:
  - `emit/repomap cargo build` passes after declaration/body invariant fixes.

### 9) Repeat step 3
- Next actionable structural task:
  - introduce missing invariants in `canon` schema and capture/analyzer wiring, then remove remaining heuristic surfaces in `layout/mod.rs` and `type_solver.rs`.
