# Canon Agent Loop — Architecture Plan

## What the tlog shows

The loop is running but doing nothing useful. Every tick produces:

```jsonl
{"kind":"loop_observed","payload":{"error_count":0,"warning_count":0,...},"source":"observe"}
{"kind":"loop_planned","payload":{"action_kind":"no_op","reason":"clean"},"source":"plan"}
{"kind":"loop_acted","payload":{"action_kind":"no_op","success":true},"source":"act"}
{"kind":"loop_verified","payload":{"passed":true},"source":"verify"}
{"kind":"loop_rewarded","payload":{"reward":0.5,"stagnant_ticks":0},"source":"reward"}
```

No LLM calls are ever made. The loop has a goal (`AGENT_GOAL.md` is loaded in tlog),
but the workspace compiles cleanly, so plan short-circuits to `no_op` every tick.
The reward counter never reaches the halt threshold because `reward: 0.5` on every
clean-but-idle tick resets `stagnant_ticks` to zero.

---

## Root Causes and Fixes

Five bugs across five files. Each is a surgical change to one function or struct.

---

### Bug 1 — `PlanConsumer` never calls LLM on a clean workspace

**File:** `canon-plan/src/lib.rs:63`

**Current code:**
```rust
fn handle_observed(&mut self, observed: &LoopObserved) {
    if self.pending.is_some() { return; }
    if observed.error_count == 0 {
        self.emit_plan(LoopPlanned { action_kind: "no_op", reason: "clean", ... });
        return;
    }
    // ... only reaches LLM if error_count > 0
}
```

**Problem:** The workspace always compiles. `error_count` is always 0.
The LLM is never called even though an active goal exists.

**Fix:** Check for a goal first. If a goal is present, always call the LLM
regardless of error count. Only `no_op` when there is no goal at all.

```rust
fn handle_observed(&mut self, observed: &LoopObserved) {
    if self.pending.is_some() { return; }

    // No goal and no errors → nothing to do
    if observed.goal_text.is_none() && observed.error_count == 0 {
        self.emit_plan(LoopPlanned { action_kind: "no_op", reason: "no_goal", ... });
        return;
    }

    let request_id = Uuid::new_v4().to_string();
    let prompt = build_prompt(observed); // includes goal + errors + context
    // emit CapabilityRequested { name: "llm.call", args: { prompt, role: "planner" } }
    self.pending = Some(PendingPlan { tick: observed.tick, request_id: request_id.clone() });
    emitter.emit(CanonEvent::CapabilityRequested(...));
}
```

**Also add `Done` to `LlmAction`** so the LLM can signal goal completion:
```rust
enum LlmAction {
    Patch { old: String, new: String },
    Command { cmd: String },
    Done { reason: String },  // NEW
}
```
Parse: `{ "done": true, "reason": "..." }` → `LlmAction::Done`
Emit: `LoopPlanned { action_kind: "done", reason: llm_reason }`

---

### Bug 2 — `LoopObserved` carries no goal context

**File:** `canon-runtime-events/src/events.rs:126`

**Current struct:**
```rust
canon_event_struct!(LoopObserved {
    tick: u64,
    error_count: usize,
    warning_count: usize,
    compiler_errors: Vec<serde_json::Value>,
    tlog_tail: Vec<serde_json::Value>,
});
```

**Problem:** `PlanConsumer` has no way to know whether there is an active goal.
The goal text is loaded via `PromptLoaded` events but is not surfaced to observers.

**Fix:** Add one field:
```rust
canon_event_struct!(LoopObserved {
    tick: u64,
    error_count: usize,
    warning_count: usize,
    compiler_errors: Vec<serde_json::Value>,
    tlog_tail: Vec<serde_json::Value>,
    goal_text: Option<String>,   // NEW
});
```

---

### Bug 3 — `ObserveConsumer` never reads the active goal

**File:** `canon-observe/src/lib.rs`

**Current state:**
- `ObserveConsumer` has no `goal_text` field
- `on_event` only handles `Tick`, ignores all other events
- `LoopObserved` is emitted with no goal information

**Fix:** Add `goal_text: Option<String>` to `ObserveConsumer` state.
Listen for `CanonEvent::PromptLoaded` events and update it:

