# Implementation Plan: Capability Collapse + Canon Introspection

## Math

G = min(C, Z, I, A) → target G = 1

| Variable | Meaning | Current |
|---|---|---|
| C | coverage of all CanonEvent variants | 0.6 — new variants added, not all wired |
| Z | zero-maintenance | 0 — tests still manually list events |
| I | invariant enforcement | 0 — routing not verified for all variants |
| A | adaptability to change | 0.7 — new structs exist, no Default |

---

## Current State (Verified)

`CapabilityRequested` was **already removed** from `events.rs` and `lib.rs`.
New typed event families are **already in `events.rs`**: `CargoEvent`, `FileEvent`, `BashInvoke`, `LlmCall`, `AnalysisEvent`.

The codebase is mid-migration. The build is broken because:
- Emitters in `canon-act`, `canon-plan`, `canon-runtime` still reference the removed variant
- `decode.rs` references it
- Storage reader (`extract_capability_request`) references it
- Tests and smoke tests reference it

**Answer to agent:** Continue forward. Do not revert. The new event types exist — complete the wiring.

---

## Phase Sequence

```
Phase 8 Fix     → repair broken emitters, storage, executor, remove decode
canon-introspect → macro extension + Default + sample_all() + assert_all_routes_safe()
Integration     → replace manual test with introspection loop
```

---

# Part 1 — Phase 8: Complete the Migration

## Step 1 — Fix Emitters: `canon-act/src/lib.rs`

Lines 228, 292, 359 emit `CanonEvent::CapabilityRequested`. Replace each with the typed event that matches the action being dispatched at that site.

Read those lines first to identify the capability name string and args, then apply the corresponding replacement:

| Old capability name | Typed replacement |
|---|---|
| `"file.write"` | `CanonEvent::File(FileEvent::Write(FileWrite { path, content }))` |
| `"file.read"` | `CanonEvent::File(FileEvent::Read(FileRead { path }))` |
| `"file.patch"` | `CanonEvent::File(FileEvent::Patch(FilePatch { path, old, new }))` |
| `"bash"` | `CanonEvent::Bash(BashInvoke { cmd, cwd })` |
| `"cargo.build"` | `CanonEvent::Cargo(CargoEvent::Build(CargoBuild { crate_name }))` |
| `"cargo.check"` | `CanonEvent::Cargo(CargoEvent::Check(CargoCheck { crate_name }))` |
| `"cargo.run"` | `CanonEvent::Cargo(CargoEvent::Run(CargoRun { crate_name, bin, args }))` |
| `"llm.call"` | `CanonEvent::Llm(LlmCall { prompt, role })` |
| `"analysis.run"` | `CanonEvent::Analysis(AnalysisEvent::Run(AnalysisRun { crate_name, batch_id }))` |
| `"analysis.workspace"` | `CanonEvent::Analysis(AnalysisEvent::Workspace(AnalysisWorkspace {}))` |

Remove `use canon_event::CapabilityRequested` import from the file.

## Step 2 — Fix Emitters: `canon-plan/src/lib.rs` and `canon-runtime/src/lib.rs`

Same substitution table as Step 1. Read each `CapabilityRequested` construction site to identify the name, then replace. Remove the imports.

## Step 3 — Fix Storage Reader: `canon-storage-eventlog/src/reader.rs`

The functions `extract_capability_request` and `parse_capability_request_value` parse tlog JSON into `CapabilityRequested`. Since `CapabilityRequested` no longer exists in the type system:

- Rename `extract_capability_request` → `extract_capability_event`
- Change return type from `Option<CapabilityRequested>` → `Option<serde_json::Value>` (raw JSON)
- Existing tlog files on disk may contain old `CapabilityRequested` JSON — the reader should return the raw payload for legacy inspection rather than attempting typed deserialization
- Update callers of these functions (scan `canon-runtime/src/bin/event_runtime.rs` and any other consumers of the eventlog reader for `extract_capability_request` usage)

