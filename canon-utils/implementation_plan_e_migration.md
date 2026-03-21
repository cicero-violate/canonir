# Implementation Plan: E-Migration — Unified Internal Emission

## Current Build Status

```
Phase 1 — ✅ complete  (ErrorLogger stores and uses emitter; direct fallback retained for pre-init)
Phase 2 — ✅ complete  (cargo check --workspace exits 0; async_consumers_preserve_order_per_consumer passes)
```

**Confirmed 2026-03-21:**
- `cargo check --workspace` — zero errors
- `cargo test -p canon-runtime` — 1 test passes (`async_consumers_preserve_order_per_consumer`)

---

## Goal

Close the last internal emission bypass. All events generated **inside the running runtime**
must flow through `EventEmitter → Bus → Log`. External tools and pre-runtime producers
remain direct-write — they are controlled boundaries, not bugs.

```
Status before:    ~90% event-driven (behavioral collapse complete, one emission bypass)
Status after:     ~95% event-driven (internal closure complete, edges deliberately kept)
```

---

## Emission Audit Results

### Internal bypass — FIX THIS

| File | Line | Form | Issue |
|------|------|------|-------|
| `canon-runtime/src/consumers/error_logger.rs` | 59 | direct `canon_emit_meta!(source, kind, payload, &self.tlog_path)` | Consumer ignores its own `set_emitter()` — `_emitter` is discarded on line 63 |

**The problem in full:**
```rust
// Current — bypasses bus
fn set_emitter(&mut self, _emitter: EventEmitterHandle) {}   // emitter thrown away

fn on_event(&mut self, event: &CanonEvent) {
    // ...
    let _ = canon_meta::canon_emit_meta!(source, "error_occurred", payload.clone(), &self.tlog_path);
}
```

`ErrorLogger` is an `EventConsumer` registered with the runtime. It receives events from W,
converts them into `error_occurred` events, but writes those back to tlog directly instead of
routing through the bus. The `error_occurred` event is therefore invisible to other consumers
until the P2 watcher picks it up from the tlog on the next filesystem notification — a
nondeterministic delay. Via the emitter it is delivered synchronously in the same cycle.

**No reentrancy risk:** `ErrorLogger::on_event(CanonEvent::ErrorOccurred)` only appends to
the JSONL file (lines 46–49). It does not emit anything. So emitting `ErrorOccurred` via
the emitter will not loop back to generate more emissions.

---

### Controlled boundaries — KEEP, DO NOT CHANGE

| File | Form | Why keep |
|------|------|---------|
| `canon-runtime-events/src/bin/emit_capability_event.rs` | direct | External CLI tool — deliberate input port |
| `canon-runtime/src/bootstrap.rs:138` | direct | Pre-runtime bootstrap — no emitter exists yet |
| `canon-runtime/src/bootstrap.rs:176` | direct | P4 watcher producer thread — writes to tlog; P2 picks it up and re-delivers through Q_e to W. This IS the correct P4 → L → P2 → Q_e → W path |
| `canon-runtime/src/lib.rs:480` | `write_event_auto` | This IS the bus writer — W's own tlog append path after emitter dispatch |
| `canon-builder/src/process.rs` | direct | External build tool, separate process |
| `canon-tools-editor/src/tlog.rs`, `loader.rs` | direct | External editor tool, separate process |
| `canon-tools-analysis/src/capabilities/events.rs` | direct | External analysis tool, separate process |

These are documented as external boundaries. They are the "Edges = Controlled Imperative"
half of the target architecture. Do not remove them.

---

## Phase 1 — Fix `ErrorLogger`

**File:** `canon-utils/canon-runtime/src/consumers/error_logger.rs`

### Step 1a — Add emitter field to struct

