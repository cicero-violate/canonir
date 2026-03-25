# Implementation Plan 02 — Reasoning Budget (Effort Levels)

## Goal

Add a per-call `effort` field to `LlmCall` that threads through the endpoint worker
as a prompt prefix. High-effort calls prepend a reasoning-instruction header that
prompts the model to think longer before responding. This eliminates analyst
phase-skipping and loop-planner stalls without changing the endpoint configuration.

---

## Step 1 — Extend `LlmCall` in `canon-utils/canon-runtime-events/src/events.rs`

Add to the `canon_event_struct!(LlmCall { ... })` block:

```
#[serde(default)]
effort: Option<LlmEffort>,
```

Add enum before the struct:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LlmEffort {
    #[default]
    Medium,
    Low,
    High,
    Max,
}
```

Export `LlmEffort` from the crate's `lib.rs`.

---

## Step 2 — Thread effort through the capability executor

**File:** `canon-utils/canon-exec/src/exec/llm.rs`

Find where `LlmCall` fields are used to build the `LlmWorkItem`. Add effort to the
`phase` string or as a separate field on `LlmWorkItem`:

Add `pub effort: LlmEffort` to `LlmWorkItem` in
`canon-utils/canon-llm-runtime/src/endpoint_worker.rs`.

In `llm_worker_send_request` and `llm_worker_send_request_with_req_id`, accept
`effort: LlmEffort` as a parameter and pass it into `LlmWorkItem`.

---

## Step 3 — Apply effort as a prompt prefix in `endpoint_worker.rs`

In `LlmWorker::send_turn`, after building `raw_prompt` and before the truncation
check, prepend the effort header:

```
Low:    (no prefix)
Medium: (no prefix — default behavior)
High:   prepend: "Think carefully and thoroughly before responding. Work through
         all required steps in order. Do not skip phases or jump to conclusions.\n\n"
Max:    prepend: "This is a complex task requiring deep analysis. Think step by
         step through every aspect before giving your final answer. Verify your
         reasoning at each stage.\n\n"
```

The prefix is injected only when `include_role` is false (continuation turns) OR
always (first turn). Keep it unconditional — the cost is negligible tokens.

---

## Step 4 — Update call sites

### `canon-utils/canon-runtime/src/consumers/analyst_consumer.rs`

- `start_session`: set `effort: Some(LlmEffort::High)` on the `LlmCall`
- `continue_session`: set `effort: Some(LlmEffort::Medium)` (Python result turns)
- `continue_session_no_python`: set `effort: Some(LlmEffort::High)` (nudge turns
  are high-stakes — model must reason about why it skipped phases)
- In `finish_session`: effort is irrelevant (no LlmCall emitted)

### `canon-utils/canon-loop/src/stage/plan.rs`

Find where `RuntimeEvent::Llm(LlmCall { ... })` is constructed. Add:
- Default: `effort: None` (medium)
- When `ctx.stagnant_ticks` (or equivalent stall counter) exceeds a threshold
  (e.g., 5 consecutive identical `action_kind`): set `effort: Some(LlmEffort::High)`

### `canon-utils/canon-runtime/src/consumers/goal_gen_consumer.rs`

- First attempt: `effort: None` (medium)
- On retry (retries > 0): `effort: Some(LlmEffort::High)` — force careful formatting

---

## Step 5 — Propagate through `canon-exec/src/exec/llm.rs`

Find the call to `llm_worker_send_request` or `llm_worker_send_request_with_req_id`
and pass `llm_call.effort.unwrap_or_default()` as the new parameter.

---

## Verification

```
cargo check --workspace
```

Manually inspect a planner LLM call at `effort: High` in the logs — confirm the
prefix appears in the raw prompt before sending.

Run the analyst with `STAGNANT_THRESHOLD = 2` (temporarily lowered) and confirm
it produces a `High`-effort synthesis call.
