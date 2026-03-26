# Plan: Context-Base Caching (Stop Sending GOAL Every Message)

## Problem

`build_context_message` bundles GOAL + workspace tree + facts + search hints + sub-agents
into the same string as LOC + errors + recent actions + recent tool results. The GOAL text
rarely changes, but the entire bundle is re-sent on every planning call that fires (i.e.,
every time recent_actions or error_count changes). This wastes tokens and makes every LLM
message unnecessarily large.

## Solution

Split context into two tiers, mirroring the system-prompt caching already in place:

| Tier | Content | Changes when | Sent |
|------|---------|--------------|------|
| System (`system` / `system_prompt_id`) | Static tool/workflow instructions | Never | First call only |
| Context base (`context_base` / `context_base_id`) | GOAL, workspace tree, facts, search hints, sub-agent section | Goal changes, facts update, workspace tree changes | First call + when hash changes |
| Delta (`prompt`) | TARGET WORKSPACE, LOC, errors, warnings, recent actions, recent tool results, destructive note | Every action | Every call |

For **stateful** endpoints: worker sends only the delta as the next "user turn"; the LLM
already has the context base in session history.

For **stateless** endpoints: worker reconstructs `system + context_base + delta` before
sending, just as it currently does for `system`.

---

## Step 1 — Add `context_base` field to `LlmCall`

File: `canon-utils/canon-runtime-events/src/events.rs`

In the `LlmCall` `canon_event_struct!`, add a new optional field after `system_prompt_id`:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
context_base: Option<String>,      // slow-changing section; cached in worker
#[serde(default, skip_serializing_if = "Option::is_none")]
context_base_id: Option<String>,   // hash of context_base for cache lookup
```

`context_base` is `Some(...)` only when it differs from the cached value in the worker.
`context_base_id` is always `Some(hash)` so the worker knows which cached base to use.

---

## Step 2 — Add `last_context_base_id` to `LoopContext`

File: `canon-utils/canon-loop/src/context.rs`

In the `LoopContext` struct, below `last_delta_hash`, add:

```rust
pub last_context_base_id: Option<u64>,
```

In `LoopContext::new()`, initialise it:

```rust
last_context_base_id: None,
```

**Do NOT clear this on `LoopActed` or `LoopVerified`.** The context base (GOAL + workspace
tree) is independent of individual actions. Only clear it when goal_text or workspace_facts
change — which is already captured by recomputing the base hash on every `handle_observed`
call.

---

## Step 3 — Split `build_context_message` into two functions

File: `canon-utils/canon-loop/src/stage/plan.rs`

### 3a — `build_context_base`

New function. Takes `observed: &LoopObserved`, `workspace: &Path`, `sub_agent_section: &str`.
Returns the slow-changing portion:

```
GOAL:
{goal_text}

## Workspace State
{workspace_tree}

Workspace facts:
{workspace_facts}

Relevant files:{search_hints}

{sub_agent_section}
```

No LOC, no errors, no recent actions, no recent results.

### 3b — `build_context_delta`

New function. Takes `observed: &LoopObserved`, `batch_acted: &[LoopActed]`,
`batch_tool_results: &[ToolResult]`, `target_workspace: &str`.

Returns the fast-changing portion:

```
TARGET WORKSPACE: {target_workspace}
All relative paths resolve against TARGET WORKSPACE (not its parent).
LOC: {loc}  |  Errors: {errors}  |  Warnings: {warnings}
{destructive_note}
Recent actions (most recent first — read_file stdout contains file contents):
{recent_actions}

Recent tool results:
{recent_results}
```

### 3c — Remove `build_context_message`

Replace all callers with calls to `build_context_base` + `build_context_delta`.

---

## Step 4 — Update `handle_observed` in `plan.rs`

Replace the current single `build_context_message` call with:

```rust
let sub_agent_section = ctx.context_merger.prompt_section();
let context_base = build_context_base(observed, &ctx.workspace.clone(), &sub_agent_section);
let context_base_hash = hash_str(&context_base);

let spec = parse_agent_goal_markdown(&goal_text);
let target_workspace = spec.target_path.map(|p| p.display().to_string())
    .unwrap_or_else(|| workspace.display().to_string());
let context_delta = build_context_delta(
    observed,
    &ctx.batch_acted,
    &ctx.batch_tool_results,
    &target_workspace,
);

let system_id = *PLANNER_SYSTEM_PROMPT_ID;
let send_system = ctx.last_system_prompt_id != Some(system_id);
let send_base = ctx.last_context_base_id != Some(context_base_hash);

