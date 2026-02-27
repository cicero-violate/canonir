# Agent State

## 2026-02-27 — Current Cycle (P2 Verification/Cleanup, End-to-End)

### 1) Investigate the problem
- Continue project with P2 verification and fix blockers discovered in representative fixtures.
- Constraint respected: no new tests added.

### 2) Gather facts
- `test_1` passed capture -> orchestration -> emit build.
- `repomap` initially failed with multiple blockers:
  - dependency noise (`Node`, `Path`, `PathBuf`, `Parser`),
  - package-name mismatches (`tree_sitter`, `tree_sitter_rust`),
  - stale capture-output behavior risk,
  - wrong module file layout under `src/main/`,
  - private/use-target injection issue (`use crate::repomap::Path`),
  - type normalization collision producing `crate::FileMap` when module name matched crate name,
  - false-positive dependency extraction (`sum`).

### 3) Break down the facts
- These failures span solver, projection layout, and capture normalization boundaries.
- Fix sequence needed:
  1. dependency extraction correctness,
  2. Cargo dependency package mapping,
  3. module layout correctness,
  4. use injection visibility guard,
  5. crate-prefix normalization safety when `module == crate`.

### 4) Write it to a state file
- This file is the current, overwritten state snapshot.

### 5) Sort structural and categorical patterns
- Structural pattern A: dependency roots should come from crate-like identifiers only.
- Structural pattern B: crate-root identifiers and package names may differ and need explicit mapping.
- Structural pattern C: `src/main.rs` is a root module file (children go in `src/`, not `src/main/`).
- Structural pattern D: cross-module import injection must not source from private `Use` aliases.
- Structural pattern E: local-crate normalization must not collapse single-segment module paths when `module == crate`.

### 6) Write it to state file
- Patterns above used as acceptance checks for all patches in this cycle.

### 7) Solve the state file
- `canon-analyzer/src/solver/dep_solver.rs`
  - added `is_probable_crate_name` filtering,
  - excluded local module roots,
  - added fallback extraction from raw code snippets (`name_intern`) for explicit `foo::bar` paths,
  - tightened fallback extraction to avoid false positives like `sum::`.
- `canon-projection/src/layout/mod.rs`
  - dependency rendering now maps:
    - `tree_sitter` -> package `tree-sitter`,
    - `tree_sitter_rust` -> package `tree-sitter-rust`.
  - fixed `module_stem_dir` so `main.rs` behaves like `lib.rs`/`mod.rs` for child module placement.
- `canon-analyzer/src/solver/use_solver.rs`
  - skip cross-module injections when resolved def is a private `Use` alias.
- `canon-capture/src/norm.rs`
  - `ty_strip_local` now rewrites crate prefixes conservatively,
  - `local_crate_path` now avoids collapsing single-segment `crate_name::Type` forms.
- `run_capture.sh`
  - restored `rm -f "$OUTPUT_JSON"` to ensure fresh capture output.
- cleanup
  - removed debug print in `canon-capture/src/canon_assemble.rs`,
  - removed stale solver comment in `canon-analyzer/src/solver/mod.rs`.

### 8) Emit and project the solution incrementally
- Verification commands executed successfully:
  - `cargo check -p canon-capture -p canon-analyzer -p canon-projection -p orchestration`
  - `test_1`: capture -> orchestration -> emitted crate build (pass)
  - `repomap`: capture -> orchestration -> emitted crate build (pass)

### 9) Repeat step 3
- Post-change fact breakdown:
  - dependency and package-name blockers are resolved,
  - module file layout for binary root modules is resolved,
  - private alias injection regression is resolved,
  - crate/module name collision normalization is resolved for this fixture,
  - representative fixtures now compile end-to-end in the current pipeline.
