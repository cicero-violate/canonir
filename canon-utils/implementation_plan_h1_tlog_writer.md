# Implementation Plan: H1 — Tlog Writer Accepts CanonEvent

## Status

```
Phase H1 — 🔴 not started
```

**Depends on:** H-Phase 2 ✅ (RuntimeEvent rename complete)

---

## Scope: One Turn, One Crate

Touch **only** `canon-runtime-events`. Do not touch any other crate.

Goal: the tlog write functions accept `&CanonEvent` instead of `&TlogEvent`.
`TlogEvent` still exists (readers use it — that is H3's job). This is additive only.

---

## Context

`append_runtime_event` in `canon-runtime/src/lib.rs` currently does:

```rust
let mut canon = TlogEvent::new(wire.meta.source.clone(), kind,
    serde_json::to_value(&wire).unwrap_or_else(|_| serde_json::json!({})));
canon.event_id = Some(self.next_id);
// ... write canon (TlogEvent) to tlog
```

The `CanonEvent` wire struct is nested inside `TlogEvent.payload`. The tlog record on disk is:
```json
{ "ts": ..., "source": "...", "kind": "loop_observed",
  "payload": { "event_id": null, "meta": {...}, "kind": "LoopObserved", "data": {...} } }
```

Target after H1+H2: the tlog record IS the `CanonEvent` struct:
```json
{ "event_id": 1, "meta": { "ts": ..., "source": "...", "file": "", "line": 0 },
  "kind": "LoopObserved", "data": { "tick": 1, ... } }
```

H1 adds the write functions. H2 (next turn) switches `append_runtime_event` to use them.

---

## Step 1 — Add `write_canon_event` to `write_event_auto` module

**File:** `canon-runtime-events/src/tlog/mod.rs` (or wherever `write_event_auto` is defined)

Find `pub fn write_event_auto(path: &Path, event: &TlogEvent) -> std::io::Result<()>`.

Add alongside it (do not delete the old function — H3 removes it):

```rust
pub fn write_canon_event_auto(path: &Path, event: &CanonEvent) -> std::io::Result<()> {
    let line = serde_json::to_string(event)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    let full_path = if path.is_dir() {
        // same segment-file resolution logic as write_event_auto
        // copy the existing path resolution from write_event_auto
    } else {
        path.to_path_buf()
    };
    let mut file = OpenOptions::new().create(true).append(true).open(&full_path)?;
    writeln!(file, "{}", line)
}
```

Copy the path resolution and append logic from the existing `write_event_auto` — do not
duplicate the logic, refactor both to call a shared `append_json_line(path, &str)` helper
if that simplifies things. The key requirement: `write_canon_event_auto` serializes
a `CanonEvent` directly (not a `TlogEvent`).

---

## Step 2 — Add `write_canon_event` to `BinarySegmentWriter`

**File:** `canon-runtime-events/src/tlog/segment.rs` (or wherever `BinarySegmentWriter` is defined)

Find `pub fn write_event(&self, event: &TlogEvent) -> std::io::Result<()>`.

Add alongside it:

```rust
pub fn write_canon_event(&self, event: &CanonEvent) -> std::io::Result<()> {
    let line = serde_json::to_string(event)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    // same append logic as write_event — write the JSON line
}
```

---

## Step 3 — Export new functions

**File:** `canon-runtime-events/src/lib.rs`

Ensure `write_canon_event_auto` and that `CanonEvent` is pub-exported (already done in Phase 1).
The `BinarySegmentWriter::write_canon_event` method is accessible via the type.

---

## Checkpoint

```bash
cargo check -p canon-runtime-events
```

Must exit 0. No other crates changed.

`write_event_auto` and `BinarySegmentWriter::write_event` still exist unchanged.
New functions `write_canon_event_auto` and `BinarySegmentWriter::write_canon_event` added.

---

## What This Does NOT Do

- Does NOT change `append_runtime_event` in `canon-runtime` (that is H2)
- Does NOT remove `TlogEvent` (that is H3+)
- Does NOT change any reader (that is H3)
- Does NOT touch macros (that is H4)
