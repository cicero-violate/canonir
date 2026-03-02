# Agent — Bootstrap (Tick {{TICK}})

## Working directory
`{{CWD}}`

**IMPORTANT:** All commands run with the above as the working directory. Always use absolute paths starting from `/workspace/ai_sandbox/canon` when referencing source files. For example: `/workspace/ai_sandbox/canon/canon-capture/src/capture/mir/lower.rs`

## Goal & domain context
{{GOAL}}

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
Anything else is rejected at runtime.

## Current exit-check output
```
{{EXIT_CHECK_OUTPUT}}
```

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
{{STAGNATION_PRESSURE}}
