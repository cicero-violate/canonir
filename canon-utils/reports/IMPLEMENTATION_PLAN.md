## Variables

(T) = `kernel.tlog`
(N) = node set
(E) = edge set
(G) = kernel graph
(A) = artifacts
(M) = metrics
(I) = invariants
(S) = semantic analysis

## Equations

[
(N,E) = replay(T)
]

[
G = (N,E)
]

[
A = serialize(G)
]

[
M = metrics(G)
]

[
I = validate(G)
]

[
S = cluster(features(G))
]

Explanation: reports reconstruct a graph from the event log, serialize artifacts, compute metrics, validate invariants, and derive semantic structure.

---

# Target Architecture

The current system mixes **replay, graph building, analysis, and report writing** in `reports.rs`.

Refactor into **5 subsystems**.

```
reports/
├── bin/
│   ├── reports_from_tlog.rs
│   └── invariants_from_graph.rs
│
├── replay/
│   ├── mod.rs
│   ├── tlog_reader.rs
│   ├── tlog_replay.rs
│   └── session_scan.rs
│
├── graph/
│   ├── mod.rs
│   ├── graph_types.rs
│   ├── graph_builder.rs
│   ├── csr.rs
│   └── graph_normalize.rs
│
├── artifacts/
│   ├── mod.rs
│   ├── artifact_writer.rs
│   ├── snapshot.rs
│   └── cache.rs
│
├── analysis/
│   ├── mod.rs
│   ├── cfg.rs
│   ├── callgraph.rs
│   ├── dead_code.rs
│   ├── dependency_cycles.rs
│   ├── structural_hotspots.rs
│   └── dataflow.rs
│
├── invariants/
│   ├── mod.rs
│   ├── kernel_invariants.rs
│   ├── invariant_discovery.rs
│   ├── invariant_validator.rs
│   ├── invariant_sat.rs
│   └── invariant_generator.rs
│
├── semantics/
│   ├── mod.rs
│   ├── semantic_features.rs
│   ├── semantic_signature.rs
│   ├── semantic_clustering.rs
│   ├── semantic_fingerprints.rs
│   └── pattern_mining.rs
│
├── health/
│   ├── mod.rs
│   ├── graph_health.rs
│   ├── tlog_integrity.rs
│   └── system_health.rs
│
├── ingest/
│   ├── mod.rs
│   └── report_ingest.rs
│
├── repair/
│   ├── mod.rs
│   └── errors.rs
│
└── consumer/
    ├── mod.rs
    └── report_consumer.rs
```

---

# Rename Plan

### Replay Layer

Current:

```
read_tlog_graph
parse_tlog_line
replay_tlog_from_offset
apply_tlog_record
```

Rename:

```
read_tlog_graph → replay_graph_from_tlog
parse_tlog_line → parse_tlog_event
replay_tlog_from_offset → replay_events_from_offset
apply_tlog_record → apply_event_to_graph
```

Move to:

```
replay/tlog_replay.rs
```

---

### Session Logic

Current:

```
read_last_session_offset
find_last_session_with_graph_events_offset
tlog_last_session_has_module_nodes
```

Rename:

```
find_last_session_offset
find_last_graph_session_offset
session_contains_module_nodes
```

Move to:

```
replay/session_scan.rs
```

---

### Graph Builder

Current:

```
build_kernel_graph_from_rows
normalize_graph_rows
remove_node_by_id
```

Rename:

```
rows_to_kernel_graph
normalize_graph
delete_node
```

Move to:

```
graph/graph_builder.rs
```

---

### CSR Graph

Current:

```
build_csr
build_callgraph_csr
```

Rename:

```
build_csr_graph
build_callgraph_csr_graph
```

Move to:

```
graph/csr.rs
```

---

### Artifact Writers

Current:

```
write_graph_artifacts
write_graph_bin
write_cfg_csv
write_callgraph_csv
write_modulegraph_csv
write_typegraph_csv
```

Rename:

```
emit_graph_artifacts
emit_graph_bin
emit_cfg_csv
emit_callgraph_csv
emit_modulegraph_csv
emit_typegraph_csv
```

Move to:

```
artifacts/artifact_writer.rs
```

---

### Snapshot System

Current:

```
write_kernel_snapshot
load_kernel_snapshot
read_snapshot_meta
write_snapshot_meta
estimate_snapshot_bytes
```