```rust
// Before:
pub struct ErrorLogger {
    tlog_path: PathBuf,
    jsonl_path: PathBuf,
    seen: HashMap<u64, Instant>,
}

// After:
pub struct ErrorLogger {
    tlog_path: PathBuf,
    jsonl_path: PathBuf,
    seen: HashMap<u64, Instant>,
    emitter: Option<EventEmitterHandle>,
}
```

### Step 1b — Initialize field in constructor

```rust
// Before:
Self { tlog_path, jsonl_path, seen: HashMap::new() }

// After:
Self { tlog_path, jsonl_path, seen: HashMap::new(), emitter: None }
```

### Step 1c — Store emitter in `set_emitter()`

```rust
// Before:
fn set_emitter(&mut self, _emitter: EventEmitterHandle) {}

// After:
fn set_emitter(&mut self, emitter: EventEmitterHandle) {
    self.emitter = Some(emitter);
}
```

### Step 1d — Replace direct write with emitter emit

In `on_event()`, replace line 59:

```rust
// Before:
let _ = canon_meta::canon_emit_meta!(source, "error_occurred", payload.clone(), &self.tlog_path);

// After:
if let Some(emitter) = &self.emitter {
    canon_meta::canon_emit_meta!(emitter; source, "error_occurred", payload.clone());
} else {
    let _ = canon_meta::canon_emit_meta!(source, "error_occurred", payload.clone(), &self.tlog_path);
}
```

The `else` branch is kept as a fallback for the case where `set_emitter()` has not yet been
called (e.g., in unit tests or before the consumer is registered with the runtime). It does
not affect any running-runtime path.

### Step 1e — Add import if needed

Check if `EventEmitterHandle` is already imported at line 1. It is:
```rust
use canon_event::{new_error_occurred, CanonEvent, EventConsumer, EventEmitterHandle, EventFilter, RustcEvent};
```
No import change needed.

**Checkpoint:** `cargo check -p canon-runtime` exits 0.

---

## Phase 2 — Verify

```bash
cargo check --workspace
cargo test -p canon-runtime
```

Expected:
- `cargo check --workspace` — zero errors
- `cargo test -p canon-runtime` — `async_consumers_preserve_order_per_consumer` passes

---

## Execution Order

```
Phase 1 — ✅ complete
Phase 2 — ✅ complete
```

---

## Files Modified

| Phase | Status | File | Change |
|-------|--------|------|--------|
| 1 | ✅ | `canon-runtime/src/consumers/error_logger.rs` | Added `emitter` field; implemented `set_emitter`; replaced direct write with emitter emit, direct fallback if None |

---

## Architecture After E-Migration

```
Internal emission paths:
  LoopStageExecutor  → emitter.emit() ✅
  RouteExecutor      → emitter.emit() ✅
  CapabilityExecutor → emitter.emit() ✅
  CheckConsumer      → emitter.emit() ✅ (already uses emitter form)
  ErrorLogger        → emitter.emit() ✅ (fixed here)

Controlled external boundaries (deliberate, kept):
  emit_capability_event binary      → direct tlog write  (input port)
  bootstrap.rs config writes        → direct tlog write  (pre-runtime)
  bootstrap.rs P4 prompt watcher    → direct tlog write  (producer thread)
  canon-builder, canon-tools-*      → direct tlog write  (external tools)

Core = Fully Event-Driven ✅
Edges = Controlled Imperative ✅
```

---

## What This Is NOT Pursuing (and Why)

**Worker threads as EventConsumers** — the `llm_executor_worker` thread dispatches LLM I/O
and emits `CapabilityCompleted` back through the emitter. The thread itself is execution
infrastructure, not a routing concern. Worker = Infra is correct. Do not change.

**Full state replay from events** — `LoopContext.artifact_counter` reads filesystem at init.
This is initialization state, not routing state. Making it event-driven would require a new
event variant and adds cost with no correctness benefit. Leave it.

**External injection** — `emit_capability_event` is a useful controlled input port.
Removing it closes the system and makes it harder to test and operate. Leave it.
