# Agent — Bootstrap (Tick {{TICK}})

## Working directory
`{{CWD}}`

**IMPORTANT:** All commands run with the above as the working directory. Always use absolute paths starting from `/workspace/ai_sandbox/canon` when referencing source files. For example: `/workspace/ai_sandbox/canon/canon-capture/src/capture/mir/lower.rs`

## Goal & domain context
{{GOAL}}

## Exit-check command (what "done" means)
The loop terminates when this command exits 0:
```
cd /workspace/ai_sandbox/canon && cargo run -p orchestration -- --all
```

The active fixture list is controlled by the `FIXTURES` constant in
`orchestration/src/main.rs`. **Always check this file before assuming scope** —
the active set may be smaller than the full list. For each active fixture it:
1. Loads `canon_capture.json` and runs the full emit pipeline
2. Scans emitted `src/` for structural gap sentinels (`canon suppressed binding`, match/call/switch gaps)
3. Runs `cargo build` on the emitted Rust source and checks for errors

Success = all active fixtures have `suppressed_count == 0` AND `build_success == true`
AND `build_error_count == 0`.

Two machine-readable artifacts are written after each run:
- `/workspace/ai_sandbox/canon/orchestration_report.json` — structured per-fixture
  breakdown including `build_error_categories` (error code counts),
  `build_error_samples` (one rendered snippet per error code), and
  `errors_by_file` (error count per source file).
- `/workspace/ai_sandbox/canon/STRUCTURAL_INVARIANTS_REPORT.md` — human-readable form.

**Always read `orchestration_report.json` after running orchestration** — do not
rely on truncated stdout. The JSON report survives output truncation and gives
you a precise breakdown of what is failing and where.

Your goal is to fix the structural return lowering in canon-capture so that
all emitted Rust source is gap-free and compiles cleanly across every active fixture.

## Your phases
Each response must declare one of:
- `"observe"` — read files, search code (`BashReadOnly` only)
- `"plan"`    — reason, no commands executed
- `"act"`     — mutate files (`ApplyPatch`, `Bash`)
- `"verify"`  — confirm fix (`BashReadOnly`; triggers exit check)

## Patch grounding rule (MANDATORY for every act phase)
Before emitting any `ApplyPatch` delta you MUST have issued a `BashReadOnly`
with `sed -n '<start>,<end>p'` covering the **exact context lines** you will
use as anchors in the patch — either in this tick or a prior observe tick.
Never write patch context lines from memory or inference; always read first.

## BashReadOnly whitelisted commands
Only these prefixes are permitted: `rg`, `cat`, `ls`, `tree`, `sed`, `awk`,
`perl`, `find`, `head`, `tail`, `wc`, `diff`, `stat`, `echo`, `pwd`, `cargo`
(`cargo` for read-only ops only: `check`, `build`, `test`).
Always use `--message-format=json` with `cargo check` and `cargo build`
so output is machine-readable: `cargo check -p <crate> --message-format=json 2>&1`
Anything else is rejected at runtime.

## Response schema
Respond with ONE fenced ```json block only. No text outside it.

```json
{
  "phase": "observe",
  "deltas": [
    { "BashReadOnly": { "command": "rg -n 'TODO' src/" } }
  ],
  "rationale": "Explain your reasoning and what you intend to do next."
}
```

> **CRITICAL — delta shape:** Each delta object uses the **variant name as its only key**
> (`"ApplyPatch"`, `"Bash"`, `"BashReadOnly"`). Never use a `"type"` discriminator field.
> `{"type":"ApplyPatch","patch":"..."}` will be **rejected** with a schema error.
>
> Correct examples for every variant:
> - `{ "BashReadOnly": { "command": "rg -n 'foo' src/" } }`
> - `{ "Bash":         { "command": "cargo fmt" } }`
> - `{ "ApplyPatch":   { "patch": "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-old\n+new\n*** End Patch" } }`

{{STAGNATION_PRESSURE}}
## ApplyPatch format (MANDATORY)
The `patch` string must use this exact format — NOT unified diff (`---`/`+++`):
```
*** Begin Patch
*** Update File: path/relative/to/repo/root/file.rs
@@
-    old line to remove
+    new line to add
 unchanged context line
*** End Patch
```
- Path relative to repo root `/workspace/ai_sandbox/canon`
- Escape for JSON: replace each newline with `\n`
- `@@` separates unrelated hunks; `-` removes, `+` adds, unprefixed = context
