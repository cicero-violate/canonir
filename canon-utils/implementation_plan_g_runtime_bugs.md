# Implementation Plan: G — Runtime Event Bugs

## Current Build Status

```
Phase 1 — ✅ complete  (append_runtime_event: RouteTick + RouteSelected arms added)
Phase 2 — ✅ complete  (goal_text: data-envelope unwrap in scan_tlog_for_goal + executor)
Phase 3 — ✅ complete  (cargo check --workspace exits 0)
```

**Confirmed 2026-03-21:**
- `cargo check --workspace` — zero errors
- `canon-runtime/src/lib.rs:456-464` — RouteTick + RouteSelected arms present
- `canon-loop/src/stage/observe.rs:41-45` — `data = payload.get("data").unwrap_or(payload)` present
- `canon-loop/src/executor.rs:70-71` — `data = prompt.payload.get("data").unwrap_or(&prompt.payload)` present

---

## Macro Emit Behaviour (confirmed)

`canon_emit_meta!` has three distinct forms — understanding which form is used at each site determines
the correct payload shape at the reader:

```
Form 1 — direct (external / bootstrap):
    canon_emit_meta!(source, kind, payload, &path)
    → wraps: { "data": payload, "meta": { file, crate_name, module, line } }
    → writes directly to tlog via write_event_auto
    → tlog record: { "payload": { "data": {...}, "meta": {...} } }

Form 2 — emitter debug (consumers emitting ad-hoc events):
    canon_emit_meta!(emitter; source, kind, payload)
    → wraps: { "data": payload, "meta": { file, crate_name, module, line } }
    → emits CanonEvent::Debug(DebugEvent { source, kind, payload: wrapped })
    → goes through bus → append_runtime_event writes it flat

Form 3 — typed variant (consumers emitting typed events):
    canon_emit_meta!(emitter; Variant(inner))
    → NO wrapping — delegates directly to canon_emit!(emitter; Variant(inner))
    → emits CanonEvent::Variant(inner) with no data/meta envelope
    → append_runtime_event must serialize the inner struct directly via serde_json::to_value
```

**Bootstrap** uses Form 1. **RouteExecutor** uses Form 3 for both `LlmCall` and `RouteSelected`.
Bug A and Bug B are direct consequences of this.

---

## Goal

Fix two runtime bugs discovered by tlog analysis that prevent the loop from making progress:

1. **`RouteTick` and `RouteSelected` silently dropped from tlog** — `append_runtime_event` has no arms for
   these two variants, so they fall into `_ => { return; }` and are never written. Consumers receive them
   correctly via the bus, but the tlog never records routing decisions.

2. **`goal_text: null` in every `loop_observed`** — `scan_tlog_for_goal` and `LoopStageExecutor::on_event`
   both assume the `prompt_loaded` payload is flat (`payload["content"]`). Bootstrap writes using Form 1
   (`canon_emit_meta!(source, kind, payload, &path)`), which wraps the payload as
   `{ "data": { "content": ..., "path": ..., "prompt_id": ... }, "meta": { file, line, ... } }`.
   Both readers look directly at `payload["content"]` — which is `null` because content is at
   `payload["data"]["content"]`. `goal_text` stays `None` forever; `plan.rs:162` short-circuits and
   the loop is stuck in observe forever.

### Tlog evidence

```
248 total events:
  loop_observed: 134  (goal_text: null in ALL of them)
  capability_completed: 21  (source=event-runtime, request_id=route-*)
  route_tick events: 0        ← RouteTick dropped by append_runtime_event
  route_selected events: 0    ← RouteSelected dropped by append_runtime_event
  debug events: 0             ← no debug events at all
  supervisor source events: 0
```

`RouteExecutor` IS receiving `RouteTick` via the bus and dispatching routing LLM calls correctly
(`capability_completed route-*` events prove this). `emit_decision()` IS called. But the resulting
`RouteSelected` event is dispatched to consumers via bus and then silently swallowed when
`append_runtime_event` hits the `_ => { return; }` arm.

---

## Bug A — `append_runtime_event` missing arms