```rust
pub struct ObserveConsumer {
    workspace: PathBuf,
    emitter: Option<EventEmitterHandle>,
    goal_text: Option<String>,   // NEW
}

fn on_event(&mut self, event: &CanonEvent) {
    // NEW: capture goal from PromptLoaded events
    if let CanonEvent::PromptLoaded(p) = event {
        if p.path.contains("AGENT_GOAL") {
            self.goal_text = Some(p.content.clone());
        }
        return;
    }

    let CanonEvent::Tick(Tick { tick }) = event else { return; };

    // ... existing cargo check + tlog tail logic ...

    let payload = LoopObserved {
        tick: *tick,
        error_count,
        warning_count,
        compiler_errors,
        tlog_tail,
        goal_text: self.goal_text.clone(),   // NEW
    };
    emitter.emit(CanonEvent::LoopObserved(payload));
}
```

---

### Bug 4 — `build_prompt` sends no goal or context to LLM

**File:** `canon-plan/src/lib.rs:230`

**Current prompt (error-only):**
```rust
fn build_prompt(span: &ErrorSpan) -> String {
    format!(
        "Fix the Rust compiler error at {file}:{line}:{column}.\nError: {message}\n\n\
         Return JSON only. Use either {{\"old\": \"...\", \"new\": \"...\"}} to patch, \
         or {{\"cmd\": \"...\"}} to run a command.",
        ...
    )
}
```

**Problem:** The LLM receives only an error location. It has no knowledge of the goal,
the workspace state, or what actions are available. Even if called, it cannot make
progress on an active goal.

**Fix:** Replace `build_prompt(span)` with `build_prompt(observed: &LoopObserved)`:

```rust
fn build_prompt(observed: &LoopObserved) -> String {
    let goal_section = match &observed.goal_text {
        Some(text) => format!("## Active Goal\n{text}\n\n"),
        None => String::new(),
    };
    let error_section = if observed.error_count > 0 {
        let first = observed.compiler_errors.first()
            .and_then(|e| e.get("message"))
            .and_then(|m| m.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        format!("## Compiler Errors ({n})\nFirst error: {first}\n\n",
            n = observed.error_count, first = first)
    } else {
        "## Compiler Errors\nNone.\n\n".to_string()
    };
    format!(
        "{goal}{errors}\
         Return exactly one JSON object. Allowed schemas:\n\
         - Run a command: {{\"cmd\": \"cargo new foo\", \"cwd\": \"/path\"}}\n\
         - Patch a file:  {{\"old\": \"...\", \"new\": \"...\", \"path\": \"...\"}}\n\
         - Signal done:   {{\"done\": true, \"reason\": \"...\"}}\n\
         No explanation. JSON only.",
        goal = goal_section,
        errors = error_section,
    )
}
```

**Also update `PendingPlan`** — remove `file_path` (no longer relevant; path comes
from the LLM response directly via `action_payload`):
```rust
struct PendingPlan {
    tick: u64,
    request_id: String,
    // file_path removed: LLM now returns path in its response
}
```

---

### Bug 5 — `RewardConsumer` never increments `stagnant_ticks` on idle loop

**File:** `canon-reward/src/lib.rs:44`

**Current reward math:**
```rust
let mut reward = (errors_before - errors_after) as f32;  // 0 when both 0
if verified.passed { reward += 0.5; }                    // always +0.5 on clean workspace
// reward = 0.5 → stagnant_ticks = 0 always reset
```

**Problem:** A clean workspace with `no_op` action earns `reward: 0.5` every tick
because verify passes. This resets `stagnant_ticks` to 0 every time, so halt never fires.

**Fix:** `RewardConsumer` must track the last action kind. No reward bonus if action
was `no_op` (idle) or `done` (halt triggers instead).

```rust
pub struct RewardConsumer {
    emitter: Option<EventEmitterHandle>,
    errors_before: usize,
    stagnant_ticks: u32,
    last_action_kind: String,   // NEW — updated on LoopActed
}

fn on_event(&mut self, event: &CanonEvent) {
    match event {
        CanonEvent::LoopObserved(o)  => { self.errors_before = o.error_count; }
        CanonEvent::LoopActed(a)     => { self.last_action_kind = a.action_kind.clone(); }  // NEW
        CanonEvent::LoopVerified(v)  => { self.handle_verified(v); }
        _ => {}
    }
}

fn handle_verified(&mut self, verified: &LoopVerified) {
    let errors_after = verified.error_count;
    let mut reward = (self.errors_before as i32 - errors_after as i32) as f32;

    // Only award the verify-pass bonus for real actions, not idle no_op
    if verified.passed && self.last_action_kind != "no_op" {
        reward += 0.5;
    }
    if !verified.passed {
        reward -= 1.0;
    }
    // Halt immediately on "done"
    if self.last_action_kind == "done" {
        let payload = LoopRewarded { reward: 1.0, halt: true, stagnant_ticks: 0, ... };
        emitter.emit(CanonEvent::LoopRewarded(payload));
        return;
    }

    if reward <= 0.0 {
        self.stagnant_ticks = self.stagnant_ticks.saturating_add(1);
    } else {
        self.stagnant_ticks = 0;
    }
    let halt = self.stagnant_ticks > 5;
    emitter.emit(CanonEvent::LoopRewarded(LoopRewarded { reward, halt, ... }));
}
```

