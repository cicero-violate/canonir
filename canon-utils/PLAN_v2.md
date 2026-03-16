# Canon Unification Plan v2

**Goal:** Reduce 14 workspace crates to ~9 by merging thin/redundant crates, deleting dead ones,
and splitting one oversized crate. Each phase is independently shippable and leaves the workspace
in a compilable state.

---

## Current Dependency Graph

```
canon-types
  └─► canon-tlog-writer
        └─► canon-event
              └─► canon-tlog-replay
                    └─► canon-event-store  (thin facade: re-exports tlog-writer + tlog-replay)
                          └─► canon-graph
                          └─► canon-capability-engine
                                └─► canon-analysis
                                      └─► canon-planner  (thin facade: re-exports graph + analysis + agent-v3)
                                            └─► event-consumers  (single fn: build_consumers)
                                                  └─► canon-kernel
                                                  └─► canon-query  ──► event-consumers
                                                  └─► canon-editor ──► canon-kernel
                                                  └─► canon-agent-v3
```

---

## Target Dependency Graph (after all phases)

```
canon-types
  └─► canon-event          (absorbs: tlog-writer)
        └─► canon-event-store  (absorbs: tlog-replay — now real code, not a facade)
              └─► canon-graph
              └─► canon-capability  (split from canon-capability-engine: core trait/registry)
              └─► canon-supervisor  (split from canon-capability-engine: process mgmt)
                    └─► canon-analysis  (absorbs: canon-query)
                          └─► canon-planner  (keep: deliberate aggregation facade)
                                └─► canon-kernel  (absorbs: event-consumers)
                                └─► canon-editor
```

**Crates deleted:** `tlog-writer`, `tlog-replay`, `event-consumers`, `canon-query` (4 deleted)
**Crates split:** `canon-capability-engine` → `canon-capability` + `canon-supervisor` (+1 net)
**Net change:** 14 → 11 crates

---

## Phase 1 — Absorb `tlog-writer` into `canon-event`

**Rationale:** `canon-event` already depends on `canon-tlog-writer` and is its only real consumer
above the type layer. They are the same layer: raw I/O format + event schema + emission. Merging
removes one indirection and makes `canon-event` the single owner of "how events are written to disk."

### Files to move

```
tlog-writer/src/binary.rs   → canon-event/src/tlog/binary.rs
tlog-writer/src/event.rs    → canon-event/src/tlog/event.rs
tlog-writer/src/rotate.rs   → canon-event/src/tlog/rotate.rs
tlog-writer/src/writer.rs   → canon-event/src/tlog/writer.rs
tlog-writer/src/bin/emit_capability_event.rs  → canon-event/src/bin/emit_capability_event.rs
tlog-writer/src/bin/emit_kernel_event.rs      → canon-event/src/bin/emit_kernel_event.rs
```

### canon-event/src/lib.rs additions

```rust
pub mod tlog;
pub use tlog::{
    is_binary_tlog, BinarySegmentWriter, BinaryTlogWriter, SegmentConfig,
    CanonEvent,
    maybe_rotate, RotateConfig,
    append_event, append_event_json, TlogWriter,
};
```

### Cargo.toml changes

| File | Change |
|------|--------|
| `canon-event/Cargo.toml` | Remove `canon-tlog-writer` dep; add its direct deps (`sha2`, `memmap2`, etc.) inline |
| `canon-event-store/Cargo.toml` | Change `canon-tlog-writer` dep → `canon-event` |
| `tlog-replay/Cargo.toml` | Change `canon-tlog-writer` dep → `canon-event` |
| Root `Cargo.toml` | Remove `canon-utils/tlog-writer` from `[workspace] members` |

### Symbol renames

| Old import | New import |
|-----------|-----------|
| `canon_tlog_writer::CanonEvent` | `canon_event::CanonEvent` |
| `canon_tlog_writer::BinaryTlogWriter` | `canon_event::BinaryTlogWriter` |
| `canon_tlog_writer::BinarySegmentWriter` | `canon_event::BinarySegmentWriter` |
| `canon_tlog_writer::append_event` | `canon_event::append_event` |
| `canon_tlog_writer::append_event_json` | `canon_event::append_event_json` |
| `canon_tlog_writer::TlogWriter` | `canon_event::TlogWriter` |
| `canon_tlog_writer::maybe_rotate` | `canon_event::maybe_rotate` |
| `canon_tlog_writer::RotateConfig` | `canon_event::RotateConfig` |
| `canon_tlog_writer::SegmentConfig` | `canon_event::SegmentConfig` |
| `canon_tlog_writer::is_binary_tlog` | `canon_event::is_binary_tlog` |

