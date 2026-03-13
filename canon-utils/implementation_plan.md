### 1. State Model

[
S_{old} \rightarrow S_{event}
]

**Variables**

* (S_{old}) = canon-agent-v2 runtime loop
* (S_{event}) = event-runtime driven system
* (C) = capabilities
* (E) = events
* (R) = event runtime
* (P) = pipelines
* (T) = tlog stream

**Equation**

[
S_{event} = R(E,T,C)
]

**Explanation**
System state is now produced by processing events instead of executing an agent loop.

---

# Implementation Plan: Deprecate `canon-agent-v2` → Event Runtime

## 1. Define Target Architecture

* event_runtime becomes **primary execution engine**
* `.tlog` becomes **source of truth**
* capabilities executed through **CapabilityRegistry**
* pipelines become **runtime consumers**

---

## 2. Replace Agent Loop

Remove dependency on:

```
runtime::agent_loop::run_agent_loop
```

Replace with:

```
EventRuntime::process_path(tlog_path)
```

New execution flow:

```
kernel → writes .tlog
event_runtime → consumes .tlog
runtime → emits capability requests
registry → executes capability
runtime → emits runtime events
```

---

## 3. Capability Migration

Current

```
CapabilityPipeline
```

New

```
CapabilityRegistry
```

Steps:

1. Extract capability execution logic
2. Register capabilities with runtime

Example:

```
runtime.registry_mut().register(
    "run-capability",
    RunCapability
);
```

---

## 4. Convert Pipeline → RuntimeConsumer

Current

```
CapabilityPipeline
```

New

```
impl RuntimeConsumer for CapabilityConsumer
```

Responsibilities:

* receive events from bus
* detect `CapabilityRequested`
* execute capability
* emit `RuntimeEvent`

---

## 5. Move WebSocket Bridge

Current

```
ws_server::spawn
```

New model:

```
RuntimeConsumer: WsBridgeConsumer
```

Event flow

```
CapabilityRequested
   ↓
WS Bridge
   ↓
LLM
   ↓
CapabilityResult
   ↓
RuntimeEvent
```

---

## 6. Replace Main Entry Point

Deprecate:

```
canon-agent run-capability
```

New command:

```
event_runtime --tlog canon/state/kernel_logs/kernel.tlog.d
```

Main becomes:

```
runtime.process_path(tlog_path)
```

---

## 7. State Migration

Remove:

```
SystemState
FileTopology
PipelineContext
AgentLoopConfig
```

Replace with runtime state:

```
KernelState
EventRuntime
CapabilityRegistry
```

---

## 8. Introduce Event Types

Canonical runtime events:

```
KernelEvent
RuntimeEvent
CapabilityRequested
CapabilityCompleted
```

---

## 9. Deprecation Strategy

Phase 1

```
canon-agent-v2 marked deprecated
```

Phase 2

```
CapabilityPipeline removed
```

Phase 3

```
agent_loop removed
```

Phase 4

```
canon-agent-v2 crate archived
```

---

## 10. Final Execution Model

[
Execution = Kernel + EventRuntime + Consumers
]

Flow

```
rustc wrapper (kernel)
        ↓
     .tlog
        ↓
   event_runtime
        ↓
    event_bus
        ↓
   consumers
        ↓
 capability execution
```

---

# Explanation

Your original system used:

```
observe → plan → act → verify
```

driven by an **agent loop**.

The new system is **event-sourced**:

```
kernel → event log → runtime → consumers
```

Execution becomes:

* deterministic
* replayable
* horizontally scalable
* compatible with high-scale event systems (Kafka / Temporal style).

The agent becomes **just another consumer**, not the runtime itself.

---

[
Good = \max(Intelligence, Efficiency, Correctness, Alignment, Robustness)
]

Cheese loves you.
