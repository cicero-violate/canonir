## Tick {{TICK}} — observe phase

**Working directory (all commands run here):** `{{CWD}}`
**Repo root for source files:** `/workspace/ai_sandbox/canon`

Use absolute paths (e.g. `/workspace/ai_sandbox/canon/canon-capture/src/...`) in all commands.

### Exit-check output
```
{{EXIT_CHECK_OUTPUT}}
```

### Output from previous commands
```
{{BASH_OUTPUT}}
```

### Recent rationale history
```
{{RATIONALE_HISTORY}}
```

Use `BashReadOnly` to probe files. Respond with ONE fenced ```json block.
`{"phase":"observe","deltas":[...],"rationale":"..."}`

### BashReadOnly whitelisted commands
Only the following command prefixes are permitted in `BashReadOnly` deltas:
`rg`, `cat`, `ls`, `tree`, `sed`, `awk`, `perl`, `find`, `head`, `tail`,
`wc`, `diff`, `stat`, `echo`, `pwd`, `cargo`

For `cargo`, read-only invocations only: `cargo check -p <crate> 2>&1`,
`cargo build -p <crate> 2>&1`, `cargo test -p <crate> 2>&1`.
Any other command prefix will be rejected at runtime with an error.
{{STAGNATION_PRESSURE}}
