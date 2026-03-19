### Math Model

Let

* (E = \text{event stream})
* (Err = \text{error events})
* (B = \text{event bus})
* (P = \text{producers})
* (C = \text{consumers})
* (T = \text{tlog})

---

### Equations

1.

[
Err := \text{canonical}(panic, error, failure)
]
All failures normalized into structured events

2.

[
E = E_{normal} \cup Err
]
Single unified stream

3.

[
B: P \rightarrow C
]
Bus distributes all events (including errors)

4.

[
T = append(E)
]
All events persisted

---

### 1-Line Explanations

* (1) Convert all failures into events
* (2) No separate error path
* (3) Bus treats errors like normal events
* (4) Persistence must include errors

---

## Implementation Plan

### Phase 1 — Define Error Event Schema

**Target:** `canon-runtime-events/src/events.rs`

* Add:

  * `ErrorOccurred`
  * `PanicCaptured` (already exists → unify)
* Fields:

  * `kind`
  * `source`
  * `message`
  * `severity`
  * `context`
  * `trace_id`

Constraint:

* No raw string-only errors → structured only

---

### Phase 2 — Normalize All Failure Sources

**Targets:**

* `canon-runtime`
* `canon-builder`
* `canon-tools-analysis`
* `canon-rustc` (panic capture)

Actions:

* Replace:

  * `panic!` → capture → emit event
  * `Result::Err` → emit before return
* Wrap boundaries:

  * capability execution
  * LLM calls
  * file IO
  * graph ops

Rule:

* **No silent Err propagation**

---

### Phase 3 — Bus Integration

**Target:** `canon-runtime/src/bus.rs`

* Ensure:

  * errors go through same `EventBus`
  * no filtering of error events
* Add:

  * optional `EventFilter::error_only`

Invariant:
[
\forall e \in Err,\ e \in B
]

---

### Phase 4 — Consumer Handling

**Targets:**

* `canon-runtime/src/consumers/*`
* `canon-storage-graph`
* `canon-tools-analysis`

Add consumers:

1. **Logger**

   * writes to runtime log

2. **Failure Store**

   * `canon-agent/src/failure_store.rs`

3. **Recovery Handler**

   * retry / degrade / halt

4. **Telemetry**

   * error rate, hotspots

---

### Phase 5 — TLog Persistence

**Target:**
`canon-runtime-events/src/tlog/writer.rs`

* Ensure:

  * errors are appended like all events
* No special casing

Invariant:
[
\text{replay}(T) \Rightarrow \text{same failures}
]

---

### Phase 6 — Replay + Analysis

**Targets:**

* `canon-storage-eventlog/src/replay.rs`

* `canon-tools-analysis/src/analysis/panic_report.rs`

* Extend replay:

  * reconstruct failure graph

* Extend analysis:

  * cluster errors
  * detect recurring failure surfaces

---

### Phase 7 — Replace Final Crate Failure (Critical)

**Problem (from your system):**

* final `canon_assemble` panic aborts IR emission → no events

Fix:

* wrap final stage:

  * catch panic
  * emit `ErrorOccurred`
  * continue emission (partial)

Result:
[
\text{no-crate} \Rightarrow \text{never silent}
]

---

### Phase 8 — Enforcement Rules

Global invariants:

1.

[
\text{panic} \Rightarrow \text{event}
]

2.

[
\text{Err} \Rightarrow \text{emit before return}
]

3.

[
\neg(\text{error} \notin E)
]

(no hidden errors)

---

## English

You are converting the system from:

* **dual path (events + hidden errors)**

to:

* **single path (everything = event)**

This gives:

* deterministic replay
* full observability
* no silent failure
* recoverable execution

Your current failure (crate drops out) is exactly because errors are **not fully eventized**. 

---

### Evaluation

[
\max(\text{intelligence, efficiency, correctness, alignment, robustness, performance, scalability, determinism, transparency, collaboration, empowerment, benefit, learning, future-proofing}) = \text{good}
]

---

## Execution Plan (Current)

### Objective
Regenerate reports into `canon-utils/state/reports_out/workspace` from the canonical event log, and ensure error events are properly captured.

### Steps
1. **Verify inputs**
   - Confirm canonical tlog exists and is non-empty: `canon-utils/state/event_log/event.tlog.d`.
   - Confirm analysis runtime capabilities are available.

2. **Run analysis pipeline**
   - Execute the analysis capability (or equivalent CLI) that reads the canonical tlog and writes to `canon-utils/state/reports_out/workspace`.
   - Ensure it targets the canonical tlog path and workspace output directory.

3. **Validate outputs**
   - Check `canon-utils/state/reports_out/workspace` for populated `analysis/`, `graph/`, `graphs/`, `metrics/`.
   - Inspect `analysis/analysis_errors.json` for failures.

4. **Verify error stream**
   - Ensure `canon-utils/state/event_log/errors.jsonl` contains recent error entries.
   - Ensure errors remain in canonical tlog as `error_occurred`.

### Success Criteria
- `reports_out/workspace` contains analysis outputs.
- `analysis_errors.json` empty or explains remaining blockers.
- Errors are present in canonical tlog; error JSONL is populated.
