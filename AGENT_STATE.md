# Agent State

## 2026-02-27 — Current Cycle (Continue Plan: Phase 4.1 + 4.3)

### 1) Investigate the problem
- Continue Phase 4 compensation removal after completing dep-solver fallback removal.
- Targets this cycle:
  - Phase 4.1: remove synthetic `Use` injection from `use_solver`.
  - Phase 4.3: remove visibility repair blocks from `visibility_solver`.

### 2) Gather facts
- `use_solver` still injected new `Use` nodes (`ir.push_node(...)`) as compensation.
- `use_solver` only consumed `Resolves` edges and did not include `Reexports`.
- `visibility_solver` still mutated IR via two repairs:
  - set `PUB` on root modules with missing visibility flags
  - stripped visibility flags from trait-impl functions

### 3) Break down the facts
- Per plan, solver stage must derive/validate only, not inject or rewrite structure.
- `use_solver` should set `Use.target` from structural edges and surface unresolved uses as diagnostics.
- `visibility_solver` should validate visibility only and emit warnings without mutating nodes.

### 4) Write it to a state file
- This file is the overwritten cycle snapshot.

### 5) Sort structural and categorical patterns
- Structural pattern A: `use_solver` consumes `Resolves` and `Reexports` edges only.
- Structural pattern B: unresolved use-sites are diagnostics, not auto-injected nodes.
- Structural pattern C: `visibility_solver` remains read-only.
- Categorical pattern A: remove solver-side structural injection.
- Categorical pattern B: remove solver-side visibility repair.

### 6) Write it to state file
- The patterns above define this cycle's acceptance criteria.

### 7) Solve the state file
- `canon-analyzer/src/solver/use_solver.rs`
  - removed synthetic `Use` injection logic and related helper code.
  - now consumes both `EdgeKind::Resolves` and `EdgeKind::Reexports` to derive use targets.
  - keeps structural dedup pass and emits warnings for unresolved use-sites instead of mutating graph/nodes.
- `canon-analyzer/src/solver/visibility_solver.rs`
  - removed repair block that forced `PUB` on root modules.
  - removed repair block that stripped visibility flags from trait-impl functions.
  - solver now performs validation/warning emission only.

### 8) Emit and project the solution incrementally
- Validation:
  - `cargo check` passed after use/visibility solver changes.

### 9) Repeat step 3
- Post-change fact breakdown:
  - `use_solver` no longer injects nodes and now treats missing resolution as diagnostic output.
  - `visibility_solver` no longer repairs capture output.
- Next pending slice:
  - continue Phase 4 with `impl_solver`/`trait_solver` graph-path validation (`ImplRef` via `G_type`),
  - continue Phase 3.5 body structuring to further reduce `CfgOp::Raw`.
