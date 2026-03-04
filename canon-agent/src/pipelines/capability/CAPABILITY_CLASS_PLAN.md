# Capability Class System Plan

## Objective

Replace the two hardcoded `HashSet` functions (`mutation_caps`,
`verify_caps`) and the name-specific checks scattered across `engine.rs` and
`dag.rs` with a static per-variant class mapping. Every `Capability` declares
its own class at definition time. Dispatch, validation, and authority checks
all derive from that single source of truth.

---

## Change 1 — Add CapabilityClass enum and class() method

**File:** `capability.rs`

Add the class enum:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CapabilityClass {
    Observe  = 0,   // read-only, no side effects
    Verify   = 1,   // status inspection and update, no file mutation
    Mutate   = 2,   // file writes, shell execution, patch application
}
```

Add a `class()` method on `Capability` as a static lookup — no heap
allocation, no HashSet, one match arm per variant:

```rust
impl Capability {
    pub fn class(self) -> CapabilityClass {
        match self {
            // Observe
            Capability::FileRead              => CapabilityClass::Observe,
            Capability::ReadDag               => CapabilityClass::Observe,
            Capability::ReadStructuralSurface => CapabilityClass::Observe,
            Capability::StdoutCapture         => CapabilityClass::Observe,
            Capability::StatelessInvoke       => CapabilityClass::Observe,
            Capability::RadiusBudgetEval      => CapabilityClass::Observe,
            Capability::ComputeDelta          => CapabilityClass::Observe,
            Capability::RewardSignalCompute   => CapabilityClass::Observe,
            Capability::PromptContractEnforce => CapabilityClass::Observe,
            Capability::GoalToSubgoals        => CapabilityClass::Observe,
            Capability::ScheduleReady         => CapabilityClass::Observe,

            // Verify
            Capability::StatusUpdateOnly          => CapabilityClass::Verify,
            Capability::UpdateStatus              => CapabilityClass::Verify,
            Capability::ParseOrchestrationReport  => CapabilityClass::Verify,
            Capability::DetectFailures            => CapabilityClass::Verify,
            Capability::InvariantCheck            => CapabilityClass::Verify,
            Capability::BoundaryGuard             => CapabilityClass::Verify,

            // Mutate
            Capability::ApplyPatch        => CapabilityClass::Mutate,
            Capability::FileWrite         => CapabilityClass::Mutate,
            Capability::Bash              => CapabilityClass::Mutate,
            Capability::CargoBuild        => CapabilityClass::Mutate,
            Capability::CargoCheck        => CapabilityClass::Mutate,
            Capability::CreateNode        => CapabilityClass::Mutate,
            Capability::AddEdge           => CapabilityClass::Mutate,
            Capability::RefineNode        => CapabilityClass::Mutate,
            Capability::DependencyRewrite => CapabilityClass::Mutate,
            Capability::ConstraintAttach  => CapabilityClass::Mutate,

            Capability::Unknown           => CapabilityClass::Observe,
        }
    }
}
```

Add a helper that returns the highest-authority class in a capability set:

```rust
pub fn dominant_class(caps: &[Capability]) -> CapabilityClass {
    caps.iter()
        .map(|c| c.class())
        .max_by_key(|&c| c as u8)
        .unwrap_or(CapabilityClass::Observe)
}
```

Replace `mutation_caps()` and `verify_caps()` with class-based equivalents
that are used only for backward-compat in validation. Keep them as thin
wrappers so call sites in `dag.rs` do not need immediate changes:

```rust
pub fn assert_class_disjoint(caps: &HashSet<Capability>) -> Result<(), String> {
    let has_mutate = caps.iter().any(|c| c.class() == CapabilityClass::Mutate);
    let has_verify = caps.iter().any(|c| c.class() == CapabilityClass::Verify);
    if has_mutate && has_verify {
        return Err(format!(
            "capability class violation: node mixes Mutate and Verify capabilities: {:?}",
            caps.iter().filter(|c| c.class() != CapabilityClass::Observe).collect::<Vec<_>>()
        ));
    }
    Ok(())
}
```

Delete `mutation_caps()`, `verify_caps()`, and `assert_mut_verify_disjoint`.
Replace all call sites with `assert_class_disjoint`.

---

## Change 2 — Update dag.rs

**File:** `dag.rs`

Replace the import:

```rust
// before
use super::capability::{assert_mut_verify_disjoint, Capability};

