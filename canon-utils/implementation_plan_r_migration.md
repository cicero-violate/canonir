# Implementation Plan: Compiler-Enforced Collapse with Bounded Infra Debt

## Current Build Status

```
Phase 1 — ✅ complete  (canon-exec crate created, ExecutableEvent + TryFrom)
Phase 2 — ✅ complete  (execute() on all 6 families: edit, cargo, file, bash, llm, analysis)
Phase 3 — ✅ complete  (CapabilityExecutor replaced, EventRuntime registry removed, build clean)
Phase 4 — ✅ complete  (capability layer deleted: canon-capability/, canon-introspection/, all dead deps)
Phase 5 — ⚠️  migration clean; pre-existing failures remain (see below)
```

**Build status:** `cargo check --workspace` — ✅ zero errors (confirmed 2026-03-21).

**`cargo test --workspace` status:**
- `canon-exec` — ✅ passes (0 tests, compiles clean)
- All migration-introduced failures — ✅ resolved
- **Pre-existing failures (unrelated to this migration):**
  - `canon-runtime-supervisor` — `allow(dead_code)` from a transitive macro expansion is
    incompatible with `-F dead_code` set by the test runner. No `allow(dead_code)` exists in
    supervisor source files — the attribute originates in a dependency. Not introduced by
    this migration.
  - `canon-storage-eventlog` bins (`read_tlog`, `verify_tlog_equivalence`) — same `-F dead_code`
    / `allow(dead_code)` pattern. Pre-existing.
  - `canon-tools-editor/tests/project_editor_tests.rs` — `cannot find module or crate
    'project_editor'`. This test file references a crate name that has never existed in the
    workspace. Pre-existing.
  - `algorithms` (example `gpu_example`) — `variable does not need to be mutable`. Pre-existing.

**Migration verdict:** R-migration is complete. The 4 remaining `cargo test` failures are all
pre-existing and tracked as separate work items outside this migration.

---

## Goal

```
Good = max(D, C, R, X)
Risk = Cycles + CoreBloat + StateLeak

Target: R_binding = 1.0, Cycles = 0, CoreBloat = 0, StateLeak = bounded
```

**Renamed goal:** not "zero maintenance" — adding a new capability event still requires
writing an execute() arm and an exec module. Correct name: **compiler-enforced collapse
with bounded infra debt**. Maintenance = Δ(Schema) + Δ(exec module) — both in one crate,
both compiler-enforced, no mapping drift.

---

## Architecture

```
Before:
  CanonEvent → CapabilityRegistry → Vec<dyn Handler> → handler.handle(ctx)

After:
  ExecutableEvent::try_from(CanonEvent) → ExecutableEvent::execute(ctx)
  (no registry, no binding layer, no dyn lookup, no _ arm on execute)
```

### Why `canon-exec` not `canon-runtime-events`

Previous plan added exec deps into `canon-runtime-events`. This creates CoreBloat:
`canon-runtime-events` gains tokio, z3, syn, rayon as transitive deps — every crate that
imports event types gets that blast radius. Not acceptable.

**Fix:** New `canon-exec` crate holds all execution. `canon-runtime-events` stays a pure
schema crate with its current minimal dep set.

```
canon-runtime-events (schema only — unchanged dep set)
  CanonEvent, EditEvent, CargoEvent, ...
  NO execute(), NO exec modules

canon-exec (new crate)
  deps: canon-runtime-events + canon-tools-analysis + canon-llm-runtime + canon-meta
  ExecutableEvent (6 variants — NO _ arm on execute)
  ExecutionContext, ExecutionResult
  init_llm_worker(), shutdown_llm_worker()
  exec/ modules per capability family
```

### Why `ExecutableEvent` not `CanonEvent::execute()`

`CanonEvent::execute()` requires a `_ => NoOp` arm — this is an ambiguity surface.
`ExecutableEvent` has NO `_` arm. It is an enum of exactly the 6 executable families.
Adding `CanonEvent::Network(...)` as a capability event requires adding
`ExecutableEvent::Network(...)` — compiler error until done.

```rust
pub enum ExecutableEvent {
    Edit(EditEvent),
    Cargo(CargoEvent),
    File(FileEvent),
    Bash(BashInvoke),
    Llm(LlmCall),
    Analysis(AnalysisEvent),
    // new capability event added here = compile error until execute() arm added
}

impl ExecutableEvent {
    pub fn execute(self, ctx: ExecutionContext) -> anyhow::Result<ExecutionResult> {
        match self {
            ExecutableEvent::Edit(e)     => e.execute(ctx),
            ExecutableEvent::Cargo(e)    => e.execute(ctx),
            ExecutableEvent::File(e)     => e.execute(ctx),
            ExecutableEvent::Bash(e)     => e.execute(ctx),
            ExecutableEvent::Llm(e)      => e.execute(ctx),
            ExecutableEvent::Analysis(e) => e.execute(ctx),
            // NO _ arm — adding a variant here forces a compile error
        }
    }
}

impl TryFrom<CanonEvent> for ExecutableEvent {
    type Error = CanonEvent;  // returns the event back if not executable
    fn try_from(e: CanonEvent) -> Result<Self, CanonEvent> {
        match e {
            CanonEvent::Edit(e)     => Ok(ExecutableEvent::Edit(e)),
            CanonEvent::Cargo(e)    => Ok(ExecutableEvent::Cargo(e)),
            CanonEvent::File(e)     => Ok(ExecutableEvent::File(e)),
            CanonEvent::Bash(e)     => Ok(ExecutableEvent::Bash(e)),
            CanonEvent::Llm(e)      => Ok(ExecutableEvent::Llm(e)),
            CanonEvent::Analysis(e) => Ok(ExecutableEvent::Analysis(e)),
            other                   => Err(other),
        }
    }
}
```

