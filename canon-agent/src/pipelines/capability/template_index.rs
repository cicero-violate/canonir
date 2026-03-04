use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::dag::TaskGraph;
use super::decompose;
use super::goal_embedding;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateEntry {
    pub hash: String,
    pub goal: String,
    #[serde(default)]
    pub goal_embedding: Vec<f32>,
    pub reward: f64,
    pub node_count: usize,
    pub edge_count: usize,
    pub max_depth: usize,
    pub analysis_count: usize,
    pub render_count: usize,
    pub capability_set: Vec<String>,
    #[serde(default)]
    pub failure_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarTemplate {
    pub entry: TemplateEntry,
    pub score: f64,
    pub goal_similarity: f64,
    pub structural_similarity: f64,
    pub used_embedding: bool,
}

#[derive(Debug, Clone)]
pub struct SimilarSearch {
    pub templates: Vec<SimilarTemplate>,
    pub cache_hits: u64,
}

pub struct TemplateIndex {
    path: PathBuf,
    entries: Vec<TemplateEntry>,
}

impl TemplateIndex {
    pub fn load(store_root: &Path) -> Self {
        let path = store_root.join("index.json");
        let entries = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Vec<TemplateEntry>>(&s).ok())
            .unwrap_or_default();
        Self { path, entries }
    }

    pub fn save(&self) {
        if let Ok(pretty) = serde_json::to_string_pretty(&self.entries) {
            let _ = std::fs::create_dir_all(self.path.parent().unwrap_or(Path::new(".")));
            let _ = std::fs::write(&self.path, pretty);
        }
    }

    pub fn upsert(&mut self, entry: TemplateEntry) {
        self.entries.retain(|e| e.hash != entry.hash);
        self.entries.push(entry);
    }

    pub fn remove(&mut self, hash: &str) {
        self.entries.retain(|e| e.hash != hash);
    }

    pub fn get(&self, hash: &str) -> Option<&TemplateEntry> {
        self.entries.iter().find(|e| e.hash == hash)
    }

    pub fn bump_failure_count(&mut self, hash: &str) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.hash == hash) {
            entry.failure_count = entry.failure_count.saturating_add(1);
        }
    }

    pub fn find_similar(
        &self,
        goal: &str,
        graph: &TaskGraph,
        top_k: usize,
        goal_w: f64,
        struct_w: f64,
        embedding_dim: usize,
    ) -> SimilarSearch {
        if self.entries.is_empty() {
            return SimilarSearch { templates: Vec::new(), cache_hits: 0 };
        }
        let mut cache = goal_embedding::load_cache();
        let g_hash = goal_embedding::goal_hash(goal);
        let mut cache_hits = 0u64;
        let g_embed = if let Some(embed) = cache.get(&g_hash) {
            cache_hits += 1;
            embed.clone()
        } else {
            let emb = goal_embedding::embed_goal(goal, embedding_dim);
            cache.insert(g_hash.clone(), emb.vector.clone());
            emb.vector
        };
        if cache_hits == 0 {
            goal_embedding::save_cache(&cache);
        }
        let (max_nodes, max_edges, max_depth) = self.maxima_with_graph(graph);
        let target_entry = entry_from_graph("target", goal, graph, 0.0);
        let target_vec = structural_features(&target_entry, max_nodes, max_edges, max_depth);

        let mut scored: Vec<SimilarTemplate> = batch_similarity(
            &self.entries,
            goal,
            &g_embed,
            &target_vec,
            max_nodes,
            max_edges,
            max_depth,
            goal_w,
            struct_w,
        )
        .into_iter()
        .filter(|s| s.score >= 0.2)
        .collect();

        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        SimilarSearch { templates: scored, cache_hits }
    }

    fn maxima_with_graph(&self, graph: &TaskGraph) -> (f64, f64, f64) {
        let mut max_nodes = graph.nodes.len() as f64;
        let mut max_edges = graph.nodes.iter().map(|n| n.deps.len()).sum::<usize>() as f64;
        let mut max_depth = compute_max_depth(graph) as f64;
        for e in &self.entries {
            max_nodes = max_nodes.max(e.node_count as f64);
            max_edges = max_edges.max(e.edge_count as f64);
            max_depth = max_depth.max(e.max_depth as f64);
        }
        (max_nodes, max_edges, max_depth)
    }
}

