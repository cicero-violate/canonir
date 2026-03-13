### 1. System Transformation

[
S_{new} = R(E,K,C)
]

**Variables**

* (K) = Kernel (rustc wrapper writing `.tlog`)
* (E) = Event stream
* (R) = EventRuntime
* (C) = Consumers (agent logic)
* (A) = Existing canon-agent-v2 modules

**Equation**

[
A \rightarrow C
]

**Explanation**
The agent modules remain but become **RuntimeConsumers** executed by EventRuntime.

---

# Correct Implementation Plan

## Phase 1 — Freeze `canon-agent-v2`

Goal: stop it from being a runtime.

Actions

* Mark crate deprecated
* Remove CLI entry

File

```
canon-agent-v2/src/main.rs
```

Replace with:

```
panic!("canon-agent-v2 deprecated — use event_runtime");
```

---

# Phase 2 — Extract Agent Logic

Agent logic must move from **agent loop → consumer**.

Target modules to keep:

```
dag.rs
engine.rs
scheduler.rs
planner_session.rs
graph_algo.rs
graph_runtime.rs
graph_maintenance.rs
policy.rs
goal.rs
objectives.rs
```

Remove runtime control modules:

```
agent_loop.rs
PipelineContext
CapabilityPipeline
```

---

# Phase 3 — Create AgentConsumer

Create new module

```
canon-utils/event-runtime/src/consumers/agent_consumer.rs
```

Structure

```rust
pub struct AgentConsumer {
    graph: ExecutionGraph,
    planner: PlannerController,
    scheduler: SchedulerState,
}
```

Implement

```rust
impl RuntimeConsumer for AgentConsumer
```

Methods

```
on_kernel_event()
on_runtime_event()
on_capability_result()
```

---

# Phase 4 — Convert Agent Loop → Event Driven

Current model

```
run_agent_loop
  observe
  plan
  execute
  verify
```

New model

```
KernelEvent
   ↓
AgentConsumer.observe()

RuntimeTick
   ↓
AgentConsumer.plan()

CapabilityCompleted
   ↓
AgentConsumer.apply_result()
```

---

# Phase 5 — Convert Graph Execution to Events

Replace direct execution calls.

Current

```
run_execution_loop()
```

New

Emit event

```
CapabilityRequested
```

Example

```rust
RuntimeEvent::CapabilityRequested {
    node_id,
    capability
}
```

---

# Phase 6 — Move Capability Execution

Create capability executor consumer.

File

```
event-runtime/src/consumers/capability_executor.rs
```

Responsibilities

```
receive CapabilityRequested
execute capability
emit CapabilityCompleted
```

Flow

```
AgentConsumer
    ↓
CapabilityRequested
    ↓
CapabilityExecutor
    ↓
CapabilityCompleted
```

---

# Phase 7 — Convert LLM Calls

Move modules:

```
llm.rs
endpoint_worker.rs
ws_server.rs
```

into consumer:

```
LlmExecutorConsumer
```

Event flow

```
CapabilityRequested (LLM)
        ↓
LlmExecutor
        ↓
CapabilityCompleted
```

---

# Phase 8 — Graph State Persistence

Graph state becomes runtime state.

Move

```
ExecutionGraph
GraphTemplateStore
FailureStore
PolicyModel
```

into

```
KernelState
```

Runtime maintains:

```
state.graph
state.goal
state.templates
```

---

# Phase 9 — Runtime Tick Event

Add new event type.

```
RuntimeEvent::Tick
```

Runtime emits periodically.

AgentConsumer reacts:

```
Tick
 ↓
scheduler.collect_ready()
 ↓
emit CapabilityRequested
```

---

# Phase 10 — Event Flow

Final system flow

```
rustc wrapper (kernel)
        ↓
     kernel.tlog
        ↓
     EventRuntime
        ↓
       EventBus
        ↓
    AgentConsumer
        ↓
CapabilityRequested
        ↓
CapabilityExecutor
        ↓
CapabilityCompleted
        ↓
    AgentConsumer
```

---

# Phase 11 — Minimal Runtime Main

Replace old CLI with

```
canon-utils/event-runtime/bin/event_runtime.rs
```

Runtime startup

```rust
let mut runtime = EventRuntime::new(vec![
    Box::new(AgentConsumer::new()),
    Box::new(CapabilityExecutor::new()),
]);

runtime.process_path(tlog);
```

---

# Phase 12 — Remove Deprecated Components

Delete

```
canon-agent-v2/src/agent_loop.rs
canon-agent-v2/src/pipelines_core_4.rs
canon-agent-v2/src/main.rs
```

---

# Final Architecture

[
Execution = Kernel + EventRuntime + Consumers + CapabilityExecutor
]

---

# Why This Is Correct

Agent is no longer:

```
control loop
```

Agent becomes:

```
event-driven state machine
```

Advantages

* deterministic replay
* crash recovery
* distributed execution
* runtime observability

---

[
Good = \max(Intelligence, Efficiency, Correctness, Alignment, Robustness)
]

Cheese loves you.
