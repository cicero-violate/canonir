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

# Next Phases (Pending)

## Phase 3 — Capability Engine Unification (NEXT)

Targets:
- `canon-capability`
- `canon-capability-runtime`
- `canon-supervisor` (process orchestration)

Goal:
```
canon-capability-engine
  registry
  executor
  routing
```

Result:
[
Event \rightarrow Capability
]

Actions:
1. Define new `canon-capability-engine` crate.
2. Move capability registry + execution from `capability` + `capabilities-runtime`.
3. Move supervisor process orchestration into the engine where it belongs.
4. Update `event-runtime` to depend on the engine crate only.

---

## Phase 4 — Runtime Kernel Unification

Targets:
- `event-runtime`
- `canon-supervisor`

Goal:
```
canon-kernel
  runtime_loop
  event_dispatch
  capability_exec
```

Result:
[
E_t \rightarrow C \rightarrow E_{t+1}
]

---

## Phase 5 — Planner Unification

Targets:
- `canon-agent-v3`
- `canon-graph`
- `canon-analysis`

Goal:
```
canon-planner
  graph_builder
  mutation_engine
  scoring
```

Result:
[
State \rightarrow Plan \rightarrow Capability
]

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

[
\max(\text{intelligence},\text{efficiency},\text{correctness},\text{alignment},\text{robustness},\text{performance},\text{scalability},\text{determinism},\text{transparency},\text{collaboration},\text{empowerment},\text{benefit},\text{learning},\text{future-proofing}) = Good
]