### Delete

```
canon-utils/tlog-writer/   (entire directory)
```

---

## Phase 2 — Absorb `tlog-replay` into `canon-event-store`

**Rationale:** `canon-event-store` is currently a zero-code facade that re-exports from
`tlog-writer` (gone after Phase 1) and `tlog-replay`. The entire point of `canon-event-store`
is to be the event persistence API — it should own that code, not re-export it. Moving
`tlog-replay`'s code in makes `canon-event-store` real.

### Files to move

```
tlog-replay/src/binary_reader.rs  → canon-event-store/src/binary_reader.rs
tlog-replay/src/graph_types.rs    → canon-event-store/src/graph_types.rs
tlog-replay/src/reader.rs         → canon-event-store/src/reader.rs
tlog-replay/src/replay.rs         → canon-event-store/src/replay.rs
tlog-replay/src/session_scan.rs   → canon-event-store/src/session_scan.rs
tlog-replay/src/snapshot.rs       → canon-event-store/src/snapshot.rs
tlog-replay/src/bin/verify_tlog_equivalence.rs → canon-event-store/src/bin/verify_tlog_equivalence.rs
```

### canon-event-store/src/lib.rs rewrite

Replace the current pure re-export facade with real module declarations:

```rust
pub mod binary_reader;
pub mod graph_types;
pub mod reader;
pub mod replay;
pub mod session_scan;
pub mod snapshot;

// writer side — re-exported from canon-event (which absorbed tlog-writer in Phase 1)
pub mod writer {
    pub use canon_event::{
        append_event_json, BinarySegmentWriter, BinaryTlogWriter, CanonEvent,
        TlogWriter, append_event, maybe_rotate, RotateConfig, SegmentConfig,
    };
}

pub mod schema {
    pub use canon_event::CanonEvent;
}

// Flat re-exports for convenience (maintain compat with existing callers)
pub use binary_reader::{is_binary_magic, read_binary_events};
pub use graph_types::{EdgeRow, NodeRow, ReplayGraph};
pub use reader::{
    AnyEvent, TlogFormat,
    detect_tlog_format, extract_capability_request, extract_edit_event,
    extract_kernel_event, extract_supervisor_event,
    parse_any_event, parse_capability_request_value, parse_edit_event_value,
    parse_kernel_event_value, read_any_events, read_any_events_from_path,
    read_any_events_from_path_with_start_seq,
};
pub use replay::{
    replay_graph_from_tlog, replay_graph_from_tlog_incremental,
    replay_events_from_offset, rebuild_symbol_index,
};
pub use session_scan::{
    find_last_graph_session_offset, find_last_session_offset,
    session_contains_module_nodes,
};
pub use snapshot::{
    SnapshotMeta, load_graph_snapshot, read_snapshot_metadata,
    save_graph_snapshot, snapshot_into_rows, write_snapshot_metadata,
};
```

### Cargo.toml changes

| File | Change |
|------|--------|
| `canon-event-store/Cargo.toml` | Remove `canon-tlog-replay` dep; add `canon-event` dep; add any deps from tlog-replay not already present |
| Root `Cargo.toml` | Remove `canon-utils/tlog-replay` from `[workspace] members` |

### Symbol renames

Callers of `canon_tlog_replay::*` switch to `canon_event_store::*`. Most were already
mirrored in the facade's `reader` module, so many callers already use `canon_event_store::reader::`.
Audit for any remaining direct `canon_tlog_replay::` imports.

| Old import | New import |
|-----------|-----------|
| `canon_tlog_replay::read_binary_events` | `canon_event_store::read_binary_events` |
| `canon_tlog_replay::AnyEvent` | `canon_event_store::AnyEvent` |
| `canon_tlog_replay::replay_graph_from_tlog` | `canon_event_store::replay_graph_from_tlog` |
| `canon_tlog_replay::SnapshotMeta` | `canon_event_store::SnapshotMeta` |
| `canon_tlog_replay::save_graph_snapshot` | `canon_event_store::save_graph_snapshot` |
| `canon_tlog_replay::EdgeRow` | `canon_event_store::EdgeRow` |
| `canon_tlog_replay::NodeRow` | `canon_event_store::NodeRow` |
| `canon_tlog_replay::ReplayGraph` | `canon_event_store::ReplayGraph` |
| *(all other tlog_replay exports)* | `canon_event_store::<same_name>` |

