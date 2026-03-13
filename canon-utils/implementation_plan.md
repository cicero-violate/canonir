## Variables

(T=) tlog stream, (R=) replay state, (G=(N,E,F)) graph (nodes,edges,files)
(CFG=(BB,FL)) control-flow graph, (CG=(Fn,C)) call graph
(MG=(M,Contains,Imports)) module graph, (TG=(Type,UsesType)) type graph
(M_r=1) dedicated replay module
(G=\max(I,E,C,A,R,P,S,D,T,K,X,B,L,F)=\text{good})

## Equations

(R = Replay(T))
Explanation: the append-only tlog deterministically reconstructs the graph state.

(G = Build(R))
Explanation: replay produces canonical nodes, edges, and files.

(CFG,CG,MG,TG = f(G))
Explanation: all analyses derive graphs only from the canonical graph.

(M_r=1)
Explanation: only one module is allowed to read and replay `.tlog`.

---

# Full Implementation Plan for Coding Agent

Goal:
Create a **dedicated tlog replay system** and **remove replay logic from the reports crate**, while enabling full reconstruction of CFG, callgraph, module graph, and type graph.

---

# Phase 1 — Introduce Dedicated Tlog Replay Crate

Create new crate:

```
canon-utils/tlog-replay
```

### Cargo.toml

```
[package]
name = "canon-tlog-replay"
edition = "2021"

[dependencies]
serde
serde_json
canon-types
graph
anyhow
```

---

# Phase 2 — Move Replay Code out of `reports`

Move the following files:

```
reports/src/replay/
  tlog_reader.rs
  tlog_replay.rs
  session_scan.rs
```

to:

```
canon-utils/tlog-replay/src/
  reader.rs
  replay.rs
  session_scan.rs
```

Rename modules accordingly.

---

# Phase 3 — Implement Dedicated Tlog Reader

File

```
canon-utils/tlog-replay/src/reader.rs
```

Responsibilities

```
read tlog line-by-line
deserialize TlogEvent
handle aliases (N/E/F etc)
validate JSON
```

Core function

```
fn read_tlog_events(path: &Path) -> Result<Vec<TlogEvent>>
```

Implementation notes

```
BufReader
streamed JSON parsing
skip malformed lines with warning
```

---

# Phase 4 — Implement Deterministic Replay Engine

File

```
canon-utils/tlog-replay/src/replay.rs
```

Responsibilities

```
convert events → graph state
maintain symbol → node id mapping
maintain edge list
maintain file registry
```

Core struct

```
pub struct ReplayGraph {
    pub nodes: Vec<NodeRow>,
    pub edges: Vec<EdgeRow>,
    pub files: Vec<String>,
}
```

Core function

```
pub fn replay_graph(path: &Path) -> Result<ReplayGraph>
```

Replay algorithm

```
for event in events:
    match event:
        NODE -> register node
        NODE_UPDATE -> update node
        NODE_REMOVE -> delete node
        EDGE -> push edge
        EDGE_REMOVE -> delete edge
        FILE -> register file
        SESSION -> optionally reset session
```

---

# Phase 5 — Implement Incremental Replay

File

```
replay_incremental.rs
```

Function

```
replay_graph_from_offset(
    tlog_path,
    start_offset,
    existing_graph
)
```

Purpose

```
fast replay for large tlogs
used by reports and query engines
```

---

# Phase 6 — Build Canonical KernelGraph

File

```
canon-utils/tlog-replay/src/kernel_graph.rs
```

Structure

```
pub struct KernelGraph {
    nodes
    edges
    adjacency
    symbol_to_id
    files
}
```

Builder

```
fn build_kernel_graph(nodes,edges,files) -> KernelGraph
```

Also build:

```
CSR adjacency
callgraph adjacency
cfg adjacency
```

---

# Phase 7 — Enforce Single Tlog Reader

Search entire workspace for:

```
rg ".tlog"
```