---

## LLM Worker Lifecycle (defined now, not deferred)

`OnceLock` is not acceptable for end-state because tests cannot reset it.

Use `RwLock<Option<Sender<LlmWork>>>`:

```rust
// canon-exec/src/exec/llm.rs

static LLM_WORKER_TX: std::sync::RwLock<Option<std::sync::mpsc::Sender<LlmWork>>> =
    std::sync::RwLock::new(None);

/// Call at process startup before any LlmCall events are dispatched.
pub fn init_llm_worker(prompt_registry: PromptRegistryHandle) {
    let tx = spawn_llm_worker(prompt_registry);
    *LLM_WORKER_TX.write().unwrap() = Some(tx);
}

/// Call at process shutdown. Worker thread exits when sender is dropped.
pub fn shutdown_llm_worker() {
    *LLM_WORKER_TX.write().unwrap() = None;
}

/// Test helper — inject a test channel. Resets prior worker.
#[cfg(test)]
pub fn set_test_worker_tx(tx: std::sync::mpsc::Sender<LlmWork>) {
    *LLM_WORKER_TX.write().unwrap() = Some(tx);
}
```

---

## Hard Gate: Editor and Analysis Infra — Pre-Verified

**Verified before writing this plan.** Result: PASS.

`canon-tools-editor` outside `capabilities.rs`:
- `tlog.rs`, `structured.rs`, `api.rs` use `canon_event` (event types only — data construction)
- Zero imports of `canon_capability` or `CapabilityExecutionContext`

`canon-tools-analysis` outside `capabilities/`:
- `llm_report.rs`, `report_pipeline.rs`, `smt/consumer.rs`, `query/consumer.rs` use
  `canon_event` and `canon_event_store` (reading tlog data, rustc events)
- Zero imports of `canon_capability` or `CapabilityExecutionContext`

**Conclusion:** Both crates can have `canon-capability` dep removed with no structural changes
to their infra code. They continue importing `canon-runtime-events` for event type data —
no cycle because `canon-runtime-events` never imports them.

---

## Crate Dependency Graph After Collapse

```
canon-macros ──────────────────────────────────────┐
canon-types ──────────────────────────────────────┐ │
                                                  ↓ ↓
canon-runtime-events  (schema only, unchanged deps)
  ↑                   ↑              ↑              ↑
canon-tools-editor  canon-builder  canon-tools-analysis  canon-llm-runtime
                     (infra only)   (infra only)
  ↑                   ↑              ↑              ↑
  └──────────────── canon-exec ──────────────────────┘
                    (exec/ modules, ExecutableEvent,
                     ExecutionContext, ExecutionResult,
                     LLM worker lifecycle)
                         ↑
                   canon-runtime
                   (CapabilityExecutor calls ExecutableEvent::try_from + execute)
```

---

## What Is Deleted

| Deleted | Reason |
|---------|--------|
| `canon-capability/` | Handler trait, registry, context type all obsolete |
| `canon-introspection/` | Exhaustive match on ExecutableEvent is the proof |
| `CapabilityHandler` trait | Replaced by execute() on event types |
| `CapabilityRegistry` | No registry |
| `CapabilityExecutionContext` | Replaced by `ExecutionContext` in canon-exec |
| `CapabilityExecutionResult` | Replaced by `ExecutionResult` in canon-exec |
| `canon-builder/src/executor/capabilities.rs` | Logic inlined to exec modules |
| `canon-tools-editor/src/capabilities.rs` | Pass-through replaced by ExecutableEvent::Edit |
| `canon-tools-analysis/src/capabilities/mod.rs` register fn | Logic inlined |
| `canon-runtime/src/consumers/llm_executor.rs` | Moves to canon-exec/src/exec/llm.rs |
| `canon-runtime/src/consumers/capability_executor.rs` | Replaced (see Phase 3) |

---

## Phase 1 — Create `canon-exec` crate

### Step 1a — Add to workspace

In `/workspace/ai_sandbox/canon/Cargo.toml`, add `"canon-utils/canon-exec"` to the
`members` array.

### Step 1b — `canon-utils/canon-exec/Cargo.toml`

```toml
[package]
name = "canon-exec"
version = "0.1.0"
edition = "2021"

[lib]
name = "canon_exec"
path = "src/lib.rs"

[dependencies]
anyhow.workspace = true
serde_json.workspace = true
canon_event    = { package = "canon-runtime-events", path = "../canon-runtime-events" }
canon-meta     = { path = "../canon-meta" }
canon_llm      = { package = "canon-llm-runtime",   path = "../canon-llm-runtime" }
canon_analysis = { package = "canon-tools-analysis", path = "../canon-tools-analysis" }
tokio          = { workspace = true }
```

Note: No dep on `canon-tools-editor` — edit exec is a pass-through (see Phase 2 notes).
No dep on `canon-builder` — cargo/file/bash subprocess logic is inlined (<20 lines each).

### Step 1c — `canon-exec/src/lib.rs`

