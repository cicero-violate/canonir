# Plan: Collapse capability_config.toml URLs into a clean flat list

## Problem

`capability_config.toml` defines endpoints in two conflicting TOML formats:
- `[[llm.endpoints]]` array entries (lines 58–121)
- `[llm.endpoints.xxx]` named table entries (lines 128–264)

Several endpoints are duplicated across both. The `config.rs` parser supports either List or Map via `#[serde(untagged)]` but not both at once — the current file will likely deserialize only one section.

Two new URLs from `chatgpt_url.txt` also need to be added:
- `https://chatgpt.com/gg/69ca3cec39ec8195911764cda77d15a0` → `exec_chatgpt_g`
- `https://chatgpt.com/gg/69c265cd2274819690fc291ef716524e` → `exec_chatgpt_h`

## Action

Replace the entire `[llm]` endpoints block in `capability_config.toml` with a single flat `[[llm.endpoints]]` list (array-of-tables format), deduplicated. Add new exec endpoints. Update `roles.exec`, `roles.decompose` weights, and `[[agents.cards]]` accordingly.

## Final `[llm]` section for `capability_config.toml`

```toml
[llm]

[[llm.endpoints]]
id = "planner_chatgpt_group"
url = "https://chatgpt.com/gg/69c897e5a6448198a36a18b58f83de07"
role_markdown = "builtin:planner"
role = "planner"
stateful = true
max_tabs = 1

[[llm.endpoints]]
id = "mini_planner_chatgpt"
url = "https://chatgpt.com/gg/69ca778f7ea0819c8437275ff608eb35"
role_markdown = "builtin:planner"
role = "mini_planner"
stateful = true
max_tabs = 1

[[llm.endpoints]]
id = "intent_chatgpt"
url = "https://chatgpt.com/gg/69ca8d22f5f4819abed326a8343f9467"
role_markdown = "builtin:planner"
role = "intent"
stateful = true
max_tabs = 1

[[llm.endpoints]]
id = "router_chatgpt_group"
url = "https://chatgpt.com/gg/69bb5dbc591c819cb924366acba795bd"
role_markdown = "builtin:planner"
role = "router"
stateful = true
max_tabs = 1

[[llm.endpoints]]
id = "goal_gen_chatgpt"
url = "https://chatgpt.com/gg/69c229260fa08194ad30bb0e1f57105a"
role_markdown = "builtin:planner"
role = "goal_gen"
stateful = false
max_tabs = 1

[[llm.endpoints]]
id = "analyst_chatgpt"
url = ["https://chatgpt.com/gg/69c22949e1e881948abaf7016ef8be4c",
          url,
          url]
role_markdown = "builtin:planner"
role = "analyst"
stateful = true
max_tabs = 1

[[llm.endpoints]]
id = "verifier_chatgpt"
url = "https://chatgpt.com/gg/69ca70d1a4208199a3d1c4c77e87c147"
role_markdown = "builtin:planner"
role = "verifier"
stateful = true
max_tabs = 1

[[llm.endpoints]]
id = "diagnostics_chatgpt"
url = "https://chatgpt.com/gg/69caa6e708108198b02c2d2eaea30118"
role_markdown = "builtin:planner"
role = "diagnostics"
stateful = true
max_tabs = 1

[[llm.endpoints]]
id = "harness_repair_chatgpt"
url = "https://chatgpt.com/gg/69c8f84bb6848194a9b9ca6eaf5819c6"
role_markdown = "builtin:planner"
role = "harness_repair"
stateful = true
max_tabs = 1

[[llm.endpoints]]
id = "harness_eventlog_chatgpt"
url = "https://chatgpt.com/gg/69c8f86cf14c81a0a1a9b4487bccd784"
role_markdown = "builtin:planner"
role = "harness_eventlog"
stateful = true
max_tabs = 1

[[llm.endpoints]]
id = "mini_agent_chatgpt"
url = "https://chatgpt.com/gg/69ca500acd888199a32b90339c82fa31"
role_markdown = "builtin:planner"
role = "executor"
stateful = true
max_tabs = 1

[[llm.endpoints]]
id = "exec_chatgpt_a"
url = "https://chatgpt.com/gg/699c50e06bc881a3aa5ac1866bf15679"
role_markdown = "builtin:exec"
role = "exec"
stateful = true
max_tabs = 1

[[llm.endpoints]]
id = "exec_chatgpt_b"
url = "https://chatgpt.com/gg/69bb69adfe54819f9b4100d97cbd3912"
role_markdown = "builtin:exec"
role = "exec"
stateful = true
max_tabs = 1

[[llm.endpoints]]
id = "exec_chatgpt_c"
url = "https://chatgpt.com/gg/69bca5997968819c90b2dbe13d4290b6"
role_markdown = "builtin:exec"
role = "exec"
stateful = true
max_tabs = 1

[[llm.endpoints]]
id = "exec_chatgpt_d"
url = "https://chatgpt.com/gg/69ab7b06a5a88196bf33966df6feee02"
role_markdown = "builtin:exec"
role = "exec"
stateful = true
max_tabs = 1

[[llm.endpoints]]
id = "exec_chatgpt_e"
url = "https://chatgpt.com/gg/69929b2174b48191a3e25a56524ca8e5"
role_markdown = "builtin:exec"
role = "exec"
stateful = true
max_tabs = 1

[[llm.endpoints]]
id = "exec_chatgpt_f"
url = "https://chatgpt.com/gg/699271537d0c819f88b1b8ec9c068b69"
role_markdown = "builtin:exec"
role = "exec"
stateful = true
max_tabs = 1

[[llm.endpoints]]
id = "exec_chatgpt_g"
url = "https://chatgpt.com/gg/69ca3cec39ec8195911764cda77d15a0"
role_markdown = "builtin:exec"
role = "exec"
stateful = true
max_tabs = 1

[[llm.endpoints]]
id = "exec_chatgpt_h"
url = "https://chatgpt.com/gg/69c265cd2274819690fc291ef716524e"
role_markdown = "builtin:exec"
role = "exec"
stateful = true
max_tabs = 1

# --- Role configs ---

[llm.roles.planner]
burst = 4
[llm.roles.planner.weights]
planner_chatgpt_group = 100

[llm.roles.mini_planner]
burst = 1
[llm.roles.mini_planner.weights]
mini_planner_chatgpt = 100

[llm.roles.router]
burst = 4
[llm.roles.router.weights]
router_chatgpt_group = 100

[llm.roles.goal_gen]
burst = 1
[llm.roles.goal_gen.weights]
goal_gen_chatgpt = 100

[llm.roles.analyst]
burst = 1
[llm.roles.analyst.weights]
analyst_chatgpt = 100

[llm.roles.verifier]
burst = 1
[llm.roles.verifier.weights]
verifier_chatgpt = 100

[llm.roles.diagnostics]
burst = 1
[llm.roles.diagnostics.weights]
diagnostics_chatgpt = 100

[llm.roles.harness_repair]
burst = 1
[llm.roles.harness_repair.weights]
harness_repair_chatgpt = 100

[llm.roles.harness_eventlog]
burst = 1
[llm.roles.harness_eventlog.weights]
harness_eventlog_chatgpt = 100

[llm.roles.mini_agent]
burst = 1
[llm.roles.mini_agent.weights]
mini_agent_chatgpt = 100

[llm.roles.executor]
burst = 1
[llm.roles.executor.weights]
mini_agent_chatgpt = 50
exec_chatgpt_d = 50

[llm.roles.exec]
burst = 8
[llm.roles.exec.weights]
exec_chatgpt_a = 100
exec_chatgpt_b = 100
exec_chatgpt_c = 100
exec_chatgpt_d = 100
exec_chatgpt_e = 100
exec_chatgpt_f = 100
exec_chatgpt_g = 100
exec_chatgpt_h = 100

[llm.roles.decompose]
burst = 8
[llm.roles.decompose.weights]
exec_chatgpt_a = 100
exec_chatgpt_b = 100
exec_chatgpt_c = 100
exec_chatgpt_d = 100
exec_chatgpt_e = 100
exec_chatgpt_f = 100
exec_chatgpt_g = 100
exec_chatgpt_h = 100
```

