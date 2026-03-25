# Schema Upgrade: Canonical Event Form

## Context

Current wire format (`wire.rs`):
```json
{ "event_id": 1, "meta": { "ts": ..., "source": "...", "file": "...", "line": 0 }, "kind": "Llm", "data": { ... } }
```

Target canonical form:
```json
{ "id": "1", "parent_ids": [], "actor": "event-runtime", "kind": "Llm", "ts": 1774356232884, "payload": { "input": {}, "output": {}, "delta": {}, "meta": { "file": "...", "line": 0 }, "data": { ... } } }
```

Field mapping:
| Canonical           | Current source                     |
|---------------------+------------------------------------|
| `id`                | `event_id` (→ String)              |
| `parent_ids`        | ❌ NEW (empty default)             |
| `actor`             | `meta.source`                      |
| `kind`              | `kind` (from CanonPayload tag)     |
| `ts`                | `meta.ts`                          |
| `payload.data`      | `data` (from CanonPayload content) |
| `payload.input`     | ❌ NEW (None default)              |
| `payload.output`    | ❌ NEW (None default)              |
| `payload.delta`     | ❌ NEW (None default)              |
| `payload.meta.file` | `meta.file`                        |
| `payload.meta.line` | `meta.line`                        |

---

## Crate Topology

```
canon-utils/
  canon-macros/          ← REPLACED entirely (declarative macros → empty or removed)
  canon-proc-macros/     ← NEW home for all event macros (proc-macro = true already set)
  canon-runtime-events/
    src/
      wire.rs            ← CanonEvent struct lives here → REPLACED
      events.rs          ← RuntimeEvent enum + all event structs → UNCHANGED structure, macro calls updated
      emit.rs            ← write path → UPDATED to construct new CanonEvent
      lib.rs             ← re-exports → UPDATE macro imports
      macros/event.rs    ← re-exports from canon-macros → point to canon-proc-macros
      macros/emit.rs     ← re-exports canon_emit → point to canon-proc-macros
```

---

## Step 1 — Rewrite `wire.rs`: new `CanonEvent` struct

**File**: `canon-utils/canon-runtime-events/src/wire.rs`

Replace the entire file. Define:

```rust
pub struct CanonPayloadMeta {
    pub file: String,
    pub line: u32,
}

pub struct CanonPayload {
    pub input:  Option<serde_json::Value>,
    pub output: Option<serde_json::Value>,
    pub delta:  Option<serde_json::Value>,
    pub meta:   Option<CanonPayloadMeta>,
    pub data:   Option<serde_json::Value>,
}

pub struct CanonEvent {
    pub id:         String,
    pub parent_ids: Vec<String>,
    pub actor:      String,
    pub kind:       String,
    pub ts:         u64,
    pub payload:    CanonPayload,
}
```

Derives: `Debug, Clone, Serialize, Deserialize` on all three structs.

Remove old types: `EventMeta`, old `CanonPayload` enum, old `CanonEvent`.

Keep `CanonPayload::from_kind` as a static constructor that wraps raw data:
```rust
impl CanonPayload {
    pub fn from_data(kind: &str, data: serde_json::Value, file: String, line: u32) -> Self {
        CanonPayload {
            input: None,
            output: None,
            delta: None,
            meta: Some(CanonPayloadMeta { file, line }),
            data: Some(data),
        }
    }
}
```

---

## Step 2 — Update `emit.rs`: write path

**File**: `canon-utils/canon-runtime-events/src/emit.rs`

Update `emit_event(source, kind, payload, tlog_path)` to construct the new `CanonEvent`:

```rust
CanonEvent {
    id:         uuid_or_seq(),   // use a monotonic counter or uuid v4
    parent_ids: vec![],
    actor:      source.to_string(),
    kind:       kind.to_string(),
    ts:         now_millis(),
    payload:    CanonPayload::from_data(kind, payload, file!().to_string(), line!()),
}
```

Remove all references to `EventMeta`, old `CanonPayload` enum, old `CanonPayload::from_kind`.

For `id` generation: use `uuid::Uuid::new_v4().to_string()` (uuid already used elsewhere in the crate for `error_id`). Add `uuid` to `Cargo.toml` if not already present (check: `canon-runtime-events/Cargo.toml`).

---

## Step 3 — Rewrite `canon-macros/src/lib.rs` → empty / tombstone

**File**: `canon-utils/canon-macros/src/lib.rs`

Delete all three macros (`canon_event_struct!`, `canon_event_enum!`, `canon_emit!`).
Replace with a compile-time deprecation notice pointing to `canon-proc-macros`:

```rust
// This crate is superseded by canon-proc-macros.
// All macros (canon_event_struct!, canon_event_enum!, canon_emit!) now live there.
```

Codex will handle migrating call sites after the proc macro implementations are live.

