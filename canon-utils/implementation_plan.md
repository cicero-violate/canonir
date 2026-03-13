## Variables

(W=) tlog writer, (S=) supervisor daemon, (K=) kernel wrapper, (P_i=) event producers, (E=) event stream (.tlog), (C_j=) consumers
(G=\max(I,E,C,A,R,P,S,D,T,K,X,B,L,F)=good)

## Equations

(E = Append(W, event))
Explanation: writer serializes and appends events.

(E = \sum Emit(P_i))
Explanation: kernel, agent, and supervisor produce events.

(State = Replay(E))
Explanation: consumers rebuild system state from the log.

---

# Implementation Plan

## Dedicated TLOG Writer + Supervisor Integration

Goal:

```text
centralized event writing
safe append
runtime + build events unified
```

---

# 1 Create New Crate

Location:

```
canon-utils/tlog-writer
```

Workspace:

```
canon workspace
```

Structure:

```
tlog-writer/
    Cargo.toml
    src/
        lib.rs
        writer.rs
        event.rs
        rotate.rs
```

Purpose:

```
single API for writing events
```

---

# 2 Event Type Definition

File:

```
src/event.rs
```

Core structure:

```rust
pub struct CanonEvent {
    pub ts: u64,
    pub source: String,
    pub kind: String,
    pub payload: serde_json::Value,
}
```

Examples:

```
build_event
runtime_event
supervisor_event
analysis_event
```

---

# 3 Writer API

File:

```
src/writer.rs
```

Primary API:

```rust
pub fn append_event(event: CanonEvent) -> Result<()>
```

Implementation steps:

```
open .tlog
seek end
write JSON line
flush
```

Example line format:

```
{"ts":123,"source":"agent","kind":"node_executed","payload":{...}}
```

---

# 4 File Locking

Prevent concurrent write corruption.

Use:

```
fs2 crate
```

Implementation:

```
acquire file lock
append event
release
```

Functions:

```
lock_tlog()
unlock_tlog()
```

---

# 5 Buffered Writer (Optional Optimization)

Add:

```
append_event_buffered()
flush()
```

Purpose:

```
reduce syscall overhead
```

But default API should remain safe append.

---

# 6 TLOG Rotation

File:

```
rotate.rs
```

Trigger conditions:

```
file size > threshold
snapshot checkpoint
manual rotation
```

Implementation:

```
rename tlog → tlog.1
create new tlog
```

---

# 7 Kernel Integration

Kernel becomes producer.

Add dependency:

```
canon-utils/tlog-writer
```

Kernel emits:

```
crate_compiled
file_processed
symbol_emitted
dependency_edge
```

Example:

```rust
append_event(CanonEvent {
    source: "kernel",
    kind: "crate_compiled",
    payload: {...}
})
```

---

# 8 Agent Integration

Agent emits runtime events.

Examples:

```
task_created
node_executed
repair_triggered
graph_patch
goal_update
```

Integration points:

```
scheduler.rs
planner_session.rs
execution_result.rs
```

---

# 9 Supervisor Integration

Supervisor becomes lifecycle event producer.

Add dependency:

```
tlog-writer
```

Events emitted:

```
build_started
build_completed
process_spawned
process_restarted
process_exit
file_change_detected
```

Implementation points:

```
process.rs
builder.rs
main.rs
```

Example:

```rust
append_event({
    source: "supervisor",
    kind: "process_restart",
    payload: {...}
})
```

---

# 10 Consumer Model (Unchanged)

Consumers read events.

Examples:

```
reports
smt-analysis
query engine
project editor
```

Consumers use:

```
canon-tlog-replay
```

Pipeline:

```
tail .tlog
parse events
update graph
emit artifacts
```

---

# 11 TLOG File Format

Use:

```
JSONL
```

Example:

```
.tlog
```

Contents:

```
{"ts":1,"source":"kernel","kind":"crate_compiled",...}
{"ts":2,"source":"supervisor","kind":"process_spawned",...}
{"ts":3,"source":"agent","kind":"node_executed",...}
```

Advantages:

```
append-only
stream-friendly
human readable
```

---

# 12 Supervisor Event Loop Update

Modify:

```
canon-supervisor/src/main.rs
```

Emit events during:

```
file change
build start
build finish
process spawn
process restart
process crash
```

Flow:

```
watch change
emit file_change event
build crate
emit build event
restart process
emit process_restart event
```

---

# 13 Failure Handling

Add:

```
retry append
fallback logging
```

If write fails:

```
write to stderr
attempt retry
```

---

# 14 Performance Safety

Guarantees:

```
append-only
no partial writes
fsync optional
```

Design:

```
line-delimited events
crash-safe
```

---

# 15 Final System Architecture

```
Producers
---------
kernel wrapper
agent runtime
supervisor

        │
        ▼

tlog-writer
        │
        ▼

.tlog event stream
        │
        ▼

Consumers
---------
reports
smt-analysis
query engine
editor tools
```

---

# 16 Implementation Order

Step sequence:

```
1 create tlog-writer crate
2 implement append_event API
3 integrate kernel writer
4 integrate supervisor writer
5 integrate agent writer
6 validate concurrent writes
7 update consumers
```

---

# Result

After implementation:

```
single canonical event log
multi-producer architecture
fully decoupled consumers
deterministic system replay
```

---

If you want, I can also show the **advanced version used in high-scale event systems** where `.tlog` becomes a **structured binary event log (similar to Kafka / Temporal history logs)** which will make replay ~50–100x faster.