**Why no wrapping needed here:** `RouteExecutor` emits both `RouteTick` and `RouteSelected` using
Form 3 (`canon_emit_meta!(emitter; Variant(inner))`). The inner struct is passed directly with no
`data`/`meta` envelope. `serde_json::to_value(payload)` in `append_runtime_event` serializes the
struct fields flat — no unwrapping required at the write site.

**File:** `canon-utils/canon-runtime/src/lib.rs`

**Location:** The `match event` block inside `fn append_runtime_event`. Currently ends with:

```rust
CanonEvent::Debug(DebugEvent { source, kind, payload }) => TlogEvent::new(source, kind, payload.clone()),
CanonEvent::ErrorOccurred(payload) => {
    let val = serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({}));
    TlogEvent::new("event-runtime", "error_occurred", val)
}
_ => {
    return;
}
```

**Fix:** Insert two arms before the `_ => { return; }` arm:

```rust
// Before (the two lines before the _ arm):
CanonEvent::Debug(DebugEvent { source, kind, payload }) => TlogEvent::new(source, kind, payload.clone()),
CanonEvent::ErrorOccurred(payload) => {
    let val = serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({}));
    TlogEvent::new("event-runtime", "error_occurred", val)
}
_ => {
    return;
}

// After:
CanonEvent::Debug(DebugEvent { source, kind, payload }) => TlogEvent::new(source, kind, payload.clone()),
CanonEvent::ErrorOccurred(payload) => {
    let val = serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({}));
    TlogEvent::new("event-runtime", "error_occurred", val)
}
CanonEvent::RouteTick(payload) => {
    let val = serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({}));
    TlogEvent::new("supervisor", "route_tick", val)
}
CanonEvent::RouteSelected(payload) => {
    let val = serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({}));
    TlogEvent::new("supervisor", "route_selected", val)
}
_ => {
    return;
}
```

**Import check:** `RouteTick` and `RouteSelected` must be imported. Check the `use` block at the top of
`lib.rs`. Add them to the existing `canon_runtime_events` import if not already present.

**Checkpoint:** `cargo check -p canon-runtime` exits 0.

---

## Bug B — `goal_text: null` — wrong payload path

Bootstrap writes `prompt_loaded` using Form 1 (`canon_emit_meta!(source, kind, payload, &path)`).
The macro wraps the caller's payload as `{ "data": <payload>, "meta": { file, crate_name, module, line } }`
and passes that object to `write_event_auto`. The JSONL record on disk therefore has:

```json
{ "payload": { "data": { "content": "...", "path": "AGENT_GOAL.md", "prompt_id": "AGENT_GOAL" },
               "meta": { "crate_name": "canon-runtime", "file": "...", "line": 138, ... } } }
```

Both readers call `payload.get("content")` / `payload.get("path")` against this wrapper object.
Those keys are `null` because they live inside `payload["data"]`. The fix in both places is identical:
extract `data = payload.get("data").unwrap_or(payload)` and do all field lookups on `data`.

Two code sites both make the same wrong assumption.

### Bug B1 — `scan_tlog_for_goal`

**File:** `canon-utils/canon-loop/src/stage/observe.rs`

**Location:** `fn scan_tlog_for_goal`, the `for line in content.lines()` loop body.

**Current:**

```rust
let payload = v.get("payload").unwrap_or(&Value::Null);
let is_goal = payload.get("path").and_then(|p| p.as_str()).map(|p| p.contains("AGENT_GOAL")).unwrap_or(false)
    || payload.get("prompt_id").and_then(|p| p.as_str()).map(|p| p == "AGENT_GOAL").unwrap_or(false);
if is_goal {
    if let Some(c) = payload.get("content").and_then(|c| c.as_str()) {
        found = Some(c.to_string());
    }
}
```

**Fix:** Unwrap the `data` layer before the field lookups:

