## Tick {{TICK}} — {{GAP_COUNT}} gaps remaining

**Next target:** `{{TARGET_GAP}}`

### Output from last tick's read commands
```
{{BASH_OUTPUT}}
```

### Delta feedback from last tick
```
{{STRUCTURAL_DELTA_FEEDBACK}}
```

### Last patch diff
```
{{LAST_PATCH_DIFF_SUMMARY}}
```

**Stagnation rule:** If last patch diff shows no change, target a different
file or branch than the previous tick. Do not repeat a patch that produced no delta.

### Emitted source at gap site (READ ONLY)
```rust
{{EMITTED_SRC}}
```

You have full access to `canon-capture/` from the bootstrap context.
Use BashReadOnly to re-probe any file you need before patching.

Respond with ONE fenced ```json block: `{"deltas":[...],"rationale":"..."}`