// Drop: nothing changed at any tier.
let delta_hash = hash_str(&context_delta);
if !send_system && !send_base && ctx.last_delta_hash == Some(delta_hash) {
    return Ok(LoopStageResult::Noop);
}

// Update tracking.
ctx.last_system_prompt_id = Some(system_id);
if send_base {
    ctx.last_context_base_id = Some(context_base_hash);
}
ctx.last_delta_hash = Some(delta_hash);

// Build LlmCall.
let llm_call = LlmCall {
    request_id: request_id.clone(),
    prompt: context_delta,                          // fast-changing delta only
    role: Some("planner".to_string()),
    agent_id: ctx.agent_id.clone(),
    dispatched: true,
    system: send_system.then(|| PLANNER_SYSTEM_INSTRUCTIONS.to_string()),
    system_prompt_id: Some(system_id.to_string()),
    context_base: send_base.then(|| context_base),
    context_base_id: Some(context_base_hash.to_string()),
    prompt_base_id: Some(system_id.to_string()),
    prev_prompt_id,
};
```

---

## Step 5 — Update worker in `llm.rs`

File: `canon-utils/canon-exec/src/exec/llm.rs`

### 5a — Extend `LlmWork`

Add to the `LlmWork` struct:

```rust
context_base: Option<String>,
context_base_id: Option<String>,
```

Populate from the `LlmCall` fields.

### 5b — Add `context_base_cache`

In the worker loop, alongside `system_cache`:

```rust
let mut context_base_cache: HashMap<String, String> = HashMap::new();
```

### 5c — Cache update

After receiving a work item, if `context_base` is `Some`:

```rust
if let (Some(id), Some(base)) = (&context_base_id, &context_base) {
    context_base_cache.insert(id.clone(), base.clone());
}
```

### 5d — Reconstruct `full_prompt`

Three-tier reconstruction. Replace the existing two-tier `full_prompt` logic:

```rust
let full_prompt = if system_was_sent {
    // First call: system was included — reconstruct system + base (if present) + delta.
    let sys = system_cache.get(system_prompt_id.as_deref().unwrap_or(""))
        .map(String::as_str).unwrap_or("");
    let base = context_base_id.as_deref()
        .and_then(|id| context_base_cache.get(id))
        .map(String::as_str).unwrap_or("");
    if base.is_empty() {
        format!("{sys}\n\n{prompt}")
    } else {
        format!("{sys}\n\n{base}\n\n{prompt}")
    }
} else if endpoint.stateful {
    // Stateful LLM already has system and (if unchanged) context_base in session history.
    // Send only the delta.
    prompt.clone()
} else {
    // Stateless: reconstruct everything from cache.
    let sys = system_prompt_id.as_ref()
        .and_then(|id| system_cache.get(id))
        .map(String::as_str).unwrap_or("");
    let base = context_base_id.as_deref()
        .and_then(|id| context_base_cache.get(id))
        .map(String::as_str).unwrap_or("");
    match (sys.is_empty(), base.is_empty()) {
        (false, false) => format!("{sys}\n\n{base}\n\n{prompt}"),
        (false, true)  => format!("{sys}\n\n{prompt}"),
        (true,  false) => format!("{base}\n\n{prompt}"),
        (true,  true)  => prompt.clone(),
    }
};
```

---

## Step 6 — Update all `LlmCall` construction sites

Every `LlmCall { ... }` literal needs the two new fields:

```rust
context_base: None,
context_base_id: None,
```

Sites to update (already have `system: None, system_prompt_id: None`):
- `canon-utils/canon-loop/src/stage/decompose.rs`
- `canon-utils/canon-route/src/executor.rs`
- `canon-utils/canon-route/src/helpers.rs`
- `canon-utils/canon-runtime/src/bin/llm_smoke_test.rs`
- `canon-utils/canon-runtime/src/consumers/goal_gen_consumer.rs`
- `canon-utils/canon-runtime/src/consumers/analyst_consumer.rs` (3 instances)

---

## Expected Outcome

- First planning call: sends `system` + `context_base` + `delta` (full context, same as today).
- Subsequent calls (goal unchanged, workspace stable): sends only `delta` (errors, LOC, actions, results).
- If goal changes mid-session: sends new `context_base` + `delta` (GOAL appears exactly once on change).
- Stateless endpoints: worker always reconstructs `system + context_base + delta` from cache before API call — same correctness guarantee as today.
- Token usage on repeated planning loops is reduced by ~60–80% of context size (GOAL + workspace tree is the bulk of the message).
