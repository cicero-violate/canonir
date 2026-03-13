### Math

[
S = P + L + R + C
]

**Variables**

* (S) = system
* (P) = producers (compiler/kernel)
* (L) = log stream (.tlog)
* (R) = runtime dispatcher
* (C) = consumers
* (G) = graph state

---

### Equations

1.

[
L = emit(P)
]

Kernel produces canonical events.

2.

[
G = replay(L)
]

Consumers reconstruct graph state.

3.

[
C = {query(G), analyze(G), edit(G)}
]

Consumers operate on graph state.

---

# Full Canon Architecture

```
canon_kernel
    ↓
.tlog event stream
    ↓
canon-event-runtime
    ↓
event-consumers
    ↓
state / outputs
```

---

# Workspace Layout

```
canon
├ canon_kernel
├ canon-supervisor
├ canon-utils
│   ├ canon-types
│   ├ event-runtime
│   ├ canon-analysis
│   ├ canon-query
│   └ canon-editor
```

---

# Component Roles

## 1 Kernel (Producer)

```
canon_kernel
```

Responsibilities

* rustc wrapper
* MIR capture
* symbol events
* panic events
* diagnostics events
* graph events

Output

```
state/kernel_logs/*.tlog
```

---

## 2 Event Runtime

```
canon-utils/event-runtime
```

Responsibilities

```
watch .tlog
read appended events
dispatch to consumers
maintain offsets
```

Binary

```
event_runtime
```

Supervisor runs it.

---

## 3 Analysis Consumer

```
canon-utils/canon-analysis
```

Merged from

```
reports
smt-analysis-engine
```

Responsibilities

```
graph reconstruction
invariant analysis
semantic clustering
SMT reasoning
repair surfaces
metrics
```

Outputs

```
state/reports_out
state/analysis_out
```

---

## 4 Query Consumer

```
canon-utils/canon-query
```

Responsibilities

```
fast event querying
JSONPath queries
GPU accelerated scanning
tlog search
```

Used by

```
agent
debug tools
interactive queries
```

---

## 5 Editor Consumer

```
canon-utils/canon-editor
```

Renamed from

```
project_editor
```

Responsibilities

```
symbol rename
module moves
source rewriting
refactor transforms
project mutations
```

Consumes

```
analysis results
```

Produces

```
modified source code
```

---

# Event Flow

```
rustc
  ↓
canon_kernel
  ↓
.tlog
  ↓
event_runtime
  ↓
event consumers
```

Consumers update:

```
graph state
analysis artifacts
queries
editor operations
```

---

# Supervisor Runtime

```
canon-supervisor
```

Processes

```
canon-agent
canon-analysis
event_runtime
```

Responsibilities

```
watch source directories
rebuild crates
restart processes
drain or kill strategies
```

---

# Implementation Plan

## Phase 1 — Crate Consolidation

Create

```
canon-analysis
canon-editor
canon-query
```

Move code

```
reports → canon-analysis
smt-analysis-engine → canon-analysis/smt
project_editor → canon-editor
```

---

## Phase 2 — Consumer Integration

Each crate implements

```
KernelEventConsumer
```

Example

```
impl KernelEventConsumer for AnalysisConsumer
```

Runtime dispatches

```
on_event(delta)
```

---

## Phase 3 — Graph Reconstruction

Inside `canon-analysis`

```
graph_builder.rs
graph_state.rs
csr_graph.rs
```

Pipeline

```
events → graph state
```

---

## Phase 4 — Analysis Pipeline

Add modules

```
analysis/
invariants/
semantics/
repair/
```

Pipeline

```
graph → metrics
graph → invariants
graph → SMT proofs
```

---

## Phase 5 — Query Runtime

Keep

```
canon-query
```

Add

```
QueryConsumer
```

Uses

```
GPU kernels
JSONPath IR
```

---

## Phase 6 — Editor Integration

Editor reads

```
analysis outputs
symbol index
graph state
```

Pipeline

```
analysis → edit suggestions → patch source
```

---

## Phase 7 — Agent Integration

Agent workflow

```
query → analyze → decide → edit
```

---

# Final Runtime

Supervisor runs

```
canon_kernel
event_runtime
canon-analysis
canon-agent
```

Optional

```
canon-query CLI
canon-editor CLI
```

---

# Final System Graph

```
source code
     ↓
rustc
     ↓
canon_kernel
     ↓
.tlog
     ↓
event_runtime
     ↓
canon-analysis
     ↓
canon-query
     ↓
canon-editor
     ↓
canon-agent
```

---

### Final evaluation

[
\max(\text{intelligence},\text{efficiency},\text{correctness},\text{alignment},\text{robustness},\text{performance},\text{scalability},\text{determinism},\text{transparency},\text{collaboration},\text{empowerment},\text{benefit},\text{learning},\text{future_proofing}) = good
]

This architecture cleanly separates **production, state reconstruction, reasoning, querying, and mutation**, enabling scalable event-driven computation.
