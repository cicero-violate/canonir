# PLAN_v2: Implement Full ID Set

## Filter Rule (Strict)

> Anything not required for **ordering**, **causality**, or **reconstruction** → does NOT go in events.

This splits the 44 target IDs into two groups:

**In events**: `event_id`, `session_id`, `schema_id`, `build_id`, `commit_id`, `trace_id`, `execution_id`, `span_id`, `parent_span_id`, `action_id`, `plan_id`, `plan_step_id`, `error_id`

**Not in events** (config, registry, future modules, or already covered): everything else — see Phase 5.

---

## Phase 1 — Event Envelope (ordering + reconstruction foundation)

Covers: `event_id`, `session_id`, `schema_id`, `build_id`, `commit_id`

These anchor every event to a unique position, a runtime session, a schema version, and a build.
They all live on `RuntimeStarted` (stamped once per process start) except `event_id` which stamps every event.

### 1.1 — Stamp `event_id` on every emitted event

**Why in events**: global ordering anchor; replay needs a stable, gap-free u64 sequence.

`EventRuntime` in `canon-runtime/src/lib.rs` already has `next_id: u64` (line 62). It increments but never stamps the value on the serialized JSON.

**File**: `canon-utils/canon-runtime/src/lib.rs`

Find the method that serializes and appends events to the tlog (the method that uses `next_id`). After incrementing `next_id`, inject it into the serialized JSON before writing:

```rust
// Wherever the event JSON object is built for tlog append:
obj["event_id"] = serde_json::Value::Number(self.next_id.into());
// then increment: self.next_id += 1;  (or before, 0-indexed — be consistent)
```

The `event_id` field must appear in every tlog line. Use `u64`, 0-indexed, monotonic, never resets within a session.

### 1.2 — Generate `session_id` at startup

**Why in events**: without it, two process restarts that share a tlog directory are indistinguishable during replay.

**File**: `canon-utils/canon-runtime/src/bin/event_runtime.rs`

1. At startup (before `runtime_started` is emitted), generate:
   ```rust
   let session_id = Uuid::new_v4().to_string();
   ```
2. Add `session_id` to the `runtime_started` payload:
   ```rust
   serde_json::json!({
       "pid": std::process::id(),
       "tlog": tlog_path.display().to_string(),
       "session_id": session_id,
       // ... schema_id, build_id, commit_id added in 1.3/1.4 below
   })
   ```
3. Save `session_id` to the cursor JSON file (`save_cursor`) so it can be recovered if the process restarts and resumes the same session.
4. Load it back in `load_cursor_seq` and pass it into the main function scope.

### 1.3 — Add `schema_id` to `RuntimeStarted`

**Why in events**: the tlog is durable across code changes; replayers need to know which schema version produced each segment.

**File**: `canon-utils/canon-runtime-events/src/events.rs`

Add a module-level constant:
```rust
pub const EVENT_SCHEMA_VERSION: &str = "1";
```

Bump this manually (as a simple integer string: "1", "2", "3") whenever a breaking change is made to any event struct.

**File**: `canon-utils/canon-runtime/src/bin/event_runtime.rs`

Add to the `runtime_started` payload:
```rust
"schema_id": canon_event::EVENT_SCHEMA_VERSION,
```

Import: `use canon_event::EVENT_SCHEMA_VERSION;`

### 1.4 — Add `build_id` and `commit_id` to `RuntimeStarted`

**Why in events**: cross-version replay is unsafe without knowing which binary produced which events.

**File**: `canon-utils/canon-runtime/build.rs` (create if it does not exist)

```rust
fn main() {
    // Capture git commit at compile time
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "unknown".to_string());
    let commit = commit.trim();
    println!("cargo:rustc-env=CANON_COMMIT_ID={commit}");
    // build_id: commit + timestamp for uniqueness across dirty rebuilds
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    println!("cargo:rustc-env=CANON_BUILD_ID={commit}-{ts}");
    println!("cargo:rerun-if-changed=.git/HEAD");
}
```