---

## Step 4 — Implement proc macros in `canon-proc-macros/src/lib.rs`

**File**: `canon-utils/canon-proc-macros/src/lib.rs`

Keep the existing `#[must_emit]` proc macro attribute. Add three new macros:

### 4a. `canon_event_struct!` as a `proc_macro` (function-like)

Input form (same call syntax as the old declarative macro):
```rust
canon_event_struct!(MyStruct { field1: Type1, #[serde(default)] field2: Type2 });
```

Output (same as old macro, but generated via proc macro):
```rust
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct MyStruct {
    pub field1: Type1,
    #[serde(default)]
    pub field2: Type2,
}
```

Implementation notes:
- Parse with `syn::parse_macro_input!` using a custom `Parse` impl for the brace-delimited field list with optional meta attributes.
- Use `quote!` to emit the struct with `#[derive(...)]`.
- This is a `#[proc_macro]` (not attribute), matching the existing `macro_rules!` call syntax.

### 4b. `canon_event_enum!` as a `proc_macro`

Input form:
```rust
canon_event_enum!(#[derive(serde::Serialize, serde::Deserialize)] MyEnum { Foo(Foo), Bar(Bar) });
// or without extra derives:
canon_event_enum!(MyEnum { Foo(Foo), Bar(Bar) });
```

Output (same as old macro):
```rust
#[derive(Debug, Clone)]
#[derive(serde::Serialize, serde::Deserialize)]  // optional
pub enum MyEnum { Foo(Foo), Bar(Bar) }

impl MyEnum {
    pub fn sample_all() -> Vec<Self> where Foo: Default, Bar: Default {
        vec![Self::Foo(Foo::default()), Self::Bar(Bar::default())]
    }
}
```

Implementation notes:
- Parse optional leading `#[...]` attribute blocks, then ident, then brace-delimited `Variant(Type)` list.
- The `sample_all` method is identical to the current declarative version.

### 4c. `canon_emit!` as a `proc_macro`

The new `canon_emit!` must produce events in the **new canonical schema** (Step 1).

Three invocation forms, semantics preserved but output schema updated:

**Form 1 — Typed variant (emitter-routed):**
```rust
canon_emit!(emitter; LoopPlanned(payload))
```
→ Calls `emitter.emit_located(RuntimeEvent::LoopPlanned(payload), file!(), line!())`.
No schema change for the `RuntimeEvent` internal bus — only the tlog wire format changes.

**Form 2 — String-keyed (emitter-routed):**
```rust
canon_emit!(emitter; "source", "kind", payload)
```
→ Constructs `DebugEvent { source, kind, payload }` and routes via emitter.
No schema change for the bus.

**Form 3 — Direct tlog write:**
```rust
canon_emit!("source", "kind", payload, &tlog_path)
```
→ Constructs new `CanonEvent`:
```rust
CanonEvent {
    id:         uuid::Uuid::new_v4().to_string(),
    parent_ids: vec![],
    actor:      source.to_string(),
    kind:       kind.to_string(),
    ts:         now_millis(),
    payload: CanonPayload {
        input:  None,
        output: None,
        delta:  None,
        meta:   Some(CanonPayloadMeta { file: file!().to_string(), line: line!() }),
        data:   Some(serde_json::to_value(payload).unwrap_or_default()),
    },
}
// then calls: canon_runtime_events::write_canon_event_auto(path, &event)
```

Implementation notes:
- `canon_emit!` is a `#[proc_macro]`.
- Parse by looking for `;` to distinguish emitter forms from direct form, then `,` count.
- Inject `file!()` and `line!()` as macro hygiene literals via `quote!(::std::file!())` and `quote!(::std::line!())`.
- The macro must import/qualify all types by path — cannot assume any `use` statements at call site.

### 4d. `Cargo.toml` additions for `canon-proc-macros`

Add `uuid` dependency (feature `v4`):
```toml
uuid = { workspace = true, features = ["v4"] }
```

Or have the macro emit a call to `canon_runtime_events::new_event_id()` (a free function that does the uuid call inside the events crate), which avoids the uuid dep in the proc macro itself. **Preferred**: emit a call to a helper function to keep the proc macro crate lean.

---

## Step 5 — Update `canon-runtime-events` re-exports

**File**: `canon-utils/canon-runtime-events/src/macros/event.rs`

Replace:
```rust
pub use canon_macros::canon_emit;
pub use canon_macros::canon_event_enum;
pub use canon_macros::canon_event_struct;
```
With:
```rust
pub use canon_proc_macros::canon_emit;
pub use canon_proc_macros::canon_event_enum;
pub use canon_proc_macros::canon_event_struct;
```

**File**: `canon-utils/canon-runtime-events/src/macros/emit.rs`