## Step 4 — Fix Capability Executor: `canon-runtime/src/consumers/capability_executor.rs`

The executor still guards on `CanonEvent::CapabilityRequested`. Replace:

```rust
// Before:
fn filter(&self) -> EventFilter { EventFilter::CapabilityOnly }

fn on_event(&mut self, event: &CanonEvent) {
    let CanonEvent::CapabilityRequested(request) = event else { return; };
    ...
    let result = registry.route(ctx);
}
```

```rust
// After:
fn filter(&self) -> EventFilter { EventFilter::All }

fn on_event(&mut self, event: &CanonEvent) {
    let is_capability_event = matches!(
        event,
        CanonEvent::Edit(_)
        | CanonEvent::Cargo(_)
        | CanonEvent::File(_)
        | CanonEvent::Bash(_)
        | CanonEvent::Llm(_)
        | CanonEvent::Analysis(_)
    );
    if !is_capability_event { return; }

    let ctx = CapabilityExecutionContext {
        workspace: self.workspace.clone(),
        event: event.clone(),
        emitter: self.emitter.clone(),
    };
    let result = match self.registry.lock() {
        Ok(registry) => registry.route(ctx),
        Err(err) => Err(anyhow!("registry lock poisoned: {err}")),
    };

    let outcome = match result {
        Ok(r) => r,
        Err(err) => CapabilityExecutionResult::Emit(
            CanonEvent::ErrorOccurred(new_error_occurred(
                "capability_execution", "capability_executor",
                err.to_string(), "error",
                serde_json::json!({ "event": format!("{:?}", event) }),
                None,
            ))
        ),
    };
    let Some(emitter) = self.emitter.as_ref() else { return; };
    match outcome {
        CapabilityExecutionResult::Emit(e) => emitter.emit(e),
        CapabilityExecutionResult::EmitMany(events) => { for e in events { emitter.emit(e); } }
        CapabilityExecutionResult::Deferred | CapabilityExecutionResult::NoOp => {}
    }
}
```

## Step 5 — Fix `registry.route()`: `canon-capability/src/registry.rs`

Replace `CapabilityRequested`-based routing with type-based dispatch. Remove the decode import.

```rust
pub fn route(&self, ctx: CapabilityExecutionContext) -> Result<CapabilityExecutionResult> {
    match &ctx.event {
        CanonEvent::Edit(_)
        | CanonEvent::Cargo(_)
        | CanonEvent::File(_)
        | CanonEvent::Bash(_)
        | CanonEvent::Llm(_)
        | CanonEvent::Analysis(_) => {
            for handler in self.map.values() {
                let result = handler.handle(ctx.clone())?;
                if !matches!(result, CapabilityExecutionResult::NoOp) {
                    return Ok(result);
                }
            }
            Ok(CapabilityExecutionResult::NoOp)
        }
        _ => Ok(CapabilityExecutionResult::NoOp),
    }
}
```

Remove `use crate::decode` and the decode call.

## Step 6 — Fix Capability Handlers: `canon-builder`, `canon-tools-analysis`, `llm_executor`

Each handler in these crates currently parses JSON args from `CapabilityRequested`. Replace with direct dispatch using `fn handle()`:

### `canon-builder/src/executor/capabilities.rs`

```rust
impl CapabilityHandler for CargoBuildCapability {
    fn name(&self) -> &'static str { "cargo.build" }
    fn handle(&self, ctx: CapabilityExecutionContext) -> Result<CapabilityExecutionResult> {
        match ctx.event {
            CanonEvent::Cargo(CargoEvent::Build(ev)) => {
                // execute cargo build for ev.crate_name
                // ... existing build logic, now using ev.crate_name instead of require_arg
            }
            _ => Ok(CapabilityExecutionResult::NoOp),
        }
    }
}
```

Apply same pattern for: `CargoRun`, `CargoCheck`, `FileRead`, `FileWrite`, `FilePatch`, `Bash`.

