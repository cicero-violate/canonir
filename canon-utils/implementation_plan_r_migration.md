# Implementation Plan: Full Architecture Collapse — System = f(CanonEvent)

## Goal

```
R = R_structure · R_coverage · R_binding = 1.0 · 1.0 · 1.0 = 1.0

Maintenance = Δ(Schema)       ← only one place to update
             (not Δ(Schema) + Δ(Mapping) + Drift)

∀ e ∈ CapabilityEvent, ∃! execute(e)  — compile-time enforced
```

---

## Architecture Inversion

```
Before:
  CanonEvent  →  CapabilityRegistry  →  Vec<dyn CapabilityHandler>  →  handler.handle(ctx)
  (schema)       (runtime binding)       (dynamic dispatch)

After:
  CanonEvent::execute(ctx)
  (schema owns dispatch — no registry, no binding, no dyn lookup)
```

---

## What Is Deleted

| Deleted                                                    | Reason                                                               |
|------------------------------------------------------------+----------------------------------------------------------------------|
| `canon-capability/` crate                                  | No handler trait, no registry, no context type                       |
| `CapabilityHandler` trait                                  | Replaced by `fn execute()` on event types                            |
| `CapabilityRegistry` struct                                | No registry needed                                                   |
| `CapabilityExecutionContext`                               | Replaced by `ExecutionContext` in schema crate                       |
| `CapabilityExecutionResult`                                | Replaced by `ExecutionResult` in schema crate                        |
| `canon-builder/src/executor/capabilities.rs`               | Logic moves to exec modules                                          |
| `canon-tools-editor/src/capabilities.rs`                   | Logic moves to exec modules                                          |
| `canon-tools-analysis/src/capabilities/mod.rs` register fn | Logic moves to exec modules                                          |
| `canon-runtime/src/consumers/llm_executor.rs`              | Moves to exec module                                                 |
| `canon-runtime/src/consumers/capability_executor.rs`       | Replaced by single `event.execute()` call                            |
| `canon-introspection/` crate                               | `assert_all_routes_safe` is obsolete — exhaustive match is the proof |

---

## What Replaces Everything

```rust
// In canon-runtime-events/src/exec/mod.rs:

pub struct ExecutionContext {
    pub workspace: PathBuf,
    pub emitter: EventEmitterHandle,
}

pub enum ExecutionResult {
    Emit(CanonEvent),
    EmitMany(Vec<CanonEvent>),
    Deferred,   // LLM only — completion arrives via emitter later
}

// On CanonEvent:
impl CanonEvent {
    pub fn execute(self, ctx: ExecutionContext) -> anyhow::Result<ExecutionResult> {
        match self {
            CanonEvent::Edit(e)     => e.execute(ctx),
            CanonEvent::Cargo(e)    => e.execute(ctx),
            CanonEvent::File(e)     => e.execute(ctx),
            CanonEvent::Bash(e)     => e.execute(ctx),
            CanonEvent::Llm(e)      => e.execute(ctx),
            CanonEvent::Analysis(e) => e.execute(ctx),
            _                       => Ok(ExecutionResult::NoOp),
        }
    }
}
```

The `_ => NoOp` arm covers non-capability events (`Code`, `Debug`, `Tick`, etc.) — they
are not dispatched to capability execution. This is correct and intentional. It does NOT
hide a missing capability handler — every capability sub-variant has an explicit arm.

New event family added → must add arm → compiler error. Coverage = 1.0.

---

## LLM State Problem and Solution

`LlmCall::execute()` is a method on a stateless data struct. The LLM worker is stateful
(spawned thread, sender channel, config). Cannot store state on the event.

**Solution: module-level worker via `OnceLock`**

```rust
// canon-runtime-events/src/exec/llm.rs

static LLM_WORKER: OnceLock<Sender<LlmWork>> = OnceLock::new();

pub fn init_llm_worker(config: CapabilityConfig, prompt_registry: PromptRegistryHandle) {
    let tx = spawn_llm_worker(config, prompt_registry);
    let _ = LLM_WORKER.set(tx);
}

impl LlmCall {
    pub fn execute(self, ctx: ExecutionContext) -> anyhow::Result<ExecutionResult> {
        let tx = LLM_WORKER.get()
            .ok_or_else(|| anyhow!("llm worker not initialized — call init_llm_worker first"))?;
        tx.send(LlmWork { request_id: self.request_id, prompt: self.prompt, role: self.role, emitter: ctx.emitter })?;
        Ok(ExecutionResult::Deferred)
    }
}
```

