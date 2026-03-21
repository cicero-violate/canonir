# Implementation Plan: H6 — Remove TlogEvent + Final Reader Upgrade

## Status

```
Phase H6 — 🔴 not started
```

**Depends on:** H5 ✅ (zero deprecated warnings; all callers use CanonEvent write path)

---

## Scope: One Turn, Focused Deletions

Goal: delete `TlogEvent` from the write path entirely. Upgrade the two remaining string-based
readers (`scan_tlog_for_goal`, `process_events` fallback) to typed `CanonEvent` decode.
After this the system has one schema, one write path, one read path — no exceptions.

---

## Step 1 — Delete `write_event_auto` and `BinarySegmentWriter::write_event`

**File:** `canon-runtime-events/src/tlog/mod.rs`

Delete:
- `pub fn write_event_auto(path: &Path, event: &TlogEvent) -> std::io::Result<()>`

**File:** `BinarySegmentWriter` impl

Delete:
- `pub fn write_event(&self, event: &TlogEvent) -> std::io::Result<()>`

If `TlogEvent` is now unreferenced in write paths, check if it is still used as a read type.

---

## Step 2 — Remove `tlog_event_to_canon` fallback from `process_events`

**File:** `canon-runtime/src/lib.rs`

Delete the `parse_tlog_line` fallback path added in H3. After H4, all tlog records are `CanonEvent`.
The fallback was for in-flight transition only.

```rust
// Before (H3 added):
fn parse_tlog_line(line: &str) -> Option<canon_event::CanonEvent> {
    if let Ok(ev) = serde_json::from_str::<canon_event::CanonEvent>(line) { return Some(ev); }
    if let Ok(tlog) = serde_json::from_str::<TlogEvent>(line) { return tlog_event_to_canon(tlog); }
    None
}

// After: direct decode only
fn parse_tlog_line(line: &str) -> Option<canon_event::CanonEvent> {
    serde_json::from_str::<canon_event::CanonEvent>(line).ok()
}
```

Delete `tlog_event_to_canon`.

---

## Step 3 — Upgrade `scan_tlog_for_goal` to typed decode

**File:** `canon-loop/src/stage/observe.rs`

Replace the JSON string matching (even with the G-migration data-unwrap fix) with typed decode:

```rust
fn scan_tlog_for_goal(tlog_path: &Path) -> Option<String> {
    // ... (existing dir resolution + sort unchanged)
    let mut found: Option<String> = None;
    for log_path in &logs {
        let content = std::fs::read_to_string(log_path).ok()?;
        for line in content.lines() {
            let Ok(ev) = serde_json::from_str::<canon_event::CanonEvent>(line) else { continue; };
            if let canon_event::CanonPayload::PromptLoaded(val) = &ev.payload {
                let data = val.get("data").unwrap_or(val);
                let is_goal = data.get("prompt_id").and_then(|v| v.as_str()) == Some("AGENT_GOAL")
                    || data.get("path").and_then(|v| v.as_str()).map(|p| p.contains("AGENT_GOAL")).unwrap_or(false);
                if is_goal {
                    if let Some(c) = data.get("content").and_then(|v| v.as_str()) {
                        found = Some(c.to_string());
                    }
                }
            }
        }
    }
    found
}
```

Note: the `data` unwrap is still needed for bootstrap-written records that went through the H4
macro update. After H4 the Form 1 macro builds `CanonPayload::PromptLoaded(payload)` where `payload`
is the caller's payload object (flat). The `data` layer only appears if the pre-H4 bootstrap format
is still present. Once H4 is confirmed clean, the `data` unwrap can be removed in a follow-up.

---

## Step 4 — Remove `TlogEvent` if fully unused

Run:
```bash
grep -r "TlogEvent" /workspace/ai_sandbox/canon/canon-utils --include="*.rs"
```

If zero references remain in write paths and `tlog_event_to_canon` is deleted, remove the
`TlogEvent` struct definition and its `TlogEvent::new` constructor from `canon-runtime-events`.

If any reference remains (e.g., in a test or external tool not yet updated), leave it and note it.

---

## Checkpoint

```bash
cargo check --workspace
cargo test -p canon-runtime
```

Both exit 0. `async_consumers_preserve_order_per_consumer` passes.

```bash
grep -r "TlogEvent" /workspace/ai_sandbox/canon/canon-utils --include="*.rs" | grep -v "test\|//\|deprecated"
```

Should produce zero lines (or note any remaining references for a follow-up).

---

## After H6 Complete

```
One schema:   CanonEvent { event_id, meta: EventMeta, payload: CanonPayload }   ✅
One write:    write_canon_event_auto / BinarySegmentWriter::write_canon_event    ✅
One read:     serde_json::from_str::<CanonEvent>(line)                           ✅
Zero shape detection in any reader                                               ✅
```
