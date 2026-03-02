# Canon Invariant Agent — Goal

## Pipeline

```
canon-capture (MIR lowering) → orchestration → emit → scan → surface
```

## Invariant

Every function in emitted Rust must have a real `__ret` assignment.
A `__ret` gap means the MIR lowering for that function failed to produce
a statement assigning to `__ret`.

## How gaps are produced

`pass_lower_match_dest_to_suppressed` in `passes.rs` converts any
`Stmt::Match { dest: Some("__ret") }` into a suppressed assignment.

`lower_return_terminator` in `terminator.rs` emits
`Stmt::Match { dest: Some("__ret") }` only when:
- `has_ret_binding` is false
- `has_match_dest` is false

Root cause: the MIR body for that function never produced a statement
assigning a value to `__ret`. Trace why and fix it.

## Exploration mandate

You are NOT limited to `lower.rs`, `terminator.rs`, or `passes.rs`.
The root cause may be in any file under `src/capture/`.
Use `BashReadOnly` to explore freely before patching.
Use `Bash` to run `cargo check` or other verification after patching.

Suggested starting points:
```
rg -n "has_ret_binding" src/
rg -n "__ret" src/
rg -n "lower_return" src/
rg -n "lower_call" src/
find src/ -name "*.rs" | xargs wc -l | sort -rn | head -20
```

## What you MUST NOT do

- Do NOT patch emitted output files
- Do NOT add `panic!("canon suppressed binding")` anywhere
- Do NOT guess or fabricate line content — read the file first
- Do NOT emit context lines with ` -` or ` +` prefix — context lines have ONE leading space then code

## Success criteria

- `unresolved_ret_gap_count` decreases after orchestration re-runs
- `cargo check` on `canon-capture` passes
- `suppressed_count` does not increase
