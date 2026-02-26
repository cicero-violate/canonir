## Execution Status (2026-02-26)

- Phase 1 — **Completed**
  - Removed emitter crate-name rewrite (`normalize_crate_path`); made `normalize_use_path` a no-op in `fmt.rs`.
  - Added capture/assemble-time local crate path normalization in `canon_assemble.rs` via `norm.rs`.
  - Verified on `test_1` and `repomap`.

- Phase 2 — **Completed**
  - Stripped project-specific hardcodes (`data::`, `traits::`, `Symbol`) from `norm::norm_path`; local-module prefix logic lives exclusively in `canon_assemble::local_module_roots`.
  - Added structured parsing for `&T`, `&mut T`, `dyn T`, `impl T` in `str_to_type_kind` (g5 minimum viable fix).
  - Deleted `normalize_extern_path` from `fmt.rs`; `render_type_kind` now emits intern'd path directly.

- Phase 3 — **Completed**
  - Added dedup pass at top of `use_solver::solve`: collapses structural duplicate Use nodes per parent module before injection.
  - Wired `use_solver` and `name_solver` into `solver/mod.rs` after `module_solver`.
  - Deleted `emitted_uses` HashSet and `normalize_use_path` import from `file.rs`.

- Phase 4 — **Completed**
  - Deleted all four string-scan inject blocks from `emit_file` in `file.rs` (h1 eliminated).
  - Removed: `Path::` / `PathBuf` / `symbol::` / `repomap::FileMap` heuristic injections.
  - Import coverage is now the responsibility of `use_solver` via `Resolves` edges in the name graph.

- Phase 5 — **Completed**
  - Changed `visibility_solver::solve` signature to `&mut CanonIR`.
  - Added Repair 1: modules at crate root with no visibility flags → set `PUB`.
  - Added Repair 2: `Fn` nodes inside trait impls → strip `PUB | PUB_CRATE | PUB_SUPER`.
  - Deleted visibility override (`if flags == 0 { "pub " }`) from `emit_module` in `items.rs` and `impls.rs`.

- Phase 6 — **Completed**
  - Added `canon-analyzer/src/solver/dep_solver.rs`: scans Use nodes, extracts external crate roots, deduplicates, interns as `PathId`, writes into `Crate.dependencies`.
  - Wired `dep_solver::solve` into `solver/mod.rs` after `name_solver`.
  - `layout/mod.rs` already reads `Crate.dependencies` directly (g1); no further layout changes needed.

## IR Fixes (2026-02-26)

- g1 — **Completed**
  - Added `dependencies: Vec<PathId>` (serde default `vec![]`) to `CanonNodeKind::Crate` in `node.rs`.
  - Updated `canon_assemble.rs` Crate constructor to emit `dependencies: vec![]`.
  - `build_plan` in `layout/mod.rs` now reads `Crate.dependencies` directly.
  - Deleted `infer_dependencies` and `roots_from_text` from `layout/mod.rs`.

- g2 — **Completed**
  - Added `target: Option<CanonId>` (serde default `None`) to `CanonNodeKind::Use` in `node.rs`.
  - Updated all Use constructors (`canon_assemble.rs`, `use_solver.rs`) to emit `target: None`.
  - Updated all Use destructuring patterns (`items.rs`, `use_solver.rs`) with `..`.

- g3 — **Completed**
  - `use_solver` and `name_solver` wired into `solver/mod.rs` after `module_solver`.

- g4 — **Completed**
  - Stripped `data::`, `traits::`, `Symbol` hardcodes from `norm::norm_path`.
  - Local-module prefix logic lives exclusively in `canon_assemble::local_module_roots`.

- g5 — **Completed (minimum viable)**
  - Structured parsing for `&T`, `&mut T`, `dyn T`, `impl T` added to `str_to_type_kind`.
  - Full recursive type parsing remains a longer-term goal.

- g6 — **Completed**
  - `visibility_solver::solve` signature changed to `&mut CanonIR`; actively repairs IR flags.

---

## All Six Phases — Done

Remaining gaps before success condition is fully met:
- `use_solver` injection covers local `Resolves` edges; `std::path::Path` / `PathBuf` injection
  now depends on capture emitting `Resolves` edges for those types (no emitter fallback remains).
- `Use.target` field exists but is not yet populated by `use_solver` injection (set to `None`);
  full target resolution is a follow-on task.
- g5 full structured type parsing (generics, tuples, arrays) remains future work.
