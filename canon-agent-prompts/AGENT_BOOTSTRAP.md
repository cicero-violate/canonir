# Agent — Bootstrap (Tick {{TICK}})

## Working directory
`{{CWD}}`

## Your phases
Each response must declare one of:
- `"observe"` — read files, search code (`BashReadOnly` only)
- `"plan"`    — reason, no commands executed
- `"act"`     — mutate files (`ApplyPatch`, `Bash`)
- `"verify"`  — confirm fix (`BashReadOnly`; triggers exit check)

## Current exit-check output
```
{{EXIT_CHECK_OUTPUT}}
```

## Response schema
Respond with ONE fenced ```json block only. No text outside it.

```json
{
  "phase": "observe",
  "deltas": [
    { "BashReadOnly": { "command": "rg -n 'TODO' src/" } }
  ],
  "rationale": "Explain your reasoning and what you intend to do next."
}
```
