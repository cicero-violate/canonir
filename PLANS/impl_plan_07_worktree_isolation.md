# Implementation Plan 07 — Git Worktree Isolation

## Goal

When the loop planner generates a plan involving large structural changes, the
executor runs that plan in an isolated git worktree rather than the main working
tree. If verification fails or a reward is not received within a timeout, the
worktree is discarded. The main tree is never left in a broken state.

---

## Step 1 — Define worktree-triggering action kinds

In `capability_config.toml`, add:

```toml
[worktree]
# Action kinds that trigger worktree isolation.
isolated_action_kinds = [
    "refactor_module",
    "rename_symbol",
    "move_module",
    "restructure",
    "large_edit",
]
# Auto-detect: if LoopPlanned action has > N file paths in payload, also isolate.
auto_isolate_file_count = 5
# Ticks to wait for LoopRewarded before abandoning the worktree.
abandon_after_ticks = 30
# Base directory for worktrees.
worktrees_dir = "/workspace/ai_sandbox/canon/.canon_worktrees"
```

---

## Step 2 — New crate: `canon-utils/canon-worktree`

### `Cargo.toml`

```toml
[package]
name = "canon-worktree"
version = "0.1.0"
edition = "2021"

[dependencies]
anyhow.workspace = true
uuid.workspace = true
```

### `src/lib.rs`

```rust
pub struct WorktreeHandle {
    pub id: String,
    pub branch: String,
    pub path: PathBuf,
}

/// Creates a new worktree at `{worktrees_dir}/{id}` on a fresh branch
/// `canon-worktree/{id}`. Returns the handle.
///
/// Runs: git worktree add {path} -b {branch}
pub fn create(repo_root: &Path, worktrees_dir: &Path) -> anyhow::Result<WorktreeHandle>;

/// Merges the worktree branch into the current HEAD of `repo_root` using
/// `git merge --squash {branch}` followed by `git commit`.
/// Returns Ok(()) on success.
pub fn merge(repo_root: &Path, handle: &WorktreeHandle, message: &str) -> anyhow::Result<()>;

/// Removes the worktree and deletes its branch. Safe to call even if the
/// worktree path doesn't exist.
/// Runs: git worktree remove --force {path} && git branch -D {branch}
pub fn abandon(repo_root: &Path, handle: &WorktreeHandle) -> anyhow::Result<()>;
```

All git commands use `std::process::Command` synchronously.

---

## Step 3 — New consumer: `WorktreeConsumer`

**New file:** `canon-utils/canon-runtime/src/consumers/worktree_consumer.rs`

```rust
pub struct WorktreeConsumer {
    config: WorktreeConfig,       // loaded from capability_config.toml [worktree]
    repo_root: PathBuf,
    active: Option<ActiveWorktree>,
    emitter: Option<EventEmitterHandle>,
}

struct ActiveWorktree {
    handle: WorktreeHandle,
    plan_tick: u64,
    ticks_active: u64,
}
```

### `on_event` logic

**`RuntimeEvent::LoopPlanned(plan)`:**
- If `active.is_some()`, skip (already in a worktree session).
- Check if `plan.action_kind` is in `isolated_action_kinds` OR the payload contains
  more than `auto_isolate_file_count` distinct file paths.
- If yes:
  1. Call `canon_worktree::create(&repo_root, &config.worktrees_dir)`.
  2. Store `ActiveWorktree { handle, plan_tick: plan.tick, ticks_active: 0 }`.
  3. Emit `RuntimeEvent::DebugEvent { source: "worktree_consumer", kind: "worktree_created", payload: json!({ "id": handle.id, "path": handle.path }) }`.
  4. Redirect the executor workspace to the worktree path by emitting
     `RuntimeEvent::RuntimeStateUpdated { payload: json!({ "workspace_override": handle.path }) }`.
  5. Return `EventOutcome::EmitMany(vec![debug_event, state_updated_event])`.

**`RuntimeEvent::LoopRewarded(r)` with `r.reward > 0.0` and `active.is_some()`:**
- Call `canon_worktree::merge(&repo_root, &handle, "canon auto-merge: worktree session")`.
- On success: clear `active`, emit `RuntimeEvent::RuntimeStateUpdated { payload: json!({ "workspace_override": null }) }`.
- On failure: call `abandon` and emit `ErrorOccurred("worktree_merge_failed")`.

**`RuntimeEvent::Tick(_)` with `active.is_some()`:**
- Increment `ticks_active`.
- If `ticks_active >= config.abandon_after_ticks`:
  - Call `canon_worktree::abandon(&repo_root, &handle)`.
  - Clear `active`.
  - Emit `RuntimeEvent::RuntimeStateUpdated { payload: json!({ "workspace_override": null }) }`.
  - Emit `ErrorOccurred("worktree_abandoned_timeout")`.

**`RuntimeEvent::LoopVerified(v)` with `!v.passed` and `active.is_some()`:**
- Increment a `failed_verify_count` in `ActiveWorktree`.
- If `failed_verify_count >= 3`: abandon early (same as tick timeout).

---

## Step 4 — Respect `workspace_override` in `CapabilityExecutor`

**File:** `canon-utils/canon-runtime/src/consumers/capability_executor.rs`

Add a `workspace_override: Arc<Mutex<Option<PathBuf>>>` field to `CapabilityExecutor`.

In `on_event`, before building `ExecutionContext`, check if override is set:

```rust
let workspace = if let Some(override_path) = self.workspace_override.lock().unwrap().as_ref() {
    override_path.clone()
} else {
    self.workspace.clone()
};
```

In the `on_event` match, add a `RuntimeEvent::RuntimeStateUpdated(u)` arm:
- If `u.payload["workspace_override"]` is a string → set the override.
- If it is `null` → clear the override.

---

## Step 5 — Register `WorktreeConsumer` in `event_runtime.rs`

```rust
let worktree_consumer = WorktreeConsumer::new(config.worktree.clone(), workspace_root.clone());
bus.register("worktree", Box::new(worktree_consumer), emitter.clone());
```

---

## Cargo.toml changes

`canon-runtime/Cargo.toml`: add `canon-worktree = { path = "../canon-worktree" }`.

---

## Verification

```
cargo check --workspace
```

1. Force an isolated action kind by temporarily setting `isolated_action_kinds = ["cargo_check"]`.
2. Run the runtime through one loop cycle.
3. Confirm `git worktree list` shows a new worktree.
4. Confirm `RuntimeStateUpdated` with `workspace_override` appears in tlog.
5. After `LoopRewarded`, confirm `git worktree list` no longer shows the worktree.
