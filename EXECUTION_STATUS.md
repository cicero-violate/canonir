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
- `Use.target` field exists but is not yet populated by `use_solver` injection (set to `None`);
  full target resolution is a follow-on task.
- g5 full structured type parsing (generics, tuples, arrays) remains future work.

---

## Post-Phase Work (2026-02-26)

### Resolves edge semantics fix — **Completed**

**Problem:** `EdgeKind::Resolves` had two colliding semantics:
- Capture emitted `impl → trait` as `Resolves` (routed into `name_edges`)
- `use_solver` read `name_graph` for `Resolves` expecting `use-site → definition` pairs
- Impls and their traits always share a module → `site_mod == def_mod` guard fired → injection was a permanent no-op

**Fix:**
- Added `EdgeKind::ImplRef` to both `canon/src/edge.rs` and `canon-capture/src/types.rs`
- `project_relations` now emits `ImplRef` (not `Resolves`) for `impl → trait`
- `canon_assemble.rs` maps `ModelEdgeKind::ImplRef → CanonEdgeKind::ImplRef`, routed into `name_edges`
- `canon-mutation/src/apply.rs` `graph_slot` handles `ImplRef` → `"name"`
- `project_item` signature changed to `(Option<Node>, Vec<EdgeHint>)`: the `DefKind::Use` branch
  now emits one `Resolves` edge per `Res::Def` in `use_path.res` (use-node → resolved definition)
- `project_def` in `mod.rs` destructures the tuple and extends `partial.edge_hints` with item edges

**Effect:** `use_solver` now receives genuine use-site `Resolves` edges from capture.
Import injection fires correctly for locally-defined types referenced across modules.

### Next steps
- Populate `Use.target: Option<CanonId>` in `use_solver` injection (currently always `None`)
- g5 full structured type parsing: generics `Vec<T>`, tuples, arrays

---

### use_solver + path normalization correctness — **Completed** (2026-02-26)

**Verified:** test_1 pipeline compiles clean end-to-end.

Fixes applied:

1. **use_solver injection prefix** — injected paths used `crate_name` (e.g. `test_rust_project::traits::Describable`) instead of `"crate"`. Fixed to always use `"crate"` prefix for local definitions.

2. **Duplicate Use injection (E0252)** — `use_solver` injected Use nodes that duplicated existing ones. Fixed by pre-populating the `seen` set with all existing Use node paths per module before the injection loop.

3. **Module target path (E0432 `::model::model`)** — when `def_idx` is a Module node, `node_display_name` returned the full path and injection appended it again as `def_name`. Fixed: when `def_idx` is Module, use its path directly.

4. **name_solver renames definition nodes** — `apply_rename` was renaming TypeAlias/Struct/etc nodes via `Renames` edges from `use X as R`, causing `Result1231` → `R`. Fixed: `apply_rename` now only updates `Use::alias`, never definition node names.

5. **Path normalization double-application** — `local_module_roots` replacements in `canon_assemble` added `crate::` prefix inside already-prefixed paths (`Box<dyn traits::` → `Box<dyn crate::traits::`). Added coverage for `Box<dyn`, `, `, `(` prefix patterns.

6. **Stale capture JSON** — `rustc_capture` skips writing if output file exists; `run_capture.sh` was not deleting the output before capture, causing rebuilt binaries to appear ineffective. Fixed: `run_capture.sh` now deletes output JSON before invoking cargo build.
