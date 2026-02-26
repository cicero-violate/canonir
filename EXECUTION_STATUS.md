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
