# Implementation Plan: Capability Collapse

## Goal

Collapse two systems — capabilities and events — into one. Eliminate the JSON boundary between them. Events become capability input. String routing and argument parsing are removed.

P = min(U, M, R, T, I) → target P = 1

---

## Current State (Verified)

### capability flow today

```
ActConsumer
  → emits CanonEvent::CapabilityRequested { name: "edit.rename_symbol", args: { "project": "...", "old": "...", "new": "..." } }
  → CapabilityExecutor.on_event matches CapabilityRequested
  → registry.execute(&request.name, ctx)          ← string routing
  → RenameSymbolCapability.execute(ctx)
      → CanonEvent::CapabilityRequested(req) = ctx.event
      → require_arg(&req.args, "project")          ← JSON parsing
      → require_arg(&req.args, "old")
      → require_arg(&req.args, "new")
      → Ok(Emit(CanonEvent::Edit(EditEvent::RenameSymbol(...))))
```

### end state

```
ActConsumer
  → emits CanonEvent::Edit(EditEvent::RenameSymbol { project, old, new })
  → CapabilityExecutor routes directly by event type
  → handler receives typed event, no parsing
```

### files (verified paths)

| File                                                 | Role                                                          |
|------------------------------------------------------+---------------------------------------------------------------|
| `canon-capability/src/trait.rs`                      | `CapabilityHandler`, `CapabilitySchema`, `ArgSpec`, `ArgKind` |
| `canon-capability/src/registry.rs`                   | `CapabilityRegistry` with `execute(name, ctx)`                |
| `canon-capability/src/context.rs`                    | `CapabilityExecutionContext { workspace, event, emitter }`    |
| `canon-capability/src/result.rs`                     | `CapabilityExecutionResult` — untouched                       |
| `canon-capability/src/lib.rs`                        | re-exports                                                    |
| `canon-tools-editor/src/capabilities.rs`             | 5 handlers all using `require_arg`                            |
| `canon-runtime/src/consumers/capability_executor.rs` | `CapabilityExecutor` on bus                                   |

---

## Migration Order (strict — do not reorder)

1. Phase 1 — deprecate `ArgSpec`, `ArgKind`, `CapabilitySchema.args`
2. Phase 2 — add `fn handle()` to trait; deprecate `fn execute()` and `fn schema()`
3. Phase 3 — add `registry.route()` with decode bridge
4. Phase 4 — refactor editor capabilities to direct dispatch (preferred form)
5. Phase 5 — add decode layer, consolidate all JSON parsing in one place
6. Phase 6 — update `CapabilityExecutor` to call `registry.route()`
7. Phase 7 — zero-maintenance test
8. Phase 8 — remove `CapabilityRequested`, `ArgSpec`, `ArgKind`, `CapabilitySchema.args`

Phases 1–3 are additive. Existing code compiles at every step until Phase 8.

---

## Phase 1 — Deprecate Arg Types

**File: `canon-capability/src/trait.rs`**

Mark the arg infrastructure deprecated. Keep compiling.

```rust
#[deprecated(note = "use typed CanonEvent fields directly")]
#[derive(Debug, Clone)]
pub struct ArgSpec {
    pub key: &'static str,
    pub kind: ArgKind,
    pub required: bool,
}

#[deprecated(note = "use typed CanonEvent fields directly")]
#[derive(Debug, Clone)]
pub enum ArgKind {
    String,
    Path,
    Symbol,
    Json,
}
```

`CapabilitySchema.args` field: add `#[deprecated]` attribute on the field is not stable Rust — instead add a doc comment:

```rust
#[derive(Debug, Clone)]
pub struct CapabilitySchema {
    pub name: &'static str,
    /// Deprecated. Args are encoded in the typed CanonEvent, not here.
    pub args: Vec<ArgSpec>,
}
```

---

## Phase 2 — Add `fn handle()`, Deprecate `fn execute()` and `fn schema()`

**File: `canon-capability/src/trait.rs`**

Rename the primary method to `handle`. Keep `execute` as a deprecated default that delegates, so all existing implementors continue to compile.

