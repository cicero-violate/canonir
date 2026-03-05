# Cargo Check Execution for Emitted Crate

## Target Crate
`test_projects/test_rust_projects/emit/repomap`

## Purpose
Run `cargo check` to collect compiler diagnostics for emitted Rust sources without performing a full build. The diagnostics will be used by subsequent analysis stages to detect structural and semantic errors in generated Rust code.

## Command

```bash
cd test_projects/test_rust_projects/emit/repomap
CARGO_NET_OFFLINE=true cargo check
```

## Expected Output
- Rust compiler diagnostics
- Error codes (e.g., E0425, E0277, E0308)
- File paths and line numbers

## Files Typically Involved
```
src/lib.rs
src/main.rs
src/repomap.rs
src/extractor.rs
src/symbol.rs
```

## Diagnostic Artifacts
Diagnostics should be captured and stored in:

```
canon_build_report.json
```

This artifact will later be parsed by the `detect_failures` analysis stage.
