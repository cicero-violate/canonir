# DAG Template Store — Implementation Plan

## Objective

Introduce a template store layer that persists planner-generated DAGs to disk.
On subsequent runs with the same goal, the system loads the DAG directly,
bypassing the LLM planner entirely.

---

## Files to Modify

### 1. `dag.rs`

**Goal:** Make `TaskGraph` and `TaskNode` serializable.

**Changes:**

- Add `use serde::{Serialize, Deserialize};` at the top.
- Derive `Clone, Serialize, Deserialize` on `TaskGraph`.
- Derive `Clone, Serialize, Deserialize` on `TaskNode`.
- Derive `Clone, Serialize, Deserialize` on `Status`.
- Derive `Clone, Serialize, Deserialize` on `NodeType` (imported from `decompose.rs` or re-exported).

**Note:** `id_index: HashMap<String, usize>` serializes fine with serde. On
load, call `graph.rebuild_index()` immediately after deserialization to ensure
the index is consistent with `nodes`.

---

### 2. `templates.rs` (new file)

**Path:** `canon-agent/src/pipelines/capability/templates.rs`

**Contents:**

```rust
use std::fs;
use std::path::{Path, PathBuf};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use crate::pipelines::capability::dag::TaskGraph;

/// Deterministic goal hash → filename.
pub fn template_path(template_dir: &Path, goal: &str) -> PathBuf {
    let mut h = DefaultHasher::new();
    goal.hash(&mut h);
    let key = h.finish();
    template_dir.join(format!("{:016x}.json", key))
}

/// Persist a DAG template to disk.
pub fn save_template(path: &Path, graph: &TaskGraph) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(graph)?;
    fs::write(path, json)?;
    Ok(())
}

/// Load a DAG template from disk and rebuild its index.
pub fn load_template(path: &Path) -> anyhow::Result<TaskGraph> {
    let data = fs::read_to_string(path)?;
    let mut graph: TaskGraph = serde_json::from_str(&data)?;
    graph.rebuild_index();
    Ok(graph)
}
```

---

### 3. `mod.rs`

**Function:** `run_capability_loop`

**Goal:** Before invoking the planner or decompose path, check for a matching
template. Load it if present; fall through to planner if not. After a
successful planner run, save the template.

**Changes:**

Add `mod templates;` at the top of `mod.rs`.

Inside `run_capability_loop`, locate where the initial `TaskGraph` is
constructed (after goal resolution, before `execute_graph_loop` or
`run_planner_execution_loop`). Insert the following logic:

```rust
let template_dir = Self::log_path("templates");
let tpl_path = templates::template_path(&template_dir, &goal.raw);

let mut graph = if tpl_path.exists() {
    log::info!("[templates] cache hit — loading {}", tpl_path.display());
    templates::load_template(&tpl_path)?
} else {
    log::info!("[templates] cache miss — invoking planner");
    // existing decompose / planner construction path goes here (unchanged)
    let g = /* existing graph construction */;
    templates::save_template(&tpl_path, &g)?;
    g
};
```

**No changes** to the execution path below this point. `execute_graph_loop`
and `run_planner_execution_loop` receive `graph` exactly as before.

---

### 4. `scheduler.rs`

**Function:** `run_planner_execution_loop`

**Goal:** After the graph converges (all nodes completed, no failures), save
the final DAG as a template. This captures planner refinements made during
execution, not just the initial decomposition.

**Changes:**

Accept two additional parameters:

```rust
template_path: Option<&std::path::Path>,
```

At the point where the loop exits cleanly (all nodes completed), add:

```rust
if let Some(tpl) = template_path {
    if let Err(e) = templates::save_template(tpl, graph) {
        log::warn!("[templates] failed to save template: {e}");
    }
}
```

Pass `Some(&tpl_path)` from `mod.rs` when calling
`run_planner_execution_loop`, or `None` for non-planner paths.

---

### 5. `mod.rs` — module registration

Add to the module declarations at the top:

```rust
mod templates;
```

---

## Template Directory Layout

```
logs/
  templates/
    3f9a1c2b4d5e6f70.json   ← hashed goal
    a1b2c3d4e5f60718.json
    ...
```

One file per unique goal string. Filenames are 16-char lowercase hex of the
64-bit hash of the goal text.

---

## Validation on Load

After `load_template`, call `graph.validate()` before proceeding. If
validation fails, treat it as a cache miss: log a warning, delete the corrupt
template file, and fall through to the planner.

```rust
match templates::load_template(&tpl_path) {
    Ok(g) if g.validate().is_ok() => g,
    Ok(_) | Err(_) => {
        log::warn!("[templates] invalid template, evicting {}", tpl_path.display());
        let _ = std::fs::remove_file(&tpl_path);
        // fall through to planner
    }
}
```

---

## Node Status Reset on Load

When loading a template for a fresh run, reset all node statuses to `Pending`
so execution starts cleanly. Add a helper to `dag.rs`:

```rust
pub fn reset_for_execution(&mut self) {
    for node in &mut self.nodes {
        node.status = Status::Pending;
        node.result = None;
        node.error = None;
        node.readonly_fail_count = 0;
    }
    self.rebuild_index();
}
```

Call `graph.reset_for_execution()` immediately after a successful template
load.

---

## Touched Files Summary

| File | Change type |
|------|-------------|
| `dag.rs` | Add serde derives + `reset_for_execution` helper |
| `templates.rs` | New file — template_path, save, load |
| `mod.rs` | Add `mod templates;`, template check in `run_capability_loop` |
| `scheduler.rs` | Save template after `run_planner_execution_loop` converges |

No other files require changes. Execution path, engine, decompose, and
planner_session are all untouched.
