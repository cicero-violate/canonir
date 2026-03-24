# Implementation Plan: Goal Gen Endpoint + agent_id Fixes

## Problems

### 1. GoalGenConsumer has no dedicated endpoint
`goal_gen_consumer.rs` emits `LlmCall` with `role: "planner"` and `agent_id: None`.
This routes goal generation through the planner tab, polluting its stateful conversation
context before any planning has started. Goal generation must have its own isolated tab
with its own URL in `capability_config.toml`.

### 2. agent_id is null in all top-level LLM calls
- `canon-loop/src/stage/plan.rs` line 269: `agent_id: ctx.agent_id.clone()` — `ctx.agent_id`
  is `None` because `LoopStageExecutor::new(...)` is called without `.with_agent_id(...)`.
- `canon-route/src/executor.rs` line 82: `agent_id: None` — hardcoded.
- `goal_gen_consumer.rs` line 71: `agent_id: None` — hardcoded.

`agent_id: null` forces the LLM worker to fall back to role-based endpoint selection on
every call. For stateful tabs this means tab assignment is non-deterministic across
restarts and cannot be pinned to a specific conversation thread.

---

## Changes

### File 1: `canon-agent-prompts/capability_config.toml`

Add a new `goal_gen` endpoint block after the `[llm.endpoints.router]` block.
The URL must be a new ChatGPT custom GPT URL distinct from all existing endpoints.

```toml
[llm.endpoints.goal_gen]
id = "goal_gen_chatgpt"
url = "PLACEHOLDER_URL"
role_markdown = "builtin:planner"
role = "goal_gen"
stateful = false
max_tabs = 1
```

Set `stateful = false` — goal generation is a single-shot call with no follow-up turns.
A new chat context every time ensures the generator is not biased by prior goals.

Also add the role weight + burst config:

```toml
[llm.roles.goal_gen.weights]
goal_gen_chatgpt = 100

[llm.roles.goal_gen]
burst = 1
```

And add the agent card so the tab is opened at startup:

```toml
[[agents.cards]]
agent_url = "PLACEHOLDER_URL"
agent_id = "goal_gen"
role = "builtin:planner"
goal = "AGENT_GOAL.md"
tool_capabilities = []
```

Replace both `PLACEHOLDER_URL` values with the actual ChatGPT custom GPT URL for the
goal generator before running.

---

### File 2: `canon-utils/canon-runtime/src/consumers/goal_gen_consumer.rs`

Change lines 69–72 from:
```rust
role: Some("planner".to_string()),
agent_id: None,
```
to:
```rust
role: Some("goal_gen".to_string()),
agent_id: Some("goal_gen_chatgpt".to_string()),
```

Also fix `extract_goal_text` — it currently calls `.to_string()` on the entire
`serde_json::Value` (which JSON-encodes it). The response value is `{"text": "..."}` when
`normalize_llm_output` falls through. Extract the inner text field:

Change line 80 from:
```rust
CapabilityResult::Llm(res) => extract_goal_text(&res.response.to_string()),
```
to:
```rust
CapabilityResult::Llm(res) => {
    let raw = res.response.get("text")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| res.response.as_str().unwrap_or(""));
    extract_goal_text(raw)
}
```

---

### File 3: `canon-utils/canon-runtime/src/bin/event_runtime.rs`

Change line 256 from:
```rust
Box::new(LoopStageExecutor::new(workspace.clone(), tlog_path.clone())),
```
to:
```rust
Box::new(LoopStageExecutor::new(workspace.clone(), tlog_path.clone())
    .with_agent_id("planner_chatgpt_group".to_string())),
```

---

### File 4: `canon-utils/canon-route/src/executor.rs`

Change line 82 from:
```rust
agent_id: None,
```
to:
```rust
agent_id: Some("router_chatgpt_group".to_string()),
```

---

## Summary

| File | Change |
|---|---|
| `capability_config.toml` | Add `goal_gen` endpoint, role weights, agent card |
| `goal_gen_consumer.rs` | Use `role="goal_gen"`, `agent_id="goal_gen_chatgpt"`, fix response extraction |
| `event_runtime.rs` | `.with_agent_id("planner_chatgpt_group")` on `LoopStageExecutor` |
| `executor.rs` (route) | `agent_id: Some("router_chatgpt_group")` |

The `PLACEHOLDER_URL` in `capability_config.toml` must be filled in with the real
goal_gen ChatGPT URL before building. All other changes are mechanical and can be
applied immediately.