**File**: `canon-utils/canon-runtime/src/bin/event_runtime.rs`

Add to the `runtime_started` payload:
```rust
"build_id": env!("CANON_BUILD_ID"),
"commit_id": env!("CANON_COMMIT_ID"),
```

---

## Phase 2 — Causality Chain (trace + execution + span)

Covers: `trace_id`, `execution_id`, `span_id`, `parent_span_id`

These form the causal DAG. A single `trace_id` groups all events from one goal-resolution cycle (one LLM call and all its downstream actions). `execution_id` identifies one full observe→plan→act→verify→reward cycle. `span_id`/`parent_span_id` form the parent-child edges within that cycle.

**Data flow**:
```
PlanConsumer generates:
  trace_id (new UUID per LLM dispatch)
  execution_id (new UUID per LLM dispatch)
  plan_span_id (new UUID, parent_span_id = null)
  → stamped on LoopPlanned

ActConsumer reads trace_id, execution_id, plan_span_id from LoopPlanned:
  act_span_id (new UUID, parent_span_id = plan_span_id)
  → stamped on LoopActed

VerifyConsumer reads trace_id, execution_id, act_span_id from LoopActed:
  verify_span_id (new UUID, parent_span_id = act_span_id)
  → stamped on LoopVerified

RewardConsumer reads trace_id, execution_id, verify_span_id from LoopVerified:
  reward_span_id (new UUID, parent_span_id = verify_span_id)
  → stamped on LoopRewarded
```

### 2.1 — Add fields to loop event structs

**File**: `canon-utils/canon-runtime-events/src/events.rs`

Add these fields to `LoopPlanned`, `LoopActed`, `LoopVerified`, `LoopRewarded`:
```rust
#[serde(default)]
pub trace_id: Option<String>,
#[serde(default)]
pub execution_id: Option<String>,
#[serde(default)]
pub span_id: Option<String>,
#[serde(default)]
pub parent_span_id: Option<String>,
```

All fields use `#[serde(default)]` so existing tlog entries without these fields deserialize to `None` (backward compatible).

### 2.2 — Generate and stamp in `PlanConsumer`

**File**: `canon-utils/canon-plan/src/lib.rs`

Add to `PendingPlan`:
```rust
trace_id: String,
execution_id: String,
span_id: String,   // the plan-phase span
```

In `handle_observed`, when dispatching:
```rust
let trace_id = Uuid::new_v4().to_string();
let execution_id = Uuid::new_v4().to_string();
let span_id = Uuid::new_v4().to_string();
self.pending = Some(PendingPlan {
    tick: observed.tick,
    request_id: request_id.clone(),
    dispatched_at_tick: observed.tick,
    goal_text: observed.goal_text.clone(),
    trace_id: trace_id.clone(),
    execution_id: execution_id.clone(),
    span_id: span_id.clone(),
});
```

In `handle_capability_completed`, when emitting each `LoopPlanned`, set:
```rust
trace_id: Some(pending.trace_id.clone()),
execution_id: Some(pending.execution_id.clone()),
span_id: Some(Uuid::new_v4().to_string()),  // unique span per planned action
parent_span_id: Some(pending.span_id.clone()),
```

### 2.3 — Propagate through `ActConsumer`

**File**: `canon-utils/canon-act/src/lib.rs`

Add to `PendingAct`:
```rust
trace_id: Option<String>,
execution_id: Option<String>,
parent_span_id: Option<String>,  // the plan-phase span_id from LoopPlanned
```

In `handle_plan`, extract from `planned`:
```rust
PendingAct {
    tick: planned.tick,
    action_kind: planned.action_kind.clone(),
    request_id: request_id.clone(),
    started_at: Instant::now(),
    trace_id: planned.trace_id.clone(),
    execution_id: planned.execution_id.clone(),
    parent_span_id: planned.span_id.clone(),
}
```

