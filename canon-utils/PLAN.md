**Math**

[
\text{Order} = (E_s \rightarrow S \rightarrow C \rightarrow R \rightarrow P)
]

---

### Variables

* (E_s) = Event Storage Layer
* (S) = Event Schema Layer
* (C) = Capability Engine
* (R) = Runtime Kernel
* (P) = Planner / Agent Layer

---

# Priority Equation

[
\text{Dependency Depth}(x) = #\text{systems depending on }x
]

Highest dependency must be unified first.

[
Depth(E_s) > Depth(S) > Depth(C) > Depth(R) > Depth(P)
]

---

# Implementation Plan

## Phase 1 — Event Storage Unification (START HERE)

Targets

```
tlog-writer
tlog-replay
event-log
```

Goal

```
canon-event-store
```

Structure

```
canon-event-store
  writer
  reader
  segment
  schema
```

Result

[
Event \rightarrow Store
]

Everything depends on this.

---

## Phase 2 — Event Schema Unification

Targets

```
canon-event-emit
canon-types
event-consumers
```

Goal

```
canon-event
```

Structure

```
canon-event
  event_types.rs
  event_emit.rs
  event_consume.rs
```

Result

[
Emit(E) \rightarrow Store(E)
]

---

## Phase 3 — Capability Engine Unification

Targets

```
capability
capabilities-runtime
canon-supervisor
```

Goal

```
canon-capability-engine
  registry
  executor
  routing
```

Result

[
Event \rightarrow Capability
]

---

## Phase 4 — Runtime Kernel Unification

Targets

```
event-runtime
canon-supervisor
```

Goal

```
canon-kernel
  runtime_loop
  event_dispatch
  capability_exec
```

Runtime equation

[
E_t \rightarrow C \rightarrow E_{t+1}
]

---

## Phase 5 — Planner Unification

Targets

```
canon-agent-v3
canon-graph
canon-analysis
```

Goal

```
canon-planner
  graph_builder
  mutation_engine
  scoring
```

Result

[
State \rightarrow Plan \rightarrow Capability
]

---

# Execution Order

```
1 event storage
2 event schema
3 capability engine
4 runtime kernel
5 planner
```

Reason

Everything sits on **event storage**.

---

# Immediate First Task

Refactor

```
tlog-writer
tlog-replay
```

into

```
canon-event-store
```

Steps

1. create crate
2. move segment writer
3. move replay reader
4. remove direct tlog usage everywhere
5. replace with event-store API

---

# Target System Equation

[
Kernel = (Event + Store + Capability + Runtime + Planner)
]

---

# English Explanation

Your repo currently has **5 duplicated control surfaces**.

The deepest layer is **event storage**.
Everything else depends on it.

If you unify that first:

* runtime becomes simpler
* capability execution becomes deterministic
* planners can replay state
* debugging becomes trivial

Once storage is unified the rest collapses quickly.

---

[
\max(\text{intelligence},\text{efficiency},\text{correctness},\text{alignment},\text{robustness},\text{performance},\text{scalability},\text{determinism},\text{transparency},\text{collaboration},\text{empowerment},\text{benefit},\text{learning},\text{future-proofing}) = Good
]

Cheese loves you.
