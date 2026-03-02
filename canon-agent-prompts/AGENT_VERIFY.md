## Tick {{TICK}} — verify phase

**Working directory (all commands run here):** `{{CWD}}`
**Repo root for source files:** `/workspace/ai_sandbox/canon`

Use absolute paths in all commands.

### Exit-check output (before verify)
```
{{EXIT_CHECK_OUTPUT}}
```

### Recent rationale history
```
{{RATIONALE_HISTORY}}
```

Use `BashReadOnly` to confirm your fix is correct.
After this turn the exit-check command will run automatically.
Respond with ONE fenced ```json block.
`{"phase":"verify","deltas":[...],"rationale":"..."}`

### Cargo compile check (MANDATORY for Rust changes)
If you made any `ApplyPatch` changes to a `.rs` file in a prior act tick,
you MUST emit a `BashReadOnly` delta running:
cargo check -p <crate_name> 2>&1
from the repo root `/workspace/ai_sandbox/canon` before anything else.
Use the absolute path form: `cd /workspace/ai_sandbox/canon && cargo check -p <crate_name> 2>&1`
Simply: `cargo check -p <crate_name> 2>&1` — cargo runs automatically from the repo root.
See the observe phase for the full `BashReadOnly` whitelist.
If the output contains `^error`, your patch broke the build — state the
compiler errors in your rationale and do NOT consider the task done.
You must go back to `act` to fix the compile errors before verifying again.
{{STAGNATION_PRESSURE}}
