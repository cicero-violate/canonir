# Implementation Plan: H2 — Switch append_runtime_event to CanonEvent

## Status

```
Phase H2 — 🔴 not started
```

**Depends on:** H1 ✅ (`write_canon_event_auto` + `BinarySegmentWriter::write_canon_event` exist)

---

## Scope: One Turn, One File

Touch **only** `canon-runtime/src/lib.rs`.

Goal: `append_runtime_event` stops wrapping `CanonEvent` inside `TlogEvent` and writes the
`CanonEvent` wire struct directly. After this change, new tlog records have the canonical shape.

---

## Context

After H1, two write paths exist in `canon-runtime-events`:
- `write_event_auto(path, &TlogEvent)` — old path (still used by external tools)
- `write_canon_event_auto(path, &CanonEvent)` — new path (added in H1)

`append_runtime_event` currently (lib.rs:332):

```rust
let Some(wire) = runtime_event_to_wire(event) else { return; };
let kind = payload_kind(&wire);
let mut canon = TlogEvent::new(wire.meta.source.clone(), kind,
    serde_json::to_value(&wire).unwrap_or_else(|_| serde_json::json!({})));
canon.event_id = Some(self.next_id);
self.next_id = self.next_id.saturating_add(1);

if is_segment_dir_path(&path) {
    // ... BinarySegmentWriter::write_event(&canon)
}
let _ = canon_event::write_event_auto(&path, &canon);
```

---

## Change

Replace the body of `append_runtime_event` from the `TlogEvent::new` call onward:

```rust
// Before:
let kind = payload_kind(&wire);
let mut canon = TlogEvent::new(wire.meta.source.clone(), kind,
    serde_json::to_value(&wire).unwrap_or_else(|_| serde_json::json!({})));
canon.event_id = Some(self.next_id);
self.next_id = self.next_id.saturating_add(1);

if is_segment_dir_path(&path) {
    if let Some(writer_arc) = self.tlog_writer.as_ref() {
        let needs_reopen = if let Ok(w) = writer_arc.lock() {
            if w.write_event(&canon).is_err() { true } else { false }
        } else { false };
        if needs_reopen {
            if let Ok(fresh) = BinarySegmentWriter::open(&path) {
                if let Ok(mut w) = writer_arc.lock() {
                    *w = fresh;
                    let _ = w.write_event(&canon);
                }
            }
        }
    }
    return;
}
let _ = canon_event::write_event_auto(&path, &canon);

// After:
let mut wire = wire;    // already have wire from runtime_event_to_wire above
wire.event_id = Some(self.next_id);
self.next_id = self.next_id.saturating_add(1);

if is_segment_dir_path(&path) {
    if let Some(writer_arc) = self.tlog_writer.as_ref() {
        let needs_reopen = if let Ok(w) = writer_arc.lock() {
            if w.write_canon_event(&wire).is_err() { true } else { false }
        } else { false };
        if needs_reopen {
            if let Ok(fresh) = BinarySegmentWriter::open(&path) {
                if let Ok(mut w) = writer_arc.lock() {
                    *w = fresh;
                    let _ = w.write_canon_event(&wire);
                }
            }
        }
    }
    return;
}
let _ = canon_event::write_canon_event_auto(&path, &wire);
```

Also delete the now-unused helper functions:
- `fn payload_kind(wire: &canon_event::CanonEvent) -> &str { ... }` — no longer called

Remove the `TlogEvent` import from `lib.rs` if it is no longer used anywhere in the file after
this change. (Check: if `process_events` still uses `TlogEvent` for reading — it does, leave the
import. Only remove if fully unused.)

---

## Checkpoint

```bash
cargo check -p canon-runtime
```

Must exit 0. Tlog records written by W after this change will have the canonical shape:
```json
{ "event_id": 1,
  "meta": { "ts": 1774106835, "source": "observe", "file": "", "line": 0 },
  "kind": "LoopObserved",
  "data": { "tick": 1, "error_count": 0, "goal_text": "..." } }
```

---

## What This Does NOT Do

- Does NOT remove `write_event_auto` or `TlogEvent` (still used by external tools and process_events)
- Does NOT change `process_events` reader (that is H3)
- Does NOT touch macros (that is H4)
- Does NOT touch any other crate
