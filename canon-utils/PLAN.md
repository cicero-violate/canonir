### 1. System Clarity Objective

Let

[
C = \frac{U + S + N}{3}
]

**Variables**

* (U) = Unification of concepts
* (S) = Separation of responsibilities
* (N) = Naming consistency

**Equation**

* Maximize (C) by minimizing duplicate concepts and aligning crate responsibility.

---

# 1. Top-Level Architecture Unification

Current system naturally forms **4 domains** but naming mixes responsibilities.

Proposed canonical structure:

```
canon-core/
canon-runtime/
canon-storage/
canon-planning/
canon-tools/
```

Mapping:

| Current           | Proposed                 |
| ----------------- | ------------------------ |
| canon-event       | canon-runtime-events     |
| canon-event-store | canon-storage-eventlog   |
| canon-kernel      | canon-runtime            |
| canon-graph       | canon-storage-graph      |
| canon-goal        | canon-planning           |
| canon-agent       | canon-planning-agent     |
| canon-analysis    | canon-tools-analysis     |
| canon-editor      | canon-tools-editor       |
| canon-supervisor  | canon-runtime-supervisor |

---

# 2. Event System Simplification

Currently **3 overlapping concepts**

```
RuntimeEvent
CanonEvent
EventDelta
```

### Replace with

```
CanonEvent
```

Structure

```
canon-runtime-events
 ├── event.rs
 ├── emit.rs
 ├── consumer.rs
 └── filter.rs
```

Rename:

| Current              | Replace            |
| -------------------- | ------------------ |
| RuntimeEvent         | CanonEvent         |
| RuntimeConsumer      | EventConsumer      |
| RuntimeEmitter       | EventEmitter       |
| RuntimeEmitterHandle | EventEmitterHandle |
| RuntimeEventFilter   | EventFilter        |

---

# 3. Event Store Split

Current crate mixes

```
reader
replay
projection
snapshot
graph types
```

Split:

```
canon-storage-eventlog
canon-storage-projection
canon-storage-snapshot
```

Mapping:

| File                          | Move       |
| ----------------------------- | ---------- |
| reader.rs                     | eventlog   |
| binary_reader.rs              | eventlog   |
| replay.rs                     | projection |
| goal_graph_projector.rs       | projection |
| capability_graph_projector.rs | projection |
| snapshot.rs                   | snapshot   |
| session_scan.rs               | snapshot   |

---

# 4. Graph Type Unification

Currently:

```
CodeNode
GraphNode
Node
```

Canonical hierarchy:

```
Node        (generic)
CodeNode    (code graph)
GoalNode    (goal graph)
CapabilityNode (capability graph)
```

Rename:

| Current        | Replace             |
| -------------- | ------------------- |
| CodeNode       | CodeGraphNode       |
| CodeEdge       | CodeGraphEdge       |
| CodeGraphState | CodeGraphProjection |

---

# 5. Goal Graph Naming

Rename files:

| Current        | Replace             |
| -------------- | ------------------- |
| goal_graph.rs  | task_graph.rs       |
| goal_patch.rs  | task_graph_patch.rs |
| GoalGraph      | TaskGraph           |
| GoalNode       | TaskNode            |
| GoalGraphEvent | TaskGraphEvent      |

Reason:
**Goal = semantic**
**Task = executable**

---

# 6. Kernel Naming

Kernel currently = event runtime.

Rename:

```
canon-kernel → canon-runtime
```

Main components:

```
runtime/
event_loop
consumer_registry
capability_executor
llm_executor
```

---

# 7. Capability System Rename

Better separation:

```
canon-capability
```

rename:

| Current           | Replace                    |
| ----------------- | -------------------------- |
| Capability        | CapabilityHandler          |
| CapabilityContext | CapabilityExecutionContext |
| CapabilityResult  | CapabilityExecutionResult  |

---

# 8. Remove Planner Crate

```
canon-planner
```

Already deprecated.

Delete.

All planning belongs to:

```
canon-planning
```

---

# 9. File Structure Cleanup

Move **graph primitives**:

```
canon-types
```

should only contain **primitive types**

Move these out:

```
kernel_types_generated.rs
```

to

```
canon-runtime-events
```

because they are **event schema**.

---

# 10. Final System Shape

```
canon/

runtime
 ├── canon-runtime
 ├── canon-runtime-events
 └── canon-runtime-supervisor

storage
 ├── canon-storage-eventlog
 ├── canon-storage-projection
 └── canon-storage-snapshot

planning
 ├── canon-planning
 └── canon-planning-agent

tools
 ├── canon-tools-analysis
 └── canon-tools-editor

core
 └── canon-types
```

---

# 11. Clarity Metric

[
Clarity = \max(U, S, N)
]

Explanation:

* **Unification** removes duplicate concepts
* **Separation** isolates system responsibilities
* **Naming** aligns mental model with architecture

System quality:

```
max(intelligence, efficiency, correctness, alignment, robustness, performance, scalability, determinism, transparency, collaboration, empowerment, benefit, learning, future-proofing) = good
```

---

If desired, the next step is:

**a precise refactor plan (≈25 file moves + 18 renames) the coding agent can execute safely.**