pub fn entry_from_graph(hash: &str, goal: &str, graph: &TaskGraph, reward: f64) -> TemplateEntry {
    let node_count = graph.nodes.len();
    let edge_count = graph.nodes.iter().map(|n| n.deps.len()).sum();
    let analysis_count = graph.nodes.iter()
        .filter(|n| n.node_type == decompose::NodeType::Analysis)
        .count();
    let render_count = graph.nodes.iter()
        .filter(|n| n.node_type == decompose::NodeType::Render)
        .count();
    let max_depth = compute_max_depth(graph);
    let mut caps: Vec<String> = graph.nodes.iter()
        .flat_map(|n| n.required_capabilities.iter())
        .map(|c| format!("{:?}", c).to_lowercase())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    caps.sort();
    TemplateEntry {
        hash: hash.to_string(),
        goal: goal.to_string(),
        goal_embedding: Vec::new(),
        reward,
        node_count,
        edge_count,
        max_depth,
        analysis_count,
        render_count,
        capability_set: caps,
        failure_count: 0,
    }
}

fn compute_max_depth(graph: &TaskGraph) -> usize {
    if graph.nodes.is_empty() {
        return 0;
    }
    let id_to_idx: HashMap<&str, usize> = graph.nodes.iter().enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); graph.nodes.len()];
    for (idx, node) in graph.nodes.iter().enumerate() {
        for dep in &node.deps {
            if let Some(&j) = id_to_idx.get(dep.as_str()) {
                adj[j].push(idx);
            }
        }
    }
    let topo = algorithms::graph::topological_sort::topological_sort(&adj);
    let mut depth = vec![0usize; graph.nodes.len()];
    for &u in &topo {
        for &v in &adj[u] {
            depth[v] = depth[v].max(depth[u] + 1);
        }
    }
    depth.into_iter().max().unwrap_or(0)
}

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

fn structural_features(entry: &TemplateEntry, max_nodes: f64, max_edges: f64, max_depth: f64) -> [f64; 5] {
    let analysis_ratio = if entry.node_count == 0 { 0.0 }
        else { entry.analysis_count as f64 / entry.node_count as f64 };
    let render_ratio = if entry.node_count == 0 { 0.0 }
        else { entry.render_count as f64 / entry.node_count as f64 };
    [
        entry.node_count as f64 / max_nodes.max(1.0),
        entry.edge_count as f64 / max_edges.max(1.0),
        entry.max_depth as f64 / max_depth.max(1.0),
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

fn batch_similarity(
    entries: &[TemplateEntry],
    goal: &str,
    goal_embed: &[f32],
    target_vec: &[f64; 5],
    max_nodes: f64,
    max_edges: f64,
    max_depth: f64,
    goal_w: f64,
    struct_w: f64,
) -> Vec<SimilarTemplate> {
    entries
        .iter()
        .filter(|e| e.reward > 0.0)
        .map(|entry| {
            let (goal_sim, used_embedding) = if !entry.goal_embedding.is_empty()
                && entry.goal_embedding.len() == goal_embed.len()
            {
                (goal_embedding::cosine_similarity(goal_embed, &entry.goal_embedding), true)
            } else {
                (jaccard(goal, &entry.goal), false)
            };
            let vec = structural_features(entry, max_nodes, max_edges, max_depth);
            let struct_sim = cosine(target_vec, &vec);
            let score = goal_w * goal_sim + struct_w * struct_sim;
            SimilarTemplate {
                entry: entry.clone(),
                score,
                goal_similarity: goal_sim,
                structural_similarity: struct_sim,
                used_embedding,
            }
        })
        .collect()
}
