# Implementation Plan: H — Canonical Event Schema Unification

## Current Build Status

```
Phase 1 — ✅ complete  (wire.rs: CanonEvent + CanonPayload + EventMeta defined)
Phase 2 — ✅ complete  (CanonEvent bus enum renamed → RuntimeEvent; async_bus.rs test updated)
Phase 3 — 🟡 partial   (runtime_event_to_wire() exists; CanonEvent still nested inside TlogEvent)
Phase 4 — 🔴 not started  (collapse macro forms)
Phase 5 — 🔴 not started  (update readers: process_events + tlog scan)
Phase 6 — 🔴 not started  (update external tools)
Phase 7 — 🔴 not started  (compile-time guard; verify)
```

## Micro-Plan Breakdown (one Codex turn each)

Phases 3–7 are split into focused single-turn plans. Start each only after the previous exits 0.

| Plan file | Status | Scope | Key change |
|-----------|--------|-------|------------|
| `implementation_plan_h1b_projector_fix.md` | 🔴 **START HERE — BLOCKING** | `wire.rs` + 2 projectors + `lib.rs` | Add `kind_str()`/`as_value()` to CanonPayload; fix `canon.kind`/`canon.ts`/`payload.get()` in projectors; add missing variants |
| `implementation_plan_h1_tlog_writer.md` | 🔴 | `canon-runtime-events` only | Add `write_canon_event_auto` + `BinarySegmentWriter::write_canon_event` alongside existing write fns |
| `implementation_plan_h2_append_switch.md` | 🔴 | `canon-runtime/src/lib.rs` only | `append_runtime_event` stops wrapping CanonEvent in TlogEvent — writes wire directly |
| `implementation_plan_h3_process_events_reader.md` | 🔴 | `canon-runtime/src/lib.rs` only | `process_events` decodes CanonEvent typed match; fallback for old TlogEvent records |
| `implementation_plan_h4_macro_collapse.md` | 🔴 | `canon-meta/src/lib.rs` only | Form 1 macro produces CanonEvent instead of TlogEvent + data/meta nesting |
| `implementation_plan_h5_cleanup.md` | 🔴 | `canon-runtime-events` + `lib.rs` | Deprecate old write path; fix any remaining callers |
| `implementation_plan_h6_tlogevent_removal.md` | 🔴 | `canon-runtime-events` + readers | Delete TlogEvent write path; upgrade scan_tlog_for_goal to typed decode |

**Confirmed 2026-03-21:**
- `cargo check --workspace` — zero errors
- IDE diagnostic `unresolved import canon_event::CanonEvent` in `async_bus.rs` is **stale** — the file
  already uses `RuntimeEvent` at line 1; cargo check is clean
- `canon-runtime-events/src/wire.rs` — Phase 1 complete
- `canon-runtime-events/src/events.rs` — `RuntimeEvent` enum, Phase 2 complete
- `canon-runtime/src/lib.rs:391` — `runtime_event_to_wire()` present, Phase 3 partial

**Prerequisite:** G-migration — ✅ complete (2026-03-21). H is ready to continue at Phase 3.

---

## Goal (from proposal)

```
|F| → 1,   E := { meta, payload }
emit(event) → serialize(E)
reader(raw) → deserialize(E)
∀ fᵢ ∈ F, fᵢ → emit(E)
```

One schema. One emit path. One reader path. No shape detection in any reader.

---

## Why This Matters

Bug B in G-migration exists because three emit forms produce three different wire shapes.
Readers must know which form wrote an event to know where content lives. That is structural
maintenance. Unifying the wire format removes that cost permanently.

Current wire shapes in tlog today (two shapes, one broken):

```json
// Form 1 (bootstrap, direct):
{ "payload": { "data": { "content": "..." }, "meta": { "file": "...", "line": 138 } } }

// Form 3 (RouteExecutor, typed variant):
{ "payload": { "tick": 1, "approved_route": "scan", ... } }
```

Target: every tlog record has the same shape:

```json
{ "meta": { "ts": 1774106835, "source": "bootstrap", "file": "src/bootstrap.rs", "line": 138 },
  "payload": { "kind": "prompt_loaded", "data": { "content": "...", "path": "..." } } }
```

---

## Corrections to Original Proposal

### Correction 1 — `CanonEvent` name is already taken

The codebase uses `CanonEvent` as the runtime bus enum (`CanonEvent::LoopObserved`, etc.).
The new wire-format struct cannot reuse that name without a rename step first.

**Resolution:**
- Rename existing `CanonEvent` enum → `RuntimeEvent` (bus/consumer dispatch type, internal only)
- New wire-format struct takes the name `CanonEvent` (tlog serialization type, crosses process boundaries)

