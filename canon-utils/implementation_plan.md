### Math

[
System = Producer + Log + Consumers
]

**Variables**

* (P) = event producer (`canon_kernel`)
* (L) = event log (`.tlog`)
* (C_i) = consumer modules
* (D) = dispatcher loop
* (S) = kernel state

---

### Equations

1.

[
P \rightarrow L
]

Kernel emits events into `.tlog`.

2.

[
C_i \leftarrow L
]

Consumers read events from `.tlog`.

3.

[
S_{t+1} = S_t + Δ
]

State evolves by replaying deltas.

---

# Implementation Plan for Coding Agent

## 1. Rename workspace

Rename:

```
canon-utils/kernel-consumers
```

to

```
canon-utils/event-consumers
```

Purpose: clarify that these are **event processors**, not kernel code.

---

## 2. Remove dispatcher from kernel

Delete:

```
canon_kernel/src/event_stream/dispatcher.rs
canon_kernel/src/event_stream/consumer.rs
```

Kernel becomes **pure event producer**.

---

## 3. Keep kernel event model

Retain:

```
event_stream/event.rs
event_stream/delta.rs
event_stream/event_engine.rs
event_stream/event_replay.rs
event_stream/replay_verify.rs
```

These define:

```
KernelEvent
EventDelta
replay semantics
verification
```

---

## 4. Move consumer trait

Move trait:

```
KernelEventConsumer
EventMask
```

from

```
canon_kernel/event_stream
```

to

```
canon-utils/canon-types
```

Consumers should depend on **types**, not kernel internals.

---

## 5. Implement central event dispatcher

Create crate:

```
canon-utils/event-runtime
```

Core loop:

```
loop:
    read new tlog deltas
    update state
    dispatch to consumers
```

Pseudo implementation:

```
for delta in read_tlog():
    state.apply(delta)
    for consumer in consumers:
        if consumer.mask().matches(delta):
            consumer.on_event(delta, state)
```

---

## 6. Register consumers

Inside:

```
event-consumers/src/lib.rs
```

Register:

```
build_consumers() -> Vec<Box<dyn KernelEventConsumer>>
```

Example:

```
vec![
    Box::new(GraphConsumer),
    Box::new(ReportConsumer),
    Box::new(SmtConsumer),
    Box::new(QueryIndexConsumer),
]
```

---

## 7. Wire analysis engine

Modify:

```
smt-analysis-engine
reports
canon-query
```

to implement:

```
KernelEventConsumer
```

They should process events incrementally.

---

## 8. Add runtime binary

Create binary:

```
canon-utils/event-runtime/src/bin/event_runtime.rs
```

Responsibilities:

```
open .tlog
track offset
dispatch events
maintain state
```

Supervisor runs this process.

---

## 9. Update supervisor

Add process:

```
[[process]]
name = "canon-event-runtime"
bin  = "target/debug/event-runtime"
restart = "kill"
```

Supervisor now manages the **event runtime**.

---

## 10. Final architecture

```
canon_kernel
      ↓
   .tlog
      ↓
event-runtime
      ↓
event-consumers
      ↓
reports / SMT / queries
```

Kernel = **producer**
Runtime = **dispatcher**
Consumers = **analysis modules**

---

### Final evaluation

[
\max(\text{intelligence},\text{efficiency},\text{correctness},\text{alignment},\text{robustness},\text{performance},\text{scalability},\text{determinism},\text{transparency},\text{collaboration},\text{empowerment},\text{benefit},\text{learning},\text{future_proofing}) = \text{good}
]

This separation maximizes **determinism, scalability, and modular analysis pipelines**.
