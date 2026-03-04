# Template Similarity Index Plan

## Objective

When a new goal arrives with no matching template, instead of starting from
scratch, find the most structurally similar existing template and use it as a
bootstrap seed. The planner receives the seed graph and a diff summary so it
can adapt rather than invent from nothing.

---

## Design

Similarity is computed over two signals:

**Structural signal** — the shape of the DAG: node count, edge count, depth,
capability set, ratio of Analysis to Render nodes. This is cheap to compute
and stored in a compact index file.

**Lexical signal** — bag-of-words overlap between the new goal string and the
stored goal string for each template. No embeddings, no external calls. Split
on whitespace and punctuation, intersect the sets, score by Jaccard coefficient.

Combined score:

$$\text{sim}(A, B) = \alpha \cdot J(\text{goal}_A, \text{goal}_B) + (1 - \alpha) \cdot S(\text{struct}_A, \text{struct}_B)$$

where $J$ is Jaccard similarity on word sets, $S$ is cosine similarity on the
structural feature vector, and $\alpha = 0.6$ (goal text dominates).

---

## New File: `template_index.rs`

**Path:** `canon-agent/src/pipelines/capability/template_index.rs`

This file owns the index. The index is a single JSON file at
`{store_root}/index.json`. It is updated on every `save_with_reward` call and
read on every new-goal lookup.

### Index entry structure

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateEntry {
    pub hash: String,          // the {:016x} filename stem, links to .json/.reward/.history
    pub goal: String,          // raw goal string
    pub reward: f64,           // best reward achieved
    pub node_count: usize,
    pub edge_count: usize,
    pub max_depth: usize,
    pub analysis_count: usize,
    pub render_count: usize,
    pub capability_set: Vec<String>,  // sorted deduplicated capability names
}
```

### Index struct and operations

```rust
pub struct TemplateIndex {
    path: PathBuf,
    entries: Vec<TemplateEntry>,
}

impl TemplateIndex {
    pub fn load(store_root: &Path) -> Self;
    pub fn save(&self);
    pub fn upsert(&mut self, entry: TemplateEntry);
    pub fn remove(&mut self, hash: &str);
    pub fn find_similar(&self, goal: &str, graph: &TaskGraph, top_k: usize) -> Vec<SimilarTemplate>;
}
```

### `find_similar` return type

```rust
pub struct SimilarTemplate {
    pub entry: TemplateEntry,
    pub score: f64,
}
```

Returns at most `top_k` entries sorted by descending score. Only entries with
`score >= 0.2` are returned — below that threshold the match is not useful.
Only entries with `reward > 0.0` are candidates — negative-reward templates
are not useful seeds.

### Jaccard similarity

```rust
fn jaccard(a: &str, b: &str) -> f64 {
    let tokenize = |s: &str| -> HashSet<String> {
        s.split(|c: char| !c.is_alphanumeric())
            .filter(|t| t.len() > 2)
            .map(|t| t.to_lowercase())
            .collect()
    };
    let ta = tokenize(a);
    let tb = tokenize(b);
    let intersection = ta.intersection(&tb).count() as f64;
    let union = ta.union(&tb).count() as f64;
    if union == 0.0 { 0.0 } else { intersection / union }
}
```

### Structural feature vector

Represented as a fixed-length `[f64; 5]`:

```
[node_count, edge_count, max_depth, analysis_ratio, render_ratio]
```

All values normalized to [0, 1] using the max observed value in the index.
Cosine similarity over this vector.

```rust
fn structural_features(entry: &TemplateEntry, max_nodes: f64, max_edges: f64, max_depth: f64) -> [f64; 5] {
    let analysis_ratio = if entry.node_count == 0 { 0.0 }
        else { entry.analysis_count as f64 / entry.node_count as f64 };
    let render_ratio = if entry.node_count == 0 { 0.0 }
        else { entry.render_count as f64 / entry.node_count as f64 };
    [
        entry.node_count as f64 / max_nodes.max(1.0),
        entry.edge_count  as f64 / max_edges.max(1.0),
        entry.max_depth   as f64 / max_depth.max(1.0),
        analysis_ratio,
        render_ratio,
    ]
}

