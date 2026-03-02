## Tick {{TICK}} — verify phase

### Recent rationale history
```
{{RATIONALE_HISTORY}}
```

### Structural progress
```
{{PROGRESS}}
```

Use `BashReadOnly` to confirm your fix is correct.
After this turn the exit-check command will run automatically.
Respond with ONE fenced ```json block.

### Delta schema (MANDATORY)
```json
{ "BashReadOnly": { "command": "cargo check -p canon-capture --message-format=json 2>&1" } }
```
Never: `{ "type": "BashReadOnly", "command": "..." }` — rejected.
Full shape: `{"phase":"verify","deltas":[...],"rationale":"..."}`

### Cargo compile check (MANDATORY for Rust changes)
If you made any `ApplyPatch` changes to a `.rs` file in a prior act tick,
you MUST emit a `BashReadOnly` delta running:
cargo check -p <crate_name> --message-format=json 2>&1
from the repo root `/workspace/ai_sandbox/canon` before anything else.
Use the absolute path form: `cd /workspace/ai_sandbox/canon && cargo check -p <crate_name> 2>&1`
Simply: `cargo check -p <crate_name> --message-format=json 2>&1` — cargo runs automatically from the repo root.
See the observe phase for the full `BashReadOnly` whitelist.
If the output contains `^error`, your patch broke the build — state the
compiler errors in your rationale and do NOT consider the task done.
You must go back to `act` to fix the compile errors before verifying again.

### Full pipeline verification (use before declaring done)
To verify all fixtures pass, run:
```
cargo run -p orchestration -- --all 2>&1
```
Then read `/workspace/ai_sandbox/canon/orchestration_report.json` and confirm:
- Every fixture has `"suppressed_count": 0`
- Every fixture has `"build_success": true`
If any fixture fails either condition, go back to `act`.
{{STAGNATION_PRESSURE}}
