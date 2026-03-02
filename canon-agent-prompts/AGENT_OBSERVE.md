## Tick {{TICK}} — observe phase

### Output from previous commands
```
{{BASH_OUTPUT}}
```

### Recent rationale history
```
{{RATIONALE_HISTORY}}
```

### Structural progress
```
{{PROGRESS}}
```

Use `BashReadOnly` to probe files. Respond with ONE fenced ```json block.

### Delta schema (MANDATORY)
```json
{ "BashReadOnly": { "command": "rg -n 'pattern' src/" } }
```
Never: `{ "type": "BashReadOnly", "command": "..." }` — rejected.
Full shape: `{"phase":"observe","deltas":[...],"rationale":"..."}`
{{STAGNATION_PRESSURE}}