// after
use super::capability::{assert_class_disjoint, Capability};
```

In `TaskGraph::validate`, replace:

```rust
assert_mut_verify_disjoint(&caps).map_err(|e| format!("node {}: {}", n.id, e))?;
```

with:

```rust
assert_class_disjoint(&caps).map_err(|e| format!("node {}: {}", n.id, e))?;
```

In `AuthorityContext::new`, replace:

```rust
assert_mut_verify_disjoint(&caps)?;
```

with:

```rust
assert_class_disjoint(&caps)?;
```

Replace `is_verify_context` and `is_mutation_context` with class-based
implementations:

```rust
pub fn is_verify_context(&self) -> bool {
    self.capabilities.iter().any(|c| c.class() == CapabilityClass::Verify)
}

pub fn is_mutation_context(&self) -> bool {
    self.capabilities.iter().any(|c| c.class() == CapabilityClass::Mutate)
}
```

Add the import for `CapabilityClass`:

```rust
use super::capability::{assert_class_disjoint, Capability, CapabilityClass};
```

---

## Change 3 — Update engine.rs

**File:** `engine.rs`

`select_mode` currently works via `MODE_RULES` which calls
`is_verify_context` and `is_mutation_context`. Those are already updated in
Change 2 to use `class()`, so `select_mode` requires no changes.

`validate_mutate` currently checks for `FileWrite` or `ApplyPatch` by name:

```rust
fn validate_mutate(ctx: &AuthorityContext, node_id: &str) -> Result<()> {
    (ctx.has(Capability::FileWrite) || ctx.has(Capability::ApplyPatch))
        .then_some(())
        .ok_or_else(|| ...)
}
```

Replace with a class check so any future Mutate-class capability satisfies it
automatically:

```rust
fn validate_mutate(ctx: &AuthorityContext, node_id: &str) -> Result<()> {
    ctx.capabilities.iter()
        .any(|c| c.class() == CapabilityClass::Mutate)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!(
            "node {} has no Mutate-class capability", node_id
        ))
}
```

`validate_verify` currently checks for `StatusUpdateOnly` by name:

```rust
fn validate_verify(ctx: &AuthorityContext, _: &str) -> Result<()> {
    ctx.require(Capability::StatusUpdateOnly).map_err(|e| anyhow::anyhow!(e))
}
```

Replace with a class check:

```rust
fn validate_verify(ctx: &AuthorityContext, node_id: &str) -> Result<()> {
    ctx.capabilities.iter()
        .any(|c| c.class() == CapabilityClass::Verify)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!(
            "node {} has no Verify-class capability", node_id
        ))
}
```

In `mutate_is_blocked`, replace the name-specific capability check:

```rust
// before
!n.required_capabilities.iter().any(|c| matches!(c, Capability::FileWrite | Capability::ApplyPatch))

// after
!n.required_capabilities.iter().any(|c| c.class() == CapabilityClass::Mutate)
```

In `apply_mutate_result`, replace:

```rust
// before
n.required_capabilities.contains(&Capability::StatusUpdateOnly)

// after
n.required_capabilities.iter().any(|c| c.class() == CapabilityClass::Verify)
```

Add the import:

```rust
use super::capability::CapabilityClass;
```

---

## What Gets Deleted

| Item | Location | Replaced by |
|------|----------|-------------|
| `mutation_caps()` | `capability.rs` | `Capability::class()` |
| `verify_caps()` | `capability.rs` | `Capability::class()` |
| `assert_mut_verify_disjoint` | `capability.rs` | `assert_class_disjoint` |
| `is_verify_context` hardcoded check | `dag.rs` | `c.class() == Verify` |
| `is_mutation_context` hardcoded check | `dag.rs` | `c.class() == Mutate` |
| `validate_mutate` name check | `engine.rs` | `c.class() == Mutate` |
| `validate_verify` name check | `engine.rs` | `c.class() == Verify` |
| `mutate_is_blocked` name check | `engine.rs` | `c.class() == Mutate` |
| `apply_mutate_result` name check | `engine.rs` | `c.class() == Verify` |

---

## What Adding a New Capability Looks Like After This Change

Before: add variant to enum, then remember to update `mutation_caps()` or
`verify_caps()`, then check if any name-specific match arms in `engine.rs`
need updating.

After: add variant to enum, add one arm to `class()`. Done. Dispatch,
validation, and authority checks all pick it up automatically.

---

## Touched Files Summary

| File | Change |
|------|--------|
| `capability.rs` | Add `CapabilityClass`, `class()`, `dominant_class()`, `assert_class_disjoint`; delete `mutation_caps`, `verify_caps`, `assert_mut_verify_disjoint` |
| `dag.rs` | Replace `assert_mut_verify_disjoint` with `assert_class_disjoint`; replace hardcoded checks in `is_verify_context` and `is_mutation_context` with `class()` comparisons |
| `engine.rs` | Replace name-specific checks in `validate_mutate`, `validate_verify`, `mutate_is_blocked`, `apply_mutate_result` with `class()` comparisons |