```rust
let payload = v.get("payload").unwrap_or(&Value::Null);
// Bootstrap writes payload inside a {data: {...}, meta: {...}} envelope.
// Unwrap if present; fall back to payload itself for any future flat form.
let data = payload.get("data").unwrap_or(payload);
let is_goal = data.get("path").and_then(|p| p.as_str()).map(|p| p.contains("AGENT_GOAL")).unwrap_or(false)
    || data.get("prompt_id").and_then(|p| p.as_str()).map(|p| p == "AGENT_GOAL").unwrap_or(false);
if is_goal {
    if let Some(c) = data.get("content").and_then(|c| c.as_str()) {
        found = Some(c.to_string());
    }
}
```

### Bug B2 — `LoopStageExecutor::on_event(PromptLoaded)`

**File:** `canon-utils/canon-loop/src/executor.rs`

**Location:** The `CanonEvent::PromptLoaded(prompt) =>` arm.

**Current:**

```rust
CanonEvent::PromptLoaded(prompt) => {
    if let Some(content) = prompt.payload.get("content").and_then(|c| c.as_str()) {
        self.ctx.goal_text = Some(content.to_string());
        self.ctx.last_prompted_goal = Some(content.to_string());
    }
}
```

**Fix:** Unwrap `data` layer first:

```rust
CanonEvent::PromptLoaded(prompt) => {
    let data = prompt.payload.get("data").unwrap_or(&prompt.payload);
    if let Some(content) = data.get("content").and_then(|c| c.as_str()) {
        self.ctx.goal_text = Some(content.to_string());
        self.ctx.last_prompted_goal = Some(content.to_string());
    }
}
```

**Checkpoint:** `cargo check --workspace` exits 0.

---

## Phase 3 — Verify

```bash
cargo check --workspace
cargo test -p canon-runtime
```

After deploying and running the runtime briefly, check the tlog:

```bash
python3 -c "
import json
log = '/workspace/ai_sandbox/canon/state/event_log/event.tlog.d/00000000000000000000.log'
kinds = {}
goal_set = 0
with open(log) as f:
    for line in f:
        ev = json.loads(line.strip()) if line.strip() else None
        if not ev: continue
        kinds[ev.get('kind','?')] = kinds.get(ev.get('kind','?'),0) + 1
        if ev.get('kind') == 'loop_observed':
            if ev.get('payload', {}).get('goal_text'): goal_set += 1
print('route_tick:', kinds.get('route_tick', 0))
print('route_selected:', kinds.get('route_selected', 0))
print('loop_observed with goal_text set:', goal_set)
"
```

Expected after fix:
- `route_tick` > 0
- `route_selected` > 0
- `loop_observed with goal_text set` > 0 (first loop_observed after the prompt_loaded event)

---

## Execution Order

```
Phase 1 — Bug A: add RouteTick + RouteSelected arms to append_runtime_event
Phase 2 — Bug B: fix data-envelope payload lookup in 2 files
Phase 3 — Verify: cargo check + tlog spot-check
```

---

## Files Modified

| Phase | Status | File | Change |
|-------|--------|------|--------|
| 1 | ✅ | `canon-runtime/src/lib.rs` | Added `RouteTick` and `RouteSelected` arms to `append_runtime_event` |
| 2a | ✅ | `canon-loop/src/stage/observe.rs` | `scan_tlog_for_goal`: unwrap `data` layer before field lookups |
| 2b | ✅ | `canon-loop/src/executor.rs` | `on_event(PromptLoaded)`: unwrap `data` layer before `content` lookup |

---

## What This Does NOT Change

- `canon_emit_meta!` macro — all three forms are correct; the writers are correct; the readers are wrong
- RouteExecutor logic — it correctly uses Form 3 for both LlmCall and RouteSelected; no change needed
- Bootstrap prompt_loaded write — Form 1 envelope is correct; do not flatten it
- `process_events` in `lib.rs` — line 169 passes `canon.payload` (the full wrapper) into
  `CanonEvent::PromptLoaded`. The fix belongs in the two downstream consumers, not in `process_events`

---

## After This Fix

```
loop_observed tick=N  →  goal_text: "# Agent Goal..."   ✅ (was null)
route_tick            →  written to tlog                ✅ (was dropped)
route_selected        →  written to tlog                ✅ (was dropped)
LoopStageExecutor     →  dispatches plan on tick 1      ✅ (was stuck in observe forever)
```
