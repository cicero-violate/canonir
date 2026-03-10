# Agent Goal

Fix the `orchestration` pipeline so `cargo run --bin orchestration -- --all` completes with zero build errors in emitted files. Try again

## Constraints
- You may modify code under `/workspace/ai_sandbox/canon/canon-capture`, `/workspace/ai_sandbox/canon/canon-projection`, `/workspace/ai_sandbox/canon/canon`
- Emitted sources are under `/workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/repomap/src/`

## Required approach
First node in the graph must run `cargo run --bin orchestration -- --all` and capture the actual build errors.
Second node must read the failing emitted files directly.
All subsequent nodes must be concrete file edits (write_file or apply_patch deltas) that fix specific errors found in step 1 and 2.
Do not generate analysis-only nodes. Every node must either read a specific file or write a specific fix.
The graph must contain at least 3 write_file or apply_patch nodes in the first 6 nodes.
