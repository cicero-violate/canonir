# Agent Goal

Diagnose and fix **one** concrete build error in emitted sources so that the next run shows **fewer** errors.

Primary command:
```
cargo run --bin orchestration -- --all
```

## Constraints
- You may modify code under:
  - `/workspace/ai_sandbox/canon/canon-capture`
  - `/workspace/ai_sandbox/canon/canon-projection`
  - `/workspace/ai_sandbox/canon/canon`
- Do not edit files outside those roots.
- Focus on `emit/repomap/src/*.rs` errors first.

## Required execution order
1. Run the orchestration command and capture diagnostics.
2. Read at least one failing emitted file directly.
3. Apply a single targeted fix (write_file or apply_patch) that addresses a specific diagnostic.
4. Re-run orchestration and confirm the **error count decreases**.

## Structural requirements
- The first node must be the orchestration command.
- The second node must read a failing emitted file.
- The third node must be a mutate node (apply_patch or write_file) tied to a specific error message.
