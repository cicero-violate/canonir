# Canon Invariant Agent — Goal

## System Context

Canon pipeline: Capture → CanonIR → Graph → Solve → Emit

`canon-capture` lowers rustc MIR into CanonIR `Body::Blocks`.
`orchestration` runs the full pipeline and emits Rust source.
The emitted source is scanned for `__ret` gaps.

## What a `__ret` gap means

An emitted `__ret` gap looks like:
```rust
let mut __ret = panic!("canon suppressed binding");
return __ret;
```

This is produced when `pass_lower_match_dest_to_suppressed` in `passes.rs`
converts a `Stmt::Match { dest: Some("__ret") }` into a suppressed assignment.

`Stmt::Match { dest: Some("__ret") }` is emitted by `lower_return_terminator`
in `terminator.rs` when BOTH of these are true:
- `has_ret_binding` is false (no statement assigned to `__ret`)
- `has_match_dest` is false (no prior match dest for `__ret`)

This means the MIR body processed for that function never produced a statement
that assigned a value to `__ret`. The function's return value was not captured.

## Your Task Each Tick

1. Look at the emitted file at the gap site — find the function with `__ret` gap.
2. Look at `src/capture/mir/lower.rs` — trace `stage_lower_block_statements` and
   `stage_lower_block_terminator` to understand what would cause `has_ret_binding=false`.
3. Look at `src/capture/mir/terminator.rs` — find which branch of `lower_call_terminator`
   is being taken and why it doesn't produce a real `__ret` assignment.
4. Patch the specific branch that should be producing an `__ret` assignment but isn't.

## What you MUST NOT do

- Do NOT remove lines from terminator.rs that insert `__canon_suppressed__` for `__ret` —
  those are intentional fallbacks, not bugs.
- Do NOT patch the emitted output files.
- Do NOT guess or fabricate line content — only use lines that appear VERBATIM in the
  source files shown to you.
- Do NOT emit context lines with a leading space followed by `-` or `+` — that is wrong.
  Context lines have NO prefix character at all (just a leading space).

## Patch format reminder

A `-` line removes an existing line. A `+` line adds a new line.
A context line (for anchoring) has exactly ONE leading space then the code.
Do NOT write ` -` or ` +` — that is invalid.

## Patch file paths

All paths are relative to the `canon-capture` working directory.
Example: `src/capture/mir/terminator.rs`

## Success criteria

- `unresolved_ret_gap_count` decreases after orchestration re-runs.
- `cargo check` on `canon-capture` passes.
