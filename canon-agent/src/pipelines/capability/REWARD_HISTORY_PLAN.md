# Reward History Plan

## Objective

Give the planner access to the reward history of prior runs so it can detect
when a template has plateaued and propose structurally different graphs instead
of making incremental refinements that no longer improve the score.

---

## What Gets Built

1. A reward history log per template — appended on every run.
2. A plateau detector — reads the log and returns a signal when the last N
   rewards have not improved beyond a threshold.
3. A history summary injected into the planner prompt — the planner sees its
   own past performance and is instructed to diverge when plateaued.
4. A structural divergence hint — when plateaued, the planner receives the
   current graph signals alongside the history so it can reason about what
   structural change to make.

---

## Change 1 — Reward History Log

**File:** `templates.rs`

Add a history sidecar path alongside the existing `.reward` sidecar:

```
{hash}.json        <- template DAG
{hash}.reward      <- best reward (existing)
{hash}.history     <- newline-delimited reward log, one f64 per run
```

Add to `TemplateStore`:

```rust
fn history_path(&self, name: &str) -> PathBuf {
    self.path_for(name).with_extension("history")
}

/// Append a reward entry to the history log.
pub fn record_reward(&self, name: &str, reward: f64) {
    let path = self.history_path(name);
    let line = format!("{}\n", reward);
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
}

/// Read the last `n` reward entries from the history log.
pub fn recent_rewards(&self, name: &str, n: usize) -> Vec<f64> {
    fs::read_to_string(self.history_path(name))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.trim().parse::<f64>().ok())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .take(n)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}
```

Extend `evict` to also remove the history file:

```rust
pub fn evict(&self, name: &str) {
    let _ = fs::remove_file(self.path_for(name));
    let _ = fs::remove_file(self.reward_path(name));
    let _ = fs::remove_file(self.history_path(name));
}
```

In `scheduler.rs`, after computing the reward and calling
`store.save_with_reward`, also call:

```rust
store.record_reward(template_name, reward);
```

This must happen unconditionally — every run is recorded, not just runs that
beat the ratchet. The history log is append-only and never pruned.

---

## Change 2 — Plateau Detector

**File:** `templates.rs`

Add a plateau detection function. A plateau is defined as: the last `window`
rewards have all improved by less than `threshold` over the first entry in
that window.

```rust
/// Returns true if the last `window` rewards show improvement less than
/// `threshold` over the oldest entry in that window.
pub fn is_plateaued(&self, name: &str, window: usize, threshold: f64) -> bool {
    let rewards = self.recent_rewards(name, window);
    if rewards.len() < window {
        return false;   // not enough history to judge
    }
    let baseline = rewards[0];
    let best_recent = rewards.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    (best_recent - baseline) < threshold
}
```

Sensible defaults to use at call sites: `window = 4`, `threshold = 0.05`.
These mean: if the best reward in the last 4 runs has not improved by more
than 5 percentage points over the oldest of those 4 runs, the template is
plateaued.

---

## Change 3 — History Summary for Planner Prompt

**File:** `planner_session.rs`

Add a `reward_context: Option<RewardContext>` field to `PlannerSession`:

```rust
pub struct RewardContext {
    pub recent_rewards: Vec<f64>,
    pub plateaued: bool,
    pub best_reward: f64,
    pub stored_reward: f64,
}
```

Add a constructor parameter or setter:

```rust
pub fn set_reward_context(&mut self, ctx: RewardContext) {
    self.reward_context = Some(ctx);
}
```

In `planner_iteration`, build the history section of the prompt from
`reward_context` when present:

```rust
let reward_section = match &self.reward_context {
    None => String::new(),
    Some(r) => {
        let trend = r.recent_rewards.iter()
            .map(|v| format!("{:.3}", v))
            .collect::<Vec<_>>()
            .join(", ");
        if r.plateaued {
            format!(
                "Reward history (last {} runs): [{}]\n\
                 Best recorded reward: {:.3}\n\
                 STATUS: PLATEAUED. The current graph structure is not improving.\n\
                 You MUST propose a structurally different graph: different node \
                 decomposition, different capability assignments, or different \
                 dependency topology. Do not make incremental edits.\n",
                r.recent_rewards.len(), trend, r.best_reward
            )
        } else {
            format!(
                "Reward history (last {} runs): [{}]\n\
                 Best recorded reward: {:.3}\n\
                 Continue refining the current graph.\n",
                r.recent_rewards.len(), trend, r.best_reward
            )
        }
    }
};
```

Insert `reward_section` into the prompt format string just before the Goal
section. The planner sees its own performance trend on every iteration.

---

## Change 4 — Structural Divergence Hint

**File:** `planner_session.rs`

When `plateaued = true`, append the current graph signals string to the
reward section so the planner can reason about the topology it needs to
change:

```rust
if r.plateaued {
    let signals_str = super::graph_algo::planner_signals_for_graph(graph);
    format!(
        "... (plateau message above) ...\n\
         Current graph topology: {}\n\
         Use these signals to identify structural bottlenecks before proposing changes.\n",
        signals_str
    )
}
```

`planner_signals_for_graph` is already in `graph_algo.rs` and returns a
compact string of roots, unreachable nodes, topo order, SCCs, and cycle
status. This is exactly the information the planner needs to identify where
the graph is structurally weak.

---

## Change 5 — Wire Into mod.rs

**File:** `mod.rs`

After constructing `PlannerSession` and before calling
`run_planner_execution_loop`, build the `RewardContext` and attach it:

```rust
let recent = store.recent_rewards(&template_name, 4);
let plateaued = store.is_plateaued(&template_name, 4, 0.05);
let reward_ctx = RewardContext {
    recent_rewards: recent,
    plateaued,
    best_reward: store.stored_reward(&template_name),
    stored_reward: store.stored_reward(&template_name),
};
planner_session.set_reward_context(reward_ctx);
```

This only applies to the planner path (`use_planner = true`). Cache-hit runs
that go directly to `execute_graph_loop` do not need a planner session and
do not need this context. They do still call `store.record_reward` after
execution via the scheduler.

---

## Data Flow End to End

```
run N:
  execute → compute_reward → record_reward (append to .history)
                           → save_with_reward (ratchet .reward)

run N+1:
  recent_rewards ← read .history
  is_plateaued   ← inspect recent_rewards
  RewardContext  → PlannerSession
  planner prompt ← includes trend + plateau flag + graph signals if plateaued
  planner output ← structurally different graph when plateaued
  apply_planner_update → new graph
  execute → compute_reward → record_reward → save_with_reward
```

---

## Touched Files Summary

| File                 | Change                                                                                         |
|----------------------+------------------------------------------------------------------------------------------------|
| `templates.rs`       | Add `history_path`, `record_reward`, `recent_rewards`, `is_plateaued`; extend `evict`          |
| `scheduler.rs`       | Call `store.record_reward` after reward computation in both execution paths                    |
| `planner_session.rs` | Add `RewardContext` struct, `reward_context` field, `set_reward_context`; inject into prompt   |
| `graph_algo.rs`      | No changes — `planner_signals_for_graph` used as-is                                            |
| `mod.rs`             | Build `RewardContext` from store after planner session construction; call `set_reward_context` |