### Delete

```
canon-utils/tlog-replay/   (entire directory)
```

---

## Phase 3 — Absorb `canon-query` into `canon-analysis`

**Rationale:** `canon-query` is used by exactly one crate (`event-consumers`) and exposes
query/consumer functionality that belongs with analysis. Its `QueryConsumer` is peer to
`SmtConsumer`, `ReportEventConsumer`, `CapabilityEventConsumer` — all of which live in
`canon-analysis`. Its `query_file` / `QueryOptions` / jsonpath modules are analytical
query tools that fit naturally there.

### Files to move

```
canon-query/src/consumer.rs  → canon-analysis/src/query/consumer.rs
canon-query/src/gpu.rs       → canon-analysis/src/query/gpu.rs
canon-query/src/jsonpath.rs  → canon-analysis/src/query/jsonpath.rs
canon-query/src/lib.rs       → canon-analysis/src/query/mod.rs  (adapt exports)
```

### canon-analysis/src/lib.rs additions

```rust
pub mod query;
pub use query::{QueryConsumer, QueryOptions, QueryError, TlogQueryResult, query_file, query_file_single};
```

### Cargo.toml changes

| File | Change |
|------|--------|
| `canon-analysis/Cargo.toml` | Add any deps from canon-query not already present |
| `event-consumers/Cargo.toml` | Change `canon-query` dep → `canon-analysis` (already a transitive dep via canon-planner) |
| Root `Cargo.toml` | Remove `canon-utils/canon-query` from `[workspace] members` |

### Symbol renames

| Old import | New import |
|-----------|-----------|
| `canon_query::QueryConsumer` | `canon_analysis::QueryConsumer` |
| `canon_query::QueryOptions` | `canon_analysis::QueryOptions` |
| `canon_query::QueryError` | `canon_analysis::QueryError` |
| `canon_query::TlogQueryResult` | `canon_analysis::TlogQueryResult` |
| `canon_query::query_file` | `canon_analysis::query_file` |
| `canon_query::query_file_single` | `canon_analysis::query_file_single` |

### Delete

```
canon-utils/canon-query/   (entire directory)
```

---

## Phase 4 — Absorb `event-consumers` into `canon-kernel`

**Rationale:** `event-consumers` contains exactly one file and one public function:
`build_consumers()`. It wires together consumers from `canon-planner`, `canon-analysis`
(via planner), `canon-editor`, and `canon-query` (absorbed in Phase 3). This is kernel
startup logic — it belongs in the kernel.

### Files to move

```
event-consumers/src/lib.rs  →  canon-kernel/src/consumers/registry.rs
```

### canon-kernel/src/consumers/mod.rs additions

```rust
pub mod registry;
pub use registry::build_consumers;
```

### Cargo.toml changes

| File | Change |
|------|--------|
| `canon-kernel/Cargo.toml` | Add `canon-analysis` dep (for `QueryConsumer` now there after Phase 3); remove `event-consumers` dep if present |
| Root `Cargo.toml` | Remove `canon-utils/event-consumers` from `[workspace] members` |

### Symbol renames

| Old import | New import |
|-----------|-----------|
| `canon_event_consumers::build_consumers` | `canon_kernel::consumers::build_consumers` |

### Delete

```
canon-utils/event-consumers/   (entire directory)
```

---

## Phase 5 (Optional) — Split `canon-capability-engine` into `canon-capability` + `canon-supervisor`

**Rationale:** `canon-capability-engine` mixes two unrelated concerns:
- **Core capability pattern** (`trait.rs`, `registry.rs`, `context.rs`, `result.rs`, `routing.rs`) —
  needed by `canon-editor` and `canon-analysis`
- **Supervisor / process management** (`supervisor/`) — file watching, process restart, config;
  only needed by `canon-kernel`
- **Build executor** (`executor/`) — cargo build/check/run; only needed by `canon-kernel`

Splitting removes the supervisor machinery from `canon-editor` and `canon-analysis`'s transitive
dependency graph.

### New crate: `canon-capability` (core)

```
canon-utils/canon-capability/
  Cargo.toml   (name = "canon-capability")
  src/
    lib.rs       (exports: Capability, CapabilityRegistry, CapabilityContext, CapabilityResult)
    trait.rs     (moved from canon-capability-engine)
    registry.rs
    context.rs
    result.rs
    routing.rs
```

Deps: `canon-event`, `canon-event-store`

### Renamed crate: `canon-supervisor` (was `canon-capability-engine`)

