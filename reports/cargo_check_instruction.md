# Cargo Check Instructions for Emitted Crate

Automated execution of `cargo check` is not permitted in the current sandbox because the `cargo` command is not included in the allowed `read_command` whitelist.

To collect compiler diagnostics for the emitted crate, run the following commands locally or in a permitted environment:

```
cd test_projects/test_rust_projects/emit/repomap
cargo check
```

Once executed, capture the compiler output (stderr/stdout) and store it for downstream analysis. The diagnostics will allow the pipeline to extract:

- Rust error codes (e.g., E0308, E0277, E0425)
- File paths and line numbers
- Type mismatches and unresolved symbols
- Macro expansion errors
- Module resolution failures

These diagnostics can then be parsed to map failures back to projection stages in the Canon IR → Rust emission pipeline.

Expected emitted source layout inspected earlier:

```
test_projects/test_rust_projects/emit/repomap/src
├── main.rs
├── repomap.rs
├── extractor.rs
└── symbol.rs
```

After running `cargo check`, save the raw output to a file such as:

```
reports/cargo_check_output.txt
```

That file can then be parsed by the analysis stage to produce structured failure records.