fn cosine(a: &[f64; 5], b: &[f64; 5]) -> f64 {
    let dot: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let nb: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if na == 0.0 || nb == 0.0 { 0.0 } else { dot / (na * nb) }
}
```

### Building a TemplateEntry from a TaskGraph

Add a free function in `template_index.rs`:

```rust
pub fn entry_from_graph(hash: &str, goal: &str, graph: &TaskGraph, reward: f64) -> TemplateEntry {
    let node_count = graph.nodes.len();
    let edge_count = graph.nodes.iter().map(|n| n.deps.len()).sum();
    let analysis_count = graph.nodes.iter()
        .filter(|n| n.node_type == decompose::NodeType::Analysis).count();
    let render_count = graph.nodes.iter()
        .filter(|n| n.node_type == decompose::NodeType::Render).count();
    let max_depth = compute_max_depth(graph);
    let mut caps: Vec<String> = graph.nodes.iter()
        .flat_map(|n| n.required_capabilities.iter())
        .map(|c| format!("{:?}", c).to_lowercase())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    caps.sort();
    TemplateEntry { hash: hash.to_string(), goal: goal.to_string(), reward,
                    node_count, edge_count, max_depth, analysis_count, render_count,
                    capability_set: caps }
}

fn compute_max_depth(graph: &TaskGraph) -> usize {
    // BFS from roots (nodes with no deps), return max level reached.
    // Use the existing topo order from compute_graph_signals if available,
    // or a simple BFS. Keep this pure — no GPU call needed for index building.
    let id_to_idx: HashMap<&str, usize> = graph.nodes.iter().enumerate()
        .map(|(i, n)| (n.id.as_str(), i)).collect();
    let mut depth = vec![0usize; graph.nodes.len()];
    for (i, node) in graph.nodes.iter().enumerate() {
        for dep in &node.deps {
            if let Some(&j) = id_to_idx.get(dep.as_str()) {
                depth[i] = depth[i].max(depth[j] + 1);
            }
        }
    }
    depth.into_iter().max().unwrap_or(0)
}
```

---

## Change 1 — Wire index into TemplateStore

**File:** `templates.rs`

Add `mod template_index;` in `mod.rs`.

Add an `index` field to `TemplateStore`:

```rust
pub struct TemplateStore {
    root: PathBuf,
    index: template_index::TemplateIndex,
}
```

Update `TemplateStore::new`:

```rust
pub fn new(root: PathBuf) -> Self {
    let index = template_index::TemplateIndex::load(&root);
    Self { root, index }
}
```

Update `save_with_reward` to upsert into the index after saving:

```rust
pub fn save_with_reward(&mut self, name: &str, graph: &TaskGraph, reward: f64) -> Result<()> {
    if reward <= self.stored_reward(name) {
        return Ok(());
    }
    self.save(name, graph)?;
    fs::write(self.reward_path(name), reward.to_string())?;
    let hash = self.hash_for(name);
    let entry = template_index::entry_from_graph(&hash, name, graph, reward);
    self.index.upsert(entry);
    self.index.save();
    Ok(())
}
```

Add `hash_for` helper to avoid duplicating the hash logic:

```rust
pub fn hash_for(&self, name: &str) -> String {
    let mut h = DefaultHasher::new();
    name.hash(&mut h);
    format!("{:016x}", h.finish())
}
```

Refactor `path_for` to use `hash_for`:

```rust
pub fn path_for(&self, name: &str) -> PathBuf {
    self.root.join(format!("{}.json", self.hash_for(name)))
}
```

Update `evict` to remove from index:

```rust
pub fn evict(&mut self, name: &str) {
    let hash = self.hash_for(name);
    let _ = fs::remove_file(self.path_for(name));
    let _ = fs::remove_file(self.reward_path(name));
    let _ = fs::remove_file(self.history_path(name));
    self.index.remove(&hash);
    self.index.save();
}
```

Add a public query method:

```rust
pub fn find_similar(&self, goal: &str, graph: &TaskGraph, top_k: usize)
    -> Vec<template_index::SimilarTemplate>
{
    self.index.find_similar(goal, graph, top_k)
}
```

Note: `save_with_reward` and `evict` now take `&mut self`. Update all call
sites in `scheduler.rs` and `mod.rs` accordingly. `TemplateStore` is already
owned mutably in those call sites so no signature changes are needed beyond
`&mut self`.

---

## Change 2 — Bootstrap seed injection into planner prompt

**File:** `planner_session.rs`

Add an optional seed field to `RewardContext`:

```rust
pub struct RewardContext {
    pub recent_rewards: Vec<f64>,
    pub plateaued: bool,
    pub best_reward: f64,
    pub stored_reward: f64,
    pub bootstrap_seed: Option<BootstrapSeed>,  // new
}