Rename:

```
save_graph_snapshot
load_graph_snapshot
read_snapshot_metadata
write_snapshot_metadata
estimate_snapshot_size
```

Move to:

```
artifacts/snapshot.rs
```

---

### Cache System

Current:

```
load_and_update_graph_cache
apply_cache_record
```

Rename:

```
update_graph_cache
apply_cache_event
```

Move to:

```
artifacts/cache.rs
```

---

### CFG Analysis

Move:

```
build_cfg_edges
build_cfg_out
build_cfg_in
trace_path
```

to

```
analysis/cfg.rs
```

Rename:

```
build_cfg_edges → extract_cfg_edges
```

---

### Callgraph Analysis

Move:

```
build_callgraph_edges
build_callgraph_adj
dfs_callgraph
find_callgraph_roots
```

to

```
analysis/callgraph.rs
```

Rename:

```
build_callgraph_edges → extract_callgraph_edges
```

---

### Dead Code

Move:

```
build_dead_code
build_dead_code_gpu
```

to

```
analysis/dead_code.rs
```

Rename:

```
build_dead_code → detect_dead_code
```

---

### Dependency Cycles

Move:

```
tarjan_scc
build_dependency_cycles
```

to

```
analysis/dependency_cycles.rs
```

Rename:

```
tarjan_scc → compute_scc
```

---

### Structural Hotspots

Move:

```
build_structural_hotspots
build_branch_complexity
build_branch_pressure
build_merge_candidates
```

to

```
analysis/structural_hotspots.rs
```

---

### Dataflow

Move:

```
build_dataflow_fanout
```

to

```
analysis/dataflow.rs
```

---

### Health Reports

Move:

```
write_graph_health_report
hash_graph_signature
```

to

```
health/graph_health.rs
```

Move:

```
write_tlog_integrity_report
apply_tlog_integrity_record
```

to

```
health/tlog_integrity.rs
```

Move:

```
write_system_health_report
```

to

```
health/system_health.rs
```

---

### Semantics

Keep existing but move into:

```
semantics/
```

Files:

```
semantic_features.rs
semantic_signature.rs
semantic_clustering.rs
semantic_fingerprints.rs
pattern_mining.rs
```

---

### Invariants

Group all invariant files:

```
invariants/
```

Files:

```
kernel_invariants.rs
invariant_discovery.rs
invariant_validator.rs
invariant_generator.rs
invariant_sat.rs
```

---

### Error Surface

Move:

```
errors.rs
```

to

```
repair/error_surface.rs
```

Rename:

```
compute_repair_surface → build_repair_surface
```

---

### Event Consumer

Move:

```
consumer.rs
```

to

```
consumer/report_consumer.rs
```

Rename:

```
ReportConsumer → ReportEventConsumer
```

---

# Entry Binaries

```
bin/
```

Keep:

```
reports_from_tlog.rs
invariants_from_graph.rs
```

Refactor:

```
reports_from_tlog.rs
```

Pipeline:

```
replay_graph_from_tlog
→ normalize_graph
→ emit_graph_artifacts
→ run_analysis
→ write_reports
```

---

# Refactor Execution Order

Agent must follow this sequence:

### Step 1

Split `reports.rs` into:

```
replay/
graph/
artifacts/
analysis/
health/
```

---

### Step 2

Move invariant modules into:

```
invariants/
```

---

### Step 3

Move semantic modules into:

```
semantics/
```

---

### Step 4

Rename all functions according to table above.

---

### Step 5

Create `mod.rs` for each subsystem.

Example:

```
analysis/mod.rs
```

```
pub mod cfg;
pub mod callgraph;
pub mod dead_code;
pub mod dependency_cycles;
pub mod structural_hotspots;
pub mod dataflow;
```

---

### Step 6

Update imports across crate.

Example:

```
use crate::analysis::cfg::extract_cfg_edges;
```

---

### Step 7

Verify build:

```
cargo build
```

---

# Final Result

Clean layered architecture:

```
tlog
 ↓
replay
 ↓
graph
 ↓
artifacts
 ↓
analysis
 ↓
invariants / semantics
 ↓
reports
```

---

[
good = \max(intelligence, efficiency, correctness, alignment, robustness, performance, scalability, determinism, transparency, collaboration, empowerment, benefit, learning, future\text{-}proofing)
]
