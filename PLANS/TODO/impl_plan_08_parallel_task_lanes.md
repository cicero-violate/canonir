# Implementation Plan 08 — Parallel Task Lanes

## Goal

Multiple Canon consumers can claim tasks from a shared file-locked queue and work
independently. When all tasks in a batch complete, a coordinator emits the aggregate
result. Applied to the analyst: loop health, capability pipeline, error analysis, and
LLM timing run as four parallel lanes whose results are synthesized once all four
finish.

---

## New crate: `canon-utils/canon-task-queue`

### `Cargo.toml`

```toml
[package]
name = "canon-task-queue"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { workspace = true, features = ["derive"] }
serde_json.workspace = true
anyhow.workspace = true
uuid.workspace = true
fs2.workspace = true    # file locking — add to workspace Cargo.toml if not present
```

Add `fs2 = "0.4"` to the workspace `Cargo.toml` `[dependencies]` table.

### `src/lib.rs`

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus { Pending, Claimed, Done, Failed }

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Task {
    pub id: String,
    pub batch_id: String,
    pub kind: String,
    pub payload: serde_json::Value,
    pub status: TaskStatus,
    pub claimed_by: Option<String>,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

pub struct TaskQueue {
    path: PathBuf,    // e.g. state/task_queue.ndjson
}

impl TaskQueue {
    pub fn new(path: PathBuf) -> Self;

    /// Appends `tasks` to the queue file (one JSON line each).
    /// Acquires exclusive file lock, appends, releases lock.
    pub fn enqueue_batch(&self, batch_id: &str, tasks: Vec<(&str, serde_json::Value)>) -> anyhow::Result<Vec<Task>>;

    /// Claims the first `Pending` task matching `kinds`. Returns None if none available.
    /// Acquires exclusive lock, rewrites file with updated status, releases lock.
    pub fn claim(&self, worker_id: &str, kinds: &[&str]) -> anyhow::Result<Option<Task>>;

    /// Marks a task as Done with `result`, or Failed with `error`.
    pub fn complete(&self, task_id: &str, result: serde_json::Value) -> anyhow::Result<()>;
    pub fn fail(&self, task_id: &str, error: &str) -> anyhow::Result<()>;

    /// Returns true if all tasks in `batch_id` are Done or Failed.
    pub fn batch_complete(&self, batch_id: &str) -> anyhow::Result<bool>;

    /// Returns all tasks in `batch_id`.
    pub fn batch_tasks(&self, batch_id: &str) -> anyhow::Result<Vec<Task>>;

    /// Removes all tasks in `batch_id` from the queue file.
    pub fn clear_batch(&self, batch_id: &str) -> anyhow::Result<()>;
}
```

The queue file is NDJSON — one `Task` JSON object per line. File locking uses
`fs2::FileExt::lock_exclusive()`.

---

## Refactor `AnalystConsumer` into four lane consumers

### New file: `canon-utils/canon-runtime/src/consumers/analyst_lanes.rs`

Define four structs, each implementing `EventConsumer`:

#### `AnalystLaneLoopHealth`
- Handles: `CapabilityCompleted` where `done.capability == "llm.call"` and
  `done.request_id` matches its claimed task's request_id.
- On `Tick`: claims a task of kind `"analyst_loop_health"` from the queue.
  Emits `RuntimeEvent::Llm(LlmCall { ... })` with the loop-health phase prompt.
- On `CapabilityCompleted`: marks task Done with result; emits `SubTaskResult`.

#### `AnalystLaneCapabilityPipeline` — same pattern, kind `"analyst_capability_pipeline"`
#### `AnalystLaneErrorAnalysis` — kind `"analyst_error_analysis"`
#### `AnalystLaneLlmTiming` — kind `"analyst_llm_timing"`

Each lane has its own focused 2–3 phase prompt (extracted from the existing 8-phase
system prompt). Lanes do NOT need phases 0 and 8 — those belong to the coordinator.

---

### New file: `canon-utils/canon-runtime/src/consumers/analyst_coordinator.rs`

`AnalystCoordinator` replaces `AnalystConsumer` as the stagnation trigger:

**State machine:**
```rust
enum CoordState {
    Idle { ticks_since_reward: u64, cooldown_ticks: u64 },
    Dispatched { batch_id: String, ticks_waiting: u64 },
    Synthesizing { request_id: String, turn: usize },
}
```

**`RuntimeEvent::Tick` in `Idle`:**
- Same stagnation logic as old `AnalystConsumer`.
- When threshold reached:
  1. Generate `batch_id = Uuid::new_v4()`.
  2. Call `task_queue.enqueue_batch(batch_id, vec![("analyst_loop_health", ...), ("analyst_capability_pipeline", ...), ("analyst_error_analysis", ...), ("analyst_llm_timing", ...)])`.
  3. Transition to `Dispatched { batch_id, ticks_waiting: 0 }`.

**`RuntimeEvent::SubTaskResult` in `Dispatched`:**
- Check `result.parent_request_id == batch_id`.
- Check `task_queue.batch_complete(&batch_id)`.
- If all lanes done: collect all results, build synthesis prompt, emit `Llm(LlmCall)` for final report.
- Transition to `Synthesizing`.

**`RuntimeEvent::Tick` in `Dispatched`:**
- Increment `ticks_waiting`. If > 60: abandon batch, transition to `Idle` with cooldown.

**`RuntimeEvent::CapabilityCompleted` in `Synthesizing`:**
- Same as old `finish_session`. Write report, transition to `Idle` with cooldown.

---

## Coordinate lane prompt content

Each lane prompt is a focused subset of the existing 8-phase prompt, extracted
from `canon-agent-prompts/skills/analyst/`:

| Lane | Skill file | Phases |
|---|---|---|
| loop_health | `analyst/lane_loop_health.md` | Phase 2 only |
| capability_pipeline | `analyst/lane_capability.md` | Phase 3 only |
| error_analysis | `analyst/lane_errors.md` | Phase 4 only |
| llm_timing | `analyst/lane_llm_timing.md` | Phase 5 only |
| coordinator | `analyst/synthesize.md` | Phases 1, 6, 7, 8 + lane results injected |

---

## Register in `event_runtime.rs`

Replace the single `AnalystConsumer` registration with:

```rust
let queue_path = PathBuf::from("state/task_queue.ndjson");
let queue = Arc::new(canon_task_queue::TaskQueue::new(queue_path));

bus.register("analyst_coord", Box::new(AnalystCoordinator::new(..., queue.clone(), tlog_path.clone())), emitter.clone());
bus.register("analyst_loop",  Box::new(AnalystLaneLoopHealth::new(queue.clone(), tlog_path.clone())), emitter.clone());
bus.register("analyst_cap",   Box::new(AnalystLaneCapabilityPipeline::new(queue.clone(), tlog_path.clone())), emitter.clone());
bus.register("analyst_err",   Box::new(AnalystLaneErrorAnalysis::new(queue.clone(), tlog_path.clone())), emitter.clone());
bus.register("analyst_llm",   Box::new(AnalystLaneLlmTiming::new(queue.clone(), tlog_path.clone())), emitter.clone());
```

---

## Verification

```
cargo check --workspace
```

1. Lower `STAGNANT_THRESHOLD` to 3 ticks.
2. Run runtime.
3. Confirm four `Task` entries appear in `state/task_queue.ndjson` with status `Pending`.
4. Confirm four `Llm` events are emitted (one per lane).
5. Confirm one synthesis `Llm` event after all lanes complete.
6. Confirm report written to `state/reports_out/analyst/`.
