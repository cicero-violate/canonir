# Canon Invariant Agent — Goal

## Pipeline

```
canon-capture (MIR lowering) → orchestration --all → emit → scan → build
```

## Invariant

Every function in emitted Rust must have a real `__ret` assignment.
A `__ret` gap means the MIR lowering for that function failed to produce
a statement assigning to `__ret`.

Clarification:
A "real assignment" means a structurally valid CanonIR statement that binds
the return place to the actual MIR return value before any `Return`
terminator is emitted. It must propagate the true return value — not `()`,
not a fabricated placeholder, and not a textual fallback.

CRITICAL EXTENSION:
The same rule applies to ALL value-producing MIR lowering.
No expression, call destination, match, or assignment may silently
fabricate `()` as a fallback for an unsupported or unhandled Rvalue.

If a MIR Rvalue cannot be lowered:
- Do NOT emit `()`
- Do NOT emit `Default::default()`
- Do NOT inject synthetic unit-typed locals

Instead:
- Preserve structural information
- Defer deterministically
- Or emit a clearly typed diverging placeholder (e.g. `panic!`)
  that does NOT collapse type flow to unit.

## How gaps are produced

`pass_lower_match_dest_to_suppressed` in `passes.rs` converts any
`Stmt::Match { dest: Some("__ret") }` into a suppressed assignment.

`lower_return_terminator` in `terminator.rs` emits
`Stmt::Match { dest: Some("__ret") }` only when:
- `has_ret_binding` is false
- `has_match_dest` is false

Root cause: the MIR body for that function never produced a statement
assigning a value to `__ret`. Trace why and fix it.

IMPORTANT:
Do not fix symptoms globally. Select ONE failing function from a fixture
(e.g. `extract_top_level`) and trace it end-to-end:

1. MIR return place
2. CanonIR body construction
3. CfgOp lowering
4. Emit layer rendering

Additionally:

5. MIR expression lowering (mir_assign_stmt, mir_rvalue_expr)
6. Terminator call destination lowering

You must prove where value-flow collapses to `()` and eliminate the
fabrication at its authoritative source.

Do not re-run the full orchestration loop repeatedly without first proving
the value-flow for a single concrete failing function.

## Exploration mandate

You are NOT limited to `lower.rs`, `terminator.rs`, or `passes.rs`.
The root cause may be in any file under `src/capture/`.
Use `BashReadOnly` to explore freely before patching.

Suggested starting points:
```
rg -n "has_ret_binding" src/
rg -n "__ret" src/
rg -n "lower_return" src/
rg -n "lower_call" src/
find src/ -name "*.rs" | xargs wc -l | sort -rn | head -20
```

## What you MUST NOT do

- Do NOT patch emitted output files under `test_projects/`
- Do NOT add `panic!("canon suppressed binding")` anywhere
- Do NOT use `rm -rf` or any destructive shell commands
- Do NOT guess or fabricate line content — read the file first with `sed -n` before patching
- Do NOT emit context lines with ` -` or ` +` prefix — context lines have ONE leading space then code

## Success criteria

The exit-check runs `cargo run -p orchestration -- --all` across all 5 fixtures:
`repomap`, `test_1`, `semantic-lint`, `conversation`, `canon`.

For each fixture, success requires:
- `suppressed_count == 0` — no `canon suppressed binding` sentinels in emitted source
- `build_success == true` — emitted Rust compiles cleanly with zero errors

Additional correctness requirement:
- No fabricated unit fallbacks (`()`) for non-unit return types.
- No synthetic `let mut __ret = ();` in emitted Rust unless the function
  explicitly returns unit.
- No silent `()` fallback in MIR expression lowering.
- No unit-typed call-destination fallback in terminator lowering.
- No comment-only or syntactically invalid placeholder expressions.

The machine-readable result is at `/workspace/ai_sandbox/canon/orchestration_report.json`.
The human-readable report is at `/workspace/ai_sandbox/canon/STRUCTURAL_INVARIANTS_REPORT.md`.

After a verify phase runs, `{{BASH_OUTPUT}}` on the next tick will contain the full
orchestration output. Read `orchestration_report.json` to confirm all fixtures pass
before declaring done.
