# PENDING

Outstanding issues, gaps, and deferred work across the canon runtime.
Ordered by severity within each category.

---

## 1. Bugs — Active / Will Cause Incorrect Behavior

### 1.1 `error_id` not generated for direct `ErrorOccurred` emitters

**Severity**: High — `error_id` was added to deduplicate errors, but 8 call sites emit
`ErrorOccurred` with `error_id: None` directly, bypassing `ErrorLogger` (which is the
only place that generates the UUID). These events hit the tlog with null IDs.

**Affected files and lines:**

| File                                                 | Line(s)                 |
|------------------------------------------------------+-------------------------|
| `canon-reward/src/lib.rs`                            | 132                     |
| `canon-runtime/src/consumers/llm_executor.rs`        | 103, 227                |
| `canon-runtime/src/consumers/capability_executor.rs` | 55                      |
| `canon-runtime/src/lib.rs`                           | 321, 338, 359, 372, 435 |

**Fix**: Extract a helper `fn new_error_occurred(...) -> ErrorOccurred` that always
generates a UUID `error_id`. Replace all inline `ErrorOccurred { ..., error_id: None }`
constructors with this helper, or generate the UUID inline at each site.

---

### 1.2 `trace_id` never propagated into `ErrorOccurred`

**Severity**: Medium — when an error fires mid-loop (e.g., a bash command fails), the
active `trace_id` for that cycle is available in `RewardConsumer`, `VerifyConsumer`, and
`ActConsumer`, but it is never passed to the `ErrorOccurred` event. Every `ErrorOccurred`
in the tlog has `"trace_id": null`. Cross-referencing errors to the loop cycle that caused
them requires timestamp heuristics instead of ID lookup.

**Affected files**: all sites in §1.1 above, plus `error_logger.rs` which sets
`"trace_id": null` hardcoded in all `CapabilityFailed`, `NodeFailed`, `LoopActed`,
`LoopVerified`, and `LoopRewarded` arms.

**Fix**: Each consumer that has a `last_trace_id` field (`RewardConsumer`, `VerifyConsumer`)
should pass it when constructing `ErrorOccurred`. For `ErrorLogger`, the `LoopActed` and
`LoopVerified` arms already receive the event which now carries `trace_id` — extract it
and set it in the payload instead of hardcoding `null`.

---

### 1.3 LLM response cache not busted on repeated action failure

**Severity**: Medium — `endpoint_worker.rs` maintains an in-process cache keyed by
`(prompt_hash, role_schema_hash)`. If a planned action fails and `build_prompt` produces
the same text as the previous tick (same goal, same error state), the cache returns the
prior response at `duration_ms: 0` without hitting the LLM. The agent loops forever on
the same failing action.

`planner_refine_on_cache: bool` exists in `CapabilityConfig` (defaulting to `false`) but
it is passed through to `llm_worker_send_request` and never used to invalidate the cache
entry before re-sending.

**File**: `canon-llm-runtime/src/endpoint_worker.rs:45–46`, `llm.rs:71–73`

**Fix**: When `planner_refine_on_cache` is `true` (which it should be for the planner
role), evict the cache entry before dispatching the request, not after. Alternatively,
include the `last_action_result` hash in the cache key so a new failure always produces
a new key. The `build_prompt` fix (adding `## Last Action Result`) partially mitigates
this by changing the prompt text, but only if the action actually changes — repeated
identical failures still hit the cache.

---

### 1.4 `VerifyConsumer` runs `cargo check` on every `LoopActed` including `no_op`

**Severity**: Low — `VerifyConsumer::on_event` triggers on every `LoopActed` with no
guard on `action_kind`. `no_op` and `done` actions each trigger a full `cargo check`
invocation (up to 30s timeout), burning CPU and blocking the loop on idle ticks.

**File**: `canon-verify/src/lib.rs:36–38`

**Fix**: Skip verification when `acted.action_kind == "no_op"`. For `done`, verification
is still correct (confirm clean exit), but `no_op` should short-circuit to
`LoopVerified { passed: true, ... }` immediately.

---

## 2. Code Quality — Technical Debt

### 2.1 `#[macro_export]` on internal macro

