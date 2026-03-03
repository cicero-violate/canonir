# _IMPLEMENTATION_PLAN.md (rev 2)
# DAG-Control Implementation Plan (Case A — 4 Agents) — multi-dag

## What Is Broken (Current State)

### Problem 1 — Only one tab opens
`call_agent_json` routes by role string ("decompose", "planner", "executor",
"verifier"). `config.card_by_role` matches on `agent_id` field. These must
agree. If `agent_config.toml` has `agent_id = "decompose"` for D_g but the
tab slot lookup uses a different key, only one tab ever opens and every
subsequent agent call reuses it.

Fix: Audit `agent_config.toml`. Confirm each card has:
```toml
[[agents.cards]]
agent_id = "decompose"   # must exactly match role key used in call_agent_json
agent_url = "https://chatgpt.com/gg/69a5aa249554819e9ac25e2df27102f1"

[[agents.cards]]
agent_id = "planner"
agent_url = "https://chatgpt.com/gg/69a32d7d1a008199948ad06498df2f4f"

[[agents.cards]]
agent_id = "executor"
agent_url = "https://chatgpt.com/gg/699c50e06bc881a3aa5ac1866bf15679"

[[agents.cards]]
agent_id = "verifier"
agent_url = "https://chatgpt.com/gg/6992c359272881a19d30c226925f575d"
```

In `llm.rs`, `get_or_open_tab` must open a NEW tab per role. The current
implementation calls `open_fresh_tab_with_url` which is correct, but only if
`get_tab_id` returns `None` for each distinct role on first access. Verify
`DagTabSlots` fields map exactly to the four role strings.

### Problem 2 — System prompt race / double send_turn
`ensure_system_prompt` sends the system prompt as a `send_turn` call, then
`call_agent_json` immediately fires the actual prompt as another `send_turn`
with no wait between them. ChatGPT processes one message at a time. The second
message arrives while the first response is still streaming → timeout.

Fix: After sending the system prompt, wait for its response before proceeding.
`ensure_system_prompt` must `await` the `send_turn` result AND read/discard
the response so the tab is idle before the real prompt is sent.

Current (broken):
```
send_turn(system_prompt)   // fires, does NOT wait for response
send_turn(actual_prompt)   // fires immediately → timeout
```

Required:
```
response = send_turn(system_prompt).await   // send AND drain response
set_system_sent = true
response = send_turn(actual_prompt).await   // now tab is idle, safe
```

### Problem 3 — DAG loop runs once then sets completed=true
`run_tick` calls `run_dag_loop` once. On any error (including timeout),
`state.completed = true` is set and the pipeline never re-enters.

Fix: `run_tick` must NOT set `completed = true` on error. Only set it on
`Ok(())`. On error, return a non-advancing outcome so the outer tick loop
retries on the next tick.
```rust
match self.run_dag_loop(ctx).await {
    Ok(()) => {
        state.completed = true;
        Ok(PipelineOutcome { reward: 1.0, summary: "dag completed".into(), advanced: true })
    }
    Err(e) => {
        // do NOT set completed — allow retry
        Ok(PipelineOutcome { reward: -1.0, summary: format!("dag error: {e}"), advanced: false })
    }
}
```

### Problem 4 — Goal is empty in D_g system prompt
`build_system_prompt("decompose")` reads `card.goal_markdown` which is loaded
from `goal_path` in `agent_config.toml`. If the file path is wrong or the
file is empty, the agent receives a blank goal.

Fix: Add a fallback. If `goal_markdown` is empty, read from a canonical goal
file at a known path, e.g.:
```
/workspace/ai_sandbox/canon/canon-agent-prompts/GOAL.md
```
Or pass the goal text explicitly into `build_system_prompt` so it does not
depend on card file content.

### Problem 5 — No retry on timeout
A single `send_turn` timeout aborts the entire DAG run. There is no retry.

Fix: Wrap `call_agent_json` with a retry loop. Retry up to 3 times with a
short delay between attempts before propagating the error.
```rust
pub async fn call_agent_json_with_retry(
    bridge: &WsBridge,
    url: &str,
    role: &str,
    prompt: &str,
    system_prompt: &str,
    tabs: &Mutex<DagTabSlots>,
    max_retries: u32,
) -> Result<Value> {
    let mut last_err = None;
    for attempt in 0..max_retries {
        match call_agent_json(bridge, url, role, prompt, system_prompt, tabs).await {
            Ok(v) => return Ok(v),
            Err(e) => {
                eprintln!("[llm] role={role} attempt={attempt} error={e}");
                last_err = Some(e);
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
    Err(last_err.unwrap())
}
```

