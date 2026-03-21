# Implementation Plan: H4 — Collapse Macro Forms

## Status

```
Phase H4 — 🔴 not started
```

**Depends on:** H3 ✅ (process_events reads CanonEvent; no shape detection in runtime core)

---

## Scope: One Turn, One File

Touch **only** `canon-meta/src/lib.rs`.

Goal: the direct Form 1 macro (`canon_emit_meta!(source, kind, payload, &path)`) produces a
`CanonEvent` record instead of a `TlogEvent` + `{ "data": payload, "meta": source_location }` wrapper.
After this, all tlog records — from both W (already done in H2) and external tools — have the same shape.

---

## Context

The three current forms in `canon-meta/src/lib.rs`:

```rust
// Form 3 — typed variant (no wrapping) — already correct
($emitter:expr; $variant:ident($inner:expr)) => {{
    canon_event::canon_emit!($emitter; $variant($inner))
}};

// Form 2 — emitter debug (wraps in data/meta) — routes through bus
($emitter:expr; $source:expr, $kind:expr, $payload:expr) => {{
    let __meta = canon_meta::capture_meta!();
    let __wrapped = serde_json::json!({ "meta": __meta, "data": $payload });
    canon_event::canon_emit!($emitter; $source, $kind, __wrapped)
}};

// Form 1 — direct write (wraps in data/meta) — writes to tlog directly
($source:expr, $kind:expr, $payload:expr, $path:expr) => {{
    let __meta = canon_meta::capture_meta!();
    let __wrapped = serde_json::json!({ "meta": __meta, "data": $payload });
    canon_event::canon_emit!($source, $kind, __wrapped, $path)
}};
```

Form 1 (`canon_emit!($source, $kind, __wrapped, $path)`) expands to:
```rust
let __event = canon_event::TlogEvent::new($source, $kind, __wrapped);
canon_event::write_event_auto($path, &__event)
```

After H1, `write_canon_event_auto` exists. This form should build a `CanonEvent` directly.

---

## Change: Update Form 1

Replace Form 1 to build and write a `CanonEvent`:

```rust
// Form 1 — direct write — NOW produces CanonEvent (no TlogEvent wrapper, no data/meta nesting)
($source:expr, $kind:expr, $payload:expr, $path:expr) => {{
    let __meta = canon_event::EventMeta {
        ts: {
            use std::time::{SystemTime, UNIX_EPOCH};
            SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
        },
        source: $source.to_string(),
        file: file!().to_string(),
        line: line!(),
    };
    let __payload = canon_event::CanonPayload::from_kind_data_str(
        stringify!($kind), serde_json::to_value($payload).unwrap_or_default()
    );
    let __wire = canon_event::CanonEvent { event_id: None, meta: __meta, payload: __payload };
    canon_event::write_canon_event_auto($path, &__wire)
}};
```

**Note:** `CanonPayload::from_kind_data_str` is a helper that does not exist yet.
The simplest implementation: since bootstrap uses specific kinds (`prompt_loaded`, `agent_registered`,
`system_config_loaded`), add a method to `CanonPayload` in `wire.rs`:

```rust
impl CanonPayload {
    pub fn from_kind(kind: &str, data: serde_json::Value) -> Self {
        match kind {
            "prompt_loaded" => CanonPayload::PromptLoaded(data),
            "agent_registered" => CanonPayload::AgentRegistered(data),
            "system_config_loaded" => CanonPayload::Debug(data), // map to Debug if no variant
            _ => CanonPayload::Debug(data),
        }
    }
}
```

Then the macro becomes:
```rust
let __payload = canon_event::CanonPayload::from_kind($kind, serde_json::to_value($payload).unwrap_or_default());
```

Add `AgentRegistered(serde_json::Value)` to `CanonPayload` in `wire.rs` if it is used by bootstrap
and not yet present. Check `canon-runtime/src/bootstrap.rs` for all `canon_emit_meta!(...)` call kinds.

---

## Form 2 — leave unchanged for now

Form 2 (emitter debug) goes through the bus, is decoded as `RuntimeEvent::Debug`, and written to tlog
by `append_runtime_event` (already canonical after H2). Its `data`/`meta` wrapping ends up as the
`Debug` payload value — that is acceptable until F-migration promotes those events to typed variants.
Do not change Form 2 in this turn.

---

## Checkpoint

```bash
cargo check --workspace
```

Must exit 0. Bootstrap `canon_emit_meta!` calls now write `CanonEvent` records. Tlog records from
all sources are uniform.

---

## What This Does NOT Do

- Does NOT change Form 2 or Form 3
- Does NOT remove `write_event_auto` or `TlogEvent` (still referenced by external tools)
- Does NOT touch external tools directly (they use the macro — updating the macro updates them)
- Does NOT touch readers (H3 already handles both old and new format via fallback)
