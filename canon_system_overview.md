# Canon Self-Repair and Invariant Discovery System

## Purpose

This system is not only a test-repair loop. It is a layered architecture for:

- enforcing invariants over agent behavior
- discovering new invariants from repeated failures
- projecting live runtime behavior into abstract state models
- testing those models with synthetic harnesses
- allowing bounded self-repair through an LLM-driven repair loop

The practical effect is that red tests are used as observable boundaries, but the deeper goal is state-space control and invariant discovery.

## Main Layers

### 1. Repair Harness

**File:** `canon-utils/canon-runtime/src/bin/harness_repair.rs`

This is the LLM-driven repair controller.

It operates as a narrow loop:

1. identify one failing test
2. read the relevant file/function
3. apply one patch
4. verify by running cargo/test commands
5. repeat until green or step budget exhausted

Key properties:

- one action per turn
- strict JSON-in-code-block response contract
- patch failures feed fresh file context back to the model
- retries are bounded
- prompt duplication was reduced by keeping one request id alive across wait windows instead of re-dispatching identical prompts

This harness is a **repair shell**, not the whole reasoning system.

### 2. Invariant Engine

**File:** `canon-utils/canon-invariant/src/lib.rs`

This is the core invariant layer.

It defines:

- `ConstraintState`
- `ConstraintContext`
- `ConstraintDecision`
- failure fingerprints
- discovered invariants
- promotion logic
- meta-invariants for repair, verification, scope, and routing

This layer serves two roles:

#### Hand-authored invariants

Examples:

- missing target forces planning
- validation is forbidden while blocked
- deterministic routes cannot be overridden
- no actionable failure forbids repair actions
- tool selection must match workspace bootstrap state

#### Discovered invariants

Repeated failure fingerprints can promote persistent invariants such as:

- `ForcePlanWhenMissingTarget`
- `ForcePlanWhenValidationBlocked`
- `ForcePlanWhenObjectiveContradiction`
- `ForceObserveWhenNoActionableFailure`

These are stored in runtime state and loaded across runs.

### 3. Runtime-Persistent Invariant Memory

**Default file:** `/workspace/ai_sandbox/canon/state/discovered_invariants.json`

This file is live runtime state.

It is used by:

- `with_threshold()`
- `load_from_disk()`
- `save_to_disk()`
- `reload_discovered_invariants_from_disk()`
- `clear_discovered_invariants_store()`

Important behavior:

- in tests, persistent storage is disabled
- in runtime, promoted/discovered invariants persist across sessions
- persisted store events are emitted for load/update

So this file is not dead residue. It is part of the system’s learned invariant memory.

## Real Runtime Surfaces

### 4. Loop Runtime

**Files:**
- `canon-loop/src/stage/mod.rs`
- `observe.rs`
- `plan.rs`
- `act.rs`
- `verify.rs`
- `reward.rs`
- `decompose.rs`

This is the real staged runtime:

- observe
- plan
- act
- verify
- reward / conclude
- decompose

It processes real `RuntimeEvent`s and maintains loop context.

### 5. Route Policy and Executor

**Files:**
- `canon-route/src/policy.rs`
- `canon-route/src/executor.rs`

This is the real control plane.

It contains state such as:

- pending request ids
- pending required successors
- awaiting control successors
- cached route behavior
- route emission suppression
- recovery behavior
- deterministic routing rules
- route cache rules
- emit effects and successor consumption

This is the area that was not fully covered by the original invariant harness.

## Why a Second Harness Was Needed

The first synthetic harness in `canon-invariant` already covered the abstract repair/invariant space well.

But there was a gap:

- the abstract `ConstraintState` space was modeled
- the real route executor control state was only partially modeled

That meant the system had good repair-policy invariants but weaker explicit coverage of:

- duplicate route emission
- pending request suppression
- successor obligations
- cached route replay vs invalidation
- fresh-route forcing
- conclude/halt semantics

To close that gap, a second synthetic harness was added.

## New Control Harness

**File:** `canon-utils/canon-invariant/src/control_harness.rs`

This harness models the route executor control plane as a synthetic state machine.

### Synthetic control state

It currently models:

- `pending_request`
- `pending_required_successor_route_selected`
// removed awaiting_control_successor (transition authority moved to invariants)
- `route_emitted_for_current_control`
- `has_cached_route`
- `cached_route_is_observe`
- `can_emit_route_selected`
- `force_fresh_route_once`
- `halted`

### Synthetic control decisions

It currently classifies states into:

- `Suppress(...)`
- `ReplayCachedRoute`
- `RequestFreshRoute`
- `EmitRoute`
- `InvariantViolation(...)`

### Added transition layer

The harness was extended from static classification to transitions through `ControlEvent`, including:

- `RouteSelectedEmitted`
- `PendingRequestStarted`
- `PendingRequestCleared`
- `AwaitingControlSuccessorSet`
- `AwaitingControlSuccessorCleared`
- `ForceFreshRouteOnce`
- `CachedObserveRouteStored`
- `CachedNonObserveRouteStored`
- `CachedRouteCleared`
- `PromptDispatched`
- `PromptCleared`
- `ConcludeEmitted`

This moves the harness closer to a real control-state model instead of a pure lookup table.

## Current Trust Model

The system is ready for **bounded automation**, not full blind trust.

### Ready now

- narrow repair loops
- one-action repair control
- invariant-guided routing
- persistent discovered invariants
- synthetic testing of both:
  - repair/invariant state
  - control-plane state

### Still not complete for full trust

Full trust still requires broader coverage of:

- full route successor consumption semantics
- recovery-event generation
- emit-effect transitions
- dispatch/event-dispatch coupling
- remaining failures in core invariant promotion tests

At the time of writing, the new control harness passes, but the full system still has a failing suite case around repeated missing-target promotion. That means architecture work for this slice is done, but total self-repair closure is not complete.

## What Was Completed In This Work Slice

1. verified that the repair harness is a test-repair shell, not the whole system
2. verified that `canon-invariant` contains real invariant discovery logic
3. verified that `discovered_invariants.json` is used at runtime
4. verified that the live runtime projects into abstract invariant state
5. identified the missing control-plane abstraction gap
6. added a second synthetic harness for control state
7. extended that harness with transition semantics
8. kept the remaining failure in the core system separate from this architecture step

## Summary

This system has three important identities at once:

### A repair loop
It can iteratively fix failing tests through a bounded LLM harness.

### An invariant engine
It encodes explicit repair/routing/verifier/tool invariants and discovers new ones from repeated failures.

### A state-space modeling system
It projects both repair-state and control-state into synthetic harnesses and tests them for exhaustiveness, convergence, and safety properties.

That is why the correct description is:

> red-test repair is the interface, but invariant discovery and state-space control are the deeper mechanism.

## Recommended Next Direction

The next step toward full trust is not more prompt tuning.

The next step is more control-plane modeling and verification around:

- successor consumption
- recovery paths
- route emit effects
- dispatch coupling
- persistent invariant promotion correctness

Once those are covered well enough, the automation boundary can be widened with more confidence.