Replace:
```rust
pub use canon_macros::canon_emit;
```
With:
```rust
pub use canon_proc_macros::canon_emit;
```

**File**: `canon-utils/canon-runtime-events/Cargo.toml`

Replace `canon-macros` dep with `canon-proc-macros`:
```toml
canon-proc-macros = { path = "../canon-proc-macros" }
```

Remove:
```toml
canon-macros = ...
```

**File**: `canon-utils/canon-runtime-events/src/lib.rs`

The `pub use wire::{CanonEvent, CanonPayload, EventMeta}` line becomes:
```rust
pub use wire::{CanonEvent, CanonPayload, CanonPayloadMeta};
```
(`EventMeta` is gone; `CanonPayloadMeta` is the new nested struct.)

---

## Step 6 — Add `new_event_id()` helper to `canon-runtime-events`

**File**: `canon-utils/canon-runtime-events/src/emit.rs` (or `src/id.rs`)

```rust
pub fn new_event_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn now_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}
```

The `canon_emit!` proc macro (Form 3) emits calls to these helpers by path:
`canon_runtime_events::new_event_id()` and `canon_runtime_events::now_millis()`.

---

## Step 7 — Update `canon-storage-eventlog` reader

**File**: `canon-utils/canon-storage-eventlog/src/` (reader, replay modules)

The reader currently parses `CanonEvent` from JSON. After schema change the wire bytes are different. Two sub-tasks:

### 7a. Legacy read support (optional but recommended)

Add a `LegacyCanonEvent` type that matches the OLD wire format:
```rust
pub struct LegacyCanonEvent {
    pub event_id: Option<u64>,
    pub meta: LegacyEventMeta,
    pub kind: String,
    pub data: Option<serde_json::Value>,
}
pub struct LegacyEventMeta { pub ts: u64, pub source: String, pub file: String, pub line: u32 }
```

In `parse_any_event`, attempt new schema first; fall back to legacy by detecting the presence of the `meta` key.

### 7b. `extract_rustc_event` update

`extract_rustc_event(&canon: &CanonEvent)` currently digs into `CanonPayload::RustcEvent(v)`.

After schema change: `canon.kind == "rustc_event"` and `canon.payload.data` holds the rustc event JSON. Update to deserialize from `canon.payload.data`.

---

## Step 8 — `events.rs` call-site compatibility check

All `canon_event_struct!(...)` and `canon_event_enum!(...)` call sites in `events.rs` use the same syntax as the old declarative macros. The proc macros in Step 4 use **identical call syntax**, so no changes are needed in `events.rs`.

The `RuntimeEvent` enum and `EventEmitter` trait are **not affected** by the wire schema change — they are internal bus types. Only the tlog serialization path (`write_canon_event_auto`) uses the wire types.

---

## Step 9 — `canon-macros` Cargo cleanup

After Step 3, `canon-macros` still exists as an empty crate. Check workspace `Cargo.toml` to see if any other crates directly depend on `canon-macros` besides `canon-runtime-events`. If so, add `canon-proc-macros` dep and update their `use` statements.

Known dependents (grep for `canon-macros` in workspace):
- `canon-utils/canon-runtime-events/Cargo.toml` ← handled in Step 5
- Any others found by grep must be updated similarly.

---

## Execution Order

1. Step 1 — new `CanonEvent` wire types in `wire.rs`
2. Step 6 — `new_event_id()` / `now_millis()` helpers
3. Step 2 — update `emit.rs` write path
4. Step 4 — implement proc macros in `canon-proc-macros/src/lib.rs`
5. Step 5 — update re-exports and `Cargo.toml`
6. Step 3 — tombstone `canon-macros/src/lib.rs`
7. Step 7 — update eventlog reader / `extract_rustc_event`
8. Step 9 — sweep remaining `canon-macros` dependents

Steps 1–3 can be done atomically (they are all within `canon-runtime-events`).
Step 4 is independent and can be done in parallel with Steps 1–3.
Steps 5 and 6 connect 1–3 and 4 together.
Steps 7 and 9 are downstream cleanup.

---

## Invariants to preserve

- `RuntimeEvent` enum and `EventEmitter` trait: **unchanged** — they are the internal bus, not wire
- `canon_event_struct!` / `canon_event_enum!` call syntax: **unchanged** — proc macros match old declarative syntax exactly
- `canon_emit!(emitter; ...)` forms: **unchanged** — still route through `EventEmitter`
- `canon_emit!("source", "kind", payload, &path)` direct form: output changes to new schema
- All event structs in `events.rs`: **unchanged** — macro syntax identical
- `#[must_emit]` proc macro: **unchanged** — keep as-is in `canon-proc-macros`
- Tlog binary segment format: the `BinarySegmentWriter` wraps JSON — wire JSON changes, binary framing unchanged