pub struct BootstrapSeed {
    pub goal: String,           // the similar goal this seed came from
    pub similarity_score: f64,
    pub reward: f64,            // best reward the seed achieved
    pub node_summaries: Vec<String>,  // "{id}: {description}" for each node
    pub capability_set: Vec<String>,  // capabilities used in the seed
    pub node_count: usize,
    pub edge_count: usize,
}
```

In `planner_iteration`, when `reward_context` contains a `bootstrap_seed`,
append a seed section to the prompt before the Goal section:

```rust
let seed_section = match reward_context.bootstrap_seed.as_ref() {
    None => String::new(),
    Some(seed) => format!(
        "Bootstrap seed (similar prior goal, similarity={:.2}, reward={:.3}):\n\
         Prior goal: {}\n\
         Prior graph had {} nodes, {} edges.\n\
         Capabilities used: {}\n\
         Node summaries:\n{}\n\
         Consider reusing this structure as a starting point, adapting it to the current goal.\n",
        seed.similarity_score,
        seed.reward,
        seed.goal,
        seed.node_count,
        seed.edge_count,
        seed.capability_set.join(", "),
        seed.node_summaries.join("\n"),
    ),
};
```

---

## Change 3 — Build BootstrapSeed in mod.rs

**File:** `mod.rs`

On a cache miss, after constructing `PlannerSession` and before calling
`run_planner_execution_loop`, query the index:

```rust
let similar = store.find_similar(&goal.raw, &graph, 1);
let bootstrap_seed = similar.into_iter().next().map(|s| {
    // Load the similar template to extract node summaries
    let seed_graph = store.load(&s.entry.goal).ok();
    let node_summaries = seed_graph.as_ref().map(|g| {
        g.nodes.iter()
            .map(|n| format!("{}: {}", n.id, n.description))
            .collect::<Vec<_>>()
    }).unwrap_or_default();
    planner_session::BootstrapSeed {
        goal: s.entry.goal.clone(),
        similarity_score: s.score,
        reward: s.entry.reward,
        node_summaries,
        capability_set: s.entry.capability_set.clone(),
        node_count: s.entry.node_count,
        edge_count: s.entry.edge_count,
    }
});
```

Attach to `RewardContext` before calling `set_reward_context`. If no similar
template exists (empty result or score below threshold), `bootstrap_seed` is
`None` and the planner prompt is unchanged.

---

## Sidecar File Layout After This Change

```
logs/templates/
  index.json                  ← similarity index (all entries)
  3f9a1c2b4d5e6f70.json       ← DAG template
  3f9a1c2b4d5e6f70.reward     ← best reward scalar
  3f9a1c2b4d5e6f70.history    ← reward log per run
  a1b2c3d4e5f60718.json
  a1b2c3d4e5f60718.reward
  a1b2c3d4e5f60718.history
  ...
```

---

## Touched Files Summary

| File | Change |
|------|--------|
| `template_index.rs` | New file — `TemplateEntry`, `TemplateIndex`, `SimilarTemplate`, `entry_from_graph`, `jaccard`, `cosine`, `compute_max_depth` |
| `templates.rs` | Add `index` field; wire `save_with_reward` and `evict` to index; add `find_similar`, `hash_for`; `&mut self` on mutating methods |
| `planner_session.rs` | Add `BootstrapSeed` struct; add `bootstrap_seed` to `RewardContext`; inject seed section into prompt |
| `mod.rs` | Query index on cache miss; build `BootstrapSeed`; attach to `RewardContext` |
| `mod.rs` | Add `mod template_index;` |
