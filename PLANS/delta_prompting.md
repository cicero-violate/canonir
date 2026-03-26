# Delta-Based LLM Prompting

**Objective:** Stop resending the full prompt every LLM call. Cache the static base; send only what's new (delta) each call. Drop calls where nothing changed.

---

## Prompt Anatomy

```
┌────────────────── BASE (slow-changing) ──────────────────┐
│ System instructions (tools, workflow, safety rules)       │
│ Goal text                                                 │
│ Workspace tree                                            │
│ Workspace facts                                           │
│ Search hints                                              │
│ Sub-agent section                                         │
└──────────────────────────────────────────────────────────┘
┌────────────────── DELTA (fast-changing) ─────────────────┐
│ Error/warning counts + destructive note                   │
│ Recent actions (batch_acted)                              │
│ Recent tool results (batch_tool_results)                  │
└──────────────────────────────────────────────────────────┘
```

Full prompt sent to LLM = `base + delta` (reconstructed in plan.rs before dispatch).

---

## Invariants

```
base_id     = H(base_string)
delta_hash  = H(delta_string)

drop iff base_id == prev_base_id ∧ delta_hash == prev_delta_hash
update_cache iff base_id ≠ prev_base_id
full_prompt = cached_base + delta
```

---

## Changes

### 1. `LoopContext` (`context.rs`)

New fields:
```rust
pub prompt_base_id: Option<u64>,         // hash of cached base
pub prompt_base_cache: Option<String>,   // the cached base string
pub last_delta_hash: Option<u64>,        // hash of last-sent delta
```
Clear `prompt_base_id`, `prompt_base_cache`, `last_delta_hash` when goal changes (new `last_prompted_goal`).

### 2. `LlmCall` event (`events.rs`)

New fields:
```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
prev_prompt_id: Option<String>,   // base_id from previous call (causal chain)
#[serde(default, skip_serializing_if = "Option::is_none")]
prompt_base_id: Option<String>,   // base_id used for this call
```
`prompt` field keeps the full reconstructed string (executor is unmodified).

### 3. `build_prompt` refactor (`plan.rs`)

Split into:
- `build_prompt_base(observed, workspace, sub_agent_section) -> String`
  — everything except error context + recent actions + recent results
- `build_prompt_delta(observed, batch_acted, batch_tool_results) -> String`
  — error context + recent actions + recent results section only
- `assemble_prompt(base, delta) -> String` — simple concatenation

### 4. `handle_observed` cache logic (`plan.rs`)

```
1. base  = build_prompt_base(...)
2. delta = build_prompt_delta(...)
3. base_id    = hash(base)
4. delta_hash = hash(delta)
5. if base_id == ctx.prompt_base_id && delta_hash == ctx.last_delta_hash → return Noop
6. if base_id != ctx.prompt_base_id → ctx.prompt_base_cache = Some(base); ctx.prompt_base_id = Some(base_id)
7. prev_prompt_id = ctx.prompt_base_id (before update in step 6)
8. ctx.last_delta_hash = Some(delta_hash)
9. full_prompt = ctx.prompt_base_cache.as_deref().unwrap_or("") + &delta
10. emit LlmCall { prompt: full_prompt, prev_prompt_id, prompt_base_id: base_id.to_string(), ... }
```

### 5. Cache invalidation

Clear `last_delta_hash` on `LoopActed` and `LoopVerified` in `executor.rs` so a subsequent
identical-base observation with new actions is not dropped.

---

## Success Criteria

| Condition | Mechanism |
|-----------|-----------|
| No resend of identical base | base_id hash gate |
| No resend if nothing new | delta_hash gate → Noop |
| Token usage decreases | delta replaces full prompt for repeated base |
| Deterministic reconstruction | `base + delta` string concatenation |
| Causal chain visible | `prev_prompt_id` in LlmCall |
