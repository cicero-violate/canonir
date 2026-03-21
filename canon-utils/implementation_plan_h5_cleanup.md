# Implementation Plan: H5 — Cleanup: Remove TlogEvent Write Path + Guard

## Status

```
Phase H5 — 🔴 not started
```

**Depends on:** H4 ✅ (all emit sites produce CanonEvent; no caller uses TlogEvent write path)

---

## Scope: One Turn, Two Files

Touch `canon-runtime-events/src/tlog/` and `canon-runtime-events/src/lib.rs`.

Goal: remove the dead `TlogEvent` write path now that all callers use `CanonEvent`. Add the
compile-time guard so no new code can reintroduce direct `write_event_auto(TlogEvent)` calls.

---

## Step 1 — Deprecate `write_event_auto`

**File:** `canon-runtime-events/src/tlog/mod.rs` (wherever `write_event_auto` is)

```rust
#[deprecated(note = "use write_canon_event_auto instead — all tlog records must be CanonEvent")]
pub fn write_event_auto(path: &Path, event: &TlogEvent) -> std::io::Result<()> {
    // keep body for now — removal in H6
}
```

```rust
#[deprecated(note = "use write_canon_event instead — all tlog records must be CanonEvent")]
impl BinarySegmentWriter {
    pub fn write_event(&self, event: &TlogEvent) -> std::io::Result<()> { ... }
}
```

This will produce compiler warnings (not errors) at all remaining call sites. Use `cargo check`
output to find any callers that still use the old path.

---

## Step 2 — Fix any remaining callers flagged by Step 1

For each `deprecated` warning, update the call site to use `write_canon_event_auto` or
`BinarySegmentWriter::write_canon_event`.

External tools (`canon-builder`, `canon-tools-editor`, `canon-tools-analysis`) should now be
going through the updated `canon_emit_meta!` macro from H4. If any external tool calls
`write_event_auto` directly (not via macro), update those call sites now.

---

## Step 3 — `TlogEvent` read path: keep for now

`TlogEvent` is still used in `process_events` via the `tlog_event_to_canon` fallback added in H3.
Do not remove `TlogEvent` in this turn. That is H6.

---

## Checkpoint

```bash
cargo check --workspace 2>&1 | grep -c "deprecated"
```

Should be 0 — all deprecated callers fixed.

```bash
cargo check --workspace
```

Must exit 0 with zero errors and zero `deprecated` warnings.

---

## What This Does NOT Do

- Does NOT remove `TlogEvent` struct (H6)
- Does NOT remove the `tlog_event_to_canon` fallback in `process_events` (H6)
- Does NOT touch `scan_tlog_for_goal` typed decode upgrade (H6 or separate)