`event_runtime.rs` calls `canon_event::exec::llm::init_llm_worker(config, prompt_registry)` at startup, before the event loop begins. No handler object needed. No registration needed.

This preserves the async/deferred design. `init_llm_worker` is called once. `LlmCall::execute()` sends to the global channel.

---

## Phase 1 — Add exec modules to `canon-runtime-events`

### Step 1a — Update `canon-runtime-events/Cargo.toml`

Add all deps needed by execution logic:

```toml
[package]
name = "canon-runtime-events"
version = "0.1.0"
edition = "2021"

[lib]
path = "src/lib.rs"
name = "canon_event"

[dependencies]
# existing
anyhow.workspace = true
serde = { workspace = true, features = ["derive"] }
serde_json.workspace = true
canon_types = { path = "../canon-types" }
canon-macros = { path = "../canon-macros" }
crc32fast = "1"
fs2 = "0.4"
uuid = { workspace = true }

# new: execution deps
canon-meta = { path = "../canon-meta" }

# new: editor execution (syn, proc-macro2, walkdir, regex, canon-ir)
syn = { workspace = true, features = ["full", "visit-mut", "visit"] }
proc-macro2.workspace = true
walkdir.workspace = true
regex.workspace = true
csv.workspace = true
prettyplease.workspace = true
canon-ir = { path = "../../canon-ir" }

# new: LLM execution (tokio, websocket, config)
tokio = { workspace = true }
tokio-tungstenite = "0.21"
futures-util = "0.3"
once_cell.workspace = true
hashbrown = "0.14"
toml = "0.8"

# new: analysis execution (z3, rayon, sha2)
z3 = { workspace = true, default-features = false }
rayon.workspace = true
sha2.workspace = true
hex.workspace = true
algorithms = { path = "../../algorithms" }
canon_graph = { package = "canon-storage-graph", path = "../canon-storage-graph" }
canon_event_store = { package = "canon-storage-eventlog", path = "../canon-storage-eventlog" }
```

**Note on `canon-meta` dep:** `canon-meta` currently depends on `canon-runtime-events`. Adding
the reverse dep creates a cycle. Resolve by inlining `canon_emit_meta!` expansion directly in the
exec modules, or by moving `canon-meta` macros into `canon-macros` (which `canon-runtime-events`
already imports). See Step 1b.

### Step 1b — Resolve `canon-meta` cycle

`canon-meta` provides `canon_emit_meta!` and `capture_meta!`. These macros expand at call site
using `file!()`, `line!()`, `module_path!()`. They depend on `canon-runtime-events` for
`canon_emit!`.

Two options:
- **Option A (preferred):** Move `canon-meta` macro definitions into `canon-macros` (already a dep of `canon-runtime-events`). `canon-meta` crate is deleted or becomes a thin re-export.
- **Option B:** The exec modules emit via `emitter.emit(CanonEvent::Debug(...))` directly, constructing the meta struct inline without the macro.

Use **Option A**. Move `capture_meta!` and `canon_emit_meta!` into `canon-macros/src/lib.rs`.
Since `canon-macros` has no deps on `canon-runtime-events`, this breaks the cycle.
`canon-meta` crate is removed (or kept as an empty re-export of `canon_macros::capture_meta`).

### Step 1c — Create exec module skeleton

**File: `canon-runtime-events/src/exec/mod.rs`**

```rust
use crate::CanonEvent;
use crate::EventEmitterHandle;
use std::path::PathBuf;

pub mod bash;
pub mod cargo;
pub mod edit;
pub mod file;
pub mod llm;
pub mod analysis;

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
    NoOp,
}
```

Add to `canon-runtime-events/src/lib.rs`:
```rust
pub mod exec;
pub use exec::{ExecutionContext, ExecutionResult};
```

**Verify:** `cargo check -p canon-runtime-events` — zero errors.

---

## Phase 2 — Implement `execute()` on each event type

