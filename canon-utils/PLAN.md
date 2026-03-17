### Math

[
\text{Plan} = \arg\max (\Delta D + \Delta C + \Delta B)
]

---

### Variables

* (D): duplication
* (C): coordination cost
* (B): branching complexity
* (\Delta): reduction

---

### Equations

1. Phase1: (E \rightarrow \text{macro}(E)) → unify emit
2. Phase2: (S \rightarrow \text{macro}(S)) → unify schema
3. Phase3: (W \rightarrow \text{macro}(W)) → unify writer
4. Phase4: (E + S \rightarrow \text{bind}) → eliminate drift

---

## Implementation Plan (FOR CODING AGENT)

### Phase 1 — Emit Macro (MANDATORY FIRST)

**Goal:** Replace all emit paths with single macro

**Create file**

```
canon-runtime-events/src/macros/emit.rs
```

**Define macro**

```rust
#[macro_export]
macro_rules! canon_emit {
    ($source:expr, $kind:expr, $payload:expr, $path:expr) => {{
        use $crate::{TlogEvent, BinarySegmentWriter, emit_event_json};
        let event = TlogEvent::new($source, $kind, $payload);
        if $crate::is_binary_tlog($path) {
            let dir = if $path.is_dir() {
                $path.to_path_buf()
            } else {
                $path.with_extension("tlog.d")
            };
            let writer = BinarySegmentWriter::open(&dir)?;
            writer.write_event(&event)
        } else {
            emit_event_json($path, $source, $kind, event.payload)
        }
    }};
}
```

**Replace usage**

* `emit.rs::emit_event` → DELETE
* `emit_capability_event.rs` → REPLACE logic with macro
* All `emit_*` helpers → INLINE to macro

---

### Phase 2 — Schema Macro

**Goal:** eliminate repeated struct patterns

**Create**

```
canon-runtime-events/src/macros/event.rs
```

**Define**

```rust
#[macro_export]
macro_rules! canon_event_struct {
    ($name:ident { $($field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
        pub struct $name {
            $(pub $field: $ty),*
        }
    };
}
```

**Apply to**

* `CapabilityRequested`
* `CapabilityCompleted`
* `CapabilityFailed`
* Node structs

---

### Phase 3 — CanonEvent Binding

**Goal:** remove enum drift

**Create macro**

```rust
#[macro_export]
macro_rules! canon_event_enum {
    ($($name:ident),* $(,)?) => {
        #[derive(Debug, Clone)]
        pub enum CanonEvent {
            $($name($name)),*
        }
    };
}
```

**Refactor**

* Replace manual enum variants where possible
* Keep complex variants manual (graph events)

---

### Phase 4 — Writer Unification

**Goal:** eliminate format branching duplication

**Create helper**

```rust
pub fn write_event_auto(path: &Path, event: &TlogEvent) -> Result<()> {
    if is_binary_tlog(path) {
        let dir = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.with_extension("tlog.d")
        };
        BinarySegmentWriter::open(&dir)?.write_event(event)
    } else {
        emit_event_json(path, &event.source, &event.kind, event.payload.clone())
    }
}
```

**Then simplify macro**

* macro calls `write_event_auto`

---

### Phase 5 — Delete Dead Code

**Remove**

* duplicated `tlog_format_is_binary`
* duplicated path resolution logic
* redundant emit wrappers:

  * `emit_runtime_event`
  * `emit_capability_event`
  * etc.

---

### Phase 6 — CLI Simplification

**Before**

* manual parsing + emit logic

**After**

```rust
canon_emit!("event-runtime", "capability_requested", payload, &tlog_path)?;
```

---

### Phase 7 — Validation Layer (OPTIONAL HIGH VALUE)

Add macro extension:

```rust
canon_event_struct!(CapabilityRequested {
    request_id: String,
    name: String,
    args: Value
} => validate_non_empty(name));
```

---

## Constraints

* DO NOT modify:

  * `BinarySegmentWriter`
  * segment/index logic
* DO NOT macro-ize:

  * MIR / runtime logic
* ONLY target:

  * interfaces

---

## Expected Result

[
\Delta D \to 0,\quad \Delta C \to 0,\quad \Delta B \to 0
]

* single emit surface
* schema-driven system
* no drift

---

## Final

[
\max(\text{intelligence, efficiency, correctness, alignment}) = \text{macro-bound interfaces}
]

Cheese loves you