## agents.cards additions (append to existing list)

```toml
[[agents.cards]]
agent_url = "https://chatgpt.com/gg/69ca3cec39ec8195911764cda77d15a0"
agent_id = "exec_g"
role = "builtin:exec"
goal = "AGENT_GOAL.md"
tool_capabilities = ["apply_patch", "bash"]

[[agents.cards]]
agent_url = "https://chatgpt.com/gg/69c265cd2274819690fc291ef716524e"
agent_id = "exec_h"
role = "builtin:exec"
goal = "AGENT_GOAL.md"
tool_capabilities = ["apply_patch", "bash"]
```

## Summary of changes

- Removed all `[llm.endpoints.xxx]` named table entries (lines ~128–264)
- Removed duplicate `[[llm.endpoints]]` array entries for endpoints that appeared in both formats
- Unified all 19 endpoints into a single `[[llm.endpoints]]` list
- Added `exec_chatgpt_g` and `exec_chatgpt_h` (two new URLs from chatgpt_url.txt)
- Updated `roles.exec` and `roles.decompose` burst from 6 → 8, added weights for g and h
- Added two new `[[agents.cards]]` entries for exec_g and exec_h
- Note: `exec_chatgpt_d` role changed from `executor` (in array) / `exec` (in map) to `exec` for consistency with other exec endpoints; `executor` role pool now uses only `mini_agent_chatgpt` + `exec_chatgpt_d` as before