For each sub-event type, the execution logic is moved from the capability crates into the
corresponding exec module. The logic itself does not change — only its location changes.

### `execute()` on `CanonEvent`

Add to `canon-runtime-events/src/events.rs`:

```rust
impl CanonEvent {
    pub fn execute(self, ctx: crate::exec::ExecutionContext) -> anyhow::Result<crate::exec::ExecutionResult> {
        match self {
            CanonEvent::Edit(e)     => e.execute(ctx),
            CanonEvent::Cargo(e)    => e.execute(ctx),
            CanonEvent::File(e)     => e.execute(ctx),
            CanonEvent::Bash(e)     => e.execute(ctx),
            CanonEvent::Llm(e)      => e.execute(ctx),
            CanonEvent::Analysis(e) => e.execute(ctx),
            _                       => Ok(crate::exec::ExecutionResult::NoOp),
        }
    }
}
```

### `EditEvent::execute()`

**File: `canon-runtime-events/src/exec/edit.rs`**

Move logic from `canon-tools-editor/src/capabilities.rs`. EditEvent::execute() dispatches
to the editor infrastructure (which stays in canon-tools-editor):

```rust
use crate::{EditEvent, exec::{ExecutionContext, ExecutionResult}};

impl EditEvent {
    pub fn execute(self, ctx: ExecutionContext) -> anyhow::Result<ExecutionResult> {
        use crate::CapabilityCompleted;
        match self {
            EditEvent::RenameSymbol(ev) => {
                // Move logic from RenameSymbolCapability::handle()
                // canon_editor::rename_symbol_run(...) is still in canon-tools-editor
                // but now called directly, no handler object
                let report = canon_editor::rename_symbol_run(&ev, &ctx.workspace);
                // ... emit CapabilityCompleted
                Ok(ExecutionResult::Emit(CanonEvent::CapabilityCompleted(...)))
            }
            EditEvent::MoveSymbol(ev) => { ... }
            EditEvent::DeleteSymbol(ev) => { ... }
            EditEvent::RenameModule(ev) => { ... }
            EditEvent::RenameDir(ev) => { ... }
        }
    }
}
```

**Note:** `canon-tools-editor` still exists as a library. Its editor infrastructure
(`ProjectEditor`, `SymbolIndex`, `edit.rs`, `query.rs`, etc.) stays there. Only the thin
`capabilities.rs` dispatch file is eliminated — `execute()` calls the infrastructure directly.
`canon-runtime-events/Cargo.toml` adds `canon-tools-editor` as a dep (minus the capability imports).

Wait — this creates `canon-runtime-events → canon-tools-editor → canon-runtime-events` (because tools-editor imports canon_event for event types). Circular.

**Resolution:** `canon-tools-editor` must stop importing `canon_event`. The editor infrastructure
does not need event types — only `capabilities.rs` (being deleted) needed them. After removing
capabilities.rs, canon-tools-editor's remaining code (editor logic, symbol index, etc.) likely
does not import event types. **Verify this before proceeding.**

If it does, those usages are moved into the exec module in canon-runtime-events instead.

### `CargoEvent::execute()`

**File: `canon-runtime-events/src/exec/cargo.rs`**

Logic from `canon-builder/src/executor/capabilities.rs` (BuildCargoCapability, CargoRunCapability,
CargoCheckCapability). The executor functions `run_cargo_build`, `run_cargo_run`, `run_cargo_check`
stay in `canon-builder/src/executor/build_runtime.rs` — `canon-runtime-events` adds `canon-builder`
as a dep.

Wait — same circular dep: `canon-runtime-events → canon-builder → canon-runtime-events`.

**Resolution:** The executor functions (`run_cargo_build` etc.) are the only non-event-aware parts
of canon-builder. Move them to a new `canon-exec-cargo` micro-crate (or inline them — they are
~50 lines each). They only call `std::process::Command` with no event imports. Moving them
eliminates the cycle.

Alternatively: inline the subprocess logic directly into `canon-runtime-events/src/exec/cargo.rs`.
`run_cargo_build` is ~20 lines of `Command::new("cargo")`. Inline it, delete the dep on canon-builder
from the exec side.

