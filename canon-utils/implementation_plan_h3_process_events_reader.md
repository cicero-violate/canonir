# Implementation Plan: H3 — Update process_events Reader

## Status

```
Phase H3 — 🔴 not started
```

**Depends on:** H2 ✅ (tlog records are now written as `CanonEvent`)

---

## Scope: One Turn, One File

Touch **only** `canon-runtime/src/lib.rs`.

Goal: `process_events` decodes tlog records as `CanonEvent` instead of `TlogEvent` (string-matching
on `canon.kind`). After this, no reader in the runtime core does shape detection.

---

## Context

`process_events` currently receives `&[AnyEvent]` where `AnyEvent` is a `TlogEvent` read from disk.
It dispatches on `canon.kind` (string) and manually constructs `RuntimeEvent` variants.

The key string-matched branches (from `lib.rs`):

```rust
canon.kind == "runtime_state.updated"
canon.kind == "crate_compiled"
canon.kind == "prompt_loaded" && canon.source != "event-runtime"
canon.kind == "capability_completed" && canon.source != "event-runtime"
canon.kind == "capability_failed" && canon.source != "event-runtime"
canon.kind == "error_occurred" && canon.source != "event-runtime"
```

Each branch then manually decodes `canon.payload` with `serde_json::from_value`.

After H2 the tlog records are `CanonEvent`, but `process_events` still reads `AnyEvent` (TlogEvent).
The `AnyEvent` type needs to be updated to decode as `CanonEvent`.

---

## Step 1 — Update `AnyEvent` type

**File:** `canon-runtime/src/lib.rs` (or wherever `AnyEvent` is defined/imported)

Find `type AnyEvent` or the struct used as the element of `&[AnyEvent]` in `process_events`.

Change the deserialization to decode `CanonEvent`:

```rust
// If AnyEvent is an alias:
type AnyEvent = canon_event::CanonEvent;

// If AnyEvent is TlogEvent and process_events reads from a Vec<TlogEvent>,
// update the read_tlog / collect_events call to deserialize as CanonEvent instead.
```

**Note:** tlog files written before H2 used `TlogEvent` format. After H2 they use `CanonEvent`.
`CanonEvent` uses `serde(flatten)` for payload, so old records (with outer `source`/`kind`/`payload`)
will fail to deserialize cleanly. The P2 watcher reads from the live tlog only — for production use,
the tlog is fresh on each runtime start. For any bootstrap events written before W starts, those are
still in `TlogEvent` format from bootstrap/P4. Handle these by trying `CanonEvent` first, then
falling back to `TlogEvent` → `CanonEvent` conversion if needed:

```rust
fn parse_tlog_line(line: &str) -> Option<canon_event::CanonEvent> {
    // Try new format first
    if let Ok(ev) = serde_json::from_str::<canon_event::CanonEvent>(line) {
        return Some(ev);
    }
    // Fall back: parse as TlogEvent, convert to CanonEvent
    if let Ok(tlog) = serde_json::from_str::<TlogEvent>(line) {
        return tlog_event_to_canon(tlog);
    }
    None
}

fn tlog_event_to_canon(tlog: TlogEvent) -> Option<canon_event::CanonEvent> {
    use canon_event::{CanonEvent, CanonPayload, EventMeta};
    // Map TlogEvent { source, kind, payload } → CanonEvent { meta, payload: CanonPayload }
    let meta = EventMeta {
        ts: tlog.ts,
        source: tlog.source.clone(),
        file: String::new(),
        line: 0,
    };
    let payload = match tlog.kind.as_str() {
        "prompt_loaded" => CanonPayload::PromptLoaded(tlog.payload),
        "capability_completed" => CanonPayload::CapabilityCompleted(tlog.payload),
        "capability_failed" => CanonPayload::CapabilityFailed(tlog.payload),
        "error_occurred" => CanonPayload::ErrorOccurred(tlog.payload),
        "runtime_state.updated" => CanonPayload::RuntimeStateUpdated(tlog.payload),
        _ => CanonPayload::Debug(tlog.payload),
    };
    Some(CanonEvent { event_id: tlog.event_id, meta, payload })
}
```

---

## Step 2 — Replace `canon.kind ==` branches with typed match

Replace the if-else chain in `process_events`:

```rust
// Before:
} else if canon.kind == "prompt_loaded" && canon.source != "event-runtime" {
    self.handle_runtime_event(RuntimeEvent::PromptLoaded(PromptLoaded { payload: canon.payload.clone() }))?;
    self.drain_emitted_events()?;
} else if canon.kind == "capability_completed" && canon.source != "event-runtime" {
    if let Ok(payload_owned) = serde_json::from_value::<CapabilityCompletedOwned>(canon.payload.clone()) {
        // ...
    }
}
// etc.

// After:
match &canon.payload {
    CanonPayload::RuntimeStateUpdated(val) => {
        // handle RuntimeStateUpdated (was "runtime_state.updated")
    }
    CanonPayload::PromptLoaded(val) if canon.meta.source != "event-runtime" => {
        let data = val.get("data").unwrap_or(val);
        self.handle_runtime_event(RuntimeEvent::PromptLoaded(PromptLoaded { payload: data.clone() }))?;
        self.drain_emitted_events()?;
    }
    CanonPayload::CapabilityCompleted(val) if canon.meta.source != "event-runtime" => {
        if let Ok(payload_owned) = serde_json::from_value::<CapabilityCompletedOwned>(val.clone()) {
            let payload = CapabilityCompleted { ... };
            self.handle_runtime_event(RuntimeEvent::CapabilityCompleted(payload))?;
            self.drain_emitted_events()?;
        }
    }
    CanonPayload::CapabilityFailed(val) if canon.meta.source != "event-runtime" => { ... }
    CanonPayload::ErrorOccurred(val) if canon.meta.source != "event-runtime" => { ... }
    _ => { /* ignore */ }
}
```

**Note on "crate_compiled":** This kind does not have a `CanonPayload` variant. Either:
- Add `CanonPayload::CrateCompiled(serde_json::Value)` to `wire.rs` in H-Phase 1 addendum, OR
- Handle it via the `tlog_event_to_canon` fallback as `CanonPayload::Debug`

Keep whichever approach is simpler.

---

## Checkpoint

```bash
cargo check --workspace
cargo test -p canon-runtime
```

Both must exit 0. `async_consumers_preserve_order_per_consumer` must pass.

---

## What This Does NOT Do

- Does NOT remove `TlogEvent` struct (still used by `tlog_event_to_canon` fallback and external tools)
- Does NOT touch macros (that is H4)
- Does NOT touch external tools (that is H5)
- Does NOT touch `scan_tlog_for_goal` — that already has the G-migration data-unwrap fix and will
  continue to work since the fallback `tlog_event_to_canon` normalizes old records. A follow-up can
  convert it to typed decode after H4.
