# Implementation Plan: Integrate rename symbol generation into analysis_capture pipeline

## Goal

After `cargo build/check`, the full `analysis/` directory is populated automatically:

## Status

Implemented (Mar 8, 2026). Spans/symbols are now emitted by `analysis_capture` via `canon_capture::collect_spans_and_symbols`, and `rename` reads directly from `analysis/`.

## File change summary

| File                                | Change                                                                      |
|-------------------------------------+-----------------------------------------------------------------------------|
| `canon_capture/src/lib.rs`          | Add `collect_spans_and_symbols(tcx, output_dir, crate_name)`                |
| `canon_capture/src/spans.rs`        | New file — HIR visitor + span writer (moved from rename)                    |
| `analysis_capture/src/main.rs`      | Add `crate_name` to `MirCaptureCallbacks`, call `collect_spans_and_symbols` |
| `analysis_capture/Cargo.toml`       | Add `canon_capture` dependency                                              |
| `rename/src/core/rustc_session.rs`  | Replace compiler driver with direct file reader                             |
| `rename/src/core/rustc_resolver.rs` | Delete or keep only `infer_crate_name`                                      |
| `rename/src/runner.rs`              | Remove `RENAME_GENERATE_SYMBOLS` path, read from `analysis/`                |
| `rename/Cargo.toml`                 | Drop rustc_* and cargo crate dependencies                                   |
| `rename/span_file/`                 | Delete                                                                      |

```
cargo build/check
    └── analysis_capture (rustc wrapper)
            ├── writes nodes.csv, edges.csv, metadata.json, ... → analysis/
            ├── writes spans.jsonl → analysis/
            ├── writes symbols.json → analysis/
            └── spawns analysis-engine --dir analysis/ --phase all
                        └── writes anomalies.json, semantic_duplicates.json, ... → analysis/
```

---

## What produces spans.jsonl and symbols.json today

`analysis_capture` runs the HIR walk inside `MirCaptureCallbacks::after_analysis` and calls
`canon_capture::collect_spans_and_symbols`, which writes `spans.jsonl` and `symbols.json`
to `analysis/`.

---




## Implementation Steps

### Step 1 — Extract span and symbol collection into a shared crate

**Crate:** `canon-utils/rename` already contains `BulkCollectorCallbacks` and all HIR visitor
logic in `src/core/rustc_session.rs`. Extract the pure collection logic into a new file or
expose it as a callable function so `analysis_capture` can call it without depending on the
full `rename` crate.

**Options (pick one):**
- A. Move `BulkCollectorCallbacks` + `write_symbols_json` into `canon_capture` (the crate
  already used by both — confirmed by `rustc_session.rs` line 226: `canon_capture::index::build_index`)
- B. Create a new thin crate `canon-utils/symbol-collect` that both `rename` and
  `analysis_capture` depend on

Option A is preferred — `canon_capture` is already a shared dependency of both binaries.

**Deliverable:** A public function in `canon_capture`:
```rust
pub fn collect_spans_and_symbols(
    tcx: TyCtxt<'_>,
    output_dir: &Path,
    crate_name: &str,
) -> Result<(), Error>
```
Writes `spans.jsonl` and `symbols.json` into `output_dir`.

---

### Step 2 — Call collect_spans_and_symbols from analysis_capture

In `rustc_wrapper/analysis_capture/src/main.rs`, inside `MirCaptureCallbacks::after_analysis`,
after the existing `extract_and_write(tcx, &config)` call, add:

```rust
if let Err(err) = canon_capture::collect_spans_and_symbols(
    tcx,
    &self.output_dir,
    crate_name,   // thread crate_name into MirCaptureCallbacks
) {
    eprintln!("analysis_capture: span/symbol collection failed: {err:?}");
}
```

`MirCaptureCallbacks` needs `crate_name: String` added to its fields, populated from the
`--crate-name` flag already parsed in `main`.

---

### Step 3 — Gate on primary package only

`spans.jsonl` and `symbols.json` are only meaningful for the primary crate being analysed,
not for every dependency rustc touches. Add the same primary-package guard already used for
`should_run_analysis_engine`:

```rust
if is_primary_package(crate_name.as_deref()) {
    canon_capture::collect_spans_and_symbols(tcx, &self.output_dir, ...)?;
}
```

---

### Step 4 — Remove manual trigger from rename

Once Step 2 is working, `RustcSession::build` no longer needs to drive a second rustc
in-process pass for span collection. The spans and symbols will already be present in
`analysis/` after any `cargo check`.

`RustcSession::build` changes:
- Remove the `cargo_rustc_args` + `run_compiler` loop
- Remove `span_output_path` + `load_spans_from_file`
- Replace with a direct read of `project_root/analysis/spans.jsonl` and
  `project_root/analysis/symbols.json`
- `RustcSession` becomes a loader, not a compiler driver

`runner.rs` changes:
- Remove `RENAME_GENERATE_SYMBOLS=1` path entirely, or keep as a forced-refresh escape hatch
- `run_rename_self` reads from `analysis/` directly

---

### Step 5 — Remove span_file directory

`canon-utils/rename/span_file/` is now dead. Delete it and remove any `.gitignore` entries.

---

## Dependency changes

`analysis_capture/Cargo.toml` gains:
```toml
canon_capture = { path = "../../canon_capture" }   # adjust path as needed
```

`rename/Cargo.toml` loses:
- The `rustc_driver` / `rustc_interface` / `rustc_middle` / `rustc_hir` / `rustc_span`
  dependencies (no longer driving rustc in-process)
- `cargo` / `cargo_util` dependencies (no longer capturing rustc args)

---

## File change summary

| File                                | Change                                                                      |
|-------------------------------------+-----------------------------------------------------------------------------|
| `canon_capture/src/lib.rs`          | Add `collect_spans_and_symbols(tcx, output_dir, crate_name)`                |
| `canon_capture/src/spans.rs`        | New file — HIR visitor + span writer (moved from rename)                    |
| `analysis_capture/src/main.rs`      | Add `crate_name` to `MirCaptureCallbacks`, call `collect_spans_and_symbols` |
| `analysis_capture/Cargo.toml`       | Add `canon_capture` dependency                                              |
| `rename/src/core/rustc_session.rs`  | Replace compiler driver with direct file reader                             |
| `rename/src/core/rustc_resolver.rs` | Delete or keep only `infer_crate_name`                                      |
| `rename/src/runner.rs`              | Remove `RENAME_GENERATE_SYMBOLS` path, read from `analysis/`                |
| `rename/Cargo.toml`                 | Drop rustc_* and cargo crate dependencies                                   |
| `rename/span_file/`                 | Delete                                                                      |

---

## Verification

After implementation, a single `cargo check` on `canon-agent-v2` must produce:

```
canon-agent-v2/analysis/
    nodes.csv
    edges.csv
    spans.jsonl          ← new, from canon_capture
    symbols.json         ← new, from canon_capture
    anomalies.json
    semantic_duplicates.json
    refactoring_candidates.json
    ...
```

And `rename` must work without any separate `RENAME_GENERATE_SYMBOLS=1` invocation.