**File**: `canon-runtime-events/src/lib.rs:16`

```rust
#[macro_export] // TODO() need to remove this
```

The macro is marked `pub` via `#[macro_export]` unintentionally. If the macro is only
used within the crate, replace with a plain `macro_rules!` block (no `#[macro_export]`).
If it needs to be pub, remove the TODO comment and document the intent.

---

### 2.2 PLAN.md has stale TODO statuses

**File**: `canon-utils/PLAN.md:318–324`

The summary table lists bugs 1–5 as `TODO` but they were implemented and are now working.
Update or archive this file to reflect the current state so it does not mislead future
agents.

---

## 3. ID Infrastructure — Deferred from PLAN_v2.md

These IDs were explicitly deferred in Phase 5.5 of PLAN_v2.md. They have no
implementation home yet. Do not stub. Implement when the relevant module is built.

| Target ID(s) | Blocked on |
|---|---|
| `memory_id`, `embedding_id`, `document_id`, `chunk_id` | Memory / retrieval module |
| `intent_id` | Intent-routing layer |
| `invariant_id`, `violation_id` | Invariant registry |
| `resource_id`, `response_id` | Resource abstraction layer |
| `task_id`, `job_id` | Async executor (if std::thread is replaced) |
| `auth_id`, `permission_id`, `audit_id` | Security layer |
| `state_id` | Snapshotting mechanism |
| `object_version` | Per-entity versioning |
| `event_batch_id` | Not applicable to current architecture |

---

## 4. Infrastructure — Missing / Incomplete

### 4.1 No schema migration path

`EVENT_SCHEMA_VERSION = "1"` is now stamped on `runtime_started`. But there is no
reader logic that handles version mismatches when replaying old segments. If the schema
is bumped to `"2"`, the tlog replayer will silently deserialize old events with missing
fields (relying on `#[serde(default)]`). This is safe for optional fields but will
silently drop required fields added in future versions.

**Needed**: A replay guard in the tlog reader that warns (or errors) when it encounters
a `schema_id` it does not recognize.

---

### 4.2 `session_id` not validated on resume

The cursor file now persists `session_id` so a restarted process can resume the same
session. However, there is no validation that the `session_id` in the cursor matches the
one in the last `runtime_started` event in the tlog. If the tlog is replaced or truncated
externally, the cursor `session_id` will be stale with no warning.

**File**: `canon-runtime/src/bin/event_runtime.rs` — `load_cursor_seq`

---

### 4.3 `system_id` file not portable across environments

`load_or_create_system_id()` writes to the hardcoded path
`/workspace/ai_sandbox/canon/state/system_id`. If the workspace root changes, the file
is not found and a new `system_id` is silently generated, breaking cross-environment
identity.

**Fix**: Derive the path from `CANON_WORKSPACE` env var or the same base directory
resolution logic used for the tlog path.

---

### 4.4 `build_id` / `commit_id` fall back to `"unknown"` silently

`build.rs` runs `git rev-parse --short HEAD` and falls back to `"unknown"` if git is
unavailable. When the binary is built in a clean CI environment without git, all events
carry `"commit_id": "unknown-<timestamp>"`. There is no warning at runtime.

**Fix**: Emit a `debug` or `warn` log line at startup if `CANON_COMMIT_ID` is `"unknown"`.

---

## 5. Observability Gaps

### 5.1 No tlog reader / replayer tool

The tlog is a binary segmented format. There is no standalone CLI tool to:
- Print events from a segment in human-readable form
- Filter by `trace_id`, `session_id`, `tick`, or `kind`
- Verify `event_id` monotonicity across segments

`state/watch_log.py` exists (87 lines) but reads the JSON tlog format, not the binary
segments.

---

### 5.2 No cross-segment `event_id` continuity check

`event_id` resets to `0` when the process restarts (it is not persisted in the cursor).
Two segments from different process invocations may have overlapping `event_id` values.
The `session_id` distinguishes them, but a replayer must use `(session_id, event_id)`
as the composite key — this is not documented anywhere.

**Fix**: Persist `next_id` in the cursor file alongside `state_version` so `event_id`
stays monotonic across restarts within the same session.

---
