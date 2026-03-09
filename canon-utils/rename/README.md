# rename (canon-utils)

This tool renames Rust symbols in a target project using rustc symbol IDs.

## Quick Start (Canonical API)

1. Generate `analysis/` for the target project (must include `spans.jsonl` and `symbols.json`):

- If you already use the `analysis_capture` rustc wrapper in your build, just run:

```bash
cd /path/to/your/project
cargo check
```

- Otherwise, run the build with the wrapper configured for your project (see your repo’s build docs).

2. Send a rename request to `rename_stdin` (JSON on stdin, JSON report on stdout):

```bash
cd /workspace/ai_sandbox/canon/canon-utils/rename
cat <<'JSON' | cargo run --bin rename_stdin
{
  "project": "/path/to/your/project",
  "renames": [
    ["crate::old_name", "crate::new_name"],
    ["crate::mod::Foo", "crate::mod::Bar"]
  ]
}
JSON
```

## Notes

- `analysis/symbols.json` entries have: `symbol_id`, `new_name`, `kind`, `rename_safe`.
- External trait impls are auto-marked unsafe and skipped.
- Bulk mode runs `cargo check` and restores on errors.
