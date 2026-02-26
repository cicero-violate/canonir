Current status (updated):

Recently closed gaps:
- Capture -> IR:
  - Use-node loss fixed (capture now preserves imports like `crate::symbol::Symbol`, `std::path::Path`).
  - Global Use dedup in capture assembly removed (no cross-file import dropping).
  - Module visibility captured into IR (`NodeKind::Module.vis`).
  - ModelIR now carries `cargo_dependencies`.
- IR -> Emit:
  - Synthetic generic inference removed from layout sanitize pass (no more `fn ...<Node>(...)` regressions).
  - Module emission now visibility-aware (`mod` vs `pub mod`), with sane default for lib/module trees.
  - Import injection now de-duplicates `use` lines and injects only targeted fallbacks.
  - Cargo emitter now writes dependency entries (captured list or conservative fallback inference).
  - Type normalization/qualification fixes:
    - `std::Path`/`std::PathBuf` -> `std::path::Path`/`std::path::PathBuf`
    - local paths like `data::model::User` -> `crate::data::model::User` in emitted type positions.

Verified:
- `capture/test_1/capture.json -> emit/test_1` now compiles clean (`cargo build` in emitted project succeeds).

Known remaining issues / follow-up:
1. Capture local type-path fidelity is still imperfect in some cases
   - Emitter currently applies local path qualification fallback.
   - Long-term fix should happen in capture normalization/projection so emit stays purely render-only.
2. Cargo dependency capture source reliability
   - Captured `[dependencies]` currently depends on wrapper environment (`CARGO_MANIFEST_DIR`) and falls back to inference.
   - Improve by attaching manifest path explicitly from wrapper/orchestration context.
3. Solver diagnostics quality
   - `impl_solver` can warn on targets that invariant solver also resolves in some runs; investigate classifier consistency.
4. Parity quality target
   - Compile parity is now strong for test cases, but textual parity still differs in ordering/normalization in some outputs.

Key files touched for these fixes:
- `capture/src/project/item.rs`
- `capture/src/assemble.rs`
- `capture/src/norm.rs`
- `capture/src/lib.rs`
- `model/src/ir/node.rs`
- `model/src/ir/node_de.rs`
- `model/src/ir/model_ir.rs`
- `projection/src/layout/mod.rs`
- `projection/src/layout/skeleton.rs`
- `projection/src/layout/passes/sanitize_generics.rs`
- `projection/src/layout/passes/inject_imports.rs`
- `projection/src/emit/items.rs`
- `projection/src/emit/fmt.rs`
- `projection/src/emit/functions.rs`
- `projection/src/emit/types.rs`
- `projection/src/emit/cargo.rs`
