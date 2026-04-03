# Violations

## 1. Invariant enforcement occurs after dispatch (CRITICAL)
- Evidence:
  - Diagnostics: dispatch runs before append validation (lib.rs:737-741)
  - Invalid events can be dispatched before being rejected
- Issue:
  - Violates invariant I16: "¬I_k ⇒ reject(E)" at write-time
  - Causes divergence between dispatched state and persisted log
- Required fix:
  - Move invariant validation before dispatch
  - Ensure invalid events are rejected before any side effects
  - Enforce ordering: validate → append → dispatch (or validate → dispatch+append atomically)

## 2. Append rejection does not roll back dispatch effects (CRITICAL)
- Evidence:
  - Invariant failure returns false but does not undo prior dispatch
  - ErrorOccurred emitted instead of preventing propagation
- Issue:
  - System state advances despite invalid event
  - Breaks deterministic replay and causal consistency
- Required fix:
  - Prevent dispatch entirely if invariant fails
  - Or implement transactional dispatch with rollback on failure

## 3. Explicit invariant bypass for LoopObserved (CRITICAL)
- Evidence:
  - Diagnostics: LoopObserved explicitly allowed to persist even if invariant fails
- Issue:
  - Violates invariant enforcement consistency
  - Introduces special-case control-flow outside invariant system
- Required fix:
  - Remove bypass
  - Ensure all event kinds are subject to invariant validation

## 4. Invariant engine is observational, not authoritative (CRITICAL)
- Evidence:
  - invariant_engine.observe returns false but system continues partial execution
- Issue:
  - Invariants do not gate system behavior
- Required fix:
  - Make invariant engine authoritative gate for event progression
  - Block append and dispatch on failure

## 5. System violates deterministic replay and no-hidden-state invariants
- Evidence:
  - Dispatch-before-append allows state changes not reflected in log
- Issue:
  - Replay(Σ) ≠ actual runtime state
- Required fix:
  - Ensure all state transitions are derived strictly from appended events
  - Eliminate side effects prior to log commit

## 6. System not spec-compliant
- Evidence:
  - Invariant enforcement order incorrect
  - Runtime state can diverge from event log
- Issue:
  - Violates core invariant: state = f(Σ)
- Required fix:
  - Enforce invariant validation before any state mutation or dispatch
  - Align runtime execution strictly with event-sourced model
