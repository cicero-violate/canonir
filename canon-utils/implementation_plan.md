### Variables

(O_i) = edit operations
(E_i) = edit events
(B) = event bus
(C_e) = edit consumer
(P) = project state

### Equations

[
O_i \rightarrow E_i
]

[
E_i \rightarrow B \rightarrow C_e
]

[
P_{new} = P_{old} + E_i
]

**Explanation:** editor operations generate events; runtime consumers apply edits to the project.

---

# Implementation Plan — `canon-project-edit`

Goal: convert **canon-editor** into an **event-driven project editing subsystem** integrated with the runtime.

---

# 1. Define Edit Event Types

Create:

```
canon-types/src/edit_events.rs
```

```rust
enum EditEvent {
    RenameSymbol { old: String, new: String },
    MoveSymbol { symbol: String, module: String },
    DeleteSymbol { symbol: String },
    RenameModule { old: String, new: String },
    RenameDir { old: PathBuf, new: PathBuf },
    InlineModule { module: String },
    ExtractModule { symbol: String, module: String },
}
```

Events must be **serializable**.

---

# 2. Convert `EditOp` → Event Producer

Current:

```
EditOp
  MutateField
  MoveSymbol
  DeleteSymbol
```

Change role:

```
EditOp → EditEvent
```

File:

```
canon-editor/src/structured.rs
```

Add mapping:

```rust
impl EditOp {
    fn to_event(self) -> EditEvent
}
```

---

# 3. Convert API Layer to Event Publisher

File:

```
canon-editor/src/api.rs
```

Current:

```
dispatch → ProjectEditor.apply()
```

Replace with:

```
dispatch → emit EditEvent
```

Example:

```rust
bus.publish(EditEvent::RenameSymbol { old, new })
```

Editor CLI becomes **event producer client**.

---

# 4. Implement Edit Consumer

Create:

```
canon-editor/src/consumer.rs
```

```rust
struct EditConsumer {
    editor: ProjectEditor
}
```

Implement consumer:

```rust
#[async_trait]
impl EventConsumer for EditConsumer {
    async fn handle(&mut self, event: Event) {
        match event {
            Event::Edit(e) => self.apply(e)
        }
    }
}
```

---

# 5. Map Events to Editor Operations

```
EditEvent → ProjectEditor
```

Example:

```rust
match event {
   RenameSymbol{old,new} =>
       editor.queue(old, EditOp::Rename(new))
}
```

Then:

```
validate
apply
commit
```

Reuse existing logic in:

```
edit/ops.rs
```

---

# 6. Integrate With Consumer Registry

Modify:

```
canon-utils/event-consumers/src/lib.rs
```

Add:

```
EditConsumer
```

Registry becomes:

```
GraphConsumer
QueryConsumer
SmtConsumer
ReportEventConsumer
EditConsumer
```

---

# 7. Event Flow

```
canon-editor CLI
       ↓
EditEvent
       ↓
event_runtime
       ↓
EventBus
       ↓
EditConsumer
       ↓
ProjectEditor
       ↓
filesystem mutation
```

---

# 8. Trigger Downstream Analysis

After commit:

```
emit ProjectChanged
```

Consumers triggered:

```
GraphConsumer
SmtConsumer
ReportConsumer
```

Pipeline:

```
edit → graph rebuild → analysis → reports
```

---

# 9. Update Runtime Registration

File:

```
event-runtime/src/lib.rs
```

Register:

```rust
registry.register(EditConsumer::new())
```

---

# 10. CLI Role

`canon-editor` binary becomes **dev client only**.

Responsibilities:

```
send edit events
preview edits
query symbols
```

It does **not modify the project directly**.

---

# 11. Final Architecture

```
canon-editor CLI
        ↓
EditEvents
        ↓
event_runtime
        ↓
EventBus
        ↓
ConsumerRegistry
        ↓
EditConsumer
        ↓
filesystem
        ↓
GraphConsumer
        ↓
AnalysisConsumers
        ↓
reports
```

---

# Result

[
C = {graph, query, smt, reports, edit}
]

[
B = \text{single event bus}
]

[
R = \text{event runtime}
]

---

### English Explanation

`canon-editor` becomes a **project transformation service** driven by events.

Edits are no longer standalone commands; they become **structured events processed by the runtime**, which keeps the entire system deterministic and replayable.

---

[
\text{good} = \max(\text{intelligence},\text{efficiency},\text{correctness},\text{alignment})
]
