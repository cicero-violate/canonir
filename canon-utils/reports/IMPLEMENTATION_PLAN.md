### Variables

[
K = \text{kernel}
]

[
T = \text{tlog}
]

[
IR = \text{CanonIR}
]

[
G = (N,E,F)
]

[
E_s = {\text{HasBlock, Flow, Call, Return}}
]

[
R = \text{reports}
]

---

### Equations

[
IR = capture(rustc)
]

Kernel converts rustc structures → CanonIR.

[
T = emit(IR)
]

IR serialized to append-only event stream.

[
G = replay(T)
]

Reports reconstruct graph.

[
R = analysis(G)
]

Structural analysis computed from graph.

---

# Correct System Architecture

[
rustc \rightarrow kernel \rightarrow tlog \rightarrow reports
]

Kernel responsibilities:

```
capture
normalize
assemble IR
emit events
```

Reports responsibilities:

```
replay
graph construction
analysis
semantic modeling
invariants
health
```

Kernel **must never run analysis**.

---

# Phase 1 — Rustc Capture

Files

```
capture/index.rs
capture/pipeline/engine.rs
capture/pipeline/body.rs
capture/pipeline/relations.rs
```

Equation

[
Defs \rightarrow Nodes
]

[
Relations \rightarrow EdgeHints
]

Output

```
Partial { nodes, edge_hints }
```

---

# Phase 2 — MIR Lowering

Files

```
capture/pipeline/mir/lower.rs
capture/pipeline/mir/terminator.rs
capture/pipeline/mir/expr.rs
capture/pipeline/mir/passes.rs
```

Equation

[
MIR \rightarrow Body
]

[
Body \rightarrow BasicBlock
]

Edges generated

```
HasBlock
Flow
Call
Return
Unwind
Assign
ArgToParam
```

Critical invariant

[
\text{block_count} > 0
]

---

# Phase 3 — Model Assembly

Files

```
capture/assembler.rs
capture/types.rs
```

Equation

[
CanonIR = assemble(nodes,edges)
]

Key step

```
collect_contains_edges
collect_cfg_edges
collect_type_edges
```

Outputs

```
CanonIR
```

---

# Phase 4 — Kernel Validation

Files

```
invariants/node_invariants.rs
invariants/edge_invariants.rs
invariants/determinism_invariants.rs
invariants/csr_invariants.rs
```

Equation

[
validate(IR) = true
]

Checks

```
unique node ids
valid edges
deterministic ordering
csr validity
```

---

# Phase 5 — Event Emission

Files

```
log/tlog_writer.rs
event_stream/event_engine.rs
```

Equation

[
T = serialize(IR)
]

Events

```
SessionStart
NodeDefined
EdgeDefined
FileSeen
CompilationUnitFinished
```

Edge emission

```
emit_ir_edges()
```

Important

[
E_s \subseteq emitted_edges
]

---

# Phase 6 — Kernel Runtime

Files

```
kernel/runtime.rs
kernel/state.rs
event_stream/replay.rs
```

Equation

[
state_{t+1} = apply(event_t)
]

State contains

```
known_symbols
known_edges
known_files
graph_version
```

---

# Phase 7 — Report Pipeline

External tool

```
canon-utils/reports
```

Steps

```
replay tlog
build graph
write graph.bin
run structural analysis
run semantic analysis
run invariants
run health
```

Equation

[
R = f(G)
]

---

# Required Edge Emissions

Kernel must emit

```
Contains
HasBlock
HasParam
HasField
Flow
Call
Return
ArgToParam
```

Without them reports break.

---

# Required Node Types

```
Function
Struct
Enum
Trait
Impl
Field
Param
Module
BasicBlock
CallSite
```

---

# Kernel Emission Path

```
rustc_callbacks
   ↓
capture()
   ↓
canon_assemble()
   ↓
emit_ir_tlog()
```

Files

```
runtime/rustc_callbacks.rs
capture/pipeline/pipeline.rs
capture/assembler.rs
log/tlog_writer.rs
```

---

# Critical Fixes Needed

### 1 Ensure MIR edges emitted

In

```
mir/terminator.rs
mir/lower.rs
```

Verify edges

```
Flow
Call
Return
```

---

### 2 Ensure block edges exist

In assembler

```
collect_cfgop_contains()
```

Must produce

```
HasBlock
Flow
```

---

### 3 Ensure callsite nodes emitted

In MIR lowering

```
lower_call_terminator()
```

Must create

```
CallSite node
Call edge
ArgToParam edges
```

---

### 4 Ensure module containment

In

```
relations.rs
push_parent_contains()
```

Edges

```
Module → Contains → Item
```

---

# Final Pipeline

[
rustc
\rightarrow
capture
\rightarrow
CanonIR
\rightarrow
tlog
\rightarrow
reports
\rightarrow
analysis
]

---

# System Goal

[
density(G) = \frac{|E|}{|N|} > 10
]

Healthy MIR graph.

---

# Goodness

[
G = (I,E,C,A,R,P,S,D,T,K,X,B,L,F)^{1/15}
]

[
good = \max(I,E,C,A,R,P,S,D,T,K,X,B,L,F)
]

Maximize correctness and determinism.