---

## Module-Level Changes Required

### llm.rs

1. `ensure_system_prompt`: drain the response after sending system prompt.
   Change signature to return the raw response string, discard it at call site.

2. `call_agent_json`: call `ensure_system_prompt` first (already does this),
   then verify the tab is idle before sending the actual prompt. Add a
   configurable inter-message delay (e.g. 500ms) between system prompt ack
   and first real prompt if the bridge does not guarantee ordering.

3. Add `call_agent_json_with_retry` as above (max 3 retries, 5s delay).

4. `DagTabSlots`: add a `system_prompt_response` field per role if needed to
   confirm the system prompt was acknowledged. Alternatively rely on
   `send_turn` blocking until the response is complete — confirm this is the
   case in `WsBridge::send_turn`.

   **Key question to resolve**: Does `WsBridge::send_turn` block until the
   full ChatGPT response is received, or does it return as soon as the message
   is sent? If it returns early, `ensure_system_prompt` must explicitly wait
   for the response before returning.

### mod.rs

1. `run_tick`: do not set `completed = true` on error. Return `advanced: false`
   on error so the outer loop retries.

2. `run_dag_loop`: the loop already runs up to `MAX_ITERS = 50`. Do not
   change the iteration logic. Fix the error propagation so a single failed
   iteration does not abort — catch per-agent errors and log them, then
   continue the loop if the graph is not fully blocked.

   Specifically: if `execute_ready` or `verify_graph` returns `Err`, log the
   error and continue to the next iteration rather than propagating with `?`.
   Only propagate if the graph is stuck (all failed, none ready).

3. Goal source: read the goal from an explicit file or from the context, not
   from `decompose_card.goal_markdown`. Pass goal text as a parameter to
   `run_dag_loop`.

### config.rs

`card_by_role` currently matches on `agent_id`. This is correct IF
`agent_config.toml` uses `agent_id` values of exactly "decompose", "planner",
"executor", "verifier". Verify this. If the TOML uses different values (e.g.
role names or URLs), fix `card_by_role` to match the actual field.

---

## Revised Implementation Steps

### Step 1 — Verify agent_config.toml
- Read the file with `python` and print all `agent_id` values.
- Confirm each matches the role keys used in `DagTabSlots` and
  `call_agent_json`.
- Fix any mismatches in the TOML (not in Rust code — the Rust is correct).

### Step 2 — Fix ensure_system_prompt in llm.rs
- Await and drain the system prompt response before returning.
- Confirm `send_turn` blocks on full response. If not, add explicit wait.
- Add 500ms delay after system prompt before real prompt is safe to send.

### Step 3 — Add retry to call_agent_json in llm.rs
- Implement `call_agent_json_with_retry(... max_retries: u32)`.
- Default max_retries = 3, delay = 5s.
- Replace all call sites with the retry variant.

### Step 4 — Fix run_tick error handling in mod.rs
- On `Err` from `run_dag_loop`, return `advanced: false`, do not set
  `completed = true`.
- This allows the outer tick loop to call `run_tick` again.

### Step 5 — Fix goal source in mod.rs
- Do not use `decompose_card.goal_markdown` as the goal text.
- Read goal from a dedicated file or from `ctx` context.
- Pass it explicitly into `run_dag_loop` and down to `decompose_goal`.

### Step 6 — Soft error handling in run_dag_loop in mod.rs
- Wrap `execute_ready` and `verify_graph` calls with explicit error capture.
- On per-iteration error: log it, do not propagate, continue loop.
- Only `bail!` if the graph is permanently stuck.

---

## Invariants Unchanged

1. Only `verify.rs` calls `TaskGraph::update_status` to `Completed`/`Failed`.
2. X and V never share a tab.
3. DAG validation runs once after planning, hard-errors on cycle.
4. Delta whitelist in `act.rs` unchanged.
5. No free-form shell.

---

## Verification Checklist (before next run)

- [ ] `agent_config.toml` has exactly 4 cards with agent_id matching
      "decompose", "planner", "executor", "verifier"
- [ ] Each card has a distinct `agent_url`
- [ ] `send_turn` in `WsBridge` blocks until full response received
- [ ] System prompt is sent and response is drained before real prompt
- [ ] `run_tick` returns `advanced: false` on error, not `true`
- [ ] All 4 tabs open on first run (check browser)
- [ ] Retry logic present in `call_agent_json`
