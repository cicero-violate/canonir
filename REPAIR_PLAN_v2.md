# REPAIR PLAN v2 — post-implementation review (2026-03-20 01:21)

## Observed vs expected per index

|    n | parse_blocks | planned | dispatched | completed | batch_status        | Notes                                        |
|------+--------------+---------+------------+-----------+---------------------+----------------------------------------------|
| 0000 |            4 |       4 |          3 | 3 ok      | completed           | OK (done counted inline)                     |
| 0001 |            4 |       4 |          3 | 1ok+1fail | **in_progress**     | STUCK — reconciled write_file not counted    |
| 0002 |            5 |       5 |          2 | 2 ok      | **empty (0 bytes)** | rm-rf executed; process crashed during write |

---

## Bug 1 — `_tool_results.json` missing `llm_request_id` in pending row

### Evidence
`0001_batch_status.json` is stuck at `in_progress` (`completed_ok=1, completed_fail=1,
planned=4`). The third action (`write_file`) was reconciled by
`reconcile_stale_pending_artifacts` with `error: "aborted_or_timeout"`, but the batch
tracker was not updated.

### Root cause
`write_tool_result_pending_artifact` does not write `llm_request_id` into the row. When
`reconcile_stale_pending_artifacts` later processes that row, it calls:
```rust
self.mark_batch_completion(row.get("llm_request_id").and_then(|v| v.as_str()), false);
```
`row.get("llm_request_id")` is `None`, so `mark_batch_completion` returns immediately —
the batch tracker is never updated.

### Fix
Add `llm_request_id` to the pending row written by `write_tool_result_pending_artifact`.
The method already receives the `planned: &LoopPlanned` argument which carries
`planned.llm_request_id`. Add it to the `serde_json::json!({...})` object:

```rust
// in write_tool_result_pending_artifact
"llm_request_id": planned.llm_request_id,
```

---

## Bug 2 — `reconcile_stale_pending_artifacts` does not resume batch dispatch

### Evidence
Index 0001: after `write_file` is reconciled, the `done` action (4th item, same batch)
remains in the queue forever. `0001_batch_status.json` stays `in_progress`. The batch
never reaches `planned=4, completed_ok+fail=4`.

