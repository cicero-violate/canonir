# Implementation Plan: Canon Analyst Agent

## What was built

A standalone `canon-analyst` binary that connects an LLM to the system's event log
via a Python execution loop. Run it at any time to get a diagnosis of system health.

```
cargo run -p canon-analyst                          # default: general health check
cargo run -p canon-analyst "why did the planner stall?"
```

## Files created / modified

| File | Status |
|---|---|
| `canon-utils/canon-analyst/Cargo.toml` | NEW |
| `canon-utils/canon-analyst/src/main.rs` | NEW — entry point, CLI arg, calls agent loop |
| `canon-utils/canon-analyst/src/tlog.rs` | NEW — reads tlog, builds summary (counts + last key events) |
| `canon-utils/canon-analyst/src/python.rs` | NEW — executes Python snippets via `python3`, captures stdout/stderr |
| `canon-utils/canon-analyst/src/agent.rs` | NEW — LLM agent loop (up to 6 turns, extracts + runs python blocks) |
| `Cargo.toml` (workspace) | MODIFIED — added `canon-utils/canon-analyst` to members |
| `capability_config.toml` | MODIFIED — added `analyst` endpoint + agent card |

## How the agent loop works

```
question + tlog summary
        ↓
   LLM (analyst tab)
        ↓
  response contains ```python block?
     YES → python::run(code, tlog_path)
           → result injected into next prompt
           → repeat (max 6 turns)
      NO → print final analysis, exit
```

The LLM receives `CANON_TLOG` via environment variable in every Python subprocess.
Standard pattern for analysis scripts:

```python
import json, os, collections

tlog = os.environ["CANON_TLOG"]
events = [json.loads(l) for l in open(tlog) if l.strip()]
counts = collections.Counter(e["kind"] for e in events)
print(counts.most_common(10))
```

## Required: fill in placeholder URLs

Two entries in `capability_config.toml` have placeholder URLs that must be replaced
before the analyst or goal_gen endpoints will work:

```toml
[llm.endpoints.goal_gen]
url = "PLACEHOLDER_GOAL_GEN_URL"   ← replace with goal gen ChatGPT URL

[llm.endpoints.analyst]
url = "PLACEHOLDER_ANALYST_URL"    ← replace with analyst ChatGPT URL
```

The same placeholder appears in the `[[agents.cards]]` entries for `goal_gen` and
`analyst`. Replace all four occurrences.

Also fixed in this plan: `goal_gen` was sharing a URL with `exec_chatgpt_d`
(`69ab7b06...`). Both the endpoint and agent card for `goal_gen` now use
`PLACEHOLDER_GOAL_GEN_URL` pending a real dedicated URL.

## URL inventory (must all be unique)

| Role | Endpoint ID | URL |
|---|---|---|
| planner | `planner_chatgpt_group` | `69bb5d85...` |
| router | `router_chatgpt_group` | `69bb5dbc...` |
| goal_gen | `goal_gen_chatgpt` | PLACEHOLDER_GOAL_GEN_URL |
| analyst | `analyst_chatgpt` | PLACEHOLDER_ANALYST_URL |
| exec_a | `exec_chatgpt_a` | `699c50e0...` |
| exec_b | `exec_chatgpt_b` | `69bb69ad...` |
| exec_c | `exec_chatgpt_c` | `69bca599...` |
| exec_d | `exec_chatgpt_d` | `69ab7b06...` |
| exec_e | `exec_chatgpt_e` | `69929b21...` |
| exec_f | `exec_chatgpt_f` | `699271537...` |

## Workspace dependency

`canon-analyst/Cargo.toml` uses `tempfile` (already in workspace deps) plus
`canon_llm` and `canon_event`. No new external crates required.
