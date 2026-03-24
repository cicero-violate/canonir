# Implementation Plan: Fix `cargo test --workspace` Failures

## Status

`cargo check --workspace` passes clean. `cargo test --workspace` has compile failures
across 4 packages. All are pre-existing issues unrelated to the must_emit work.

| # | Package | Error | Fix |
|---|---|---|---|
| 1 | `canon-runtime-supervisor`, `canon-analyst`, `canon-tools-editor` (bin), `canon-storage-eventlog` | `allow(dead_code) incompatible with previous forbid` | Add `test = false` to each `[[bin]]` entry |
| 2 | `canon-tools-editor` (test `project_editor_tests`) | `cannot find module or crate project_editor` × 4 | Fix test imports: `project_editor::` → `canon_editor::` |
| 3 | `algorithms` (example `gpu_example`) | `variable does not need to be mutable` × 2 | Remove `mut` on lines 223 and 251 |

Run order: 1 → 2 → 3 → `cargo test --workspace` to verify.

---

## Task 1 — Add `test = false` to binary targets in failing crates

**Root cause:** When `cargo test` compiles a binary with `--test`, the Rust test harness
wraps the user's `main` function in `#[allow(dead_code)]` (because `main` is unused in
the test binary). This `#[allow(dead_code)]` conflicts with `-Fdead-code` (forbid), which
cannot be overridden by any allow — including one emitted by the compiler itself.
The error has no `-->` file location because the offending attribute is injected by the
test harness, not written in any source file.

The fix is `test = false` on each binary target. This tells cargo not to build a test
binary from that entry, so the harness injection never occurs. The binary itself still
compiles and runs normally; it just won't be included in `cargo test` runs.

---

### 1a — `canon-utils/canon-runtime-supervisor/Cargo.toml`

Current:
```toml
[[bin]]
name = "canon-runtime-supervisor"
path = "src/bin/supervisor.rs"
```

Replace with:
```toml
[[bin]]
name = "canon-runtime-supervisor"
path = "src/bin/supervisor.rs"
test = false
```

---

### 1b — `canon-utils/canon-analyst/Cargo.toml`

Current:
```toml
[[bin]]
name = "canon-analyst"
path = "src/main.rs"
```

Replace with:
```toml
[[bin]]
name = "canon-analyst"
path = "src/main.rs"
test = false
```

---

### 1c — `canon-utils/canon-tools-editor/Cargo.toml`

Current:
```toml
[[bin]]
name = "editor_capability_smoke_test"
path = "src/bin/capability_smoke_test.rs"
```

Replace with:
```toml
[[bin]]
name = "editor_capability_smoke_test"
path = "src/bin/capability_smoke_test.rs"
test = false
```

---

### 1d — `canon-utils/canon-storage-eventlog/Cargo.toml`

This crate has two binaries auto-discovered from `src/bin/` with no explicit `[[bin]]`
entries. Add explicit entries with `test = false` for both:

Add at the end of the file:
```toml
[[bin]]
name = "read_tlog"
path = "src/bin/read_tlog.rs"
test = false

[[bin]]
name = "verify_tlog_equivalence"
path = "src/bin/verify_tlog_equivalence.rs"
test = false
```

---

## Task 2 — Fix test crate name: `project_editor` → `canon_editor`

**File:** `canon-utils/canon-tools-editor/tests/project_editor_tests.rs`

**Root cause:** `Cargo.toml` declares `name = "canon_editor"` for the lib target.
The test file uses `project_editor` as the crate name, which does not exist.

Replace lines 1–4:
```rust
// Before:
use project_editor::edit::ProjectEditor;
use project_editor::structured::FieldMutation;
use project_editor::symbol_index::SymbolIndex;
use project_editor::verify::verify_renames_applied;

// After:
use canon_editor::edit::ProjectEditor;
use canon_editor::structured::FieldMutation;
use canon_editor::symbol_index::SymbolIndex;
use canon_editor::verify::verify_renames_applied;
```

---

## Task 3 — Fix unused `mut` in `algorithms/examples/gpu_example.rs`

Two variables declared `mut` are never mutated:

**Line 223:**
```rust
// Before:
let mut pred_adj: Vec<Vec<usize>> = vec![vec![], vec![0], vec![0], vec![1, 2]];
// After:
let pred_adj: Vec<Vec<usize>> = vec![vec![], vec![0], vec![0], vec![1, 2]];
```

**Line 251:**
```rust
// Before:
let mut kill = vec![0u64; block_count * words];
// After:
let kill = vec![0u64; block_count * words];
```

---

## Final verification

```
cargo test --workspace
```

Must pass with zero `error[E...]` lines.