---

### Bug 6 — `PLANNER_ROLE.md` contradicts the actual prompt schema (hangs)

**File:** `canon-agent-prompts/PLANNER_ROLE.md`

**Problem:** The role file told the LLM to return a graph patch with `new_nodes`,
`new_edges`, `retract_nodes`, `rewrite_nodes`. The prompt body (from `build_prompt`)
told it to return `{"cmd"}` or `{"old/new/path"}` or `{"done"}`. Two conflicting schemas
in the same request. The LLM sometimes returned a large graph patch (which
`parse_llm_action` couldn't parse → `no_op`) and sometimes hung.

**Fix:** Replace `PLANNER_ROLE.md` with a minimal role that:
- Sets the persona (planning agent, one action at a time)
- Does NOT define the schema (schema comes from every request body via `build_prompt`)
- States the constraint: JSON only, no explanation, one action

**Also fixed (Bug 6b):** `write_file` existed in `ActConsumer` but was never reachable.
`parse_llm_action` had no `Write` variant, and `build_prompt` didn't list the `write`
schema. The AGENT_GOAL requires writing a README.md — without `write_file`, the LLM
had no clean way to create new files. Fixed:
- Added `Write { path, content }` to `LlmAction` enum in `canon-plan/src/lib.rs`
- Added `{"write": "/abs/path", "content": "..."}` parsing in `parse_llm_action`
- Added `Write` match arm emitting `LoopPlanned { action_kind: "write_file" }`
- Added `write` schema line to `build_prompt` output
- Updated `PLANNER_ROLE.md` to use `{"write": "/abs/path", "content": "..."}` notation

---

## Summary of changes

| File                                  | Change                                                       | Status |
|---------------------------------------+--------------------------------------------------------------+--------|
| `canon-runtime-events/src/events.rs`  | Add `goal_text: Option<String>` to `LoopObserved`            | DONE   |
| `canon-observe/src/lib.rs`            | Add `goal_text` state; handle `PromptLoaded`; populate field | DONE   |
| `canon-plan/src/lib.rs`               | Fix `handle_observed` gate; call LLM when goal present       | DONE   |
| `canon-reward/src/lib.rs`             | Add `last_action_kind`; fix stagnant logic; halt on `done`   | DONE   |
| `canon-plan/src/lib.rs`               | Add `Write` to `LlmAction`; parse `{"write":...}`            | DONE   |
| `canon-plan/src/lib.rs`               | Add `write` schema to `build_prompt`                         | DONE   |
| `canon-agent-prompts/PLANNER_ROLE.md` | Replace graph-patch role with simple one-action role         | DONE   |

No new crates. No new events. No schema changes beyond the one new field.

---

## Expected tlog after fixes

When AGENT_GOAL.md is loaded and the workspace is clean:

```jsonl
{"kind":"loop_observed","payload":{"error_count":0,"goal_text":"# Agent Goal\n..."},"source":"observe"}
{"kind":"capability_requested","payload":{"name":"llm.call","args":{"prompt":"## Active Goal\n..."}},"source":"plan"}
{"kind":"capability_completed","payload":{"result":{"cmd":"cargo new test_rust_project"}},"source":"llm-runtime"}
{"kind":"loop_planned","payload":{"action_kind":"run_command","action_payload":{"cmd":"cargo new ..."}}}
{"kind":"capability_requested","payload":{"name":"bash","args":{"cmd":"cargo new ..."}},"source":"act"}
{"kind":"capability_completed","payload":{"stdout":"Created binary...","exit_code":0},"source":"capability-executor"}
{"kind":"loop_acted","payload":{"action_kind":"run_command","success":true},"source":"act"}
{"kind":"loop_verified","payload":{"passed":true},"source":"verify"}
{"kind":"loop_rewarded","payload":{"reward":0.5,"stagnant_ticks":0},"source":"reward"}
```
