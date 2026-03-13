I now have everything needed for a complete diagnosis. Here is the implementation plan.

---

## Root Cause Analysis

There are **three distinct bugs** in the event → report pipeline:

---

### Bug 1: Duplicate execution — `CapabilityEventConsumer` and `ReportEventConsumer` both fire on the same event, doing the same work

Both consumers are registered in `build_consumers()` and both mask on `COMPILATION_UNIT_FINISHED`. When a compilation finishes:

- `ReportEventConsumer` calls `generate_reports_from_tlog` directly
- `CapabilityEventConsumer` dispatches → `runner::run_full_analysis` → also calls `generate_reports_from_tlog`

They race. The `RUN_GUARD` in `runner.rs` deduplicates by `crate_name:batch_id`, but `ReportEventConsumer` bypasses the guard entirely — it has its own code path. So you get two concurrent writes to the same report files. Whichever finishes second wins, but if the first write partially completed, the second may read stale graph state.

**This is the primary cause of stale/missing report updates.**

---

### Bug 2: `in_flight` guard in `ReportEventConsumer` is never reset on success path... actually it is reset, but it's synchronous and blocking — the real issue is it never skips a pending update

In `report_consumer.rs` line 63–65: if `in_flight` is true, the event is silently dropped with no retry. Since both consumers are synchronous on the same thread (the event bus dispatches sequentially per `bus.dispatch`), `in_flight` will always be `false` when `on_event` is called. So this guard does nothing useful and masks the real problem — if a compilation fires rapidly twice, the second `COMPILATION_UNIT_FINISHED` is dropped entirely by `ReportEventConsumer`.

---

### Bug 3: `CapabilityEventConsumer` calls `resolve_tlog_path()` at emit time, not construction time — env var inconsistency

In `capability_consumer.rs` line 13, `tlog_path` is resolved via `resolve_tlog_path()` at construction — this picks up `CANON_REPORTS_TLOG` or `CANON_TLOG_PATH` or falls back to the hardcoded path. But on line 60, `emit_analysis_event` calls `resolve_tlog_path()` **again** at emit time. If env vars differ between construction and the moment of emission (e.g. due to process restart timing), the completed event goes to a different tlog than the one being watched. In practice with the current supervisor config this is stable, but it's a latent bug.

---

## Implementation Plan

### Fix 1 — Remove `ReportEventConsumer` from `build_consumers()`

**File:** `canon-utils/event-consumers/src/lib.rs`

Remove the `ReportEventConsumer` line from `build_consumers()`. The `CapabilityEventConsumer` already handles the full pipeline end-to-end: dispatch → `run_full_analysis` → `generate_reports_from_tlog` → emit `analysis.completed`. `ReportEventConsumer` is documented in `report_pipeline.rs` as the "incremental" path but it duplicates the capability path and races against it.

`ReportEventConsumer` should be retained as a type (it is useful for offline/test replay via `--once` mode) but must not be in the live consumer list.

---

### Fix 2 — Make `CapabilityEventConsumer` the single authority; harden the `RUN_GUARD`

**File:** `canon-analysis/src/capabilities/runner.rs`

The current `RUN_GUARD` key is `crate_name:batch_id`. The `batch_id` is a millisecond timestamp generated in `dispatcher.rs`. If two `COMPILATION_UNIT_FINISHED` events fire within the same millisecond for the same crate, they get the same `batch_id` and the second is correctly skipped. But if they fire 1ms apart, both run. 

Change the guard key to just `crate_name`, and add a cooldown: after a run completes for a crate, hold the guard for a configurable window (e.g. 2000ms, or read from `CANON_ANALYSIS_COOLDOWN_MS`). Clear it after the cooldown, not immediately after the run. This prevents redundant back-to-back runs when a crate emits multiple finish events in a burst.

---

### Fix 3 — Pin `tlog_path` at construction in `CapabilityEventConsumer`; remove the second `resolve_tlog_path()` call

**File:** `canon-analysis/src/capability_consumer.rs`

On line 60, replace the call to `crate::capabilities::events::resolve_tlog_path()` with `&self.tlog_path` (the already-resolved path stored at construction). This ensures the `analysis.completed` / `analysis.failed` events always go to the same tlog the consumer was configured against.

---

### Fix 4 — Drop the `in_flight` guard from `ReportEventConsumer` or replace it with a pending-rerun flag

**File:** `canon-analysis/src/report_consumer.rs`

Since `ReportEventConsumer` is being removed from the live consumer list (Fix 1), this only matters for offline/test use. But for correctness: replace the `in_flight` drop with a `pending: bool` flag. If `in_flight` is true when an event arrives, set `pending = true` instead of dropping. After the run completes, if `pending` is true, clear it and re-run immediately. This way no compilation finish event is ever silently lost.

---

### Summary of files to change

| File | Change |
|---|---|
| `canon-utils/event-consumers/src/lib.rs` | Remove `ReportEventConsumer` from `build_consumers()` |
| `canon-analysis/src/capabilities/runner.rs` | Change guard key to crate-only + add cooldown window |
| `canon-analysis/src/capability_consumer.rs` | Use `self.tlog_path` instead of second `resolve_tlog_path()` call |
| `canon-analysis/src/report_consumer.rs` | Replace `in_flight` drop with `pending` re-run flag |
