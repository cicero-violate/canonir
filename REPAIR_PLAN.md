# REPAIR PLAN — ISSUES.md (snapshot 2026-03-20)

## Source
`ISSUES.md` (derived from `state/reports_out/llm` snapshot at 01:01).

## Issue Summary

| # | Severity | Title                                                        |
|---+----------+--------------------------------------------------------------|
| 1 | High     | Partial execution visibility for multi-block planner output  |
| 2 | High     | Pending tool result remains open in artifact                 |
| 3 | Medium   | Destructive command dispatched without guardrail             |
| 4 | Medium   | No explicit batch-complete marker per planner response index |

---

## Issue 1 — Partial execution visibility

### Root cause

`ActConsumer.write_tool_call_artifact` is called inside `dispatch_plan`, which is only
invoked when an action is actually dispatched to a capability. Actions that are enqueued
(via `enqueue_plan`) but not yet dispatched have **no artifact record**. At any snapshot
between the start of a batch and its full completion, N−k actions are invisible.

The sequential chaining (`dispatch_next_in_active_batch`) is correct, but the visibility
gap exists because artifacts are written at dispatch time, not queue time.

File: `canon-utils/canon-act/src/lib.rs`

### Fix — write a "queued" record at enqueue time

**`enqueue_plan`** — after pushing to `self.queue`, immediately write a lightweight queued
record to the batch artifact so all planned actions are observable from the moment they are
enqueued.

```rust
// In enqueue_plan, after self.queue.push_back(planned.clone()):
let artifact_n = self.artifact_index_for_plan(planned);
self.write_tool_call_queued_artifact(artifact_n, planned);
```

Add a new helper `write_tool_call_queued_artifact`:

```rust
fn write_tool_call_queued_artifact(&self, artifact_n: u32, planned: &LoopPlanned) {
    let value = serde_json::json!({
        "n": artifact_n,
        "status": "queued",
        "queued_ms": now_ms_u64(),
        "action_kind": planned.action_kind,
        "llm_request_id": planned.llm_request_id,
        "plan_id": planned.plan_id,
        "plan_step_id": planned.plan_step_id,
        "action_id": planned.action_id,
        "payload": planned.action_payload,
    });
    append_tool_artifact(&self.artifact_dir, artifact_n, "tool_call", &value);
}
```

When `dispatch_plan` later fires, `write_tool_call_artifact` appends the actual dispatch
record (with `tool_call_id`, `request_id`, etc.) as a second entry in the same array.
This keeps the array append-only and shows the full lifecycle: `queued → dispatched`.

**Note**: `artifact_index_for_plan` in `enqueue_plan` must be called carefully — it calls
`next_tool_artifact_n()` as a fallback when no request file is found yet. Cache the
chosen `artifact_n` per `(llm_request_id, plan_step_id)` to avoid double-incrementing the
counter. Add a `queued_artifact_index: HashMap<String, u32>` to `ActConsumer` keyed by
`plan_step_id` or `action_id`.

### Acceptance criteria
- Immediately after a `LoopPlanned` event, the corresponding `{n}_tool_call.json` contains
  at least one entry with `status: "queued"`.
- The entry count in `{n}_tool_call.json` equals `valid_action_count` in
  `{n}_response.json` once all `LoopPlanned` events are processed.
- Dispatched entries appear as additional array elements with `tool_call_id` populated.

---

## Issue 2 — Pending tool result remains open

### Root cause

`reconcile_stale_pending_artifacts` is called exactly once: inside `set_emitter`, which
runs at startup. If the process terminates after writing a `pending` artifact but before
finalizing it (crash, SIGKILL), the artifact stays pending until the next process starts.

Additionally, `set_emitter` is called before the emitter is stored, so the reconciliation
path that emits `ToolResult` / `LoopActed` fires correctly only if the emitter is set
before the call — verify the ordering in `set_emitter`.

File: `canon-utils/canon-act/src/lib.rs`

### Fix — periodic reconciliation on Tick

1. **Also call `reconcile_stale_pending_artifacts` on `Tick`** events so any pending
   records created mid-run (e.g., if a capability handler thread panics without completing)
   are caught without waiting for the next restart:

