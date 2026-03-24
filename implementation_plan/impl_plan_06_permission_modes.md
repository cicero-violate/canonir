# Implementation Plan 06 — Permission Modes

## Goal

Each agent role is assigned a `PermissionMode` that restricts which `RuntimeEvent`
types it may emit. A `ReadOnly` analyst cannot trigger `Bash` or `File::Write`. A
`PlanOnly` planner cannot emit `LoopActed`. Enforcement is in the hooks layer
(plan 01) — no consumer code needs to change.

---

## Step 1 — Define `PermissionMode` in `canon-runtime-events`

Add to `events.rs` (or a new `permissions.rs` re-exported from the crate):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    /// May only emit NoOp or Analysis-class events. Cannot trigger capabilities.
    ReadOnly,
    /// May emit LoopPlanned but not LoopActed. No Bash, File::Write, or Cargo::Build.
    PlanOnly,
    /// Default — may trigger any capability.
    Execute,
    /// May also emit PolicyBaselineUpdated, SystemConfigLoaded, GoalSelected.
    Privileged,
}

impl Default for PermissionMode {
    fn default() -> Self { PermissionMode::Execute }
}
```

---

## Step 2 — Map roles to modes in `capability_config.toml`

Add a `[permissions]` table:

```toml
[permissions]
analyst  = "read_only"
goal_gen = "plan_only"
planner  = "plan_only"
router   = "plan_only"
exec     = "execute"
```

---

## Step 3 — Load permissions into `CapabilityConfig`

In `canon-utils/canon-llm-runtime/src/config.rs` (or wherever `CapabilityConfig` is
defined), add:

```rust
pub permissions: HashMap<String, PermissionMode>,
```

Parse from the `[permissions]` toml table. Default to `Execute` for unknown roles.

---

## Step 4 — Build a `PermissionEnforcer` hook

**New file:** `canon-utils/canon-runtime/src/hooks/permission_enforcer.rs`

Implements `PreHook`:

```rust
pub struct PermissionEnforcer {
    /// Maps agent_id → PermissionMode. Loaded from config at startup.
    role_modes: HashMap<String, PermissionMode>,
}

impl PreHook for PermissionEnforcer {
    fn name(&self) -> &'static str { "permission_enforcer" }

    fn on_pre(&self, event: &RuntimeEvent) -> HookDecision {
        // Extract role from the event. LlmCall carries agent_id.
        // Capability events carry capability name but not role — skip those.
        // Only LlmCall emitted events (i.e., the emit-outcomes from consumers)
        // carry a role we can check.
        let (role, violation) = match event {
            RuntimeEvent::Bash(_) => ("unknown", Some("bash")),
            RuntimeEvent::File(FileEvent::Write(_)) | RuntimeEvent::File(FileEvent::Patch(_)) => ("unknown", Some("file_write")),
            RuntimeEvent::LoopActed(_) => ("unknown", Some("loop_acted")),
            RuntimeEvent::PolicyBaselineUpdated(_) | RuntimeEvent::SystemConfigLoaded(_) | RuntimeEvent::GoalSelected(_) => ("unknown", Some("privileged_event")),
            _ => return HookDecision::Allow,
        };
        // Permission enforcement requires knowing which consumer emitted this event.
        // Since EventOutcome::Emit events go back through dispatch(), we need the
        // originating role. See Step 5 for how role is carried.
        let _ = (role, violation);
        HookDecision::Allow // replaced after Step 5
    }
}
```

---

## Step 5 — Carry emitting role through `EventOutcome`

The challenge: when a consumer emits `EventOutcome::Emit(e)`, the bus re-dispatches
`e` but doesn't know which consumer emitted it.

**Solution:** Add `EmitAs` variant to `EventOutcome`:

```rust
pub enum EventOutcome {
    Emit(RuntimeEvent),
    EmitAs { event: RuntimeEvent, role: String },  // new
    EmitMany(Vec<RuntimeEvent>),
    NoOp(&'static str),
    Error(RuntimeEvent),
}
```

In `bus.rs`, `EmitAs` is handled like `Emit` but the role is passed to
`hooks.run_pre_with_role(&event, Some(&role))` before dispatch.

Add `run_pre_with_role` to `HookChain`:

```rust
pub fn run_pre_with_role(&self, event: &RuntimeEvent, role: Option<&str>) -> HookDecision;
```

**Consumers that need to use `EmitAs`:** analyst (role = "analyst"), goal_gen
(role = "goal_gen"). Replace `EventOutcome::Emit(...)` with
`EventOutcome::EmitAs { event: ..., role: ANALYST_ROLE.to_string() }` at all emit
sites in `analyst_consumer.rs` and `goal_gen_consumer.rs`.

---

## Step 6 — Complete `PermissionEnforcer::on_pre`

With `role` now available via `run_pre_with_role`:

```rust
fn on_pre_with_role(&self, event: &RuntimeEvent, role: Option<&str>) -> HookDecision {
    let Some(role) = role else { return HookDecision::Allow; };
    let mode = self.role_modes.get(role).copied().unwrap_or_default();

    let violation = match mode {
        PermissionMode::ReadOnly => match event {
            RuntimeEvent::Bash(_) => Some("bash"),
            RuntimeEvent::File(FileEvent::Write(_) | FileEvent::Patch(_)) => Some("file_write"),
            RuntimeEvent::LoopActed(_) => Some("loop_acted"),
            RuntimeEvent::LoopPlanned(_) => Some("loop_planned"),
            _ => None,
        },
        PermissionMode::PlanOnly => match event {
            RuntimeEvent::Bash(_) => Some("bash"),
            RuntimeEvent::File(FileEvent::Write(_) | FileEvent::Patch(_)) => Some("file_write"),
            RuntimeEvent::LoopActed(_) => Some("loop_acted"),
            _ => None,
        },
        PermissionMode::Execute | PermissionMode::Privileged => None,
    };

    match violation {
        None => HookDecision::Allow,
        Some(v) => HookDecision::Deny {
            reason: format!("permission_denied: role={role} mode={mode:?} blocked={v}"),
        },
    }
}
```

---

## Step 7 — Register in `event_runtime.rs`

```rust
let enforcer = PermissionEnforcer::from_config(&config);
hooks.add_pre(Box::new(enforcer));
```

---

## Verification

```
cargo check --workspace
```

1. Temporarily add a `Bash` emit to `analyst_consumer.rs` (for testing).
2. Run runtime.
3. Confirm `ErrorOccurred("hook_denied")` with reason
   `"permission_denied: role=analyst mode=ReadOnly blocked=bash"` appears in tlog.
4. Remove the test emit.
