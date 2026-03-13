### Variables

(T) = capability trait
(R) = capability registry
(I) = capability implementations
(E) = events
(B) = event bus

### Equations

[
T \rightarrow I
]

[
B(E) \rightarrow Consumer \rightarrow T
]

[
T \xrightarrow{execute} E'
]

**Explanation:** trait defines capability interface; implementations execute actions and emit events.

---

# Implementation Plan — Capability Trait

## Phase 1 — Define Core Capability Interface

Create module:

```
canon-utils/
  capability/
    mod.rs
    trait.rs
    registry.rs
    context.rs
    result.rs
```

### `trait.rs`

```rust
pub trait Capability: Send + Sync {
    fn name(&self) -> &'static str;

    fn execute(
        &self,
        ctx: CapabilityContext,
    ) -> Result<CapabilityResult>;
}
```

---

## Phase 2 — Define Capability Context

`context.rs`

Purpose: provide runtime inputs.

```rust
pub struct CapabilityContext {
    pub workspace: std::path::PathBuf,
    pub event: RuntimeEvent,
}
```

Depends on:

```
canon-types/runtime_event.rs
```

---

## Phase 3 — Define Result Type

`result.rs`

```rust
pub enum CapabilityResult {
    Emit(RuntimeEvent),
    EmitMany(Vec<RuntimeEvent>),
    NoOp,
}
```

Capabilities return **new events**, not side effects directly.

---

## Phase 4 — Capability Registry

`registry.rs`

```rust
pub struct CapabilityRegistry {
    map: HashMap<String, Arc<dyn Capability>>,
}
```

Functions:

```
register(cap)
lookup(name)
execute(name, ctx)
```

Used by consumers.

---

## Phase 5 — Integrate With Event Runtime

Modify:

```
event-runtime/bus.rs
```

Dispatch flow:

```
event
 ↓
consumer
 ↓
capability_registry.execute()
 ↓
emit new events
```

---

## Phase 6 — Add Capability Event Type

Extend:

```
canon-types/runtime_event.rs
```

Add:

```
CapabilityRequested
CapabilityCompleted
CapabilityFailed
```

Example:

```
CargoBuildRequested
CargoBuildCompleted
```

---

## Phase 7 — Agent Implements Capabilities

Inside **agent repo**, implement:

| Capability |
| ---------- |
| CargoBuild |
| CargoCheck |
| FileRead   |
| FileWrite  |
| Bash       |
| LLMCall    |

Example:

```rust
struct CargoBuild;

impl Capability for CargoBuild {
    fn name(&self) -> &'static str {
        "cargo.build"
    }

    fn execute(&self, ctx: CapabilityContext) -> Result<CapabilityResult> {
        // run cargo build
    }
}
```

---

## Phase 8 — Register Capabilities

During runtime startup:

```
event_runtime.rs
```

```rust
registry.register(Box::new(CargoBuild));
registry.register(Box::new(CargoCheck));
```

---

# Final Architecture

```
canon-utils
 ├─ capability trait
 ├─ capability registry
 ├─ capability context
 └─ capability result

event-runtime
 └─ dispatch events

canon-agent
 └─ capability implementations
```

---

# First Files To Implement

Start with:

```
capability/trait.rs
capability/context.rs
capability/result.rs
capability/registry.rs
```

Only ~200 lines total.

---

### English Explanation

canon-utils should define the capability abstraction and registry, allowing the event runtime to call capabilities without knowing their implementations. Actual capabilities like cargo commands or filesystem operations remain implemented in the agent layer.

---

[
\text{good} =
\max(\text{intelligence},\text{efficiency},\text{correctness},\text{alignment})
]