```rust
CanonEvent::Tick(_) => {
    self.check_timeout();
    self.reconcile_stale_pending_artifacts(); // add this
}
```

   Use a rate-limiter inside `reconcile_stale_pending_artifacts` (e.g., skip if called
   within the last 10 seconds) to avoid per-tick filesystem scans:

```rust
// Add field: last_reconcile: Option<Instant>
// In reconcile_stale_pending_artifacts:
if self.last_reconcile.map_or(false, |t| t.elapsed() < Duration::from_secs(10)) {
    return;
}
self.last_reconcile = Some(Instant::now());
```

2. **Reduce crash-recovery window**: lower the default `CANON_TOOL_PENDING_TIMEOUT_MS`
   from 30 000 ms to 10 000 ms (10 s). Long-running commands can still override via the
   env var; 10 s catches crashed dispatch entries promptly.

3. **Verify `set_emitter` order**: confirm `self.emitter = Some(emitter)` is assigned
   *before* `self.reconcile_stale_pending_artifacts()` is called; otherwise the emitter
   field is still `None` during the reconcile pass and the synthetic `ToolResult` /
   `LoopActed` events are silently dropped.

### Acceptance criteria
- A `pending` entry written at time T transitions to `failed` (with `finalized_ms`) within
  `CANON_TOOL_PENDING_TIMEOUT_MS` even without a process restart.
- No `pending` entry survives beyond `CANON_TOOL_PENDING_TIMEOUT_MS` + 10 s under normal
  runtime conditions.
- After crash-restart, previously-pending entries are reconciled on the first Tick cycle
  that runs `reconcile_stale_pending_artifacts`.

---

## Issue 3 — Destructive command dispatched without guardrail

### Root cause

`dispatch_plan` for `run_command` forwards the `cmd` string directly to the `bash`
capability without inspecting it. The LLM planner can emit any shell command, including
`rm -rf <path>`, and it will execute immediately.

File: `canon-utils/canon-act/src/lib.rs`

### Fix — policy gate in `dispatch_plan`

Add a `destructive_cmd_policy` field to `ActConsumer` (read once at construction from
`CANON_DESTRUCTIVE_CMD_POLICY` env var; values: `"allow"` | `"warn"` | `"block"`; default
`"warn"`).

Add `is_potentially_destructive(cmd: &str) -> bool`:

```rust
fn is_potentially_destructive(cmd: &str) -> bool {
    let trimmed = cmd.trim();
    // Recursive/force deletion
    if trimmed.contains("rm -rf") || trimmed.contains("rm -fr")
        || trimmed.contains("rm -r ") || trimmed.contains("rm -f ")
    {
        return true;
    }
    // Hard git resets
    if trimmed.contains("git reset --hard") || trimmed.contains("git clean -f") {
        return true;
    }
    // Disk-level writes
    if trimmed.starts_with("dd ") || trimmed.starts_with("mkfs") || trimmed.starts_with("shred ") {
        return true;
    }
    false
}
```

Inside `dispatch_plan` for `"run_command"`, before emitting `ToolCall`:

```rust
if is_potentially_destructive(cmd) {
    match self.destructive_cmd_policy.as_str() {
        "block" => {
            self.emit_missing_args(planned, "rejected_destructive_command");
            return;
        }
        "warn" => {
            // Emit structured warning event but still execute
            if let Some(emitter) = self.emitter.as_ref() {
                emitter.emit(CanonEvent::Debug(canon_event::DebugEvent {
                    source: "act_consumer".to_string(),
                    kind: "destructive_command_warning".to_string(),
                    payload: serde_json::json!({
                        "cmd": cmd,
                        "policy": "warn",
                        "action_id": planned.action_id,
                    }),
                }));
            }
        }
        _ => {} // "allow" — pass through silently
    }
}
```

**Default policy** should be `"warn"` to avoid breaking existing usage while making
destructive commands visible. Set `CANON_DESTRUCTIVE_CMD_POLICY=block` in environments
where data loss risk is not acceptable.

### Acceptance criteria
- With `CANON_DESTRUCTIVE_CMD_POLICY=block`: a planned `rm -rf` action emits `LoopActed`
  with `success=false` and `stderr="rejected_destructive_command"` without any filesystem
  side effect.
