## Tick {{TICK}} — act phase

### Last error (if any)
```
{{LAST_ERROR}}
```

### Recent rationale history
```
{{RATIONALE_HISTORY}}
```

### Structural progress
```
{{PROGRESS}}
```

Use `ApplyPatch` or `Bash` to make changes.
Respond with ONE fenced ```json block.

### Delta schema (MANDATORY — serde externally-tagged enum)
Each delta must use the variant name as the key. Examples:
```json
{ "ApplyPatch": { "patch": "*** Begin Patch\n*** Update File: path/to/file.rs\n@@\n-old\n+new\n*** End Patch" } }
{ "Bash":        { "command": "cargo fmt" } }
{ "BashReadOnly":{ "command": "rg -n 'foo' src/" } }
```
**WRONG** — never use a `"type"` field:
```json
{ "type": "ApplyPatch", "patch": "..." }
```
Full response shape: `{"phase":"act","deltas":[...],"rationale":"..."}`

### ApplyPatch format (MANDATORY)
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
Rules:
- Path is relative to repo root `/workspace/ai_sandbox/canon`
- `@@` separates unrelated hunks in the same file
- `-` lines are removed, `+` lines are added, unprefixed lines are context
- Use `*** Add File:` to create new files, `*** Delete File:` to remove
- Escape the string for JSON: replace each newline with `\n`

### Patch grounding rule (MANDATORY)
Before emitting any `ApplyPatch` delta you MUST have previously issued a
`BashReadOnly` with `sed -n '<start>,<end>p'` (or equivalent) that covers
the **exact lines** you intend to use as context anchors in the patch.
**Do NOT write context lines from memory or inference.**
**EXCEPTION — injected error sample:** If `## Concrete error sample` appears
in your **Structural progress** block this tick, that sample IS your anchor.
You MAY emit an ApplyPatch targeting the exact lines shown there without
a prior `sed` read. Then immediately follow with a `BashReadOnly`
`cargo check -p <crate>` in the same response
to measure ΔE. This is the $P \rightarrow B$ protocol: patch then check,
same tick. Do not defer the check to the next tick.
### After patching Rust files (MANDATORY)
If any of your `ApplyPatch` deltas touch a `.rs` file, your **next phase
must be `verify`** and you must run `cargo check -p <crate_name> 2>&1`
as the first `BashReadOnly` delta in that verify tick — always with `--message-format=json`:
`cargo check -p <crate_name>`. Do not observe or
plan between an act and its compile verification.

{{STAGNATION_PRESSURE}}