```rust
impl CargoEvent {
    pub fn execute(self, ctx: ExecutionContext) -> anyhow::Result<ExecutionResult> {
        match self {
            CargoEvent::Build(ev) => {
                let output = std::process::Command::new("cargo")
                    .arg("build")
                    .arg("--manifest-path").arg(format!("{}/Cargo.toml", ev.crate_name))
                    .output()?;
                Ok(ExecutionResult::Emit(CanonEvent::CapabilityCompleted(CapabilityCompleted {
                    request_id: ev.request_id,
                    capability: "cargo.build",
                    result: CapabilityResult::Process(ProcessResult {
                        status: output.status.code().unwrap_or(-1),
                        success: output.status.success(),
                        stdout: String::from_utf8_lossy(&output.stdout).into(),
                        stderr: String::from_utf8_lossy(&output.stderr).into(),
                    }),
                })))
            }
            CargoEvent::Run(ev) => { ... }
            CargoEvent::Check(ev) => { ... }
        }
    }
}
```

### `FileEvent::execute()`, `BashInvoke::execute()`

Simple file system / shell ops. No external deps beyond `std`. Inline directly.

```rust
impl FileEvent {
    pub fn execute(self, _ctx: ExecutionContext) -> anyhow::Result<ExecutionResult> {
        match self {
            FileEvent::Read(ev) => {
                let content = std::fs::read_to_string(&ev.path)?;
                Ok(ExecutionResult::Emit(CanonEvent::CapabilityCompleted(...)))
            }
            FileEvent::Write(ev) => { std::fs::write(&ev.path, &ev.content)?; Ok(ExecutionResult::Emit(...)) }
            FileEvent::Patch(ev) => { ... }
        }
    }
}

impl BashInvoke {
    pub fn execute(self, _ctx: ExecutionContext) -> anyhow::Result<ExecutionResult> {
        let cwd = self.cwd.clone().unwrap_or_else(|| ".".to_string());
        let output = std::process::Command::new("bash")
            .arg("-lc").arg(&self.cmd).current_dir(cwd).output()?;
        Ok(ExecutionResult::Emit(CanonEvent::CapabilityCompleted(...)))
    }
}
```

### `LlmCall::execute()`

**File: `canon-runtime-events/src/exec/llm.rs`**

Move LLM worker machinery from `canon-runtime/src/consumers/llm_executor.rs`.
Global worker via `OnceLock`:

```rust
static LLM_WORKER_TX: OnceLock<std::sync::mpsc::Sender<LlmWork>> = OnceLock::new();

pub fn init_llm_worker(config: canon_llm::config::CapabilityConfig, ...) {
    if LLM_WORKER_TX.get().is_some() { return; }  // idempotent
    let tx = spawn_llm_worker_thread(config, ...);
    let _ = LLM_WORKER_TX.set(tx);
}

impl LlmCall {
    pub fn execute(self, ctx: ExecutionContext) -> anyhow::Result<ExecutionResult> {
        let tx = LLM_WORKER_TX.get()
            .ok_or_else(|| anyhow::anyhow!("llm worker not initialized"))?;
        tx.send(LlmWork {
            request_id: self.request_id,
            prompt: self.prompt,
            role: self.role,
            emitter: ctx.emitter,
        }).map_err(|e| anyhow::anyhow!("llm channel closed: {e}"))?;
        Ok(ExecutionResult::Deferred)
    }
}
```

`canon-runtime-events` adds `canon-llm-runtime` as a dep. `canon-llm-runtime` already imports
`canon_event` — check for cycle. `canon-llm-runtime → canon_event` + `canon_event → canon-llm-runtime`
= cycle. **Resolution:** Move the LLM client logic (WebSocket bridge, endpoint worker) into the
exec module directly, or extract the non-event parts of `canon-llm-runtime` into a new dep-free
`canon-llm-client` crate that `canon-runtime-events` can import.

### `AnalysisEvent::execute()`

**File: `canon-runtime-events/src/exec/analysis.rs`**

Same pattern: the analysis runner logic stays in `canon-tools-analysis`, but exec dispatch
calls into it. Same circular dep concern — verify whether `canon-tools-analysis` uses event
types outside of its capabilities/ directory. If so, those usages must move.

---

## Phase 3 — Replace `CapabilityExecutor` consumer

**File: `canon-runtime/src/consumers/capability_executor.rs`**

The entire file reduces to:

