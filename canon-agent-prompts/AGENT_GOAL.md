# Agent Goal

Fix the `orchestration` pipeline so:

```
cargo run --bin orchestration -- --all
```

completes with zero build errors in emitted files (focus on `emit/repomap/src/*.rs` first).

## Constraints
- You may modify code under:
  - `/workspace/ai_sandbox/canon/canon-capture`
  - `/workspace/ai_sandbox/canon/canon-projection`
  - `/workspace/ai_sandbox/canon/canon`
- Do not edit files outside those roots.

## Required execution order
1. Run `cargo run --bin orchestration -- --all` and capture diagnostics.
2. Read failing emitted files directly.
3. Apply targeted fixes (write_file or apply_patch) to eliminate the reported errors.

## Structural requirements
- At least 3 mutate nodes (apply_patch or write_file) must appear in the first 6 nodes.
- Every node must either read a specific file or write a specific fix.
- Avoid analysis-only nodes without concrete I/O.