```rust
pub trait CapabilityHandler: Send + Sync {
    fn name(&self) -> &'static str;

    /// Primary entrypoint. Implement this.
    fn handle(&self, ctx: CapabilityExecutionContext) -> Result<CapabilityExecutionResult>;

    /// Deprecated. Calls handle(). Do not override.
    #[deprecated(note = "implement handle() instead")]
    fn execute(&self, ctx: CapabilityExecutionContext) -> Result<CapabilityExecutionResult> {
        self.handle(ctx)
    }

    /// Deprecated. Schema is encoded in the CanonEvent type system.
    #[deprecated(note = "schema is derived from CanonEvent variants")]
    fn schema(&self) -> CapabilitySchema {
        CapabilitySchema { name: self.name(), args: Vec::new() }
    }
}
```

**File: `canon-capability/src/registry.rs`**

Update `execute` to call `handle` (not the deprecated `execute`):

```rust
pub fn execute(&self, name: &str, ctx: CapabilityExecutionContext) -> Result<CapabilityExecutionResult> {
    let capability = self.map.get(name).ok_or_else(|| anyhow!("capability not registered: {name}"))?;
    capability.handle(ctx)
}
```

---

## Phase 3 — Add `registry.route()` with Decode Bridge

**File: `canon-capability/src/registry.rs`**

Add `route()` alongside `execute()`. During migration, `route()` decodes `CapabilityRequested` → typed event, then delegates to the named handler. After Phase 8, routing is purely by event type.

```rust
use canon_event::{CanonEvent, CapabilityRequested};

impl CapabilityRegistry {
    /// Route a CanonEvent to the appropriate handler.
    /// During migration: decodes CapabilityRequested → typed event via the decode layer.
    /// After migration: routes directly by event type.
    pub fn route(&self, ctx: CapabilityExecutionContext) -> Result<CapabilityExecutionResult> {
        let name = match &ctx.event {
            CanonEvent::CapabilityRequested(req) => req.name.clone(),
            _ => return Ok(CapabilityExecutionResult::NoOp),
        };

        // Decode JSON args → typed event. Temporary bridge — removed in Phase 8.
        let typed_event = crate::decode::decode_capability_event(&ctx.event)?;
        let typed_ctx = CapabilityExecutionContext {
            workspace: ctx.workspace.clone(),
            event: typed_event,
            emitter: ctx.emitter.clone(),
        };

        self.execute(&name, typed_ctx)
    }
}
```

---

## Phase 4 — Refactor Editor Capabilities (Direct Dispatch — Preferred)

**File: `canon-tools-editor/src/capabilities.rs`**

Remove `require_arg`. Each handler implements `handle()` using direct match on the typed event. No JSON parsing at call sites.

Remove the `require_arg` function entirely.

Replace all five handlers:

```rust
impl CapabilityHandler for RenameSymbolCapability {
    fn name(&self) -> &'static str { CAP_RENAME_SYMBOL }

    fn handle(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        match ctx.event {
            CanonEvent::Edit(EditEvent::RenameSymbol(ev)) => Ok(emit_edit(EditEvent::RenameSymbol(ev))),
            _ => Ok(CapabilityExecutionResult::NoOp),
        }
    }
}

impl CapabilityHandler for MoveSymbolCapability {
    fn name(&self) -> &'static str { CAP_MOVE_SYMBOL }

    fn handle(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        match ctx.event {
            CanonEvent::Edit(EditEvent::MoveSymbol(ev)) => Ok(emit_edit(EditEvent::MoveSymbol(ev))),
            _ => Ok(CapabilityExecutionResult::NoOp),
        }
    }
}

impl CapabilityHandler for DeleteSymbolCapability {
    fn name(&self) -> &'static str { CAP_DELETE_SYMBOL }

    fn handle(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        match ctx.event {
            CanonEvent::Edit(EditEvent::DeleteSymbol(ev)) => Ok(emit_edit(EditEvent::DeleteSymbol(ev))),
            _ => Ok(CapabilityExecutionResult::NoOp),
        }
    }
}

impl CapabilityHandler for RenameModuleCapability {
    fn name(&self) -> &'static str { CAP_RENAME_MODULE }

    fn handle(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        match ctx.event {
            CanonEvent::Edit(EditEvent::RenameModule(ev)) => Ok(emit_edit(EditEvent::RenameModule(ev))),
            _ => Ok(CapabilityExecutionResult::NoOp),
        }
    }
}

impl CapabilityHandler for RenameDirCapability {
    fn name(&self) -> &'static str { CAP_RENAME_DIR }

    fn handle(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        match ctx.event {
            CanonEvent::Edit(EditEvent::RenameDir(ev)) => Ok(emit_edit(EditEvent::RenameDir(ev))),
            _ => Ok(CapabilityExecutionResult::NoOp),
        }
    }
}
```