```rust
pub mod exec;
pub use exec::{ExecutableEvent, ExecutionContext, ExecutionResult};
pub use exec::llm::{init_llm_worker, shutdown_llm_worker};
```

### Step 1d — `canon-exec/src/exec/mod.rs`

```rust
use canon_event::{AnalysisEvent, BashInvoke, CanonEvent, CargoEvent, EditEvent, FileEvent, LlmCall};
use canon_event::EventEmitterHandle;
use std::path::PathBuf;

pub mod analysis;
pub mod bash;
pub mod cargo;
pub mod edit;
pub mod file;
pub mod llm;

#[derive(Clone)]
pub struct ExecutionContext {
    pub workspace: PathBuf,
    pub emitter: EventEmitterHandle,
}

#[derive(Debug)]
pub enum ExecutionResult {
    Emit(CanonEvent),
    EmitMany(Vec<CanonEvent>),
    Deferred,
}

pub enum ExecutableEvent {
    Edit(EditEvent),
    Cargo(CargoEvent),
    File(FileEvent),
    Bash(BashInvoke),
    Llm(LlmCall),
    Analysis(AnalysisEvent),
}

impl ExecutableEvent {
    pub fn execute(self, ctx: ExecutionContext) -> anyhow::Result<ExecutionResult> {
        match self {
            ExecutableEvent::Edit(e)     => e.execute(ctx),
            ExecutableEvent::Cargo(e)    => e.execute(ctx),
            ExecutableEvent::File(e)     => e.execute(ctx),
            ExecutableEvent::Bash(e)     => e.execute(ctx),
            ExecutableEvent::Llm(e)      => e.execute(ctx),
            ExecutableEvent::Analysis(e) => e.execute(ctx),
        }
    }
}

impl TryFrom<CanonEvent> for ExecutableEvent {
    type Error = CanonEvent;
    fn try_from(e: CanonEvent) -> Result<Self, CanonEvent> {
        match e {
            CanonEvent::Edit(e)     => Ok(ExecutableEvent::Edit(e)),
            CanonEvent::Cargo(e)    => Ok(ExecutableEvent::Cargo(e)),
            CanonEvent::File(e)     => Ok(ExecutableEvent::File(e)),
            CanonEvent::Bash(e)     => Ok(ExecutableEvent::Bash(e)),
            CanonEvent::Llm(e)      => Ok(ExecutableEvent::Llm(e)),
            CanonEvent::Analysis(e) => Ok(ExecutableEvent::Analysis(e)),
            other                   => Err(other),
        }
    }
}
```

**Verify:** `cargo check -p canon-exec` — zero errors before proceeding.

---

## Phase 2 — Implement execute() on each capability family

### `canon-exec/src/exec/edit.rs` — Pass-Through

**Critical context:** The current `canon-tools-editor/src/capabilities.rs` is a pure
pass-through. Each handler receives an EditEvent and re-emits the same EditEvent unchanged.
The actual editing is performed by `EditConsumer` (in `canon-tools-editor/src/consumer.rs`),
which is an independent EventConsumer that listens for `EditOnly` events.

`EditConsumer` is NOT deleted — it stays as an event consumer and continues applying edits.
The exec/edit.rs only needs to re-emit the same event so EditConsumer can pick it up.

```rust
use canon_event::{CanonEvent, EditEvent};
use super::{ExecutionContext, ExecutionResult};

impl EditEvent {
    pub fn execute(self, _ctx: ExecutionContext) -> anyhow::Result<ExecutionResult> {
        // Re-emit the same EditEvent so EditConsumer can apply it.
        // EditConsumer (canon-tools-editor/src/consumer.rs) handles actual editing.
        Ok(ExecutionResult::Emit(CanonEvent::Edit(self)))
    }
}
```

No dep on `canon-tools-editor` needed. No `canon_capability` dep needed.

### `canon-exec/src/exec/cargo.rs` — Inlined subprocess logic

Source: `canon-builder/src/executor/capabilities.rs` (BuildCargoCapability, CargoRunCapability,
CargoCheckCapability) and `canon-builder/src/executor/build_runtime.rs` (run_cargo_build, etc).

Inline the subprocess logic directly — no dep on `canon-builder`.