- With `CANON_DESTRUCTIVE_CMD_POLICY=warn` (default): a `destructive_command_warning`
  debug event appears in the tlog before the command executes.
- With `CANON_DESTRUCTIVE_CMD_POLICY=allow`: no change in behavior (full pass-through).

---

## Issue 4 — No batch-complete marker

### Root cause

Artifacts are per-action (`{n}_tool_call.json`, `{n}_tool_results.json`) but there is no
file that records the lifecycle of the full planner-response batch: how many actions were
planned, how many dispatched, how many completed.

A snapshot taken mid-execution looks identical to a snapshot of a partial/failed batch.

File: `canon-utils/canon-act/src/lib.rs`

### Fix — per-batch status tracker + `{n}_batch_status.json`

Add a `batch_tracker: HashMap<String, BatchStatus>` to `ActConsumer` keyed by
`llm_request_id`.

```rust
#[derive(Default)]
struct BatchStatus {
    artifact_n: u32,
    planned: usize,
    dispatched: usize,
    completed_ok: usize,
    completed_fail: usize,
}
```

**On `enqueue_plan`**: if this is the first action for a given `llm_request_id`, insert a
new `BatchStatus` with the resolved `artifact_n`. Increment `planned`. Call
`write_batch_status_artifact(artifact_n, &status, "in_progress")`.

**On `dispatch_plan`** (for non-no_op/done actions): increment `dispatched`. Call
`write_batch_status_artifact`.

**On `handle_completed` / `handle_failed`**: look up the `llm_request_id` from the
completed `PendingAct`, increment the appropriate counter. When
`completed_ok + completed_fail == planned`, write final `{n}_batch_status.json` with
`status: "completed"` (all ok) or `status: "failed_partial"` (some failed).

Artifact schema (`{n}_batch_status.json`):
```json
{
  "n": 0,
  "llm_request_id": "...",
  "status": "in_progress | completed | failed_partial",
  "planned": 4,
  "dispatched": 3,
  "completed_ok": 3,
  "completed_fail": 0,
  "updated_ms": 1773983027598
}
```

Note: `done` and `no_op` action kinds contribute to `planned` and `completed_ok` counts
but are handled inline (no async pending), so they should be counted immediately upon
`dispatch_plan` returning without setting `self.pending`.

### Acceptance criteria
- A `{n}_batch_status.json` exists for every LLM response index `n` that produced at
  least one `LoopPlanned` event.
- `status` transitions from `"in_progress"` to `"completed"` or `"failed_partial"` once
  `completed_ok + completed_fail == planned`.
- External observers can determine batch health from a single file read without inspecting
  `{n}_tool_results.json`.

---

## Implementation Order

Execute in this order to minimise regressions:

1. **Issue 2** (pending reconciliation on Tick) — lowest risk, isolated to startup + Tick
   handler. Test: kill the process mid-run, restart, verify pending → failed transition.

2. **Issue 1** (queued artifact at enqueue time) — requires adding `queued_artifact_index`
   cache. Test: compare `valid_action_count` in `_response.json` with entry count in
   `_tool_call.json` immediately after LoopPlanned events land.

3. **Issue 4** (batch-complete marker) — depends on the `queued_artifact_index` cache from
   Issue 1. Test: verify `{n}_batch_status.json` shows `status=completed` once all results
   land.

4. **Issue 3** (destructive command guardrail) — isolated to `dispatch_plan`. Test with
   `CANON_DESTRUCTIVE_CMD_POLICY=block`, emit a goal that produces `rm -rf`, verify no
   deletion occurs and `LoopActed.success=false` is emitted.

## Files to Modify

| File                               | Issues     |
|------------------------------------+------------|
| `canon-utils/canon-act/src/lib.rs` | 1, 2, 3, 4 |

No other files require changes for these fixes. The `llm_executor.rs` and
`event_runtime.rs` changes from the previous REPAIR_PLAN remain valid and are not
superseded.

## Out of Scope

- Multi-process locking for artifact files (currently fine; single W writer).
- Parser `parse_ok` flag promotion for multi-block outputs (addressed in prior plan).
- Route oscillation guard (addressed in prior plan).
