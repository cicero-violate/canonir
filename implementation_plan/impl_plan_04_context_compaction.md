# Implementation Plan 04 — Context Compaction

## Goal

When the analyst's multi-turn session grows long, automatically summarize the
accumulated history into a compact digest and continue the session from a clean
context. This makes analyst sessions effectively unbounded in length without hitting
LLM context limits.

---

## Background

The current analyst state tracks `turn: usize`. At high turn counts, earlier Python
results and analysis observations are stale — the model only needs the synthesis so
far. Compaction: send accumulated context to a low-cost model (or same model),
receive a 200-word digest, restart conversation from `[system_prompt, digest,
current_result]`.

---

## Step 1 — Add compaction threshold config

In `analyst_consumer.rs`, add:

```rust
/// After this many turns, compact the conversation before continuing.
const COMPACTION_TURN_THRESHOLD: usize = 8;
```

---

## Step 2 — Extend `State::PendingLlm`

Add a `history` field to carry the compacted digest (replacing full history):

```rust
enum State {
    Idle { ticks_since_reward: u64, cooldown_ticks: u64 },
    PendingLlm {
        request_id: String,
        turn: usize,
        /// Optional compact digest of earlier turns. If Some, injected before
        /// the current prompt instead of the full system prompt.
        compact_digest: Option<String>,
    },
    CompactionPending {
        /// The compaction summary request ID.
        compaction_request_id: String,
        /// The pending Python result or nudge to deliver after compaction.
        next_prompt: String,
        /// The original turn count, preserved across compaction.
        turn: usize,
    },
}
```

---

## Step 3 — Add `new_compaction_request_id` to `LlmCall`

To distinguish compaction calls from analysis calls, set `role: Some("analyst_compact")`
and `agent_id: Some("analyst_chatgpt")` on the compaction `LlmCall`. Route it to the
same endpoint but with a specific role name so the endpoint worker can be stateful=false
for this call (compaction is always a one-shot summarize).

Add a new endpoint entry to `capability_config.toml`:

```toml
[llm.endpoints.analyst_compact]
id = "analyst_compact_chatgpt"
url = "https://chatgpt.com/gg/69c22949e1e881948abaf7016ef8be4c"  # same URL as analyst
role_markdown = ""
role = "analyst_compact"
stateful = false
max_tabs = 1
```

---

## Step 4 — Implement compaction logic in `AnalystConsumer`

Add method `compact_session`:

```rust
fn compact_session(&mut self, accumulated_text: String, next_prompt: String, turn: usize) -> EventOutcome {
    let compaction_prompt = format!(
        "Summarize the analysis session below in ≤200 words. Capture: phases completed, \
         key findings, Python results that matter, and any conclusions reached. \
         Omit raw data. Output ONLY the summary.\n\n---\n{accumulated_text}"
    );
    let request_id = Uuid::new_v4().to_string();
    self.state = State::CompactionPending {
        compaction_request_id: request_id.clone(),
        next_prompt,
        turn,
    };
    EventOutcome::Emit(RuntimeEvent::Llm(LlmCall {
        request_id,
        prompt: compaction_prompt,
        role: Some("analyst_compact".to_string()),
        agent_id: Some("analyst_chatgpt".to_string()),
        effort: None,
    }))
}
```

`accumulated_text` is built by storing each (prompt, response) pair in a
`Vec<(String, String)>` inside `PendingLlm`. See accumulation below.

---

## Step 5 — Accumulate turn text in `PendingLlm`

Add `turns_text: Vec<String>` to `State::PendingLlm`. Each time `continue_session`
is called, push the Python result block to `turns_text` before returning the
`EventOutcome`. In `on_event` when `CapabilityCompleted` arrives for the analysis
role, push the response text to `turns_text` as well.

---

## Step 6 — Trigger compaction in `continue_session`

At the top of `continue_session`:

```rust
if turn >= COMPACTION_TURN_THRESHOLD {
    let accumulated = turns_text.join("\n\n---\n\n");
    return self.compact_session(accumulated, result_block, turn);
}
```

Where `result_block` is the Python result that would have been the next prompt.

---

## Step 7 — Handle `CompactionPending` in `on_event`

When `CapabilityCompleted` arrives and `self.state` is `CompactionPending`:
- Check `done.request_id == compaction_request_id`
- Extract the digest text from `done.result`
- Build a fresh `LlmCall`:
  - `prompt = format!("## Session summary\n{digest}\n\n## Python result\n```\n{next_prompt}\n```")`
  - `role = Some("analyst")`
  - `effort = Some(LlmEffort::Medium)`
- New `request_id`, transition to `State::PendingLlm { request_id, turn, compact_digest: Some(digest), turns_text: Vec::new() }`
- Return `EventOutcome::Emit(RuntimeEvent::Llm(...))`

---

## Step 8 — Use digest in `start_session` continuation

When `compact_digest` is `Some(digest)` and the session continues, the prompt
is built from the digest rather than the full system prompt:

```
## Session digest (prior turns)
{digest}

## Current result
{result_block}
```

The full `SYSTEM_PROMPT` is NOT re-sent after compaction (the stateful endpoint
already has it). Only the digest + new result.

---

## Verification

```
cargo check -p canon-runtime
```

Set `COMPACTION_TURN_THRESHOLD = 3` temporarily. Run runtime until analyst fires.
Confirm:
- At turn 3, a `Llm` event with `role = "analyst_compact"` is emitted
- After the compaction response, a new `Llm` event with the digest in its prompt
  is emitted
- The session continues to produce a final report
