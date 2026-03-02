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
{{STAGNATION_PRESSURE}}