### Root cause
`reconcile_stale_pending_artifacts` emits `LoopActed` for the reconciled item but **does
not call `dispatch_next_in_active_batch`**. The regular completion paths
(`handle_completed`, `handle_failed`, `check_timeout`) all call it; reconciliation does
not. After reconciliation clears `self.pending` (reconciliation writes the file
directly — it doesn't touch `self.pending` at all), the dispatch chain is never resumed.

Actually more precisely: reconciliation does not clear `self.pending` either — it only
modifies the file on disk. If the in-memory `self.pending` still holds the pending action
(e.g. this process dispatched it and it timed out on-disk but the in-memory pending was
not cleared by `check_timeout` because it is a stale artifact from a previous process),
nothing re-dispatches.

The clearest case: previous-process artifacts with `status: "pending"` are reconciled at
startup via `set_emitter → reconcile_stale_pending_artifacts`. The current process has no
in-memory `self.pending`, the queue may contain remaining actions, but nothing triggers
`dispatch_batch_on_execute` for them.

### Fix
After the reconciliation loop finishes processing all changed rows, call
`dispatch_next_in_active_batch` to resume any queued items from the same batch:

```rust
// in reconcile_stale_pending_artifacts, after the `if changed { ... }` block:
if changed {
    let _ = std::fs::write(path, serde_json::to_string_pretty(&out_rows).unwrap_or_default());
    // Resume dispatch chain so remaining queued actions (e.g. "done") execute.
    self.dispatch_next_in_active_batch();
}
```

This is safe because `reconcile_stale_pending_artifacts` is called from `Tick` and
`set_emitter`, both of which run on the single writer thread (W=1). No concurrency hazard.

---

## Bug 3 — `_batch_status.json` written non-atomically; crashes leave 0-byte file

### Evidence
`0002_batch_status.json` is 0 bytes. The process was killed between `std::fs::write`
opening (truncating) the file and writing its content.

### Root cause
`write_batch_status_artifact` uses:
```rust
let _ = std::fs::write(path, serde_json::to_string_pretty(&value).unwrap_or_default());
```
`std::fs::write` truncates the file first, then writes. A crash between truncate and
write-completion leaves a 0-byte file.

### Fix
Write atomically using a temp file + rename (same pattern as `save_cursor` in
`event_runtime.rs`):

```rust
fn write_batch_status_artifact(&self, artifact_n: u32, llm_request_id: &str, status: &str, batch: &BatchStatus) {
    let _ = std::fs::create_dir_all(&self.artifact_dir);
    let path = self.artifact_dir.join(format!("{artifact_n:04}_batch_status.json"));
    let tmp_path = self.artifact_dir.join(format!("{artifact_n:04}_batch_status.tmp"));
    let value = serde_json::json!({
        "n": artifact_n,
        "llm_request_id": llm_request_id,
        "status": status,
        "planned": batch.planned,
        "dispatched": batch.dispatched,
        "completed_ok": batch.completed_ok,
        "completed_fail": batch.completed_fail,
        "updated_ms": now_ms_u64(),
    });
    if let Ok(content) = serde_json::to_string_pretty(&value) {
        if std::fs::write(&tmp_path, &content).is_ok() {
            let _ = std::fs::rename(&tmp_path, &path);
        }
    }
}
```

Apply the same atomic-write pattern to `write_tool_call_queued_artifact` and
`write_tool_result_pending_artifact` — both also use non-atomic writes via
`append_tool_artifact` / `upsert_tool_result_artifact`. Those use a
read-modify-write cycle that is equally crash-unsafe (truncation during write). Use
a temp-file rename pattern in `append_tool_artifact` and `upsert_tool_result_artifact`:

```rust
fn append_tool_artifact(log_dir: &Path, artifact_n: u32, suffix: &str, value: &Value) {
    let _ = std::fs::create_dir_all(log_dir);
    let path = log_dir.join(format!("{artifact_n:04}_{suffix}.json"));
    let tmp  = log_dir.join(format!("{artifact_n:04}_{suffix}.tmp"));
    let mut rows = read_artifact_rows(&path);
    rows.push(value.clone());
    if let Ok(content) = serde_json::to_string_pretty(&rows) {
        if std::fs::write(&tmp, &content).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}
// Same pattern for upsert_tool_result_artifact.
```

---

## Bug 4 — Issue 3 (destructive command) still executes: default policy is `"warn"` not `"block"`

### Evidence
`0002_tool_results.json` entry 1: `cmd: "rm -rf /workspace/ai_sandbox/canon/test_rust_project_v3"`,
`status: "completed"`, `success: true`. The guardrail emitted a debug event but allowed
the deletion.

### Root cause
`DestructiveCmdPolicy::from_env()` defaults to `Warn` when
`CANON_DESTRUCTIVE_CMD_POLICY` is unset. `Warn` only emits a debug event; it does not
block execution.

### Fix
Change the default from `Warn` to `Block`:

```rust
fn from_env() -> Self {
    match env::var("CANON_DESTRUCTIVE_CMD_POLICY")
        .unwrap_or_else(|_| "block".to_string())   // was "warn"
        .to_ascii_lowercase()
        .as_str()
    {
        "allow" => Self::Allow,
        "warn"  => Self::Warn,
        _       => Self::Block,
    }
}
```

Operators who need to allow destructive commands (e.g. `rm -rf target/` as part of a
clean build) must set `CANON_DESTRUCTIVE_CMD_POLICY=allow` explicitly. This is a
**safe-by-default** change.

---

## Bug 5 — `_tool_call.json` two incompatible schemas in the same array

### Evidence
`0000_tool_call.json` entries 1–4 are "queued" records:
```json
{ "n":0, "status":"queued", "action_id":"...", "action_kind":"run_command", "payload":{...} }
```
Entries 5–7 are "dispatched" records:
```json
{ "n":0, "kind":"bash", "node_id":"...", "tool_call_id":"...", "request_id":"..." }
```
The schemas are incompatible. No field links a queued entry to its corresponding
dispatched entry. An external reader cannot tell whether `action_id` X was dispatched
as `tool_call_id` Y.

### Root cause
`write_tool_call_queued_artifact` appends one schema; `write_tool_call_artifact` appends
a completely different schema. No shared key (e.g. `action_id`) is present in both.

### Fix
**Option A — upsert by `action_id`** (preferred): replace `append_tool_artifact` in both
paths with an `upsert_tool_call_artifact(log_dir, artifact_n, action_id, value)` that
merges fields into a single row keyed by `action_id`.

- On enqueue: write initial row `{ action_id, status:"queued", action_kind, payload, ... }`
- On dispatch: merge `{ status:"dispatched", kind, node_id, tool_call_id, request_id, dispatched_ms }` into the same row

```rust
fn upsert_tool_call_artifact(log_dir: &Path, artifact_n: u32, action_id: &str, patch: &Value) {
    let _ = std::fs::create_dir_all(log_dir);
    let path = log_dir.join(format!("{artifact_n:04}_tool_call.json"));
    let tmp  = log_dir.join(format!("{artifact_n:04}_tool_call.tmp"));
    let mut rows = read_artifact_rows(&path);
    let found = rows.iter_mut().find(|r| {
        r.get("action_id").and_then(|v| v.as_str()) == Some(action_id)
    });
    match found {
        Some(row) => {
            // Merge patch fields into existing row
            if let (Some(map), Some(patch_map)) = (row.as_object_mut(), patch.as_object()) {
                for (k, v) in patch_map {
                    map.insert(k.clone(), v.clone());
                }
            }
        }
        None => rows.push(patch.clone()),
    }
    if let Ok(content) = serde_json::to_string_pretty(&rows) {
        if std::fs::write(&tmp, &content).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}
```

Change `write_tool_call_queued_artifact` and `write_tool_call_artifact` (in all four
action-kind branches) to use `upsert_tool_call_artifact` keyed on `action_id` /
`node_id` (same value — `tool_node_id(planned)` == `action_id`).

The dispatch patch must include `action_id: node_id` so the upsert finds the right row.

**Option B** (simpler, lower risk): keep the two separate entries but add a
`status: "dispatched"` field and include `action_id` in the dispatch entry so the two
entries for the same action are linkable by `action_id`:

```rust
// in write_tool_call_artifact, add to the json object:
"status": "dispatched",
"action_id": node_id,       // node_id == action_id
"dispatched_ms": now_ms_u64(),
```

Option B is a minimal change; Option A is cleaner for consumers. Implement Option B first,
then Option A in a follow-up.

---

## Summary of changes — all in `canon-utils/canon-act/src/lib.rs`

| Bug | Change |
|-----|--------|
| 1 | Add `"llm_request_id": planned.llm_request_id` to `write_tool_result_pending_artifact` json object |
| 2 | Call `self.dispatch_next_in_active_batch()` after changed rows are flushed in `reconcile_stale_pending_artifacts` |
| 3 | Make `append_tool_artifact` and `upsert_tool_result_artifact` atomic (write to `.tmp`, rename) |
| 3 | Make `write_batch_status_artifact` atomic (write to `.tmp`, rename) |
| 4 | Change `from_env()` default from `"warn"` to `"block"` |
| 5 | Add `status: "dispatched"`, `action_id: node_id`, `dispatched_ms` to dispatch entries in `write_tool_call_artifact` (Option B) |

## Acceptance criteria

| Check | Pass condition |
|-------|----------------|
| `_batch_status.json` never 0 bytes | Atomic write; crash leaves `.tmp`, not truncated final file |
| `_batch_status.json` `status=completed` or `failed_partial` for every finished index | `completed_ok + completed_fail == planned` with correct counts from reconciliation |
| No `_batch_status.json` stuck at `in_progress` after run ends | `dispatch_next_in_active_batch` called after reconcile |
| `rm -rf` not executed without explicit opt-in | Default policy is `block`; rejected command emits `LoopActed(success=false)` |
| `_tool_call.json` entries linkable by `action_id` | Every dispatch entry contains `action_id` matching its queued entry |

## Implementation order

1. Bug 4 (default `block`) — one-line change, no risk
2. Bug 1 (add `llm_request_id` to pending row) — one-field addition
3. Bug 2 (dispatch resume after reconcile) — add one call after flush
4. Bug 5 Option B (add fields to dispatch entry) — add three fields
5. Bug 3 (atomic writes) — refactor two helper functions