In `handle_completed` / wherever `LoopActed` is emitted, set:
```rust
trace_id: pending.trace_id.clone(),
execution_id: pending.execution_id.clone(),
span_id: Some(Uuid::new_v4().to_string()),
parent_span_id: pending.parent_span_id.clone(),
```

### 2.4 — Propagate through `VerifyConsumer`

**File**: `canon-utils/canon-verify/src/lib.rs`

`VerifyConsumer` already listens to `LoopActed`. Store the last `trace_id`, `execution_id`, and `span_id` (from `LoopActed`) in the consumer state. When emitting `LoopVerified`:
```rust
trace_id: self.last_trace_id.clone(),
execution_id: self.last_execution_id.clone(),
span_id: Some(Uuid::new_v4().to_string()),
parent_span_id: self.last_act_span_id.clone(),
```

### 2.5 — Propagate through `RewardConsumer`

**File**: `canon-utils/canon-reward/src/lib.rs`

Same pattern: store `trace_id`, `execution_id`, `span_id` from `LoopVerified`. When emitting `LoopRewarded`, set:
```rust
trace_id: self.last_trace_id.clone(),
execution_id: self.last_execution_id.clone(),
span_id: Some(Uuid::new_v4().to_string()),
parent_span_id: self.last_verify_span_id.clone(),
```

---

## Phase 3 — Action Linkage (`action_id`, `plan_id`, `plan_step_id`)

Covers: `action_id`, `plan_id`, `plan_step_id`

These let a replayer answer: "which planned action produced which act execution?"

- `plan_id` — one UUID per LLM response; the same `plan_id` stamps all `LoopPlanned` events from one LLM call (currently `llm_request_id` is used as proxy — make this explicit)
- `plan_step_id` — one UUID per planned action within a plan (one per `LoopPlanned` event)
- `action_id` — same value as `plan_step_id`; carried forward onto `LoopActed` to link plan → execution

### 3.1 — Add fields to `LoopPlanned` and `LoopActed`

**File**: `canon-utils/canon-runtime-events/src/events.rs`

Add to `LoopPlanned`:
```rust
#[serde(default)]
pub plan_id: Option<String>,
#[serde(default)]
pub plan_step_id: Option<String>,
#[serde(default)]
pub action_id: Option<String>,
```

Add to `LoopActed`:
```rust
#[serde(default)]
pub plan_id: Option<String>,
#[serde(default)]
pub plan_step_id: Option<String>,
#[serde(default)]
pub action_id: Option<String>,
```

### 3.2 — Generate in `PlanConsumer`

**File**: `canon-utils/canon-plan/src/lib.rs`

Add `plan_id: String` to `PendingPlan`. Generate once when dispatching the LLM call:
```rust
plan_id: Uuid::new_v4().to_string(),
```

In `handle_capability_completed`, for each action emitted, generate:
```rust
let plan_step_id = Uuid::new_v4().to_string();
let action_id = plan_step_id.clone();  // same value; aliased for semantic clarity
// stamp on LoopPlanned:
plan_id: Some(pending.plan_id.clone()),
plan_step_id: Some(plan_step_id),
action_id: Some(action_id),
```

### 3.3 — Carry through `ActConsumer`

**File**: `canon-utils/canon-act/src/lib.rs`

Add to `PendingAct`:
```rust
plan_id: Option<String>,
plan_step_id: Option<String>,
action_id: Option<String>,
```

Extract from `LoopPlanned` in `handle_plan`. Stamp on `LoopActed` in `handle_completed`.

---

## Phase 4 — Error Deduplication (`error_id`)

Covers: `error_id`

**Why in events**: `ErrorOccurred` events are emitted multiple times for the same logical failure (observed: 7 duplicates for one `cargo new` failure). Without a stable `error_id`, replayers and monitors cannot deduplicate. Downstream consumers (failure store, LLM prompt context) need to know if they've seen this error before.

