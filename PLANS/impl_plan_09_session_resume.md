# Implementation Plan 09 — Session Resume

## Goal

Canon agent consumer states are serialized to disk after every state transition.
When the runtime restarts, consumers restore from their checkpoint rather than
starting fresh. An analyst mid-session at turn 4 resumes from turn 4 on restart.

---

## Step 1 — Define checkpoint interface

**New file:** `canon-utils/canon-runtime/src/checkpoint.rs`

```rust
use std::path::{Path, PathBuf};

pub struct CheckpointStore {
    dir: PathBuf,  // e.g. state/checkpoints/
}

impl CheckpointStore {
    pub fn new(dir: PathBuf) -> Self;

    /// Serializes `value` (must be `serde::Serialize`) to
    /// `{dir}/{consumer_name}.json`.
    pub fn save<T: serde::Serialize>(&self, consumer_name: &str, value: &T) -> anyhow::Result<()>;

    /// Deserializes from `{dir}/{consumer_name}.json`. Returns `None` if
    /// the file does not exist or fails to parse.
    pub fn load<T: serde::de::DeserializeOwned>(&self, consumer_name: &str) -> anyhow::Result<Option<T>>;

    /// Deletes the checkpoint for `consumer_name`.
    pub fn clear(&self, consumer_name: &str);

    /// Returns the mtime of the checkpoint file, or None.
    pub fn checkpoint_age_secs(&self, consumer_name: &str) -> Option<u64>;
}
```

Writes are atomic: write to `{name}.json.tmp` then `rename` to `{name}.json`.

---

## Step 2 — Make `AnalystConsumer::State` serializable

In `analyst_consumer.rs`:

Add `#[derive(serde::Serialize, serde::Deserialize)]` to `State` and both variants.

The `PendingLlm` variant carries `request_id: String` and `turn: usize` — both
serializable. The `compact_digest` field from plan 04 is also serializable.

Add `CheckpointStore` to `AnalystConsumer`:

```rust
pub struct AnalystConsumer {
    tlog_path: PathBuf,
    memory: MemoryIndex,
    checkpoint: CheckpointStore,
    state: State,
}
```

---

## Step 3 — Add `CheckpointedState` wrapper

```rust
#[derive(serde::Serialize, serde::Deserialize)]
struct AnalystCheckpoint {
    state: State,
    saved_at_secs: u64,
}
```

`saved_at_secs` is `SystemTime::now()` as Unix seconds.

---

## Step 4 — Save on every transition

Add a private `fn save_checkpoint(&self)` that calls:

```rust
let checkpoint = AnalystCheckpoint {
    state: self.state_snapshot(),
    saved_at_secs: unix_now(),
};
self.checkpoint.save("analyst_consumer", &checkpoint).ok();
```

Call `save_checkpoint()` at the end of every method that transitions `self.state`:
- `start_session` (after setting `PendingLlm`)
- `continue_session` (after updating `PendingLlm`)
- `finish_session` (after setting `Idle`)
- In `on_event` on `CapabilityFailed` when state resets to `Idle`

---

## Step 5 — Restore in `AnalystConsumer::new`

```rust
pub fn new(tlog_path: PathBuf, memory: MemoryIndex, checkpoint: CheckpointStore) -> Self {
    let state = try_restore_checkpoint(&checkpoint, &tlog_path)
        .unwrap_or_else(|| State::Idle { ticks_since_reward: 0, cooldown_ticks: 0 });

    Self { tlog_path, memory, checkpoint, state }
}

fn try_restore_checkpoint(store: &CheckpointStore, tlog_path: &Path) -> Option<State> {
    let cp: AnalystCheckpoint = store.load("analyst_consumer").ok()??;

    // Reject stale checkpoints older than 10 minutes.
    let age = unix_now().saturating_sub(cp.saved_at_secs);
    if age > 600 { return None; }

    // Validate: if state is PendingLlm, check whether the tlog already has a
    // CapabilityCompleted event for that request_id (i.e., it completed while
    // the runtime was down). If so, replay it.
    if let State::PendingLlm { request_id, .. } = &cp.state {
        if tlog_has_capability_completed(tlog_path, request_id) {
            // Return Idle — the capability completed but we missed the event.
            // The analyst will re-fire on next stagnation threshold.
            return Some(State::Idle { ticks_since_reward: 0, cooldown_ticks: 0 });
        }
    }

    Some(cp.state)
}
```

`tlog_has_capability_completed`: opens the tlog file, scans for a
`CapabilityCompleted` event whose `request_id` matches. Returns bool.

---

## Step 6 — Apply same pattern to `GoalGenConsumer`

`GoalGenConsumer::State` is already simple (Waiting/Pending/Done). Add:

```rust
#[derive(serde::Serialize, serde::Deserialize)]
struct GoalGenCheckpoint {
    state: State,
    retries: u32,
    saved_at_secs: u64,
}
```

- Save after every state transition.
- Restore in `GoalGenConsumer::new`.
- On restore: if `State::Done` and `AGENT_GOAL_PATH` exists with valid content →
  stay Done. If `State::Done` but goal file missing → reset to Waiting.
- Reject checkpoints older than 60 minutes (goal generation shouldn't span sessions).

---

## Step 7 — Register `CheckpointStore` in `event_runtime.rs`

```rust
let checkpoint_store = CheckpointStore::new(
    PathBuf::from("/workspace/ai_sandbox/canon/state/checkpoints")
);
std::fs::create_dir_all(&checkpoint_store.dir).ok();
```

Pass the store into `AnalystConsumer::new` and `GoalGenConsumer::new`.

---

## Cargo.toml changes

`checkpoint.rs` lives inside `canon-runtime` — no new crate. Add `serde` derive
features to `canon-runtime/Cargo.toml` if not already present (they are).

---

## Verification

```
cargo check -p canon-runtime
```

1. Start the runtime. Wait for analyst to transition to `PendingLlm`.
2. Kill the runtime process with `kill -9`.
3. Check `state/checkpoints/analyst_consumer.json` exists with `status: PendingLlm`.
4. Restart the runtime.
5. Confirm logs show `[analyst_consumer] restored from checkpoint at turn N`.
6. Confirm the analyst does not restart from turn 0.
