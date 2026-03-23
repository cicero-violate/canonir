# Migration Plan: canon_rustc → canon/canon-rustc

Move the rustc wrapper from its sibling directory into the canon repo as an isolated
sub-workspace. The canon main workspace stays on stable; `canon-rustc` stays on nightly.

---

## Overview

| Item              | Before                                    | After                                        |
|-------------------|-------------------------------------------|----------------------------------------------|
| Directory         | `/workspace/ai_sandbox/canon_rustc/`      | `/workspace/ai_sandbox/canon/canon-rustc/`   |
| Package name      | `canon-rustc`                             | `canon-rustc` (unchanged)                    |
| Binary name       | `canon-rustc`                             | `canon-rustc` (unchanged)                    |
| Crate name (Rust) | `canon_rustc`                             | `canon_rustc` (unchanged)                    |
| Workspace         | own root at `canon_rustc/Cargo.toml`      | own root at `canon/canon-rustc/Cargo.toml`   |
| Toolchain         | `nightly` + `rustc-dev` (isolated)        | same, isolated — `rust-toolchain.toml` stays |

---

## Step 1 — Move the Directory

```bash
mv /workspace/ai_sandbox/canon_rustc \
   /workspace/ai_sandbox/canon/canon-rustc
```

No source files change. Directory rename from `canon_rustc` (underscore) to `canon-rustc`
(hyphen) matches the canon naming convention for all other crates.

---

## Step 2 — Update Path Dependencies in `canon/canon-rustc/Cargo.toml`

All four path deps point one level too deep after the move. Change each `../canon/...`
to `../...`:

```toml
# Before
canon-ir     = { path = "../canon/canon-ir" }
algorithms   = { path = "../canon/algorithms" }
canon_event  = { package = "canon-runtime-events", path = "../canon/canon-utils/canon-runtime-events" }
canon_types  = { path = "../canon/canon-utils/canon-types" }

# After
canon-ir     = { path = "../canon-ir" }
algorithms   = { path = "../algorithms" }
canon_event  = { package = "canon-runtime-events", path = "../canon-utils/canon-runtime-events" }
canon_types  = { path = "../canon-utils/canon-types" }
```

No other `Cargo.toml` changes — package/binary names are already correct.

---

## Step 3 — Exclude from the Canon Main Workspace

Add `"canon-rustc"` to the `exclude` list in `canon/Cargo.toml` so `cargo build` from the
repo root never tries to pull in the nightly sub-workspace:

```toml
# canon/Cargo.toml
[workspace]
exclude = [
  "test_projects",
  "canon-rustc",   # <-- add this
]
```

---

## Step 4 — Update `.cargo/config.toml` Wrapper Path

`canon/.cargo/config.toml` has a commented-out `rustc-wrapper` entry with the old path.
Update it so re-enabling it in future points to the new location:

```toml
# Before (commented out)
# rustc-wrapper = "/workspace/ai_sandbox/canon_rustc/target/debug/canon-rustc"

# After
# rustc-wrapper = "/workspace/ai_sandbox/canon/canon-rustc/target/debug/canon-rustc"
```

---

## Step 5 — Fix Hardcoded Absolute Path in `panic_capture.rs`

`src/log/panic_capture.rs:129` has a hardcoded absolute path that will still work after
the move (it points into `canon/state/`, not `canon_rustc/`), but it should be driven by
the `CANON_REPORTS_OUT` env var (already set in `.cargo/config.toml`) rather than a
literal string. Flag for follow-up — not a blocker for the move.

```rust
// src/log/panic_capture.rs:129  — note for follow-up
let path = PathBuf::from("/workspace/ai_sandbox/canon/state/reports_out/mir_errors.jsonl");
// Should be: std::env::var("CANON_REPORTS_OUT").map(|p| PathBuf::from(p).join("mir_errors.jsonl"))
```

---

## Step 6 — Update `scripts/canon_rustc_wrapper.sh`

The wrapper script references `canon_kernel`, not `canon_rustc`, but update the comment
header and any internal path references to reflect the new location if the script is
extended to reference the binary directly:

```bash
# If the script is updated to invoke canon-rustc directly, use:
CANON_RUSTC="/workspace/ai_sandbox/canon/canon-rustc/target/debug/canon-rustc"
```

---

## Step 7 — Verify Build

```bash
# From the new location — uses its own nightly toolchain automatically
cd /workspace/ai_sandbox/canon/canon-rustc
cargo build

# Canon main workspace should be unaffected
cd /workspace/ai_sandbox/canon
cargo check
```

---

## What Does NOT Change

| Item                          | Reason                                                    |
|-------------------------------|-----------------------------------------------------------|
| All `*.rs` source files       | No source references the old directory path               |
| `rust-toolchain.toml`         | Stays in `canon-rustc/` — applies to that dir only        |
| `build.rs`                    | Contains no paths — only `rustc-check-cfg` directive      |
| Crate name (`canon_rustc`)    | Package name `canon-rustc` → crate `canon_rustc` unchanged|
| `AGENT.md`, `IMPLEMENTATION_PLAN.md`, other docs | Move with directory, no edits needed |
| `canon-ir`, `algorithms` crates | Already inside canon, no changes                        |

---

## File Change Summary

| File                                         | Change                                 |
|----------------------------------------------|----------------------------------------|
| `canon_rustc/` (directory)                   | Moved to `canon/canon-rustc/`          |
| `canon/canon-rustc/Cargo.toml`               | 4 path dep updates                     |
| `canon/Cargo.toml`                           | Add `"canon-rustc"` to `exclude`       |
| `canon/.cargo/config.toml`                   | Update commented rustc-wrapper path    |
| `canon/canon-rustc/src/log/panic_capture.rs` | Note only — follow-up to env-var-ify   |