```
canon-utils/canon-supervisor/
  Cargo.toml   (name = "canon-supervisor")
  src/
    lib.rs       (re-exports canon-capability core + supervisor + executor)
    executor.rs + executor/
    supervisor.rs + supervisor/
```

Deps: `canon-capability`, `canon-event`, `canon-event-store`

Re-export the core types so `canon_supervisor::CapabilityRegistry` resolves,
avoiding a flag day for canon-kernel call sites.

### Cargo.toml changes

| File | Change |
|------|--------|
| `canon-editor/Cargo.toml` | Change `canon-capability-engine` → `canon-capability` |
| `canon-analysis/Cargo.toml` | Change `canon-capability-engine` → `canon-capability` |
| `canon-kernel/Cargo.toml` | Change `canon-capability-engine` → `canon-supervisor` |
| Root `Cargo.toml` | Replace `canon-utils/canon-capability-engine` with `canon-utils/canon-capability` + `canon-utils/canon-supervisor` |

### Symbol renames

| Old import | New import | Used by |
|-----------|-----------|---------|
| `canon_capability_engine::Capability` | `canon_capability::Capability` | canon-editor, canon-analysis |
| `canon_capability_engine::CapabilityRegistry` | `canon_capability::CapabilityRegistry` | canon-editor, canon-analysis, canon-kernel |
| `canon_capability_engine::CapabilityContext` | `canon_capability::CapabilityContext` | canon-editor, canon-analysis |
| `canon_capability_engine::CapabilityResult` | `canon_capability::CapabilityResult` | canon-editor, canon-analysis |
| `canon_capability_engine::BuildEvent` | `canon_supervisor::BuildEvent` | canon-kernel |
| `canon_capability_engine::BuildRequest` | `canon_supervisor::BuildRequest` | canon-kernel |
| `canon_capability_engine::CAP_BUILD_CARGO` | `canon_supervisor::CAP_BUILD_CARGO` | canon-kernel |
| `canon_capability_engine::SupervisorConfig` | `canon_supervisor::SupervisorConfig` | canon-kernel |
| `canon_capability_engine::ProcessManager` | `canon_supervisor::ProcessManager` | canon-kernel |
| `canon_capability_engine::start_watcher` | `canon_supervisor::start_watcher` | canon-kernel |
| `canon_capability_engine::affected_crates` | `canon_supervisor::affected_crates` | canon-kernel |

### Delete

```
canon-utils/canon-capability-engine/   (entire directory, replaced by two new crates)
```

---

## Summary Table

| Crate | Action | Net |
|-------|--------|-----|
| `tlog-writer` | Absorbed into `canon-event` | **-1** |
| `tlog-replay` | Absorbed into `canon-event-store` | **-1** |
| `canon-query` | Absorbed into `canon-analysis` | **-1** |
| `event-consumers` | Absorbed into `canon-kernel` | **-1** |
| `canon-capability-engine` | Split → `canon-capability` + `canon-supervisor` | **±0** |
| `canon-event` | Gains tlog-writer code | grows |
| `canon-event-store` | Gains tlog-replay code, becomes real | grows |
| `canon-analysis` | Gains query modules | grows |
| `canon-kernel` | Gains build_consumers | grows |

**Phases 1–4: 14 → 10 crates**
**Phase 5: 10 → 11 crates** (split adds 1, canonical naming gain)

---

## Crates Unchanged

| Crate | Why |
|-------|-----|
| `canon-types` | Foundational, no deps, imported everywhere |
| `canon-graph` | Self-contained graph management |
| `canon-agent-v3` | Large, complex, well-scoped planning engine |
| `canon-planner` | Deliberate aggregation facade — keeps kernel's dep list manageable |
| `canon-editor` | Substantial editor functionality, not just a consumer |

---

## Doc Files to Delete (no code change needed)

Completed or superseded planning artifacts:

```
canon-utils/implementation_plan_v2.md
canon-utils/PHASE3_COMPLETION.md
canon-utils/PHASE4_COMPLETION.md
canon-utils/PHASE5_COMPLETION.md
canon-utils/PLAN.md
canon-utils/event-log/              (empty directory, not in workspace)
```

---

## Recommended Execution Order

```
Phase 1  →  Phase 2  →  Phase 3  →  Phase 4  →  Phase 5 (optional)
```

Run `cargo check --workspace` after each phase before proceeding.
Phases 3 and 4 can be done in either order.
Phase 5 is independent of 3 and 4.