### Correction 2 — `&'static str` is too restrictive for source

```rust
// Proposed — broken for dynamic sources:
pub source: &'static str,

// Correct:
pub source: String,
```

All existing source strings are `String`. Keep it `String`.

### Correction 3 — External tools cannot use an emitter

`canon-builder`, `canon-tools-editor`, `canon-tools-analysis`, bootstrap are separate processes.
They write to tlog directly and cannot route through `EventEmitter`. The fix is NOT to remove the
direct write path — it is to make the direct write path produce the same wire format. One serializer,
two transport modes:

```
Mode A (emitter): RuntimeEvent → CanonEvent { meta: auto_meta!(), payload } → bus → append_tlog
Mode B (direct):  payload → CanonEvent { meta: auto_meta!(), payload } → write_event_auto
```

Both modes serialize the same `CanonEvent` struct. The schema is uniform. The transport differs.

---

## Phase 1 — Define Wire Types ✅ COMPLETE

**File:** `canon-runtime-events/src/wire.rs` (new file)

Define the canonical tlog wire format as a standalone struct separate from the bus enum:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonEvent {
    pub event_id: Option<u64>,
    pub meta: EventMeta,
    #[serde(flatten)]
    pub payload: CanonPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMeta {
    pub ts: u64,
    pub source: String,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data")]
pub enum CanonPayload {
    // map all existing TlogEvent kinds here
    LoopObserved(LoopObserved),
    LoopPlanned(serde_json::Value),
    LoopActed(serde_json::Value),
    LoopVerified(serde_json::Value),
    LoopRewarded(serde_json::Value),
    RouteTick(RouteTick),
    RouteSelected(RouteSelected),
    CapabilityCompleted(serde_json::Value),
    CapabilityFailed(serde_json::Value),
    ErrorOccurred(serde_json::Value),
    Debug(serde_json::Value),
    #[serde(other)]
    Unknown,
}
```

The `serde(tag = "kind", content = "data")` adjacently-tagged form means:
```json
{ "kind": "route_selected", "data": { "tick": 1, "approved_route": "scan", ... } }
```

This is the `payload` field of the outer `CanonEvent`. No more flat fields at the top level.

**Checkpoint:** `cargo check -p canon-runtime-events` exits 0.

---

## Phase 2 — Rename Existing `CanonEvent` → `RuntimeEvent` ✅ COMPLETE

**Files affected:** Every file that imports `CanonEvent` from `canon_event`/`canon_runtime_events`.

This is a mechanical rename. The bus enum that consumers receive in `on_event(&mut self, event: &RuntimeEvent)`
changes name only. All variant names (`RuntimeEvent::LoopObserved`, etc.) are unchanged.

Affected crates:
- `canon-runtime-events/src/events.rs` — definition site
- `canon-runtime-events/src/lib.rs` — re-export
- `canon-runtime/src/lib.rs` — EventRuntime, handle_runtime_event, all arms
- `canon-runtime/src/bin/event_runtime.rs`
- `canon-runtime/src/consumers/*.rs` — all EventConsumer impls
- `canon-loop/src/executor.rs`, `stage/*.rs`
- `canon-route/src/executor.rs`
- `canon-exec/src/exec/mod.rs`

Use `sed -i 's/CanonEvent/RuntimeEvent/g'` across crates, then hand-fix the wire type uses.

**Checkpoint:** `cargo check --workspace` exits 0.

---

## Phase 3 — Single Serializer 🟡 PARTIAL

**What is done:**
- `runtime_event_to_wire(event: &RuntimeEvent) -> Option<canon_event::CanonEvent>` exists at `lib.rs:391`
- All `RuntimeEvent` variants mapped to `CanonPayload` variants
- `source` derived per-variant in the same function
- `append_runtime_event` calls `runtime_event_to_wire` — the old per-variant `match` arms are gone

**What is NOT done — the critical gap:**

`append_runtime_event` at line 332 still wraps the `CanonEvent` wire struct inside a `TlogEvent`:

```rust
// Current — CanonEvent nested inside TlogEvent.payload (WRONG):
let mut canon = TlogEvent::new(wire.meta.source.clone(), kind,
    serde_json::to_value(&wire).unwrap_or_else(|_| serde_json::json!({})));
```

This produces a hybrid tlog record on disk:
```json
{
  "event_id": 1, "ts": ..., "source": "observe", "kind": "loop_observed",
  "payload": {
    "event_id": null,
    "meta": { "ts": ..., "source": "observe", "file": "", "line": 0 },
    "kind": "LoopObserved",
    "data": { "tick": 1, ... }
  }
}
```

The outer `TlogEvent` still drives `process_events` via `canon.kind` and `canon.source` string matching.
`CanonEvent` is redundantly nested as `TlogEvent.payload`. Source and kind are duplicated.

**Phase 3 completion requires:**

Replace `TlogEvent` as the tlog record type with `CanonEvent` directly. This means:

1. Update `write_event_auto` and `BinarySegmentWriter::write_event` to accept `&CanonEvent` instead
   of `&TlogEvent`.

2. Update `append_runtime_event` to write the wire struct directly:
   ```rust
   // After — CanonEvent IS the tlog record:
   let mut wire = runtime_event_to_wire(event)?;
   wire.event_id = Some(self.next_id);
   self.next_id = self.next_id.saturating_add(1);
   // write wire directly — no TlogEvent wrapper
   ```

3. Update `process_events` readers (overlap with Phase 5 — coordinate).

**The tlog record on disk after Phase 3 complete:**
```json
{
  "event_id": 1,
  "meta": { "ts": 1774106835, "source": "observe", "file": "", "line": 0 },
  "kind": "LoopObserved",
  "data": { "tick": 1, "error_count": 0, "goal_text": "..." }
}
```

No `payload` wrapper. No `source`/`kind` duplication. `CanonPayload`'s `serde(tag="kind", content="data")`
flattens directly into the outer struct via `serde(flatten)`.

**Files:**
- `canon-runtime-events/src/tlog/mod.rs` (or wherever `write_event_auto` is defined) — accept `&CanonEvent`
- `canon-runtime-events/src/tlog/segment.rs` (or `BinarySegmentWriter`) — accept `&CanonEvent`
- `canon-runtime/src/lib.rs:append_runtime_event` — remove `TlogEvent::new` wrapper

**Checkpoint:** `cargo check -p canon-runtime` exits 0. Tlog records match target shape above.

---

## Phase 4 — Collapse Macro Forms

**File:** `canon-meta/src/lib.rs`

Replace the three forms with one form that produces a `RuntimeEvent` carrying `EventMeta`:

```rust
// New canon_emit! — single form for emitter path
macro_rules! canon_emit {
    ($emitter:expr; $variant:ident($inner:expr)) => {{
        $emitter.emit(canon_event::RuntimeEvent::$variant($inner))
    }};
}

// New canon_emit_meta! — captures source location, wraps into RuntimeEvent
// Emitter form — routes through bus
macro_rules! canon_emit_meta {
    ($emitter:expr; $variant:ident($inner:expr)) => {{
        // typed variant — no payload wrapping needed; meta captured at Phase 3 serializer
        canon_event::canon_emit!($emitter; $variant($inner))
    }};
    // Direct form — external tools / bootstrap
    ($source:expr, $kind:expr, $payload:expr, $path:expr) => {{
        let __meta = canon_meta::EventMeta {
            ts: canon_event::now_ms(),
            source: $source.to_string(),
            file: file!().to_string(),
            line: line!(),
        };
        let __wire = canon_event::CanonEvent {
            event_id: None,
            meta: __meta,
            payload: canon_event::CanonPayload::from_kind_data($kind, $payload),
        };
        canon_event::write_event_auto($path, &__wire)
    }};
}
```

This collapses Form 1 and Form 3. Form 2 (emitter debug string form) is removed entirely —
callers must use Form 3 (typed variant) instead.

**What gets deleted:**
- The `{ "data": payload, "meta": source_location }` wrapping in both macro forms — gone
- The `serde_json::json!({ "meta": __meta, "data": $payload })` pattern in the old macros — gone

**Checkpoint:** `cargo check --workspace` exits 0.

---

## Phase 5 — Update All Readers

Every place that reads a tlog JSONL line and pattern-matches on `kind` or digs into `payload` structure
must be updated to deserialize `CanonEvent` directly.

### 5a — `process_events` in `lib.rs`

Replace the manual `canon.kind == "prompt_loaded"` etc. checks with:

```rust
// Before:
} else if canon.kind == "prompt_loaded" && canon.source != "event-runtime" {
    self.handle_runtime_event(RuntimeEvent::PromptLoaded(PromptLoaded { payload: canon.payload.clone() }))?;

// After — payload is typed, no shape detection:
CanonPayload::PromptLoaded(p) if canon.meta.source != "event-runtime" => {
    self.handle_runtime_event(RuntimeEvent::PromptLoaded(PromptLoaded { payload: serde_json::to_value(&p)? }))?;
}
```

### 5b — `scan_tlog_for_goal` in `observe.rs`

Replace manual JSON parsing with:

```rust
let event: CanonEvent = serde_json::from_str(line)?;
if let CanonPayload::PromptLoaded(p) = event.payload {
    if p.prompt_id.as_deref() == Some("AGENT_GOAL") {
        found = Some(p.content.clone());
    }
}
```

No `payload["data"]["content"]` unwrapping. No `is_goal` flag. Just typed decode.

### 5c — `LoopStageExecutor::on_event(PromptLoaded)` in `executor.rs`

The `PromptLoaded` variant now carries a typed struct — `payload.get("data")` pattern gone entirely.
The `content` field is directly accessible.

### 5d — External reader tools (`canon-tools-analysis`, etc.)

All tools that read tlog JSONL must update to deserialize `CanonEvent` instead of ad-hoc JSON parsing.

**Checkpoint:** `cargo check --workspace` exits 0.

---

## Phase 6 — Update External Tools

`canon-builder`, `canon-tools-editor`, `canon-tools-analysis` write to tlog directly.
Each call to `canon_emit_meta!(source, kind, payload, &path)` already uses the macro —
Phase 4 changes the macro output format, so these sites compile automatically.

Verify each binary still writes valid `CanonEvent` wire format by running their existing tests.

---

## Phase 7 — Compile-Time Guard

**File:** `canon-runtime-events/src/lib.rs` or a `build.rs`

```rust
// Make write_event_auto pub(crate) inside canon-runtime — external callers must use macros
// In canon-event:
#[deprecated(note = "use canon_emit_meta! instead of calling write_event_auto directly")]
pub fn write_event_auto_raw(...) { ... }
// write_event_auto itself is now only called by the macro
```

Optional: add a `build.rs` grep that rejects any direct call to `write_event_auto` outside of
`canon-meta/src/lib.rs` (the one place the macro calls it).

---

## Execution Order

```
Prerequisite: G-migration complete
Phase 1 — define wire types (CanonEvent wire struct, CanonPayload enum)
Phase 2 — rename CanonEvent → RuntimeEvent (mechanical, workspace-wide)
Phase 3 — single serializer in append_runtime_event
Phase 4 — collapse macro forms
Phase 5 — update readers (no shape detection anywhere)
Phase 6 — update external tools
Phase 7 — compile-time guard
```

Phases 1–3 can proceed without breaking anything (additive). Phase 4 is the breaking point —
old macro forms removed, all callers updated. Phases 5–7 follow Phase 4.

---

## Files Modified

| Phase | Status | File | Change |
|-------|--------|------|--------|
| 1 | ✅ | `canon-runtime-events/src/wire.rs` | CanonEvent wire struct + CanonPayload enum |
| 1 | ✅ | `canon-runtime-events/src/lib.rs` | re-export new wire types |
| 2 | ✅ | `canon-runtime-events/src/events.rs` | renamed CanonEvent → RuntimeEvent |
| 2 | ✅ | all consumer/executor files | import updated |
| 3a | 🟡 | `canon-runtime/src/lib.rs` | `runtime_event_to_wire()` exists; `TlogEvent` wrapper not yet removed |
| 3b | 🔴 | `canon-runtime-events/src/tlog/` | `write_event_auto` + `BinarySegmentWriter` accept `&CanonEvent` |
| 3c | 🔴 | `canon-runtime/src/lib.rs` | `append_runtime_event`: remove `TlogEvent::new` wrapper |
| 4 | 🔴 | `canon-meta/src/lib.rs` | collapse three macro forms into one |
| 5a | 🔴 | `canon-runtime/src/lib.rs` | `process_events`: typed decode, no kind-string matching |
| 5b | 🔴 | `canon-loop/src/stage/observe.rs` | `scan_tlog_for_goal`: typed decode |
| 5c | 🔴 | `canon-loop/src/executor.rs` | `on_event(PromptLoaded)`: typed field access |
| 5d | 🔴 | `canon-tools-analysis/src/` | update tlog readers |
| 6 | 🔴 | `canon-builder/`, `canon-tools-editor/` | verify macro output still valid |
| 7 | 🔴 | `canon-runtime-events/src/lib.rs` | visibility guard on write_event_auto |

---

## What This Achieves

```
Before:  3 wire shapes, readers must detect shape, bugs like B possible
After:   1 wire shape, readers deserialize CanonEvent directly, no shape detection anywhere

Maintenance cost:
  Before:  O(forms × readers)  — every new emit site must be read by every reader correctly
  After:   O(1)                — add a variant to CanonPayload; all readers get it for free

Bug class eliminated: "content is at payload.data.content not payload.content" — impossible
                      when payload is typed enum; the compiler enforces the shape
```

---

## What Does NOT Change

- The `RuntimeEvent` bus enum and all its variants — these are internal dispatch types, unchanged
- EventConsumer trait — `on_event(&mut self, event: &RuntimeEvent)` unchanged
- The P1/P2/P3/P4/W/Q_e/Q_c architecture — unchanged
- External tool existence — they keep writing directly; only the wire format output changes
