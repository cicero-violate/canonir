# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CANON_BODY_STRUCTURAL_PRIMARY_V1`
- date: `2026-02-27`
- mode: `execution`
- invariant: `No heuristics. Structural invariants only.`

### 1) Investigate the problem
- Requested continuation after initial commit.
- Target: complete elimination of remaining raw body/op surfaces and keep pipeline green.

### 2) Gather facts
- Residual raw surfaces were still present in type/model definitions:
  - `CfgOp::Raw`,
  - capture `Body::Raw`,
  - capture `Stmt::Raw`.
- Fixture validation had to remain green (`repomap`, `test_1`).

### 3) Break down the facts
- True structural completion requires removing residual raw variants, not only bypassing them in execution.
- Projection and analyzer must compile without raw branches.

### 4) Write it to a state file
- File overwritten in canonical numbered format for `AGENT_STATE_SECTION_DELTAS.sh`.

### 5) Sort structural and categorical patterns
- Pattern A: raw variants are schema-level escape hatches and violate structural-only objective.
- Pattern B: safe completion needs variant removal + compile + fixture-sweep confirmation.

### 6) Write it to state file
- Implemented:
  - removed `CfgOp::Raw` from Canon node schema,
  - removed capture `Body::Raw` and capture `Stmt::Raw` variants,
  - removed raw match branches in projection/analyzer/capture assembly.

### 7) Solve the state file
- Raw body/op variant surfaces are now removed from active type/model path.
- Structured MIR-body primary flow remains active.

### 8) Emit and project the solution incrementally
- Validation:
  - workspace `cargo check`: pass.
  - `repomap`: capture -> orchestration -> emitted `cargo build`: pass.
  - `test_1`: capture -> orchestration -> emitted `cargo build`: pass.

### 9) Repeat step 3
- No pending items remain under current plan id.
