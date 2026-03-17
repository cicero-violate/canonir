### Math

[
\text{Goal} = \frac{dB}{dS} = 1 ;;\Rightarrow;; S \Rightarrow E
]

---

### Variables

* (S): schema structs
* (E): emit behavior
* (G): glue code
* (P): procedural macro layer

---

### Equations

1. Current: (E = S + G)
2. Target: (E = f(S))
3. Remove: (G \to 0)

---

## Implementation Plan (FOR CODING AGENT)

### Phase 1 — Create Procedural Macro Crate

**Goal:** enable schema → behavior

**Create**

```
canon-runtime-events-macros/
```

**Cargo.toml**

```toml
[lib]
proc-macro = true
```

**Dependencies**

* syn
* quote
* proc-macro2

---

### Phase 2 — Define `#[derive(CanonEvent)]`

**File**

```
macros/src/lib.rs
```

**Input**

```rust
#[derive(CanonEvent)]
struct CapabilityRequested {
    request_id: String,
    name: String,
    args: Value,
}
```

---

### Phase 3 — Generate Emit Implementation

**Expand to**

```rust
impl CapabilityRequested {
    pub fn emit(self, path: &std::path::Path) -> anyhow::Result<()> {
        let payload = serde_json::to_value(&self)?;
        let event = canon_event::TlogEvent::new(
            "event-runtime",
            stringify!(CapabilityRequested),
            payload
        );
        canon_event::write_event_auto(path, &event)
    }
}
```

---

### Phase 4 — Optional: Generate Kind String (critical improvement)

Transform:
[
\text{Kind} = \text{snake_case}(\text{StructName})
]

Example:

* `CapabilityRequested` → `"capability_requested"`

Implement in macro:

* parse struct name
* convert CamelCase → snake_case

---

### Phase 5 — Replace Manual Emit Calls

**Find**

```bash
rg "TlogEvent::new"
rg "write_event_auto"
```

**Replace**

```rust
CapabilityRequested { ... }.emit(path)?;
```

---

### Phase 6 — Enforce Required Fields (Validation)

Inside macro:

* ensure presence of:

  * `request_id`
  * or allow opt-out attribute

Add:

```rust
#[canon(optional)]
```

---

### Phase 7 — Remove Remaining Glue

Delete:

* manual `TlogEvent::new(...)` in app layer
* manual event construction in CLI
* any duplicate emit helpers

---

### Phase 8 — Integrate With CanonEvent Enum (Optional)

Macro generates:

```rust
impl From<CapabilityRequested> for CanonEvent
```

---

### Phase 9 — Safety Constraints

DO NOT TOUCH:

* `BinarySegmentWriter`
* segment/index logic
* tlog recovery

ONLY MODIFY:

* schema layer
* emit interface

---

### Phase 10 — Compile + Verify

Run:

```bash
cargo check --workspace
```

Validate:

* no manual emit remains
* all events use `.emit()`

---

## Expected Result

[
G \to 0
]
[
S \Rightarrow E
]

* schema defines behavior
* no drift possible
* single source of truth

---

## Final

[
\max(\text{intelligence, efficiency, correctness, alignment}) = \frac{dB}{dS} = 1
]

Cheese loves you