### `canon-tools-analysis/src/capabilities/`

```rust
match ctx.event {
    CanonEvent::Analysis(AnalysisEvent::Run(ev)) => { /* use ev.crate_name, ev.batch_id */ }
    CanonEvent::Analysis(AnalysisEvent::Workspace(_)) => { /* workspace analysis */ }
    _ => Ok(CapabilityExecutionResult::NoOp),
}
```

### `canon-runtime/src/consumers/llm_executor.rs`

```rust
match ctx.event {
    CanonEvent::Llm(LlmCall { prompt, role }) => { /* use prompt, role */ }
    _ => Ok(CapabilityExecutionResult::NoOp),
}
```

## Step 7 — Delete Dead Code

Once the build is clean after Steps 1–6:

| Target | Action |
|---|---|
| `canon-capability/src/decode.rs` | Delete file |
| `canon-capability/src/lib.rs` | Remove `pub mod decode;` |
| `canon-capability/src/trait.rs` | Remove `ArgSpec`, `ArgKind`, deprecated `fn execute()`, deprecated `fn schema()`. Clean `CapabilitySchema` to remove `args` field. |
| `canon-capability/src/lib.rs` | Remove `ArgKind`, `ArgSpec` from `pub use` |
| `canon-capability/src/registry.rs` | Remove `schemas()` method |
| `canon-tools-editor/src/bin/capability_smoke_test.rs` | Remove `CapabilityRequested` usage |

## Step 8 — Verify Build

```bash
cargo check --workspace
rg "CapabilityRequested" canon-utils --type rust   # must be zero
rg "require_arg" canon-utils --type rust            # must be zero
rg "ArgSpec|ArgKind" canon-utils --type rust        # must be zero
```

---

# Part 2 — canon-introspection

## Goal

A system where all possible events are automatically generated, executed, and validated with zero manual maintenance. When a new `CanonEvent` variant is added, it is automatically covered by the test suite with no code changes.

```
CanonEvent::sample_all()  →  registry.route(each)  →  assert safe(result)
```

## Architecture

```
canon-macros
  canon_event_struct! → adds Default derive (foundation)
  canon_event_enum!   → adds sample_all() generation (zero-maintenance)

canon-introspection (new crate)
  Sampleable trait
  assert_all_routes_safe(registry)

canon-capability/src/tests.rs
  replaces manual event list with introspection loop
```

---

## Step I.1 — Extend `canon_event_struct!` to Derive Default

**File: `canon-utils/canon-macros/src/lib.rs`**

Add `Default` to the derive list. This is the only change needed to give all 40+ event structs a free zero-value constructor.

```rust
#[macro_export]
macro_rules! canon_event_struct {
    ($name:ident { $($(#[$meta:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
        pub struct $name {
            $($(#[$meta])* pub $field: $ty),*
        }
    };
}
```

This gives every struct generated by the macro a `Default` impl where:
- `String` → `""`
- `u64`, `u32` → `0`
- `bool` → `false`
- `Option<T>` → `None`
- `Vec<T>` → `vec![]`
- `serde_json::Value` → `Value::Null`
- `PathBuf` → empty path

**Exception:** `Code { delta: EventDelta, state: RustcState }` is not generated by `canon_event_struct!` — it is a manual struct. Add `Default` impl manually in `events.rs`:
```rust
impl Default for Code {
    fn default() -> Self {
        // EventDelta and RustcState must each implement Default — add manually
        Self { delta: EventDelta::default(), state: RustcState::default() }
    }
}
```

Also add `Default` to `EventDelta` and `RustcState` in `canon-types/src/kernel_types.rs` manually.

## Step I.2 — Extend `canon_event_enum!` to Generate `sample_all()`

**File: `canon-utils/canon-macros/src/lib.rs`**

Extend the macro to generate a `sample_all()` method on every enum it produces. The method calls `Default::default()` on each inner type. When a new variant is added to `CanonEvent`, `sample_all()` automatically includes it — zero maintenance.

