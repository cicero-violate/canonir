# System Prompt Caching

**Objective:** System instructions (tools/workflow/safety/output format) are immutable.
Assign `prompt_id = hash(system_prompt)` and send them only once per executor session.
Every subsequent call sends only the dynamic context (goal, workspace, errors, actions).

---

## Prompt Structure

```
┌──── SYSTEM PROMPT (const, ~600 tokens, sent once) ────────────┐
│ Role definition                                                │
│ ━━━ TOOLS ━━━ (list_dir, read_file, apply_patch, run_command) │
│ ━━━ WORKFLOW ━━━ (Step 1 Discover, Step 2 Edit)               │
│ Safety rules / Workspace rules                                 │
│ ━━━ OUTPUT FORMAT ━━━ (JSON array rules, example)             │
└────────────────────────────────────────────────────────────────┘
┌──── CONTEXT MESSAGE (dynamic, ~200–400 tokens, every call) ───┐
│ TARGET WORKSPACE + LOC                                         │
│ Errors / Warnings / Destructive note                           │
│ GOAL text                                                      │
│ Workspace tree + facts                                         │
│ Relevant files (search hints)                                  │
│ Sub-agent section                                              │
│ Recent actions                                                 │
│ Recent tool results                                            │
└────────────────────────────────────────────────────────────────┘
```

Full prompt for LLM API = `system_from_cache + "\n\n" + context_message`.
Reconstruction happens in the LLM executor worker thread.

---

## Invariants

```
system_prompt_id = H(PLANNER_SYSTEM_INSTRUCTIONS)   // computed once at startup
send_system      = ctx.last_system_prompt_id ≠ system_prompt_id
context_hash     = H(context_message)
drop             = context_hash == ctx.last_delta_hash ∧ !send_system
```

---

## Layer Changes

### 1. `plan.rs` — extract const + build context message

```rust
const PLANNER_SYSTEM_INSTRUCTIONS: &str = r#"..."#;  // ~600 token static block

static PLANNER_SYSTEM_PROMPT_ID: LazyLock<u64> =
    LazyLock::new(|| hash_str(PLANNER_SYSTEM_INSTRUCTIONS));

fn build_context_message(observed, batch_acted, batch_tool_results, workspace, sub_agent_section) -> String {
    // goal + workspace tree + facts + errors + recent actions + results
}
```

In `handle_observed`:
1. `system_id = *PLANNER_SYSTEM_PROMPT_ID`
2. `send_system = ctx.last_system_prompt_id != Some(system_id)`
3. `context = build_context_message(...)`
4. `context_hash = hash_str(&context)`
5. If `!send_system && context_hash == ctx.last_delta_hash` → Noop
6. Update `ctx.last_system_prompt_id = Some(system_id)`, `ctx.last_delta_hash = Some(context_hash)`
7. Emit `LlmCall { system: send_system.then(|| PLANNER_SYSTEM_INSTRUCTIONS), system_prompt_id, prompt: context, ... }`

### 2. `LlmCall` event (`events.rs`)

Add fields:
```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
system: Option<String>,            // static block — Some on first call, None thereafter
#[serde(default, skip_serializing_if = "Option::is_none")]
system_prompt_id: Option<String>,  // hash ID for cache lookup
```

`prompt` field remains but now contains only dynamic context (~200–400 tokens vs ~1000+).

### 3. `LlmWork` + worker (`llm.rs`)

```rust
pub(crate) struct LlmWork {
    // ...existing...
    system: Option<String>,            // static block to store in cache
    system_prompt_id: Option<String>,  // cache key
}
```

Worker maintains `system_cache: HashMap<String, String>` (local to the thread).

```rust
// At top of worker loop, before the LLM call:
if let (Some(id), Some(sys)) = (system_prompt_id.as_ref(), system) {
    system_cache.insert(id.clone(), sys);
}
let full_prompt = match system_prompt_id.as_ref().and_then(|id| system_cache.get(id)) {
    Some(sys) => format!("{}\n\n{}", sys, prompt),
    None      => prompt.clone(),
};
// Use full_prompt for prompt_with_request_id instead of prompt.
```

### 4. `LoopContext` cleanup

Replace `prompt_base_id: Option<u64>` + `prompt_base_cache: Option<String>` with:
```rust
pub last_system_prompt_id: Option<u64>,  // tracks which system prompt executor has seen
```
Keep `last_delta_hash: Option<u64>`.

Clear `last_system_prompt_id` and `last_delta_hash` on `LoopActed` / `LoopVerified` (same as `last_emitted_plan_hash`).

---

## Success Criteria

| Condition | Mechanism |
|-----------|-----------|
| System instructions sent only once | `send_system` gate + worker cache |
| Repeated loops → near-zero context | `last_delta_hash` Noop gate |
| No duplicate instructions | `system` field is `None` after first call |
| Full prompt always correct | Reconstructed in worker: `system + context` |
| Deterministic reconstruction | `system + "\n\n" + context` concatenation |