---

## Phase 5 — Add Decode Layer

**New file: `canon-capability/src/decode.rs`**

Consolidates all JSON → typed event conversion in one place. This is the only remaining location that reads `CapabilityRequested.args`. Removed entirely in Phase 8.

```rust
use anyhow::{anyhow, Result};
use canon_event::{CanonEvent, EditEvent, RenameDir, RenameModule, RenameSymbol, MoveSymbol, DeleteSymbol};

fn require_str<'a>(args: &'a serde_json::Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("missing or invalid arg: {key}"))
}

/// Decode a CapabilityRequested event into a typed CanonEvent.
/// Temporary bridge during capability collapse migration.
pub fn decode_capability_event(event: &CanonEvent) -> Result<CanonEvent> {
    let CanonEvent::CapabilityRequested(req) = event else {
        return Ok(event.clone());
    };

    match req.name.as_str() {
        "edit.rename_symbol" => {
            let project = require_str(&req.args, "project")?.to_string();
            let old = require_str(&req.args, "old")?.to_string();
            let new = require_str(&req.args, "new")?.to_string();
            Ok(CanonEvent::Edit(EditEvent::RenameSymbol(RenameSymbol { project, old, new })))
        }
        "edit.move_symbol" => {
            let project = require_str(&req.args, "project")?.to_string();
            let symbol = require_str(&req.args, "symbol")?.to_string();
            let module = require_str(&req.args, "module")?.to_string();
            Ok(CanonEvent::Edit(EditEvent::MoveSymbol(MoveSymbol { project, symbol, module })))
        }
        "edit.delete_symbol" => {
            let project = require_str(&req.args, "project")?.to_string();
            let symbol = require_str(&req.args, "symbol")?.to_string();
            Ok(CanonEvent::Edit(EditEvent::DeleteSymbol(DeleteSymbol { project, symbol })))
        }
        "edit.rename_module" => {
            let project = require_str(&req.args, "project")?.to_string();
            let old = require_str(&req.args, "old")?.to_string();
            let new = require_str(&req.args, "new")?.to_string();
            Ok(CanonEvent::Edit(EditEvent::RenameModule(RenameModule { project, old, new })))
        }
        "edit.rename_dir" => {
            let project = require_str(&req.args, "project")?.to_string();
            let old = require_str(&req.args, "old")?.to_string();
            let new = require_str(&req.args, "new")?.to_string();
            Ok(CanonEvent::Edit(EditEvent::RenameDir(RenameDir { project, old: old.into(), new: new.into() })))
        }
        name => Err(anyhow!("no decode for capability: {name}")),
    }
}
```

**File: `canon-capability/src/lib.rs`**

Add `pub mod decode;`

---

## Phase 6 — Update CapabilityExecutor to Use `route()`

**File: `canon-runtime/src/consumers/capability_executor.rs`**

Replace `registry.execute(&request.name, ctx)` with `registry.route(ctx)`. Remove the explicit name extraction — `route()` handles it.

```rust
fn on_event(&mut self, event: &CanonEvent) {
    let CanonEvent::CapabilityRequested(_) = event else {
        return;
    };

    let ctx = CapabilityExecutionContext {
        workspace: self.workspace.clone(),
        event: event.clone(),
        emitter: self.emitter.clone(),
    };

    let result = match self.registry.lock() {
        Ok(registry) => registry.route(ctx),
        Err(err) => Err(anyhow!("capability registry lock poisoned: {err}")),
    };

    // error handling and emission unchanged below
    ...
}
```

---

## Phase 7 — Zero-Maintenance Test

**New file: `canon-capability/src/tests.rs`** (or inline with `#[cfg(test)]`)