```rust
#[macro_export]
macro_rules! canon_event_enum {
    ($(#[$($attr:tt)*])* $enum_name:ident { $($variant:ident($inner:ty)),* $(,)? }) => {
        #[derive(Debug, Clone)]
        $(#[$($attr)*])*
        pub enum $enum_name {
            $($variant($inner)),*
        }

        impl $enum_name {
            /// Returns one sample instance of every variant using Default inner values.
            /// Auto-updated when new variants are added. Zero maintenance.
            pub fn sample_all() -> Vec<Self>
            where
                $($inner: Default),*
            {
                vec![
                    $(Self::$variant(<$inner>::default())),*
                ]
            }
        }
    };
}
```

**Result:** `CanonEvent::sample_all()` is now callable and returns one instance of every variant. `EditEvent::sample_all()`, `CargoEvent::sample_all()`, etc. also get this method automatically.

**Note on sub-enums:** `CanonEvent::Edit(EditEvent)` — `EditEvent::default()` must exist. Since `EditEvent` is also generated by `canon_event_enum!`, it gets `sample_all()` but NOT `Default`. Add `Default` to the sub-enum impls manually, or extend `canon_event_enum!` to also derive `Default` picking the first variant:

```rust
impl Default for EditEvent {
    fn default() -> Self { Self::RenameSymbol(Default::default()) }
}
impl Default for CargoEvent {
    fn default() -> Self { Self::Build(Default::default()) }
}
impl Default for FileEvent {
    fn default() -> Self { Self::Read(Default::default()) }
}
impl Default for AnalysisEvent {
    fn default() -> Self { Self::Run(Default::default()) }
}
```

These 4 impls go in `canon-runtime-events/src/events.rs`. They are the only manual defaults needed.

## Step I.3 — Create `canon-introspection` Crate

**New file: `canon-utils/canon-introspection/Cargo.toml`**

```toml
[package]
name = "canon-introspection"
version = "0.1.0"
edition = "2021"

[lib]
path = "src/lib.rs"
name = "canon_introspection"

[dependencies]
canon-runtime-events = { path = "../canon-runtime-events" }
canon-capability = { path = "../canon-capability" }
```

Add `"canon-utils/canon-introspection"` to workspace `Cargo.toml` members.

**New file: `canon-utils/canon-introspection/src/lib.rs`**

```rust
use canon_capability::{CapabilityExecutionContext, CapabilityExecutionResult, CapabilityRegistry};
use canon_event::CanonEvent;
use std::path::PathBuf;

/// Verify that every CanonEvent variant can be routed without panicking.
/// Call this from any test that registers capabilities.
///
/// Invariant: ∀ e ∈ CanonEvent, route(e) does not panic and result is printable.
pub fn assert_all_routes_safe(registry: &CapabilityRegistry) {
    for event in CanonEvent::sample_all() {
        let ctx = CapabilityExecutionContext {
            workspace: PathBuf::from("/tmp"),
            event,
            emitter: None,
        };
        let result = registry.route(ctx);
        // Must not panic. Result must be Debug-printable.
        let _ = format!("{:?}", result);
    }
}
```

No manual event list. No maintenance. When a new `CanonEvent` variant is added, `sample_all()` automatically includes it.

## Step I.4 — Replace Manual Test with Introspection Loop

**File: `canon-capability/src/tests.rs`**