Rules

```
only canon-tlog-replay may read tlog
all other crates depend on replay output
```

Remove tlog reading from

```
reports
canon-query
smt-analysis-engine
project_editor
```

---

# Phase 8 — Update Reports Crate

Remove

```
reports/src/replay/*
```

Replace with dependency

```
canon-tlog-replay
```

Modify entrypoint

```
reports/src/bin/reports_from_tlog.rs
```

Old

```
replay_graph_from_tlog()
```

New

```
let graph = canon_tlog_replay::replay_graph(tlog_path)?;
```

---

# Phase 9 — Graph Reconstruction Pipeline

Reports pipeline becomes

```
tlog
 ↓
canon-tlog-replay
 ↓
KernelGraph
 ↓
analysis modules
 ↓
report artifacts
```

---

# Phase 10 — Graph Builders

Add builder modules

```
reports/analysis/
  cfg.rs
  callgraph.rs
  modulegraph.rs
  typegraph.rs
```

Each consumes

```
KernelGraph
```

---

## CFG Builder

Inputs

```
BasicBlock nodes
Flow edges
HasBlock edges
```

Output

```
cfg adjacency
block owner mapping
```

---

## Callgraph Builder

Inputs

```
Function nodes
Call edges
Callsite nodes
```

Output

```
caller → callee adjacency
callgraph centrality
```

---

## Module Graph Builder

Inputs

```
Module nodes
Contains edges
Imports edges
```

Output

```
module dependency graph
```

---

## Type Graph Builder

Inputs

```
Type nodes
UsesType
ForType
Bounds
Implements
```

Output

```
type dependency graph
```

---

# Phase 11 — Replay Validation

Add deterministic replay verification

File

```
canon-utils/tlog-replay/src/verify.rs
```

Check

```
node ids deterministic
edge counts stable
hash graph signature
```

Function

```
verify_replay_determinism()
```

---

# Phase 12 — Panic Capture Integration

Use existing event

```
TlogEvent::PANIC
```

Extend replay

```
collect panic records
store in graph metadata
```

Expose

```
graph.panic_records
```

Reports module

```
analysis/panic_report.rs
```

---

# Phase 13 — Snapshot Support

Add snapshot module

```
canon-utils/tlog-replay/src/snapshot.rs
```

Artifacts

```
graph_snapshot.bin
snapshot_meta.json
```

Usage

```
replay snapshot + tail of tlog
```

---

# Phase 14 — Performance Optimizations

Implement

```
CSR graph building
parallel replay
memory pooling
```

Possible future

```
GPU replay support
```

---

# Phase 15 — Update Canon Query

Modify

```
canon-query/src/tlog.rs
```

Remove tlog reading.

Instead load

```
graph_bin
nodes.csv
edges.csv
```

or

```
KernelGraph
```

---

# Phase 16 — Update SMT Engine

Modify

```
smt-analysis-engine/loader.rs
```

Replace

```
tlog parsing
```

with

```
KernelGraph loader
```

---

# Phase 17 — Update Project Editor

Modify

```
project_editor/query/session.rs
```

Use

```
KernelGraph
symbol index
```

instead of

```
tlog
```

---

# Final Architecture

```
canon_kernel
     │
     │ append-only
     ▼
.tlog
     │
     ▼
canon-tlog-replay
     │
     ▼
KernelGraph
     │
 ┌──────┬─────────┬─────────┬─────────┐
 ▼      ▼         ▼         ▼
reports canon-query smt-engine project-editor
```

---

# Acceptance Criteria

1. Only one crate reads `.tlog`
2. Reports pipeline still generates

```
CFG
Callgraph
Module graph
Type graph
Semantic clusters
Invariant reports
```

3. Replay deterministic
4. Snapshot replay works
5. All existing reports compile and run

---

If you send the **repomap of the whole workspace**, I can produce the **exact patch plan (file-by-file edits)** for the coding agent.
