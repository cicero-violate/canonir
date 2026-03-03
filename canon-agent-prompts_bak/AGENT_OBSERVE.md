## Tick {{TICK}} — observe phase

### Output from previous commands
```
{{BASH_OUTPUT}}
```

### Structural progress
```
{{PROGRESS}}
```

Use `BashReadOnly` to probe files. ONE query per observe tick — no redundant reads.
If you have enough context to act, choose `act` instead.

Respond with ONE fenced ```json block.
`{"phase":"observe","deltas":[{ "BashReadOnly": { "command": "..." } }],"rationale":"..."}`

Never: `{ "type": "BashReadOnly", "command": "..." }` — rejected.

**Stop observing when:**
- You have seen the error sample from `orchestration_report.json`
- You have seen the source lines around the failing site
That is sufficient. Emit `act` next.

{{STAGNATION_PRESSURE}}