Replace the manual `sample_capability_events()` function and `CapabilityRequested` event list entirely.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use canon_tools_editor::capabilities::register_editor_capabilities;
    use std::path::PathBuf;

    #[test]
    fn all_routes_safe() {
        let mut registry = CapabilityRegistry::new();
        register_editor_capabilities(&mut registry);
        // Add other capability registration here as needed.
        // The loop automatically covers all CanonEvent variants — zero maintenance.
        canon_introspection::assert_all_routes_safe(&registry);
    }
}
```

**Add `canon-introspection` to `canon-capability/Cargo.toml`:**
```toml
[dev-dependencies]
canon-introspection = { path = "../canon-introspection" }
```

---

# Execution Order

Do not skip steps. Do not proceed if the build is broken.

| Step | File(s) | Action | Compile check |
|---|---|---|---|
| 1 | `canon-act/src/lib.rs` | Replace `CapabilityRequested` emissions | ✓ |
| 2 | `canon-plan/src/lib.rs`, `canon-runtime/src/lib.rs` | Replace `CapabilityRequested` emissions | ✓ |
| 3 | `canon-storage-eventlog/src/reader.rs` | Update storage reader functions | ✓ |
| 4 | `canon-runtime/src/consumers/capability_executor.rs` | Update filter and guard | ✓ |
| 5 | `canon-capability/src/registry.rs` | Replace `route()` with type dispatch | ✓ |
| 6 | `canon-builder`, `canon-tools-analysis`, `llm_executor` | Handler typed dispatch | ✓ |
| 7 | `decode.rs`, `trait.rs`, `registry.rs` | Delete dead code | ✓ |
| 8 | grep verify | Zero references | ✓ |
| I.1 | `canon-macros/src/lib.rs` | Add `Default` to `canon_event_struct!` | ✓ |
| I.2 | `canon-macros/src/lib.rs` | Add `sample_all()` to `canon_event_enum!` | ✓ |
| I.2b | `canon-runtime-events/src/events.rs` | Add 4 sub-enum `Default` impls + `Code::default()` | ✓ |
| I.2c | `canon-types/src/kernel_types.rs` | Add `Default` to `EventDelta`, `RustcState` | ✓ |
| I.3 | `canon-introspection/` | Create crate | ✓ |
| I.4 | `canon-capability/src/tests.rs` | Replace manual list with `assert_all_routes_safe` | ✓ |

Steps 1–8 fix the broken build. Steps I.1–I.4 build the zero-maintenance invariant system. All independent of each other once the build is clean.

---

## Files Summary

| File | Action |
|---|---|
| `canon-act/src/lib.rs` | Replace 3 `CapabilityRequested` emissions |
| `canon-plan/src/lib.rs` | Replace 1 `CapabilityRequested` emission |
| `canon-runtime/src/lib.rs` | Replace 2 `CapabilityRequested` emissions |
| `canon-storage-eventlog/src/reader.rs` | Update `extract_capability_request` → raw value return |
| `canon-runtime/src/consumers/capability_executor.rs` | Filter → All with guard; update error handling |
| `canon-capability/src/registry.rs` | Type-based `route()`; remove decode import |
| `canon-builder/src/executor/capabilities.rs` | Typed dispatch in all handlers |
| `canon-tools-analysis/src/capabilities/` | Typed dispatch |
| `canon-runtime/src/consumers/llm_executor.rs` | Typed dispatch |
| `canon-capability/src/decode.rs` | **Delete** |
| `canon-capability/src/trait.rs` | Remove `ArgSpec`, `ArgKind`, deprecated methods |
| `canon-capability/src/lib.rs` | Remove dead re-exports |
| `canon-capability/src/registry.rs` | Remove `schemas()` |
| `canon-macros/src/lib.rs` | Add `Default` to `canon_event_struct!`; add `sample_all()` to `canon_event_enum!` |
| `canon-runtime-events/src/events.rs` | Add 4 sub-enum `Default` impls + `Code::default()` |
| `canon-types/src/kernel_types.rs` | Add `Default` to `EventDelta`, `RustcState` |
| `canon-introspection/Cargo.toml` | Create |
| `canon-introspection/src/lib.rs` | Create — `assert_all_routes_safe()` |
| `canon-capability/src/tests.rs` | Replace manual list with introspection loop |
| `canon-capability/Cargo.toml` | Add `canon-introspection` to dev-dependencies |
| Root `Cargo.toml` | Add `canon-utils/canon-introspection` to members |
