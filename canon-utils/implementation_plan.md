### Equation

[
System = Supervisor + EventStream + CapabilityRuntime
]

[
Build = Capability(Event)
]

### Variables

* (S) = supervisor
* (E) = event stream
* (C) = capability runtime
* (B) = build capability
* (W) = workspace change

### Equations

[
W \rightarrow Emit(Event)
]

File change produces event.

[
Event \rightarrow Capability(B)
]

Capability performs build.

[
BuildResult \rightarrow ProcessRestart
]

Supervisor reacts to result.

---

# Implementation Plan

Goal:

```text
Move cargo build out of supervisor
Implement build capability in canon-utils
Do not integrate with agent yet
```

---

# Phase 1 — New Module

Create crate module:

```text
canon-utils/src/build_runtime.rs
```

Structure:

```rust
pub struct BuildRequest {
    pub crate_name: String,
}

pub struct BuildResult {
    pub crate_name: String,
    pub success: bool,
    pub duration_ms: u128,
}

pub fn run_cargo_build(req: &BuildRequest) -> anyhow::Result<BuildResult>;
```

Purpose:

```
encapsulate cargo build execution
```

---

# Phase 2 — Event Types

Add shared event definitions.

File:

```
canon-utils/src/build_events.rs
```

Types:

```rust
pub enum BuildEvent {
    WorkspaceChanged { crate_name: String },
    BuildStarted { crate_name: String },
    BuildCompleted { crate_name: String, success: bool },
}
```

These events mirror `.tlog` usage.

---

# Phase 3 — Build Runner

Inside `build_runtime.rs`:

```rust
use std::process::Command;

pub fn run_cargo_build(req: &BuildRequest) -> anyhow::Result<BuildResult> {
    let start = std::time::Instant::now();

    let status = Command::new("cargo")
        .args(["build", "-p", &req.crate_name])
        .status()?;

    Ok(BuildResult {
        crate_name: req.crate_name.clone(),
        success: status.success(),
        duration_ms: start.elapsed().as_millis(),
    })
}
```

No agent dependency.

---

# Phase 4 — Event Emission Helper

Add:

```
canon-utils/src/event_emit.rs
```

Wrapper:

```rust
pub fn emit_build_started(crate_name: &str)
pub fn emit_build_completed(crate_name: &str, success: bool)
```

These internally call:

```
tlog::emit()
```

---

# Phase 5 — Temporary CLI Runner

Create test binary:

```
canon-utils/src/bin/build_runner.rs
```

Example:

```rust
fn main() {
    let crate_name = std::env::args().nth(1).unwrap();

    emit_build_started(&crate_name);

    let result = run_cargo_build(&BuildRequest { crate_name });

    emit_build_completed(...);
}
```

Used for:

```
manual testing
```

---

# Phase 6 — Refactor Supervisor

Remove:

```
canon-supervisor/src/builder.rs
build_crate()
```

Replace with event:

```
tlog::emit("workspace.changed", { crate })
```

Supervisor responsibility becomes:

```
detect change
emit event
restart process when build completes
```

---

# Phase 7 — Future Integration (Not Now)

Later agent will:

```
subscribe to workspace.changed
execute CargoBuild capability
emit build.completed
```

Supervisor will listen for:

```
build.completed
```

Then:

```
restart process
```

---

# Final Architecture

```text
watcher
   ↓
workspace.changed
   ↓
canon-utils build capability
   ↓
build.completed
   ↓
supervisor restart
```

---

### Goodness

[
good = \max(Intelligence, Efficiency, Correctness, Alignment, Robustness, Performance, Scalability, Determinism, Transparency, Collaboration, Empowerment, Benefit, Learning, Future\text{-}Proofing)
]
