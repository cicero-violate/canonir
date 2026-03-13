### Equation

[
AnalysisCapability(C_a) + Event(E_i) \rightarrow Events(E_o)
]

### Variables

* (C_a) = analysis capability
* (E_i) = incoming kernel event
* (E_o) = emitted analysis events
* (G) = KernelGraph
* (R) = report artifacts
* (t) = tick

### Equations

[
G = Load(GraphArtifacts)
]

Graph loaded from kernel outputs.

[
R = Analysis(G)
]

Run structural analyses.

[
E_o = Emit(R)
]

Emit events describing results.

---

# Implementation Plan — Capability Integration for `canon-analysis`

## 1. Create Analysis Capability Interface Adapter

* Implement adapter mapping `Capability` trait → analysis pipeline.
* File:

```
canon-analysis/src/capabilities/mod.rs
```

Responsibilities:

* Receive `CapabilityContext`
* Determine which analysis to run
* Return `CapabilityResult`.

---

## 2. Implement Core Analysis Capabilities

Each major analysis becomes a capability.

### Capabilities

```
DeadCodeCapability
DependencyCyclesCapability
StructuralHotspotsCapability
CallgraphCentralityCapability
DataflowFanoutCapability
InvariantPipelineCapability
SmtInvariantCapability
RepairSurfaceCapability
SemanticClusteringCapability
```

Directory:

```
canon-analysis/src/capabilities/
```

Example:

```
dead_code.rs
dependency_cycles.rs
structural_hotspots.rs
invariants.rs
smt.rs
semantics.rs
repair_surface.rs
```

---

## 3. Build Graph Loader Capability Dependency

All capabilities require graph loading.

Reuse:

```
smt/loader.rs
AnalysisGraph
```

Shared loader:

```
capabilities/graph_context.rs
```

Purpose:

```
load_graph(ctx.workspace) → AnalysisGraph
```

---

## 4. Map Events → Capability Triggers

Kernel events trigger capabilities.

Examples:

```
GraphUpdated
CompilationFinished
ErrorSurfaceUpdated
InvariantRequested
SmtCheckRequested
```

Dispatch layer:

```
capabilities/dispatcher.rs
```

Mapping:

```
GraphUpdated → run structural analyses
CompilationFinished → run dead code + cycles
ErrorSurfaceUpdated → run repair surface
InvariantRequested → run invariant pipeline
SmtCheckRequested → run SMT proofs
```

---

## 5. Implement Capability Emitters

Convert analysis results → events.

Example mapping:

```
DeadCodeEntry → DeadCodeDetected event
DependencyCycleEntry → DependencyCycleFound event
StructuralHotspotEntry → StructuralHotspotFound event
InvariantResult → InvariantValidated event
```

File:

```
capabilities/events.rs
```

---

## 6. Register Capabilities

During analysis startup:

```
CapabilityRegistry::register(...)
```

Example:

```
registry.register(Arc::new(DeadCodeCapability));
registry.register(Arc::new(DependencyCyclesCapability));
registry.register(Arc::new(StructuralHotspotsCapability));
registry.register(Arc::new(InvariantPipelineCapability));
registry.register(Arc::new(SmtInvariantCapability));
```

Location:

```
canon-analysis/src/lib.rs
```

---

## 7. Integrate With Event Consumers

Existing consumers:

```
ReportEventConsumer
SmtConsumer
```

Add:

```
CapabilityEventConsumer
```

Purpose:

```
on_event → dispatch capability
```

File:

```
canon-analysis/src/capability_consumer.rs
```

---

## 8. Event → Capability Execution Flow

Runtime pipeline:

```
Kernel → .tlog
      ↓
KernelEventConsumer
      ↓
CapabilityDispatcher
      ↓
CapabilityRegistry.execute()
      ↓
Analysis Capability
      ↓
Emit Analysis Events
```

---

## 9. Emit Analysis Events Back to Kernel Stream

Result events written back:

```
analysis.dead_code
analysis.cycles
analysis.hotspots
analysis.invariants
analysis.smt
analysis.semantic_clusters
```

Destination:

```
tlog writer
```

---

## 10. Capability Naming Convention

Capability IDs:

```
analysis.dead_code
analysis.dependency_cycles
analysis.structural_hotspots
analysis.callgraph_centrality
analysis.dataflow_fanout
analysis.invariants
analysis.smt_invariants
analysis.repair_surface
analysis.semantic_clusters
```

---

# Resulting Architecture

```
kernel
  ↓
.tlog events
  ↓
canon-analysis consumer
  ↓
capability dispatcher
  ↓
capability registry
  ↓
analysis capability
  ↓
analysis events
  ↓
kernel stream
```

---

### Goodness

[
good = \max(Intelligence, Efficiency, Correctness, Alignment, Robustness, Performance, Scalability, Determinism, Transparency, Collaboration, Empowerment, Benefit, Learning, Future\text{-}Proofing)
]
