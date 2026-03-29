# Implementation Plan: Dedicated ChatGPT URLs for Repair and Eventlog Harnesses

## Goal

Assign dedicated ChatGPT GPT URLs to the two harness agents:

| Harness | New URL |
|---|---|
| Repair harness (`canon-harness-repair`) | `https://chatgpt.com/gg/69c8f84bb6848194a9b9ca6eaf5819c6` |
| Eventlog harness (`canon-eventlog-repair`) | `https://chatgpt.com/gg/69c8f86cf14c81a0a1a9b4487bccd784` |

Currently both route through `https://chatgpt.com/gg/69c229260fa08194ad30bb0e1f57105a` (the `goal_gen` endpoint / planner role).

---

## File 1: `canon-agent-prompts/capability_config.toml`

### Add two new endpoint sections (after the `[llm.endpoints.analyst]` block, before `[llm.roles.*]`)

```toml
[llm.endpoints.harness_repair]
id = "harness_repair_chatgpt"
url = "https://chatgpt.com/gg/69c8f84bb6848194a9b9ca6eaf5819c6"
role_markdown = "builtin:planner"
role = "harness_repair"
stateful = true
max_tabs = 1

[llm.endpoints.harness_eventlog]
id = "harness_eventlog_chatgpt"
url = "https://chatgpt.com/gg/69c8f86cf14c81a0a1a9b4487bccd784"
role_markdown = "builtin:planner"
role = "harness_eventlog"
stateful = true
max_tabs = 1
```

### Add two new role weight + burst sections (after `[llm.roles.goal_gen]`)

```toml
[llm.roles.harness_repair.weights]
harness_repair_chatgpt = 100

[llm.roles.harness_repair]
burst = 1

[llm.roles.harness_eventlog.weights]
harness_eventlog_chatgpt = 100

[llm.roles.harness_eventlog]
burst = 1
```

### Add two new agent cards (after the existing `[[agents.cards]]` entries)

```toml
[[agents.cards]]
agent_url = "https://chatgpt.com/gg/69c8f84bb6848194a9b9ca6eaf5819c6"
agent_id = "harness_repair"
role = "builtin:planner"
goal = "AGENT_GOAL.md"
tool_capabilities = ["apply_patch", "bash"]

[[agents.cards]]
agent_url = "https://chatgpt.com/gg/69c8f86cf14c81a0a1a9b4487bccd784"
agent_id = "harness_eventlog"
role = "builtin:planner"
goal = "AGENT_GOAL.md"
tool_capabilities = ["apply_patch", "bash"]
```

---

## File 2: `canon-utils/canon-runtime/src/bin/harness_repair.rs`

### Change the dispatched role from `"planner"` to `"harness_repair"`

Find (around line 939):
```rust
role: Some("planner".to_string()),
```
Change to:
```rust
role: Some("harness_repair".to_string()),
```

---

## File 3: `canon-utils/canon-runtime/src/bin/canon-eventlog-repair.rs`

The eventlog repair binary delegates to `canon-harness-repair` via subprocess (line 92), which itself dispatches with a role. After the change above, `canon-harness-repair` will already use `harness_repair`. However, `canon-eventlog-repair` needs its own role for any direct LLM calls it makes.

Check if `canon-eventlog-repair.rs` makes direct LLM dispatch calls (search for `role:`). If it does, change those from `"planner"` to `"harness_eventlog"`. If it only delegates via subprocess, no change is needed in this file.

---

## Summary of changes

| File | Change |
|---|---|
| `capability_config.toml` | Add 2 endpoints, 2 role configs, 2 agent cards |
| `harness_repair.rs` | Change role string from `"planner"` to `"harness_repair"` |
| `canon-eventlog-repair.rs` | Change role string (if any direct LLM call) to `"harness_eventlog"` |
