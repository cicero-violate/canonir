# Agent State

## 2026-02-27 — Current Cycle (Continue Plan: Phase 5.1/5.3/5.4)

### 1) Investigate the problem
- Continue from analyzer completion into projection/layout cleanup.
- Targets this cycle:
  - Phase 5.1: handle `TypeKind::Unresolved` in projection as an error surface.
  - Phase 5.3: remove dependency rendering special-cases and carry Cargo package naming structurally.
  - Phase 5.4: remove `normalize_use_path` helper and related use-path heuristics.

### 2) Gather facts
- `emit/types.rs` still rendered `TypeKind::Unresolved` as plain path text.
- `layout/mod.rs` still contained `render_dependency_entry` with `tree_sitter*` special-cases.
- `emit/fmt.rs` still exposed `normalize_use_path`, and `emit/items.rs` used additional path-filtering logic.
- `CanonNodeKind::Crate` lacked a dedicated cargo package name field.

### 3) Break down the facts
- Projection should fail-fast on unresolved types instead of silently emitting best-effort text.
- Dependency lines should be emitted directly from canonical dependency roots; no crate-specific special-case mapping in layout.
- Cargo package name should be represented on the Crate node (`cargo_name`) and consumed by layout metadata.
- Use emission should consume canonical path directly; no normalize/filter helper in projection.

### 4) Write it to a state file
- This file is the overwritten cycle snapshot.

### 5) Sort structural and categorical patterns
- Structural pattern A: unresolved types at emit boundary are hard projection errors.
- Structural pattern B: cargo package naming is data on `Crate`, not inferred in layout.
- Structural pattern C: use paths are emitted as stored canonical paths without projection-time normalization.
- Categorical pattern A: schema extension (`Crate.cargo_name`).
- Categorical pattern B: projection heuristic removal.

### 6) Write it to state file
- The patterns above define this cycle's acceptance criteria.

### 7) Solve the state file
- `canon/src/node.rs`
  - added `cargo_name: Option<NameId>` to `CanonNodeKind::Crate` (`serde(default)`).
- `canon-capture/src/canon_assemble.rs`
  - sets `Crate.cargo_name` during assembly (`name_with_underscores` -> hyphenated cargo package name).
- `canon-projection/src/emit/types.rs`
  - changed `TypeKind::Unresolved` rendering to fail-fast panic with path context.
- `canon-projection/src/layout/mod.rs`
  - removed `render_dependency_entry` special-case function.
  - dependency lines now emit directly as `"{dep} = \"*\""` from canonical roots.
  - `crate_meta` now prefers `Crate.cargo_name` and falls back to `name_id`.
- `canon-projection/src/emit/fmt.rs`
  - removed `normalize_use_path`.
- `canon-projection/src/emit/items.rs`
  - removed use of `normalize_use_path`.
  - removed single-segment uppercase filtering heuristic; use paths are emitted directly.

### 8) Emit and project the solution incrementally
- Validation:
  - `cargo check` passed across full workspace after projection/layout cleanup.

### 9) Repeat step 3
- Post-change fact breakdown:
  - projection no longer normalizes/filters use paths.
  - layout no longer contains crate-specific dependency mapping heuristics.
  - unresolved type emission now trips a hard projection error boundary.
  - crate cargo package naming is now structurally represented.
- Next pending slice:
  - continue Phase 3.5 body structuring to further reduce `CfgOp::Raw`,
  - run representative fixture executions to validate runtime behavior after projection hardening.