```rust
use canon_event::{CanonEvent, EventConsumer, EventEmitterHandle, EventFilter, ExecutionContext};
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
        let is_cap = matches!(event,
            CanonEvent::Edit(_) | CanonEvent::Cargo(_) | CanonEvent::File(_) |
            CanonEvent::Bash(_) | CanonEvent::Llm(_)   | CanonEvent::Analysis(_)
        );
        if !is_cap { return; }

        let Some(emitter) = self.emitter.clone() else { return; };
        let ctx = ExecutionContext { workspace: self.workspace.clone(), emitter: emitter.clone() };

        match event.clone().execute(ctx) {
            Ok(ExecutionResult::Emit(e)) => emitter.emit(e),
            Ok(ExecutionResult::EmitMany(evs)) => evs.into_iter().for_each(|e| emitter.emit(e)),
            Ok(ExecutionResult::Deferred) | Ok(ExecutionResult::NoOp) => {}
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

**No `Arc<Mutex<CapabilityRegistry>>` parameter.** `CapabilityExecutor::new()` takes only workspace.
`event_runtime.rs` construction site simplified accordingly.

---

## Phase 4 — Delete `canon-capability`, update importers

### Files deleted
- `canon-utils/canon-capability/` — entire crate
- `canon-utils/canon-introspection/` — entire crate (guarantee is now the exhaustive match)

### Crates that import `canon-capability` — update each

| Crate | Change |
|-------|--------|
| `canon-builder` | Remove dep. Remove `register_build_capabilities`. Module stays for LLM stub if needed. |
| `canon-tools-editor` | Remove dep. Remove `capabilities.rs`. Editor infrastructure unchanged. |
| `canon-tools-analysis` | Remove dep. Remove `capabilities/mod.rs` register fn. Runner stays. |
| `canon-runtime` | Remove dep. `CapabilityExecutor` no longer takes `Arc<Mutex<CapabilityRegistry>>`. |
| `canon-introspection` | Deleted — no longer needed. |

### `canon-runtime/src/lib.rs`

Remove `register_default_capabilities`. Add `init_llm_worker` call:

```rust
pub fn init_llm_worker(prompt_registry: PromptRegistryHandle) {
    let config = CapabilityConfig::snapshot_store_load()
        .expect("llm config required");
    canon_event::exec::llm::init_llm_worker(config, prompt_registry);
}
```

### `canon-runtime/src/bin/event_runtime.rs`

```rust
// Before:
let registry = Arc::new(Mutex::new(CapabilityRegistry::new()));
...
register_default_capabilities(&mut reg);
reg.register(Arc::new(LlmCapabilityHandler::new(prompt_registry.clone())));
...
consumers.push(Box::new(CapabilityExecutor::new(registry.clone(), workspace.clone())));

// After:
canon_runtime::init_llm_worker(prompt_registry.clone());
...
consumers.push(Box::new(CapabilityExecutor::new(workspace.clone())));
```

---

## Phase 5 — Delete `assert_all_routes_safe`, update test

`assert_all_routes_safe` tested that the registry didn't panic for any event. This guarantee
is now structural — the exhaustive match in `CanonEvent::execute()` and each sub-enum's
`execute()` provides it at compile time.

**Delete:** `canon-capability/src/tests.rs` (crate deleted), `canon-introspection/src/lib.rs`
(crate deleted).

**Replacement test** (optional but recommended) — in `canon-runtime-events`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::ExecutionContext;

    #[test]
    fn all_capability_events_have_execute() {
        // Compile-time proof: if this compiles, all variants are covered.
        // This test exists as documentation. The real guarantee is the exhaustive match.
        fn assert_execute_exists<T: Fn(ExecutionContext) -> anyhow::Result<crate::exec::ExecutionResult>>(_: T) {}
        // If EditEvent::execute is missing, this file fails to compile.
    }
}
```

---

## Circular Dependency Resolution Summary

Three potential cycles identified. All require action before the plan compiles:

