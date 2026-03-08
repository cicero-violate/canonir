# rename (canon-utils)

This tool renames Rust symbols in a target project using rustc symbol IDs.

## Quick Start

1. Run `cargo check` (or `cargo build`) in the target project to populate `analysis/`:

```bash
cd /path/to/your/project
cargo check
```

2. Edit `analysis/symbols.json` (set `new_name` for the symbols you want to rename).

3. Apply renames in bulk:

```bash
RENAME_MODE=bulk cargo run --example rename_self
```

## Stdin Suggestions + Apply

Pipe JSON suggestions and immediately apply:

```bash
printf "[%s]" symbol_id:crate::path::to::fn | \
  RENAME_SUGGEST_NAMES=1 RENAME_SUGGEST_STDIN=1 cargo run --example rename_self
```

## List Symbols

```bash
RENAME_LIST_SYMBOLS=1 RENAME_LIST_FILTER=graph_runtime RENAME_LIST_LIMIT=20 \
  cargo run --example rename_self
```

## Notes

- `analysis/symbols.json` entries have: `symbol_id`, `new_name`, `kind`, `rename_safe`.
- External trait impls are auto-marked unsafe and skipped.
- Bulk mode runs `cargo check` and restores on errors.
