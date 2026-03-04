use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::dag::TaskGraph;
use super::decompose;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateEntry {
    pub hash: String,
    pub goal: String,
    pub reward: f64,
    pub node_count: usize,
    pub edge_count: usize,
    pub max_depth: usize,
    pub analysis_count: usize,
    pub render_count: usize,
    pub capability_set: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarTemplate {
    pub entry: TemplateEntry,
    pub score: f64,
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

    pub fn find_similar(&self, goal: &str, graph: &TaskGraph, top_k: usize) -> Vec<SimilarTemplate> {
        if self.entries.is_empty() {
            return Vec::new();
        }
        let (max_nodes, max_edges, max_depth) = self.maxima_with_graph(graph);
        let target_entry = entry_from_graph("target", goal, graph, 0.0);
        let target_vec = structural_features(&target_entry, max_nodes, max_edges, max_depth);

        let mut scored: Vec<SimilarTemplate> = self.entries.iter()
            .filter(|e| e.reward > 0.0)
            .map(|entry| {
                let goal_sim = jaccard(goal, &entry.goal);
                let vec = structural_features(entry, max_nodes, max_edges, max_depth);
                let struct_sim = cosine(&target_vec, &vec);
                let score = 0.6 * goal_sim + 0.4 * struct_sim;
                SimilarTemplate { entry: entry.clone(), score }
            })
            .filter(|s| s.score >= 0.2)
            .collect();

        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        scored
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
        reward,
        node_count,
        edge_count,
        max_depth,
        analysis_count,
        render_count,
        capability_set: caps,
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