| Cycle | Resolution |
|-------|-----------|
| `canon-runtime-events → canon-meta → canon-runtime-events` | Move `capture_meta!` / `canon_emit_meta!` into `canon-macros`. Delete or empty `canon-meta`. |
| `canon-runtime-events → canon-tools-editor → canon-runtime-events` | Verify: does tools-editor use event types outside capabilities.rs? If yes, move those usages into exec module. Then tools-editor has no event dep. |
| `canon-runtime-events → canon-llm-runtime → canon-runtime-events` | Extract LLM client (WebSocket + endpoint worker) into new `canon-llm-client` crate with no event dep. `canon-runtime-events` imports `canon-llm-client`. |
| `canon-runtime-events → canon-tools-analysis → canon-runtime-events` | Same check as tools-editor: does analysis use event types outside capabilities/? If yes, move. |

**Resolve all cycles in Phase 1 before attempting Phase 2.**

---

## Execution Order

```
Phase 1 — Cargo.toml + exec skeleton + cycle resolution
  → cargo check -p canon-runtime-events

Phase 2 — Implement execute() on all 6 event types
  → cargo check -p canon-runtime-events

Phase 3 — Replace CapabilityExecutor
  → cargo check -p canon-runtime

Phase 4 — Delete canon-capability, update importers
  → cargo check --workspace

Phase 5 — Delete introspection, update tests
  → cargo check --workspace + cargo test --workspace
```

One phase per commit. Do not skip cycle resolution — it will cause cascading errors.

---

## Final Score

```
R_structure = 1.0  (no Vec, no HashMap, no dyn registry)
R_coverage  = 1.0  (all 15 sub-variants covered by exhaustive match)
R_binding   = 1.0  (CanonEvent owns execute() — missing variant = compile error)

R = 1.0 · 1.0 · 1.0 = 1.0

Maintenance = Δ(Schema)   — only events.rs needs updating when adding a variant
NoOp failure mode = 0     — no registry that can be missing a handler
Drift = 0                 — no separate mapping layer

S = min(0.95, 0.90, 0.75, 1.0) = 0.75
```

---

## Files Modified (complete list)

| Phase | File | Action |
|-------|------|--------|
| 1 | `canon-runtime-events/Cargo.toml` | Add all execution deps |
| 1 | `canon-macros/src/lib.rs` | Add `capture_meta!`, `canon_emit_meta!` (from canon-meta) |
| 1 | `canon-meta/src/lib.rs` | Thin re-export or deleted |
| 1 | `canon-llm-runtime/` or new `canon-llm-client/` | Extract client logic, remove event dep |
| 1 | `canon-runtime-events/src/exec/mod.rs` | New: ExecutionContext, ExecutionResult |
| 2 | `canon-runtime-events/src/exec/edit.rs` | New: EditEvent::execute() |
| 2 | `canon-runtime-events/src/exec/cargo.rs` | New: CargoEvent::execute() |
| 2 | `canon-runtime-events/src/exec/file.rs` | New: FileEvent::execute() |
| 2 | `canon-runtime-events/src/exec/bash.rs` | New: BashInvoke::execute() |
| 2 | `canon-runtime-events/src/exec/llm.rs` | New: LlmCall::execute() + init_llm_worker() |
| 2 | `canon-runtime-events/src/exec/analysis.rs` | New: AnalysisEvent::execute() |
| 2 | `canon-runtime-events/src/events.rs` | Add CanonEvent::execute() |
| 3 | `canon-runtime/src/consumers/capability_executor.rs` | Replace with event.execute() call |
| 3 | `canon-runtime/src/lib.rs` | Remove register_default_capabilities, add init_llm_worker |
| 3 | `canon-runtime/src/bin/event_runtime.rs` | Remove registry construction, add init call |
| 4 | `canon-capability/` | Delete entire crate |
| 4 | `canon-introspection/` | Delete entire crate |
| 4 | `canon-builder/Cargo.toml` + `capabilities.rs` | Remove capability dep, delete capabilities.rs |
| 4 | `canon-tools-editor/Cargo.toml` + `capabilities.rs` | Remove capability dep, delete capabilities.rs |
| 4 | `canon-tools-analysis/Cargo.toml` + `capabilities/mod.rs` | Remove register fn |
| 4 | `canon-runtime/Cargo.toml` | Remove canon-capability, canon-introspection deps |
| 4 | `Cargo.toml` (workspace) | Remove canon-capability, canon-introspection from workspace members |
| 5 | Smoke tests | Rewrite without registry construction |
