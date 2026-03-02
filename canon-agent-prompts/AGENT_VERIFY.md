### Step 0 — Type authority check (run first if return types are suspect)
If you are investigating `Option<()>` vs `Option<&str>` or other return type
mismatches, read the capture-time type authority report before touching any code:
```
cat /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/repomap/canon_type_authority_report.json
```
This shows, for each function, whether `__ret Local.ty` matches `FnSig.ret` at
capture time. A mismatch here means the bug is in canon-capture, not in projection.
Fix it at the authoritative layer (capture), not by patching emitted Rust.

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
{ "BashReadOnly": { "command": "cargo run -p orchestration -- --all 2>&1" } }
```
Never: `{ "type": "BashReadOnly", "command": "..." }` — rejected.
Full shape: `{"phase":"verify","deltas":[...],"rationale":"..."}`

### Step 1 — Optional fast pre-check (Rust changes only)
If you made `ApplyPatch` changes to a `.rs` file, you may first run a fast
compile check on the affected crate:
```
cargo check -p <crate_name> 
```
If it emits `"level":"error"` lines, go back to `act` to fix them before proceeding.

### Step 2 — Full pipeline verification (MANDATORY before declaring done)
Run the full orchestration pipeline:
```
cargo run -p orchestration -- --all 2>&1
```
**Do not rely on truncated stdout.** After it completes, read the structured
JSON report which contains error categories, per-file counts, and sample snippets:
```
cat /workspace/ai_sandbox/canon/orchestration_report.json
```
Confirm every fixture satisfies ALL of:
- `"suppressed_count": 0`
- `"build_success": true`
- `"build_error_count": 0`

If any fixture fails any condition, go back to `act`.
The `build_error_categories` and `build_error_samples` fields in the JSON report
give you the exact error codes and a concrete example of each — use them to
identify what to fix next.

### Step 3 — Confirm active fixture list
The active fixtures are controlled by `orchestration/src/main.rs` (`FIXTURES` constant).
Check it matches your expectations before declaring success.
{{STAGNATION_PRESSURE}}