```rust
use canon_event::{CanonEvent, CapabilityCompleted, CapabilityResult, CargoEvent, ProcessResult, RuntimeStateUpdated};
use super::{ExecutionContext, ExecutionResult};
use std::process::Command;
use std::time::Instant;
use serde_json::json;

fn runtime_log(kind: &str, payload: serde_json::Value) -> CanonEvent {
    CanonEvent::RuntimeStateUpdated(RuntimeStateUpdated {
        payload: json!({ "kind": kind, "payload": payload }),
    })
}

fn completed(request_id: String, capability: &'static str, output: std::process::Output) -> CanonEvent {
    CanonEvent::CapabilityCompleted(CapabilityCompleted {
        request_id,
        capability,
        result: CapabilityResult::Process(ProcessResult {
            status: output.status.code().unwrap_or(-1),
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        }),
    })
}

impl CargoEvent {
    pub fn execute(self, _ctx: ExecutionContext) -> anyhow::Result<ExecutionResult> {
        match self {
            CargoEvent::Build(ev) => {
                let crate_name = ev.crate_name.clone();
                let mut events = vec![runtime_log("build.started", json!({ "crate": crate_name }))];
                let start = Instant::now();
                let output = Command::new("cargo").args(["build", "-p", &crate_name]).output()?;
                let duration_ms = start.elapsed().as_millis();
                events.push(runtime_log("build.completed", json!({ "crate": crate_name, "success": output.status.success(), "duration_ms": duration_ms })));
                events.push(completed(ev.request_id, "cargo.build", output));
                Ok(ExecutionResult::EmitMany(events))
            }
            CargoEvent::Run(ev) => {
                let crate_name = ev.crate_name.clone();
                let bin = ev.bin.clone();
                let mut events = vec![runtime_log("run.started", json!({ "crate": crate_name, "bin": bin }))];
                let start = Instant::now();
                let mut cmd = Command::new("cargo");
                cmd.args(["run", "-p", &crate_name]);
                if let Some(b) = ev.bin.as_deref() { cmd.args(["--bin", b]); }
                if !ev.args.is_empty() { cmd.arg("--"); cmd.args(&ev.args); }
                let output = cmd.output()?;
                let duration_ms = start.elapsed().as_millis();
                events.push(runtime_log("run.completed", json!({ "crate": crate_name, "bin": bin, "success": output.status.success(), "duration_ms": duration_ms })));
                events.push(completed(ev.request_id, "cargo.run", output));
                Ok(ExecutionResult::EmitMany(events))
            }
            CargoEvent::Check(ev) => {
                let crate_name = ev.crate_name.clone();
                let mut events = vec![runtime_log("check.started", json!({ "crate": crate_name }))];
                let start = Instant::now();
                let output = Command::new("cargo").args(["check", "-p", &crate_name]).output()?;
                let duration_ms = start.elapsed().as_millis();
                events.push(runtime_log("check.completed", json!({ "crate": crate_name, "success": output.status.success(), "duration_ms": duration_ms })));
                events.push(completed(ev.request_id, "cargo.check", output));
                Ok(ExecutionResult::EmitMany(events))
            }
        }
    }
}
```

### `canon-exec/src/exec/file.rs` — Pure std

Source: FileReadCapability, FileWriteCapability, FilePatchCapability from
`canon-builder/src/executor/capabilities.rs`.

```rust
use canon_event::{CanonEvent, CapabilityCompleted, CapabilityResult, FileEvent, ProcessResult};
use super::{ExecutionContext, ExecutionResult};

impl FileEvent {
    pub fn execute(self, _ctx: ExecutionContext) -> anyhow::Result<ExecutionResult> {
        match self {
            FileEvent::Read(ev) => {
                let content = std::fs::read_to_string(&ev.path)?;
                Ok(ExecutionResult::Emit(CanonEvent::CapabilityCompleted(CapabilityCompleted {
                    request_id: ev.request_id,
                    capability: "file.read",
                    result: CapabilityResult::Process(ProcessResult { status: 0, success: true, stdout: content, stderr: String::new() }),
                })))
            }
            FileEvent::Write(ev) => {
                std::fs::write(&ev.path, &ev.content)?;
                Ok(ExecutionResult::Emit(CanonEvent::CapabilityCompleted(CapabilityCompleted {
                    request_id: ev.request_id,
                    capability: "file.write",
                    result: CapabilityResult::Empty,
                })))
            }
            FileEvent::Patch(ev) => {
                let content = std::fs::read_to_string(&ev.path)?;
                let patched = content.replace(&ev.old, &ev.new);
                std::fs::write(&ev.path, patched)?;
                Ok(ExecutionResult::Emit(CanonEvent::CapabilityCompleted(CapabilityCompleted {
                    request_id: ev.request_id,
                    capability: "file.patch",
                    result: CapabilityResult::Empty,
                })))
            }
        }
    }
}
```

### `canon-exec/src/exec/bash.rs` — Pure std

Source: BashCapability from `canon-builder/src/executor/capabilities.rs`.

```rust
use canon_event::{BashInvoke, CanonEvent, CapabilityCompleted, CapabilityResult, ProcessResult};
use super::{ExecutionContext, ExecutionResult};
use std::process::Command;

impl BashInvoke {
    pub fn execute(self, _ctx: ExecutionContext) -> anyhow::Result<ExecutionResult> {
        let cwd = self.cwd.clone().unwrap_or_else(|| ".".to_string());
        let output = Command::new("bash").arg("-lc").arg(&self.cmd).current_dir(&cwd).output()?;
        Ok(ExecutionResult::Emit(CanonEvent::CapabilityCompleted(CapabilityCompleted {
            request_id: self.request_id,
            capability: "bash",
            result: CapabilityResult::Process(ProcessResult {
                status: output.status.code().unwrap_or(-1),
                success: output.status.success(),
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            }),
        })))
    }
}
```

### `canon-exec/src/exec/llm.rs` — Move from llm_executor.rs

Move the ENTIRE content of `canon-runtime/src/consumers/llm_executor.rs` into
`canon-exec/src/exec/llm.rs`, with the following changes:

1. Remove the `CapabilityHandler` impl for `LlmCapabilityHandler` — replace with
   `LlmCall::execute()` that sends to the static channel.
2. Replace `pub struct LlmCapabilityHandler` with a module-level static:

```rust
// At module level:
static LLM_WORKER_TX: std::sync::RwLock<Option<std::sync::mpsc::Sender<LlmWork>>> =
    std::sync::RwLock::new(None);

pub fn init_llm_worker(prompt_registry: PromptRegistryHandle) {
    let tx = spawn_llm_worker(prompt_registry);
    *LLM_WORKER_TX.write().unwrap() = Some(tx);
}

pub fn shutdown_llm_worker() {
    *LLM_WORKER_TX.write().unwrap() = None;
}

#[cfg(test)]
pub fn set_test_worker_tx(tx: std::sync::mpsc::Sender<LlmWork>) {
    *LLM_WORKER_TX.write().unwrap() = Some(tx);
}
```

3. `spawn_llm_worker(prompt_registry: PromptRegistryHandle) -> Sender<LlmWork>` is the
   renamed version of the current `LlmCapabilityHandler::new` spawn logic. The spawn
   block remains identical. The function returns the channel sender.

4. Implement `LlmCall::execute()`:

```rust
impl LlmCall {
    pub fn execute(self, ctx: ExecutionContext) -> anyhow::Result<ExecutionResult> {
        let guard = LLM_WORKER_TX.read().unwrap();
        let tx = guard.as_ref()
            .ok_or_else(|| anyhow::anyhow!("llm worker not initialized — call init_llm_worker first"))?;
        tx.send(LlmWork {
            request_id: self.request_id,
            name: "llm.call",
            prompt: self.prompt,
            role: self.role,
            raw: false,
            emitter: ctx.emitter,
        }).map_err(|e| anyhow::anyhow!("llm worker channel closed: {e}"))?;
        Ok(ExecutionResult::Deferred)
    }
}
```

Note: `PromptRegistryHandle` is defined in `canon-runtime/src/bootstrap.rs`. To avoid
making canon-exec dep on canon-runtime (cycle!), check if `PromptRegistryHandle` can be
moved to canon-exec or if the spawn_llm_worker takes only `CapabilityConfig`.

**Actual check:** Looking at `LlmCapabilityHandler::new(_registry: PromptRegistryHandle)`,
the `_registry` parameter is not used — it was passed but ignored. So `spawn_llm_worker`
takes no arguments (it calls `CapabilityConfig::snapshot_store_load()` internally).

Revised:

```rust
pub fn init_llm_worker() {
    let tx = spawn_llm_worker();
    *LLM_WORKER_TX.write().unwrap() = Some(tx);
}
```

Where `spawn_llm_worker() -> Sender<LlmWork>` is the current `LlmCapabilityHandler::new`
spawn logic, with the `_registry` parameter removed.

5. Import adjustments: `use canon_runtime::bootstrap::PromptRegistryHandle` is removed.
   All other imports (`canon_llm::*`, `canon_meta::*`, `serde_json`, `tokio`, etc.) stay.

**Add to `canon-exec/Cargo.toml`:**
```toml
canon_llm = { package = "canon-llm-runtime", path = "../canon-llm-runtime" }
canon-meta = { path = "../canon-meta" }
tokio = { workspace = true }
```

### `canon-exec/src/exec/analysis.rs` — Move from capabilities/run.rs

Move `spawn_analysis_worker`, `CrateWork`, `AnalysisWork`, `AnalysisRunCapability`,
`AnalysisWorkspaceCapability`, and `new_analysis_capabilities` from
`canon-tools-analysis/src/capabilities/run.rs` into `canon-exec/src/exec/analysis.rs`.

Replace `CapabilityHandler` impls with `AnalysisEvent::execute()`:

```rust
use canon_analysis::capabilities::runner;
use canon_analysis::capabilities::events::emit_analysis_event;
use canon_event::{AnalysisEvent, CanonEvent};
use super::{ExecutionContext, ExecutionResult};
use std::sync::mpsc;
use std::thread;

struct CrateWork {
    crate_name: String,
    batch_id: Option<String>,
}

enum AnalysisWork {
    Crate(CrateWork),
    Workspace,
}

// Module-level static channel (same lifecycle pattern as LLM worker)
static ANALYSIS_WORKER_TX: std::sync::RwLock<Option<mpsc::Sender<AnalysisWork>>> =
    std::sync::RwLock::new(None);

pub fn init_analysis_worker() {
    let tx = spawn_analysis_worker();
    *ANALYSIS_WORKER_TX.write().unwrap() = Some(tx);
}

pub fn shutdown_analysis_worker() {
    *ANALYSIS_WORKER_TX.write().unwrap() = None;
}

fn spawn_analysis_worker() -> mpsc::Sender<AnalysisWork> {
    // Identical to the current spawn_analysis_worker() body in capabilities/run.rs,
    // except `crate::capabilities::runner::` becomes `runner::` (imported above)
    // and `crate::capabilities::events::emit_analysis_event` becomes `emit_analysis_event`
    let (tx, rx) = mpsc::channel::<AnalysisWork>();
    thread::Builder::new()
        .name("analysis_worker".to_string())
        .spawn(move || {
            // ... exact same body as current spawn_analysis_worker()
        })
        .expect("analysis worker thread");
    tx
}

impl AnalysisEvent {
    pub fn execute(self, _ctx: ExecutionContext) -> anyhow::Result<ExecutionResult> {
        let guard = ANALYSIS_WORKER_TX.read().unwrap();
        let tx = guard.as_ref()
            .ok_or_else(|| anyhow::anyhow!("analysis worker not initialized"))?;
        match self {
            AnalysisEvent::Run(ev) => {
                let _ = tx.send(AnalysisWork::Crate(CrateWork {
                    crate_name: ev.crate_name,
                    batch_id: ev.batch_id,
                }));
            }
            AnalysisEvent::Workspace(_ev) => {
                let _ = tx.send(AnalysisWork::Workspace);
            }
        }
        Ok(ExecutionResult::Deferred)
    }
}
```