### 4.1 — Add `error_id` to `ErrorOccurred`

**File**: `canon-utils/canon-runtime-events/src/events.rs`

Add to `ErrorOccurred`:
```rust
#[serde(default)]
pub error_id: Option<String>,
```

### 4.2 — Generate `error_id` in `ErrorLogger`

**File**: `canon-utils/canon-runtime/src/consumers/error_logger.rs`

In `event_to_payload`, inject a UUID `error_id` into every constructed payload:
```rust
use uuid::Uuid;
// at the start of each arm:
let error_id = Uuid::new_v4().to_string();
// add to the json!({}) payload:
"error_id": error_id,
```

Add `uuid` to `canon-runtime/Cargo.toml` if not already present (it already is via `canon-plan`).

### 4.3 — Fix duplicate `ErrorOccurred` emission (root cause)

The duplication stems from `ErrorLogger` receiving the same `LoopActed(success=false)` event multiple times because `capability_completed` is emitted twice to the tlog (visible on lines 35–36 of the tlog).

Investigate why `capability_completed` is written twice. The likely cause is that `emit_runtime_event` is called both from the capability executor AND from `append_runtime_event` in the writer loop. Once identified, ensure each capability result is written to the tlog exactly once.

This is a separate bug from the ID work but `error_id` is the dedup safety net in case duplicates slip through.

---

## Phase 5 — Non-Event IDs

These IDs do NOT belong in events. They live in config, runtime internals, or future modules.

### 5.1 — `system_id` (config only)

Read from env var `CANON_SYSTEM_ID` at startup. If absent, generate once and persist to
`state/system_id` file. Log it at startup via a non-event log line or `runtime_started` payload field.
Do NOT stamp on every event — it never changes within a deployment.

**File**: `canon-utils/canon-runtime/src/bin/event_runtime.rs`
Add to `runtime_started` payload: `"system_id": system_id`.

### 5.2 — Thread naming (covers `worker_id` / `queue_id` conceptually)

Name the runtime threads for debuggability. These are not IDs in events; they appear in `ps`/`top` output and stack traces.

**File**: `canon-utils/canon-runtime/src/bin/event_runtime.rs`

Replace bare `std::thread::spawn` with:
```rust
std::thread::Builder::new().name("canon-p1-bootstrap".to_string()).spawn(...)?;
std::thread::Builder::new().name("canon-p2-watcher".to_string()).spawn(...)?;
std::thread::Builder::new().name("canon-p3-tick".to_string()).spawn(...)?;
```

No events emitted. Names appear in `/proc/PID/task/*/comm` and debuggers.

### 5.3 — `state_version` (cursor file only)

The cursor file already stores `processed: usize` (event count) which is the de-facto state version.
Rename this field to `state_version` in the cursor JSON schema. The `save_cursor` and `load_cursor_seq` functions in `event_runtime.rs` should read/write `state_version` instead of `processed` as the key name.

No event change required. Replay tools read the cursor to find the resume point.

### 5.4 — `event_stream_id` (RuntimeStarted only)

The tlog path already serves as the stream identity implicitly. To make it explicit for multi-agent scenarios, add it to `runtime_started`:

```rust
"event_stream_id": tlog_path.display().to_string(),
```

This is a cosmetic rename of the existing `tlog` field. Keep `tlog` as an alias.

### 5.5 — Deferred (future modules)

These IDs have no implementation home yet. Do not create stubs. Implement when the module is built:

| ID group                                               | When to implement                            |
|--------------------------------------------------------+----------------------------------------------|
| `memory_id`, `embedding_id`, `document_id`, `chunk_id` | When memory/retrieval module is added        |
| `intent_id`                                            | When intent-routing layer is added           |
| `invariant_id`, `violation_id`                         | When invariant registry is added             |
| `resource_id`, `response_id`                           | When resource abstraction layer is added     |
| `task_id`, `job_id`                                    | If async executor replaces std::thread       |
| `auth_id`, `permission_id`, `audit_id`                 | When security layer is added                 |
| `event_batch_id`                                       | Not applicable to this system's architecture |
| `object_version`                                       | When per-entity versioning is required       |
| `state_id`                                             | When snapshotting is implemented             |

---

## Implementation Order

Execute phases in order. Each phase is independently compilable and testable.

```
Phase 1  →  Phase 2  →  Phase 3  →  Phase 4  →  Phase 5
(envelope)  (causality)  (action)   (errors)    (non-event)
```

Within each phase, make struct changes first (`events.rs`), then wire producers
(`plan`/`act`/`verify`/`reward`), then verify with `cargo check`.

---

## Verification Checklist

After all phases:

- [ ] Every tlog line has an `event_id` field (u64, monotonically increasing from 0)
- [ ] First event in every tlog is `runtime_started` and has `session_id`, `schema_id`, `build_id`, `commit_id`
- [ ] All `LoopPlanned` events have `trace_id`, `execution_id`, `span_id`, `parent_span_id`, `plan_id`, `plan_step_id`, `action_id`
- [ ] All `LoopActed` events carry the same `trace_id`, `execution_id` as the `LoopPlanned` that triggered them
- [ ] `LoopActed.parent_span_id` == `LoopPlanned.span_id` for each linked pair
- [ ] `LoopVerified.parent_span_id` == `LoopActed.span_id`
- [ ] `LoopRewarded.parent_span_id` == `LoopVerified.span_id`
- [ ] Every `ErrorOccurred` event has a unique `error_id` (no two events share the same value)
- [ ] `cargo check` passes with no warnings introduced by the changes
- [ ] Existing tlog files still deserialize without error (all new fields use `#[serde(default)]`)

---

## Files Changed Summary

| File                                          | Changes                                                                                                                                                                                                   |
|-----------------------------------------------+-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `canon-runtime-events/src/events.rs`          | Add fields to `LoopPlanned`, `LoopActed`, `LoopVerified`, `LoopRewarded`, `ErrorOccurred`; add `EVENT_SCHEMA_VERSION` const                                                                               |
| `canon-runtime/src/lib.rs`                    | Stamp `event_id` from `next_id` on every serialized tlog event                                                                                                                                            |
| `canon-runtime/src/bin/event_runtime.rs`      | Generate `session_id`; add to `runtime_started`; name threads; add `system_id`, `build_id`, `commit_id`, `schema_id`, `event_stream_id` to `runtime_started`; rename cursor `processed` → `state_version` |
| `canon-runtime/build.rs`                      | New file; capture `CANON_BUILD_ID` and `CANON_COMMIT_ID` env vars at compile time                                                                                                                         |
| `canon-runtime/src/consumers/error_logger.rs` | Add `error_id` UUID to every constructed `ErrorOccurred` payload                                                                                                                                          |
| `canon-plan/src/lib.rs`                       | Add `trace_id`, `execution_id`, `span_id`, `plan_id`, `plan_step_id`, `action_id` to `PendingPlan`; generate and stamp on `LoopPlanned`                                                                   |
| `canon-act/src/lib.rs`                        | Add `trace_id`, `execution_id`, `parent_span_id`, `plan_id`, `plan_step_id`, `action_id` to `PendingAct`; extract from `LoopPlanned`; stamp on `LoopActed`                                                |
| `canon-verify/src/lib.rs`                     | Store last `trace_id`, `execution_id`, `span_id` from `LoopActed`; stamp on `LoopVerified`                                                                                                                |
| `canon-reward/src/lib.rs`                     | Store last `trace_id`, `execution_id`, `span_id` from `LoopVerified`; stamp on `LoopRewarded`                                                                                                             |
