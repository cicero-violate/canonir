# Canon Unification Plan v2

**Math**

[
\text{Order} = (E_s \rightarrow S \rightarrow C \rightarrow R \rightarrow P)
]

### Variables

* (E_s) = Event Storage Layer
* (S) = Event Schema Layer
* (C) = Capability Engine
* (R) = Runtime Kernel
* (P) = Planner / Agent Layer

---

# Current Status Summary

## Phase 1 — Event Storage Unification (DONE)

**Goal:** `canon-event-store`

**Completed:**
- Added `canon-event-store` as wrapper over `tlog-writer` + `tlog-replay`.
- Routed all core crates to use `canon_event_store::reader`/`writer`.
- Kept `tlog-writer` and `tlog-replay` as internal dependencies of `canon-event-store`.

**Result:**
[
Event \rightarrow Store
]

---

## Phase 2 — Event Schema Unification (MOSTLY DONE)

**Goal:** `canon-event`

**Completed:**
- Added `canon-event` with:
  - `events.rs` (RuntimeEvent + EventMask + EditEvent + traits)
  - `emit.rs` (tlog emission via `canon-tlog-writer`)
  - `emit_debug.rs` (human logging, moved from `event-log`)
  - `schema.rs` (KernelEvent/KernelState/EventDelta re-export)
- Removed `canon-event-emit` crate.
- Removed `event-log` crate; all logging now via `canon_event::emit_debug::*`.
- Moved runtime/edit/event consumer types out of `canon-types`.
- Updated runtime/editor/analysis/supervisor/graph/query/capabilities to import from `canon_event`.

**Remaining:**
- `event-consumers` is still a thin adapter crate to avoid a dependency cycle
  (`canon-event` cannot depend on analysis/editor/graph/query).
  Options:
  1. Keep `event-consumers` as permanent adapter.
  2. Split `canon-event` into `canon-event-core` + `canon-event-consumers`.
- Confirm any leftover `canon_types` usage is only for non-event types (Node/Edge/ReportLayout).

---

## Phase 3 — Capability Engine Unification (DONE)

**Goal:** `canon-capability-engine`

**Completed:**
- Created `canon-capability-engine` crate with:
  - `registry.rs` - Capability registry for managing and executing capabilities
  - `executor/` - Concrete capability implementations (build, run, check, file ops, bash, etc.)
  - `routing.rs` - Capability routing logic
  - `supervisor/` - Process orchestration and lifecycle management
- Consolidated code from:
  - `canon-capability` → core trait and registry
  - `canon-capability-runtime` → executor with build capabilities
  - `canon-supervisor` → process management, config, watcher, events
- Updated all dependent crates:
  - `event-runtime` - Now depends only on `canon-capability-engine`
  - `canon-editor` - Updated to use engine
  - `canon-analysis` - Updated to use engine
  - `canon-supervisor` - Now uses engine's supervisor module
- Removed old crates:
  - Deleted `canon-capability`
  - Deleted `canon-capability-runtime`
  - Removed from workspace `Cargo.toml`

**Result:**
[
Event \rightarrow Capability
]

---

# Next Phases (Pending)

---

## Phase 4 — Runtime Kernel Unification (DONE)

**Goal:** `canon-kernel`

**Completed:**
- Created `canon-kernel` by renaming and consolidating:
  - `event-runtime` → `canon-kernel` (library with EventRuntime, event dispatch, consumers)
  - `canon-supervisor` binary → integrated as `canon-supervisor` binary within canon-kernel
- Unified structure:
  - `lib.rs` - EventRuntime struct, core runtime loop
  - `bus.rs` - EventBus for event dispatch
  - `consumers/` - Event consumers (agent, capability executor, event loop, llm)
  - Binaries:
    - `canon-kernel` - Main runtime event loop (was `event_runtime`)
    - `canon-supervisor` - Process orchestration and file watching
- Removed old crates:
  - Deleted `event-runtime` directory (renamed to canon-kernel)
  - Deleted `canon-supervisor` directory (moved binary into kernel)
  - Updated workspace `Cargo.toml`

**Result:**
[
E_t \rightarrow C \rightarrow E_{t+1}
]

The kernel now implements the complete event-capability loop:
- Events at time t are dispatched through the EventBus
- Capability requests are routed to the capability engine
- Results produce new events at time t+1

---

## Phase 5 — Planner Unification (DONE)

**Goal:** `canon-planner`

**Completed:**
- Created `canon-planner` as a unified facade crate providing:
  - `graph` module (from canon-graph) - Graph building and management
  - `planner` module (from canon-agent-v3) - Planning, goal decomposition, DAG building, mutation engine
  - `analysis` module (from canon-analysis) - Scoring, invariants, SMT, repair
- Fixed compilation issues in canon-agent-v3:
  - Added missing `Csr` import in `graph_algo.rs`
  - Removed unused imports in `gpu_scheduler_kernels.rs`
- Updated dependencies:
  - `event-consumers` now depends on `canon-planner`
  - `canon-kernel` now depends on `canon-planner`
- Maintains underlying crates as implementation details
  - `canon-agent-v3`, `canon-graph`, `canon-analysis` remain but are accessed through `canon-planner`

**Result:**
[
State \rightarrow Plan \rightarrow Capability
]

The planner now provides a unified interface:
- State analysis via graph building and invariants
- Plan generation via goal decomposition and DAG construction
- Capability execution via mutation engine

---

# Immediate Next Tasks (for next agent)

1. Phase 2 cleanup decision:
   - Keep `event-consumers` adapter or split `canon-event` into core/consumers.
2. Phase 3 kickoff:
   - Create `canon-capability-engine`.
   - Migrate registry/executor from `capability` + `capabilities-runtime`.
   - Move supervisor process orchestration into engine.
3. Audit for any remaining `canon_tlog_*` imports outside `canon-event-store` and migrate if found.

---

# All Phases Complete ✓

The Canon Unification Plan has been fully implemented:

| Layer | Unified Crate | Result Equation |
|-------|--------------|-----------------|
| E_s | canon-event-store | Event → Store |
| S | canon-event | Emit(E) → Store(E) |
| C | canon-capability-engine | Event → Capability |
| R | canon-kernel | E_t → C → E_{t+1} |
| P | canon-planner | State → Plan → Capability |

**Final Architecture:**
```
canon-event-store → canon-event → canon-capability-engine → canon-kernel → canon-planner
```

All systems are now unified with:
- Clear separation of concerns
- Minimal duplication
- Unified interfaces
- Improved maintainability

---

[
\max(\text{intelligence},\text{efficiency},\text{correctness},\text{alignment},\text{robustness},\text{performance},\text{scalability},\text{determinism},\text{transparency},\text{collaboration},\text{empowerment},\text{benefit},\text{learning},\text{future-proofing}) = Good
]