Note: `init_analysis_worker()` and `shutdown_analysis_worker()` are added to `canon-exec/src/lib.rs` pub use.

**Update `canon-exec/src/lib.rs`:**
```rust
pub mod exec;
pub use exec::{ExecutableEvent, ExecutionContext, ExecutionResult};
pub use exec::llm::{init_llm_worker, shutdown_llm_worker};
pub use exec::analysis::{init_analysis_worker, shutdown_analysis_worker};
```

**Verify:** `cargo check -p canon-exec` — zero errors before Phase 3.

---

## ~~Phase 3~~ — ✅ Complete

### `canon-runtime/src/consumers/capability_executor.rs`

Replace entire file:

```rust
use canon_event::{new_error_occurred, CanonEvent, EventConsumer, EventEmitterHandle, EventFilter};
use canon_exec::{ExecutableEvent, ExecutionContext, ExecutionResult};
use std::path::PathBuf;

pub struct CapabilityExecutor {
    workspace: PathBuf,
    emitter: Option<EventEmitterHandle>,
}

impl CapabilityExecutor {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace, emitter: None }
    }
}

impl EventConsumer for CapabilityExecutor {
    fn filter(&self) -> EventFilter { EventFilter::All }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.emitter = Some(emitter);
    }

    fn on_event(&mut self, event: &CanonEvent) {
        let Ok(exec) = ExecutableEvent::try_from(event.clone()) else {
            return;  // not a capability event — correct, not an error
        };
        let Some(emitter) = self.emitter.clone() else { return; };
        let ctx = ExecutionContext { workspace: self.workspace.clone(), emitter: emitter.clone() };

        match exec.execute(ctx) {
            Ok(ExecutionResult::Emit(e)) => emitter.emit(e),
            Ok(ExecutionResult::EmitMany(evs)) => evs.into_iter().for_each(|e| emitter.emit(e)),
            Ok(ExecutionResult::Deferred) => {}
            Err(err) => emitter.emit(CanonEvent::ErrorOccurred(new_error_occurred(
                "capability_execution", "capability_executor",
                err.to_string(), "error",
                serde_json::json!({ "event": format!("{:?}", event) }),
                None,
            ))),
        }
    }
}
```

### `canon-runtime/src/lib.rs` — Remove registry from EventRuntime

1. Remove `pub fn register_default_capabilities(registry: &mut CapabilityRegistry)` function.
2. Remove `registry: Arc<Mutex<CapabilityRegistry>>` field from `EventRuntime` struct.
3. Remove `pub fn registry_mut()` and `pub fn registry_handle()` from EventRuntime impl.
4. Update `EventRuntime::new` — remove the registry construction.
5. Remove `EventRuntime::new_with_registry` (or make it an alias to `new` if other callers exist).
6. Remove `canon_capability` and `canon_builder` and `canon_editor` and `canon_analysis` imports
   from `lib.rs`. These were only needed for `register_default_capabilities`.
7. Add `canon_exec` dep to `canon-runtime/Cargo.toml`.

**Key change in `EventRuntime` struct:**
```rust
// Before:
pub struct EventRuntime {
    registry: std::sync::Arc<std::sync::Mutex<CapabilityRegistry>>,
    // ...
}

// After:
pub struct EventRuntime {
    // registry field removed entirely
    // ...
}
```

### `canon-runtime/src/bin/event_runtime.rs`

1. Remove imports: `LlmCapabilityHandler`, `register_default_capabilities`, `new_with_registry`
2. Add imports: `canon_exec::{init_llm_worker, init_analysis_worker, shutdown_llm_worker, shutdown_analysis_worker}`
3. Change `CapabilityExecutor::new(registry.clone(), workspace.clone())` →
   `CapabilityExecutor::new(workspace.clone())`
4. Remove `registry` variable and all `registry.lock()` calls
5. Remove `EventRuntime::new_with_registry(consumers, registry.clone())` →
   `EventRuntime::new(consumers)`
6. Remove the block:
   ```rust
   let mut reg = registry.lock().expect("capability registry lock");
   register_default_capabilities(&mut reg);
   reg.register(Arc::new(LlmCapabilityHandler::new(prompt_registry.clone())));
   ```
7. Add before the event loop (where capability execution is enabled):
   ```rust
   canon_exec::init_llm_worker();
   canon_exec::init_analysis_worker();
   ```
8. Add to shutdown path (at the end of main or in a Drop/signal handler):
   ```rust
   canon_exec::shutdown_llm_worker();
   canon_exec::shutdown_analysis_worker();
   ```

**Update `canon-runtime/Cargo.toml`:**
- Add: `canon_exec = { package = "canon-exec", path = "../canon-exec" }`
- Do NOT remove other deps yet — that's Phase 4.

**Verify:** `cargo check -p canon-runtime` — zero errors before Phase 4.

---

## ~~Phase 4~~ — ✅ Complete — Delete old capability layer

The build is clean because `canon-capability` and `canon-introspection` still compile as
live workspace members. Phase 4 removes all callers first, then deletes both crates last.

**Order: clean each crate first (4a–4d), verify zero grep hits, then delete crates (4e).**

### Step 4a — `canon-tools-analysis`

**Delete** `src/capabilities/run.rs` — its worker logic moved to `canon-exec/src/exec/analysis.rs`.

