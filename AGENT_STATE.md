# Agent State

## 2026-02-27 — Current Cycle (PLAN_v2 structural implementation)

### 1) Investigate the problem
- User-required constraint: no heuristics, no fallback, no regression.
- Targets this cycle:
  - implement `/workspace/ai_sandbox/canon/PLAN_v2.md` structural gaps A/B/C/D/E.

### 2) Gather facts
- Grouped `use` was encoded as brace-literal path text (invalid path semantics).
- `Use::target` was capture-empty and filled later by solver.
- Raw/macro body text still flowed through `name_intern` consumers in emission/analysis.
- `path_intern` had no validity contract and accepted non-path text.
- Dependency derivation needed to stay structural after grouped-use cleanup.

### 3) Break down the facts
- Capture must emit one `Use` node per resolved import item.
- Grouped use expansion must happen at capture-time (not in solver).
- `Use` relations (`Resolves`, `Reexports`) must be emitted at capture-time per expanded node.
- `intern_path` must reject malformed path strings (brace-literals/body fragments).
- Raw body/macro text must use `body_intern` and be read via `lookup_body`.

### 4) Write it to a state file
- This file is the overwritten cycle snapshot.

### 5) Sort structural and categorical patterns
- Structural pattern A: `Use` node paths are always single resolved canonical paths (no braces).
- Structural pattern B: `Use` edges are capture-derived and explicit (`Resolves`, `Reexports`, `Contains`).
- Structural pattern C: body text is isolated in `body_intern`.
- Structural pattern D: `dep_solver` consumes structural roots (Use + PathRef) after capture filtering.
- Categorical pattern A: capture API change to emit multiple nodes per DefId.
- Categorical pattern B: path contract enforcement.
- Categorical pattern C: runtime fixture verification.

### 6) Write it to state file
- The patterns above define this cycle's acceptance criteria.

### 7) Solve the state file
- `canon-capture/src/project/item.rs`
  - changed `project_item` to return `Vec<Node>` (multi-node emission).
  - `DefKind::Use` now expands each `Res::Def` into a dedicated `NodeKind::Use` with canonical `path`.
  - emits per-node `Contains`, `Resolves`, and `Reexports` edges.
  - assigns deterministic synthetic IDs for additional grouped-use entries.
- `canon-capture/src/project/mod.rs`
  - adapted to collect multi-node `project_item` output.
  - skips duplicate relation pass for `DefKind::Use` (use relations now emitted in capture item pass).
- `canon/src/ir.rs`
  - strengthened `intern_path` normalization contract to reject malformed entries (`{}`, `=>`, `!`, invalid colon forms).
- `canon-capture/src/canon_assemble.rs`
  - moved `CfgOp::Raw` and `MacroCall.tokens_id` to `body_intern` via `canon.intern_body(...)`.
  - tightened external `PathRef` extraction to crate-root lexical form (`snake_case` crate id roots).
  - kept body emission regression-free by preserving whole raw body op (no statement splitting).
- `canon-projection/src/emit/body.rs` and `canon-projection/src/emit/macros.rs`
  - switched raw/macro text lookup from `lookup_name` to `lookup_body`.
- `canon-analyzer/src/solver/exhaustiveness_solver.rs`
  - switched raw CFG text lookup to `lookup_body`.
- `canon-analyzer/src/solver/dep_solver.rs`
  - derives dependencies from structural `Use` + filtered `PathRef` roots.
- `canon-projection/src/layout/mod.rs`
  - dependency rendering now uses structural cargo package mapping rule:
    - crate-id key (`tree_sitter`) with package name transform (`tree-sitter`) when `_` exists.

### 8) Emit and project the solution incrementally
- Validation:
  - `cargo check` passed for full workspace.
  - `test_1` fixture passed end-to-end:
    - capture -> orchestration -> emitted crate `cargo build`.
  - `repomap` fixture passed end-to-end:
    - capture -> orchestration -> emitted crate `cargo build`.

### 9) Repeat step 3
- Post-change fact breakdown:
  - grouped use is now structurally expanded at capture boundary.
  - malformed path strings are blocked at `intern_path`.
  - body text no longer aliases name/path lookups.
  - dependency derivation is structural and no longer emits symbol/type false positives.
- Next pending slice:
  - continue PLAN phase work on body structural lowering without regression,
  - continue reducing remaining solver warnings through upstream capture completeness.
