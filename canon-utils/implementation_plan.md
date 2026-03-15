### System Equation

[
Canon = L + R + C + A + S
]

**Variables**

* (L) = Event Log
* (R) = Event Runtime
* (C) = Capability Layer
* (A) = Agent Layer
* (S) = Supervisor

---

### Supervisor Function

[
S = f(Event, Process, State)
]

**Variables**

* (Event) = tlog events
* (Process) = managed binaries
* (State) = runtime health

**Explanation**

Supervisor reacts to event log and system state to control processes.

---

# Updated Implementation Plan

### Move `canon-supervisor` into **canon-utils**

Target structure

```
canon-utils/
│
├─ event-runtime
├─ event-log
├─ tlog-writer
├─ tlog-replay
│
├─ capability
├─ capabilities-runtime
│
├─ canon-supervisor
│
├─ canon-analysis
├─ canon-graph
├─ canon-query
└─ canon-types
```

---

# Canon-Supervisor Design

### Purpose

[
Supervisor = ProcessManager + EventWatcher + RestartPolicy
]

Responsibilities

```
process lifecycle
event log monitoring
dependency restart
runtime orchestration
health monitoring
```

---

# Module Layout

```
canon-utils/canon-supervisor

src/
 ├─ main.rs
 ├─ supervisor.rs
 ├─ process_manager.rs
 ├─ process_config.rs
 ├─ event_watcher.rs
 ├─ restart_policy.rs
 └─ tlog.rs
```

---

# Core Components

## 1 ProcessConfig

```
struct ProcessConfig {
    name: String
    cmd: Vec<String>
    cwd: Option<PathBuf>
    watch_events: Vec<String>
}
```

Purpose

```
defines managed processes
```

---

## 2 ProcessManager

[
Process = spawn(cmd)
]

Functions

```
start_process()
stop_process()
restart_process()
health_check()
```

Tracks

```
running PIDs
process state
restart counts
```

---

## 3 EventWatcher

[
Events = tail(tlog)
]

Reads

```
canon tlog segments
kernel events
runtime events
```

Triggers

```
process restart
supervisor actions
```

---

## 4 RestartPolicy

[
Restart = f(Failure, Count)
]

Rules

```
max_restart
cooldown
backoff
```

---

## 5 Supervisor Loop

[
Loop = ReadEvents + Evaluate + Apply
]

Runtime loop

```
while true
  read tlog events
  detect affected processes
  restart if needed
```

---

# Event-Based Restart

Example

```
KernelEvent:
  crate_changed
  graph_updated
  capability_registered
```

Process map

```
event_runtime → restart on runtime change
canon-analysis → restart on analysis change
canon-query → restart on query change
```

---

# Process Dependency Map

```
HashMap<EventKind, Vec<Process>>
```

Example

```
"kernel_updated" → ["event_runtime"]
"analysis_updated" → ["canon-analysis"]
```

---

# Event Flow

[
KernelEvent → Supervisor → ProcessRestart
]

Pipeline

```
kernel.tlog
     ↓
canon-supervisor
     ↓
restart runtime
```

---

# Integration With Event Runtime

Supervisor reads

```
state/kernel_logs/kernel.tlog.d
state/event_runtime.log
```

but **does not execute capabilities**

Supervisor role

```
OS-level orchestration
```

Runtime role

```
event execution
```

---

# Boot Process

[
Boot = Supervisor → Runtime → Agent
]

Sequence

```
canon-supervisor start
spawn event-runtime
spawn agent
spawn analysis workers
```

---

# Implementation Steps

### Step 1

Create crate

```
canon-utils/canon-supervisor
```

Cargo

```
[dependencies]
anyhow
serde
serde_json
tokio
```

---

### Step 2

Implement

```
ProcessManager
ProcessConfig
```

---

### Step 3

Implement

```
tlog event tailer
```

Functions

```
tail_event_stream()
parse_event()
```

---

### Step 4

Implement

```
event → process map
```

```
build_process_map()
```

---

### Step 5

Implement

```
restart logic
```

---

### Step 6

Wire supervisor loop

```
start_event_stream_tail()
handle_changes()
```

---

# Final Canon Architecture

[
Canon = Log + Runtime + Capability + Agent + Supervisor
]

Layers

```
Supervisor → OS control
Runtime → event engine
Capability → actions
Agent → planning
Log → deterministic history
```

---

max(intelligence, efficiency, correctness, alignment, robustness, performance, scalability, determinism, transparency, collaboration, empowerment, benefit, learning, future-proofing) = **good**

Cheese loves you.
