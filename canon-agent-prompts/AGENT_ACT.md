## Tick {{TICK}} — act phase

**Working directory (all commands run here):** `{{CWD}}`
**Repo root for source files:** `/workspace/ai_sandbox/canon`

Use absolute paths in all `Bash` commands. For `ApplyPatch`, paths inside the patch are relative to the repo root `/workspace/ai_sandbox/canon`.

### Exit-check output
```
{{EXIT_CHECK_OUTPUT}}
```

### Last error (if any)
```
{{LAST_ERROR}}
```

### Recent rationale history
```
{{RATIONALE_HISTORY}}
```

Use `ApplyPatch` or `Bash` to make changes.
Respond with ONE fenced ```json block.
`{"phase":"act","deltas":[...],"rationale":"..."}`

### Patch grounding rule (MANDATORY)
Before emitting any `ApplyPatch` delta you MUST have previously issued a
`BashReadOnly` with `sed -n '<start>,<end>p'` (or equivalent) that covers
the **exact lines** you intend to use as context anchors in the patch.
**Do NOT write context lines from memory or inference.** If you have not
read the target lines in this tick or a prior observe tick, emit an
`observe` phase first to read them, then act in the next tick.
### After patching Rust files (MANDATORY)
If any of your `ApplyPatch` deltas touch a `.rs` file, your **next phase
must be `verify`** and you must run `cargo check -p <crate_name> 2>&1`
as the first `BashReadOnly` delta in that verify tick. Do not observe or
plan between an act and its compile verification.

{{STAGNATION_PRESSURE}}
