# PROJECT_STATUS.md

## Current State

- Workspace builds.
- Pipeline is Canon-only end-to-end.
- Legacy `model/`, `capture/`, `analyzer/`, `projection/`, and `mutation/` crates are removed.
- Canon ownership includes:
  - Node/edge graph identity (`NodeId`, `EdgeKind`, `CsrGraph`)
  - IR runtime (`CanonIR`)
  - analysis, projection, mutation, and orchestration.

## Working Paths

- `run_capture.sh` captures `canon_capture.json` from rustc frontend.
- `orchestration` accepts CanonIR JSON only.
- Emission validated on:
  - `test_projects/test_rust_projects/capture/test_1`
  - `test_projects/test_rust_projects/capture/repomap`

## Known Warnings

- `canon-analyzer` still emits non-fatal diagnostics in some test cases
  (notably `impl_solver`/provenance-style warnings).

## Next Work

1. Add regression tests for capture/analyze/emit on `test_1` and `repomap`.
2. Improve analyzer warning precision.
3. Tighten emitter fidelity around module shaping and import minimization.

System invariant:
`Capture -> CanonIR -> Graph -> Solve -> Emit` remains stable.
