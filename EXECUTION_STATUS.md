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

- Phase 5 — **Completed**
  - Changed `visibility_solver::solve` signature to `&mut CanonIR`.
  - Added Repair 1: modules at crate root with no visibility flags → set `PUB`.
  - Added Repair 2: `Fn` nodes inside trait impls → strip `PUB | PUB_CRATE | PUB_SUPER`.
  - Deleted visibility override (`if flags == 0 { "pub " }`) from `emit_module` in `items.rs` and `impls.rs`.

- Phase 4 — **Blocked** on g2 (Use node target field) and g3 fully active. h1 path injection still in `file.rs`.
- Phase 6 — **Blocked** on g1 (Crate node missing `dependencies` field).

## IR Fixes (2026-02-26)

- g1 — **Completed**
  - Added `dependencies: Vec<PathId>` (serde default `vec![]`) to `CanonNodeKind::Crate` in `node.rs`.
  - Updated `canon_assemble.rs` Crate constructor to emit `dependencies: vec![]`.
  - `build_plan` in `layout/mod.rs` now reads `Crate.dependencies` directly.
  - Deleted `infer_dependencies` and `roots_from_text` from `layout/mod.rs`.
  - Unblocks Phase 6.

- g2 — **Completed**
  - Added `target: Option<CanonId>` (serde default `None`) to `CanonNodeKind::Use` in `node.rs`.
  - Updated all Use constructors (`canon_assemble.rs`, `use_solver.rs`) to emit `target: None`.
  - Updated all Use destructuring patterns (`items.rs`, `use_solver.rs`) with `..`.
  - Unblocks Phase 4.

- Phase 4 — **Unblocked** — g2 resolved, g3 active. h1 path injection remains in `file.rs`; ready for use_solver injection work.
- Phase 6 — **Unblocked** — g1 resolved. Ready to add `dep_solver.rs` and wire into solver chain.

---


