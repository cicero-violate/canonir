# PROJECT_STATUS.md

## Current State

- Workspace builds.
- Model pipeline remains available as legacy orchestration mode (`--model`).
- Canon pipeline is integrated end-to-end as default orchestration mode:
  - `ModelIR -> seal -> CanonIR -> canon_analyze -> canon_projection -> Rust source`
- Canon emitter is split into multi-file architecture matching `projection/src/emit` layout.
- Canon output compile parity verified on:
  - `test_projects/test_rust_projects/capture/test_1`
  - `test_projects/test_rust_projects/capture/repomap`
- Canon `Cargo.toml` emission now includes dependencies and `[workspace]` isolation.

## What Is Working

- 8 CSR graphs wired through both Model and Canon pipelines.
- Model solver stack remains active (S9/S11/S12/S13/S15/S16 included).
- Canon graph derivation is no longer a no-op:
  - all 8 builders derive structural edges and union with sealed hint edges.
- Canon seal enrichment now populates composite payload required for emit fidelity:
  - struct fields/generics/derives
  - enum variants/generics/derives
  - trait methods/generics
  - fn/method/typealias/impl generics
- Canon emitter improvements:
  - structural type rendering (`TypeKind`)
  - enum named-variant emission preserved (`Variant { field: Ty }`)
  - path normalization fixes for `Path`/`PathBuf`, local module paths, dyn trait paths
  - fallback imports for `Path`/`PathBuf` when raw bodies reference them
  - use-dedup in file emission
- Orchestration behavior is now explicit and non-confusing:
  - no dual emit in one run
  - default runs Canon pipeline
  - `--model` runs legacy Model pipeline

## Known Non-Blocking Warnings

- Canon `impl_solver` still warns on some `Impl.for_ty` targets represented as canonical type nodes.
- Canon provenance warnings remain noisy on some symbol shadow cases.

## Next Highest Value

1. Reduce Canon analyzer warning noise (`impl_solver` + provenance) without masking true issues.
2. Improve capture/seal type fidelity to reduce normalization heuristics further.
3. Add golden parity tests for Canon emit on `test_1` and `repomap` (compile + semantic diff expectations).
4. Add focused unit tests for Canon graph builders and Canon emitter modules.
5. Evaluate Canon mutation/verify path parity with Model mutation pipeline.

System invariant:
IR -> Graph -> Solve -> Emit remains stable for both pipelines.
