# Agent State

## 2026-02-27 — Current Cycle (Phase 3 body/path structuralization)

### 1) Investigate the problem
- `canon_assemble` still synthesized `PathRef` by scanning raw body text (`extract_external_paths`), which is heuristic and outside structural boundaries.
- Plan requires structural-only path reference emission.

### 2) Gather facts
- `project/body.rs` already traverses MIR and has structural `DefId` references for calls/const dependencies.
- `project_def` can already merge projected nodes and edges from sub-projections.
- `dep_solver` reads `PathRef` nodes structurally; it does not require text source scans.

### 3) Break down the facts
- Extend body projection to emit `PathRef` nodes from MIR-referenced `DefId` paths.
- Add containment edges from function/method node to emitted `PathRef` node.
- Remove assemble-time raw-text `PathRef` synthesis and helper functions.

### 4) Write it to a state file
- This file is the overwritten cycle snapshot.

### 5) Sort structural and categorical patterns
- Structural pattern A: body-level external references come from MIR `DefId` traversal.
- Structural pattern B: `PathRef` node creation occurs in project phase, not assemble repair.
- Categorical pattern A: delete heuristic text extraction surface from capture pipeline.

### 6) Write it to state file
- Acceptance criteria for this cycle:
  - no `extract_external_paths` body text scanner in assemble,
  - `project_body` emits structural `PathRef` nodes,
  - pipeline validates on baseline projects.

### 7) Solve the state file
- `canon-capture/src/project/body.rs`
  - changed `project_body` return to `(Vec<Node>, Vec<EdgeHint>)`.
  - emits structural `PathRef` nodes from MIR call/const `DefId` references.
  - emits `Contains` edge from body owner node to each emitted `PathRef`.
- `canon-capture/src/project/mod.rs`
  - integrated body-projected nodes and edges into `Partial`.
- `canon-capture/src/canon_assemble.rs`
  - removed raw-body `PathRef` synthesis block from `assemble_model_like`.
  - removed `extract_external_paths` and `is_crate_root_ident` helpers.

### 8) Emit and project the solution incrementally
- Validation:
  - `cargo check` passed.
  - capture + orchestration passed for:
    - `test_projects/test_rust_projects/capture/test_1`
    - `test_projects/test_rust_projects/capture/repomap`

### 9) Repeat step 3
- Post-change fact breakdown:
  - `PathRef` is now emitted structurally from MIR traversal.
  - assemble no longer scans body text for path extraction.
- Next pending slice:
  - continue remaining Phase 3/4 structural cleanup surfaces (body CFG op structuralization and any remaining compensation boundaries).
