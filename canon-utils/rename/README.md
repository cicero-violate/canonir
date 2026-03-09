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

2. Send an op envelope to `rename_stdin` (JSON on stdin, JSON report on stdout):

```bash
cd /workspace/ai_sandbox/canon/canon-utils/rename
cat <<'JSON' | cargo run --bin rename_stdin
{
  "project": "/path/to/your/project",
  "/workspace/ai_sandbox/canon/canon-agent-v2",
  "verify": true,
  "check": true,
  "ops": [
    { "op": "ListSymbols" },
    { "op": "GetSymbol", "args": { "symbol_id": "crate::mod::Foo" } },
    { "op": "RenameSymbol", "args": { "old": "crate::old_name", "new": "crate::new_name" } },
    { "op": "MoveSymbol",   "args": { "symbol_id": "crate::mod::Foo", "new_module_path": "crate::mod2" } },
    { "op": "RenameModule", "args": { "old_module_path": "crate::old_mod", "new_name": "new_mod" } },
    { "op": "RenameDir",    "args": { "old_dir": "src/old_dir", "new_dir": "src/new_dir" } },
    { "op": "DeleteSymbol", "args": { "symbol_id": "crate::mod::OldThing" } }
  ]
}
JSON
```

### Op Envelope Schema

Each operation is one of:

- `RenameSymbol`: `{ "op": "RenameSymbol", "args": { "old": "...", "new": "..." } }`
- `MoveSymbol`: `{ "op": "MoveSymbol", "args": { "symbol_id": "...", "new_module_path": "..." } }`
- `RenameModule`: `{ "op": "RenameModule", "args": { "old_module_path": "...", "new_name": "..." } }`
- `RenameDir`: `{ "op": "RenameDir", "args": { "old_dir": "...", "new_dir": "..." } }`
- `ListSymbols`: `{ "op": "ListSymbols" }`
- `GetSymbol`: `{ "op": "GetSymbol", "args": { "symbol_id": "..." } }`
- `PreviewRename`: `{ "op": "PreviewRename" }`
- `CheckErrors`: `{ "op": "CheckErrors" }` (use `check: true` at request level)
- `DeleteSymbol`: `{ "op": "DeleteSymbol", "args": { "symbol_id": "..." } }`
- `InlineModule`: `{ "op": "InlineModule", "args": { "module_path": "..." } }` (unsupported)
- `ExtractModule`: `{ "op": "ExtractModule", "args": { "symbol_id": "...", "target_file": "..." } }` (unsupported)
- `SuggestRenames`: `{ "op": "SuggestRenames", "args": { "batch_size": 20, "model": "..." } }` (unsupported)
- `ApplySuggestions`: `{ "op": "ApplySuggestions", "args": { "suggestions": [[\"old\", \"new\"], ...] } }`
- `Help`: `{ "op": "Help" }`
- `ListOps`: `{ "op": "ListOps" }`

### Verification Flags

- `verify: true` runs span-based rename verification after apply.
- `check: true` runs `cargo check` before and after apply and returns delta error counts.

## Notes

- `analysis/symbols.json` entries have: `symbol_id`, `new_name`, `kind`, `rename_safe`.
- External trait impls are auto-marked unsafe and skipped.
- Bulk mode runs `cargo check` and restores on errors.