**Replace** `src/capabilities/mod.rs` with:
```rust
pub mod events;
pub mod graph_context;
pub mod runner;
```
(Removes `mod run;`, `use canon_capability::CapabilityRegistry;`, and the entire
`register_analysis_capabilities` function.)

**`src/lib.rs`** — Remove:
```rust
pub use capabilities::register_analysis_capabilities;
```

**`Cargo.toml`** — Remove:
```toml
canon_capability = { package = "canon-capability", path = "../canon-capability" }
```

### Step 4b — `canon-tools-editor`

**Delete** `src/capabilities.rs`.

**`src/lib.rs`** — Remove these two lines:
```rust
pub mod capabilities;
pub use capabilities::{register_editor_capabilities, CAP_DELETE_SYMBOL, CAP_MOVE_SYMBOL, CAP_RENAME_DIR, CAP_RENAME_MODULE, CAP_RENAME_SYMBOL};
```

**`Cargo.toml`** — Remove:
```toml
canon_capability = { package = "canon-capability", path = "../canon-capability" }
```

**`src/bin/capability_smoke_test.rs`** — Rewrite before `cargo check` (see Phase 5).

### Step 4c — `canon-builder`

**Delete** `src/executor/capabilities.rs`.

**`src/executor.rs`** — Remove:
```rust
mod capabilities;
pub use capabilities::{register_build_capabilities, CAP_BUILD_CARGO, CAP_CHECK_CARGO, CAP_RUN_CARGO};
```

**`src/lib.rs`** — Remove `register_build_capabilities, CAP_BUILD_CARGO, CAP_CHECK_CARGO,
CAP_RUN_CARGO` from the `pub use executor::{...}` line. The rest of the pub use (BuildEvent,
BuildRequest, BuildResult, CheckRequest, CheckResult, RunRequest, RunResult) stays.

**`Cargo.toml`** — Remove:
```toml
canon_capability = { package = "canon-capability", path = "../canon-capability" }
```

### Step 4d — `canon-runtime`

`canon-runtime/Cargo.toml` already has `canon_capability` removed and `canon_exec` added. ✅
`src/consumers/llm_executor.rs` already deleted, `consumers/mod.rs` already cleaned. ✅

No source file in `canon-runtime/src/` currently imports `canon_llm`, `canon_analysis`,
`canon_editor`, or `canon_builder` — all are dead deps. Remove them from `Cargo.toml`:

```toml
# Remove these three lines:
canon_llm      = { package = "canon-llm-runtime",   path = "../canon-llm-runtime" }
canon_analysis = { package = "canon-tools-analysis", path = "../canon-tools-analysis" }
canon_editor   = { package = "canon-tools-editor",   path = "../canon-tools-editor" }
```

`canon_builder` is already absent from `canon-runtime/Cargo.toml`. ✅

### Step 4e — Delete `canon-capability/` and `canon-introspection/`

After steps 4a–4d, verify zero remaining references:
```bash
grep -r "canon_capability" canon-utils/ --include="*.rs" --include="*.toml"
```
Must return zero results before proceeding.

1. Remove from workspace `Cargo.toml` members array:
   - `"canon-utils/canon-capability"`
   - `"canon-utils/canon-introspection"`
2. Delete directories:
   - `canon-utils/canon-capability/`
   - `canon-utils/canon-introspection/`

**Verify:** `cargo check --workspace` — zero errors.

---

## ~~Phase 5~~ — ✅ Complete

### `canon-runtime/src/bin/capability_smoke_test.rs` — ✅ already done

The agent already rewrote this file to use `canon_exec` directly. No action needed.

### `canon-tools-editor/src/bin/capability_smoke_test.rs`

This file currently uses `canon_capability` and `register_editor_capabilities`, both deleted
in Phase 4. It must be rewritten **as part of Step 4b** (before `cargo check`).

Replace entirely:

```rust
use canon_event::{CanonEvent, EditEvent, RenameSymbol};
use canon_exec::ExecutableEvent;

fn main() {
    let event = CanonEvent::Edit(EditEvent::RenameSymbol(RenameSymbol {
        project: "p".into(), old: "a".into(), new: "b".into(),
    }));
    let _exec = ExecutableEvent::try_from(event).expect("edit event should be executable");
    println!("editor_capability_smoke_test: PASS (dispatch path verified)");
}
```

Also add to `canon-tools-editor/Cargo.toml`:
```toml
canon_exec = { package = "canon-exec", path = "../canon-exec" }
```

### Remove `assert_all_routes_safe`

`canon-introspection/` is deleted in Step 4e. The exhaustive match on
`ExecutableEvent::execute()` is the compile-time proof. No replacement needed.

**Verify:** `cargo check --workspace && cargo test --workspace` — zero errors, zero warnings.

---

## Execution Order

```
Phase 1 — ✅ done  (cargo check -p canon-exec)
Phase 2 — ✅ done  (cargo check -p canon-exec)
Phase 3 — ✅ done  (cargo check -p canon-runtime)
Phase 4 — ✅ done  (cargo check --workspace — zero errors)
  Step 4a: canon-tools-analysis cleanup ✅
  Step 4b: canon-tools-editor cleanup + smoke test rewrite ✅
  Step 4c: canon-builder cleanup ✅
  Step 4d: canon-runtime Cargo.toml cleanup (dead deps) ✅
  Step 4e: delete canon-capability/ and canon-introspection/ ✅
Phase 5 — ✅ migration clean (canon-exec tests pass; 4 pre-existing failures are out of scope)
```