The test exercises every route call with sample events. When new capabilities are added, they add a sample event — the test loop needs no changes.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use canon_event::{CanonEvent, CapabilityRequested, EditEvent, RenameSymbol};

    fn sample_capability_events() -> Vec<CanonEvent> {
        vec![
            CanonEvent::CapabilityRequested(CapabilityRequested {
                request_id: "test-1".to_string(),
                name: "edit.rename_symbol".to_string(),
                args: serde_json::json!({ "project": "p", "old": "foo", "new": "bar" }),
            }),
            CanonEvent::CapabilityRequested(CapabilityRequested {
                request_id: "test-2".to_string(),
                name: "edit.move_symbol".to_string(),
                args: serde_json::json!({ "project": "p", "symbol": "foo", "module": "bar" }),
            }),
            CanonEvent::CapabilityRequested(CapabilityRequested {
                request_id: "test-3".to_string(),
                name: "edit.delete_symbol".to_string(),
                args: serde_json::json!({ "project": "p", "symbol": "foo" }),
            }),
            CanonEvent::CapabilityRequested(CapabilityRequested {
                request_id: "test-4".to_string(),
                name: "edit.rename_module".to_string(),
                args: serde_json::json!({ "project": "p", "old": "foo", "new": "bar" }),
            }),
            CanonEvent::CapabilityRequested(CapabilityRequested {
                request_id: "test-5".to_string(),
                name: "edit.rename_dir".to_string(),
                args: serde_json::json!({ "project": "p", "old": "src/a", "new": "src/b" }),
            }),
        ]
    }

    #[test]
    fn all_capability_routes_are_safe() {
        use std::path::PathBuf;
        use std::sync::Arc;
        use crate::registry::CapabilityRegistry;

        let mut registry = CapabilityRegistry::new();
        canon_tools_editor::capabilities::register_editor_capabilities(&mut registry);

        for event in sample_capability_events() {
            let ctx = CapabilityExecutionContext {
                workspace: PathBuf::from("/tmp"),
                event,
                emitter: None,
            };
            let result = registry.route(ctx);
            // All routes must not panic. Result shape is not checked here.
            let _ = format!("{:?}", result);
        }
    }
}
```

---

## Phase 8 — Remove CapabilityRequested (Final State)

After Phase 7 passes and `ActConsumer` emits typed events directly:

1. Remove `CanonEvent::CapabilityRequested` from `events.rs`
2. Remove `CapabilityRequested` struct
3. Remove `decode.rs`
4. Remove `ArgSpec`, `ArgKind`, `CapabilitySchema.args`
5. Update `registry.route()` to match typed events directly (no decode step)
6. Remove `CapabilityExecutor` filter for `CapabilityRequested` — it becomes `EventFilter::EditOnly` or direct match

**End state `registry.route()`:**

```rust
pub fn route(&self, ctx: CapabilityExecutionContext) -> Result<CapabilityExecutionResult> {
    match &ctx.event {
        CanonEvent::Edit(_) => {
            // Route edit events to the editor capability suite.
            // Handlers match their specific variant; others return NoOp.
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

---

## Files Modified (complete list)

| File                                                 |      Phase | Action                                                                                     |
|------------------------------------------------------+------------+--------------------------------------------------------------------------------------------|
| `canon-capability/src/trait.rs`                      |       1, 2 | Deprecate `ArgSpec/ArgKind`, add `fn handle()`, deprecate `fn execute()` and `fn schema()` |
| `canon-capability/src/registry.rs`                   | 2, 3, 6, 8 | Update internal call to `handle()`; add `route()`; final route by type                     |
| `canon-capability/src/decode.rs`                     |       5, 8 | Create — JSON bridge; remove in Phase 8                                                    |
| `canon-capability/src/lib.rs`                        |          5 | Add `pub mod decode`                                                                       |
| `canon-tools-editor/src/capabilities.rs`             |          4 | Remove `require_arg`; implement `handle()` with direct dispatch on all 5 handlers          |
| `canon-runtime/src/consumers/capability_executor.rs` |          6 | Replace `registry.execute(name, ctx)` with `registry.route(ctx)`                           |
| `canon-capability/src/tests.rs`                      |          7 | Create — `sample_capability_events()` + `all_capability_routes_are_safe()`                 |
| `canon-runtime-events/src/events.rs`                 |          8 | Remove `CapabilityRequested` variant and struct                                            |
