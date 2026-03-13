### Variables

[
L = \text{Canonical binary log}
]

[
E = {CanonEvent_i}
]

[
R = \text{Replay pipeline}
]

[
T = \text{Tlog writer}
]

[
C = \text{Consumers}
]

[
S = \text{Segments}
]

---

### Equations

**Unified Log**

[
L = append(E_i)
]

All system events append to one binary log.

*Explanation:* Kernel, supervisor, and agent share the same event history.

---

**Segment Rotation**

[
L = \bigcup_{i=0}^{n} S_i
]

Segments are rotated append files.

*Explanation:* Enables infinite history with bounded files.

---

**Deterministic Replay**

[
R(L) \rightarrow (Graph,State)
]

Replay reconstructs the system.

*Explanation:* State derives from event history.

---

# Implementation Plan for Coding Agent

## Phase 1 — Remove JSONL Path Completely

### Objective

Ensure binary segments are the only log when `CANON_TLOG_FORMAT=binary`.

### Tasks

1. Audit writers

   * `canon_kernel/src/log/tlog_writer.rs`
   * `canon-supervisor/src/tlog.rs`
   * `canon-agent-v2/src/tlog.rs`

2. Enforce binary-only logic

```rust
if env("CANON_TLOG_FORMAT") == "binary" {
    write_binary_segment(event);
} else {
    write_jsonl(event);
}
```

3. Remove JSON index writes in binary mode.

4. Verify no `.tlog` JSON lines appear when binary mode enabled.

---

## Phase 2 — Retention Control

### Objective

Prevent infinite disk growth.

### Environment Variable

```
CANON_TLOG_RETAIN_SEGMENTS=10
```

### Implementation

File:

```
canon-utils/tlog-writer/src/rotate.rs
```

Algorithm

```
segments = list_segments()
if len(segments) > retain:
    delete oldest
```

Trigger retention on:

```
segment rotation
writer open
```

---

## Phase 3 — Binary Log Detection

### Objective

Allow tools to read binary logs automatically.

File:

```
canon-utils/reports/src/bin/reports_from_tlog.rs
```

Detection logic:

```
if path.is_dir():
    use binary replay
else:
    use JSONL replay
```

Example

```
reports_from_tlog --tlog kernel.tlog.d
```

---

## Phase 4 — CanonEvent Schema Stabilization

### Objective

Define stable event schema.

File

```
canon-utils/tlog-writer/src/event.rs
```

Canonical format

```rust
struct CanonEvent {
    ts: u64,
    kind: String,
    payload: serde_json::Value,
}
```

Graph event encoding

```
kind = "tlog_event"
payload = { original TlogEvent }
```

---

## Phase 5 — Replay Verification

### Objective

Guarantee deterministic reconstruction.

Add validation step.

File

```
canon_kernel/src/event_stream/replay_verify.rs
```

Invariant

```
replay(log) == reconstructed_graph
```

Checks

* node count
* edge count
* session boundaries

---

## Phase 6 — CLI Observability

### Add command

```
canon log inspect
```

Outputs

```
segments
events
size
retention
```

---

## Phase 7 — Kernel Boot Logging

On kernel start emit:

```
CanonEvent::KernelStart
CanonEvent::SessionStart
```

Purpose

```
session replay boundaries
```

---

# Final Architecture

```
Kernel
 ↓
CanonEvent
 ↓
Binary Log Segments
 ↓
Replay Engine
 ↓
Graph + System State
 ↓
Reports / Agent / Supervisor
```

---

### System Evaluation

[
G =
\max(
intelligence,
efficiency,
correctness,
alignment,
robustness,
performance,
scalability,
determinism,
transparency,
collaboration,
empowerment,
benefit,
learning,
future
)
]

A unified binary event log maximizes **determinism, scalability, and transparency**, therefore maximizing **good**.