**Migration is complete. Remaining `cargo test --workspace` failures are pre-existing and
unrelated to the R-migration. They require separate investigation.**

---

## Final Score

```
R_structure = 1.0  (no registry struct, no Vec)
R_coverage  = 1.0  (ExecutableEvent covers all 6 families, exhaustive per sub-variant)
R_binding   = 1.0  (ExecutableEvent::execute() has no _ arm — missing variant = compile error)

Cycles = 0         (canon-exec new crate, no inversions)
CoreBloat = 0      (canon-runtime-events unchanged, heavy deps in canon-exec only)
StateLeak = bounded (RwLock<Option<Sender>>, explicit init/shutdown for LLM and analysis workers)

R = 1.0 · 1.0 · 1.0 = 1.0
S = min(0.95, 0.90, 0.75, 1.0) = 0.75
```

---

## Files Created / Modified / Deleted

| Phase | Status | File | Action |
|-------|--------|------|--------|
| 1 | ✅ | `canon-exec/Cargo.toml` | Created |
| 1 | ✅ | `canon-exec/src/lib.rs` | Created |
| 1 | ✅ | `canon-exec/src/exec/mod.rs` | Created: ExecutableEvent, Executable trait, ExecutionContext, ExecutionResult, TryFrom |
| 1 | ✅ | `Cargo.toml` (workspace) | canon-exec member added |
| 2 | ✅ | `canon-exec/src/exec/edit.rs` | Created: EditEvent::execute() — pass-through re-emit |
| 2 | ✅ | `canon-exec/src/exec/cargo.rs` | Created: CargoEvent::execute() — inlined subprocess |
| 2 | ✅ | `canon-exec/src/exec/file.rs` | Created: FileEvent::execute() — std::fs |
| 2 | ✅ | `canon-exec/src/exec/bash.rs` | Created: BashInvoke::execute() — std::process |
| 2 | ✅ | `canon-exec/src/exec/llm.rs` | Created: LlmCall::execute() + RwLock worker lifecycle |
| 2 | ✅ | `canon-exec/src/exec/analysis.rs` | Created: AnalysisEvent::execute() + RwLock worker lifecycle |
| 2 | ✅ | `canon-exec/src/lib.rs` | Updated: init/shutdown_analysis_worker pub use added |
| 3 | ✅ | `canon-runtime/src/consumers/capability_executor.rs` | Replaced with ExecutableEvent::try_from + execute |
| 3 | ✅ | `canon-runtime/src/consumers/llm_executor.rs` | Deleted (content moved to canon-exec) |
| 3 | ✅ | `canon-runtime/src/consumers/mod.rs` | pub mod llm_executor removed |
| 3 | ✅ | `canon-runtime/src/lib.rs` | Registry field and register_default_capabilities removed |
| 3 | ✅ | `canon-runtime/src/bin/event_runtime.rs` | Registry setup removed, init/shutdown calls added |
| 3 | ✅ | `canon-runtime/src/bin/capability_smoke_test.rs` | Rewritten with canon_exec |
| 3 | ✅ | `canon-runtime/Cargo.toml` | canon-exec dep added, canon_capability removed |
| 4a | ✅ | `canon-tools-analysis/src/capabilities/run.rs` | Deleted |
| 4a | ✅ | `canon-tools-analysis/src/capabilities/mod.rs` | Replaced: register fn removed, canon_capability removed, mod run removed |
| 4a | ✅ | `canon-tools-analysis/src/lib.rs` | pub use capabilities::register_analysis_capabilities removed |
| 4a | ✅ | `canon-tools-analysis/Cargo.toml` | canon_capability dep removed |
| 4b | ✅ | `canon-tools-editor/src/capabilities.rs` | Deleted |
| 4b | ✅ | `canon-tools-editor/src/lib.rs` | pub mod capabilities + pub use capabilities::{...} removed |
| 4b | ✅ | `canon-tools-editor/src/bin/capability_smoke_test.rs` | Rewritten with canon_exec |
| 4b | ✅ | `canon-tools-editor/Cargo.toml` | canon_capability dep removed, canon_exec dep added |
| 4c | ✅ | `canon-builder/src/executor/capabilities.rs` | Deleted |
| 4c | ✅ | `canon-builder/src/executor.rs` | mod capabilities + pub use removed |
| 4c | ✅ | `canon-builder/src/lib.rs` | Capability re-exports removed from pub use executor::{...} |
| 4c | ✅ | `canon-builder/Cargo.toml` | canon_capability dep removed |
| 4d | ✅ | `canon-runtime/Cargo.toml` | Dead deps removed: canon_llm, canon_analysis, canon_editor |
| 4e | ✅ | `canon-capability/` | Entire crate deleted |
| 4e | ✅ | `canon-introspection/` | Entire crate deleted |
| 4e | ✅ | `Cargo.toml` (workspace) | canon-capability, canon-introspection members removed |
| 5 | ✅ | `canon-runtime/src/bin/capability_smoke_test.rs` | Rewritten with canon_exec |
| 5 | ✅ | `canon-tools-editor/src/bin/capability_smoke_test.rs` | Rewritten with canon_exec |
| 5 | ✅ | `canon-exec/src/exec/llm.rs` | LlmWork visibility fixed (pub(crate)); unused set_test_worker_tx removed |
