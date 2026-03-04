use std::collections::HashMap;
use std::path::{Path, PathBuf};

use algorithms::graph::adj_list::AdjList;
use algorithms::graph::csr::Csr;

use super::{dag, decompose};
use super::capability::Capability;

fn algo_log_path(log_dir: &Path, iter: u32, name: &str) -> PathBuf {
    if iter == 0 {
        log_dir.join(name)
    } else {
        log_dir.join(format!("iter_{:03}_{}", iter, name))
    }
}

pub fn emit_planned_graph(graph: &dag::TaskGraph, log_dir: &Path, iter: u32) {
    #[derive(serde::Serialize)]
    struct GraphSnapshot<'a> {
        nodes: Vec<GraphNode<'a>>,
        edges: Vec<GraphEdge<'a>>,
    }
    #[derive(serde::Serialize)]
    struct GraphNode<'a> {
        id: &'a str,
        deps: &'a [String],
        node_type: decompose::NodeType,
    }
    #[derive(serde::Serialize)]
    struct GraphEdge<'a> {
        from: &'a str,
        to: &'a str,
    }

    let mut edges = Vec::new();
    for n in &graph.nodes {
        for dep in &n.deps {
            edges.push(GraphEdge { from: dep.as_str(), to: n.id.as_str() });
        }
    }
    let nodes = graph
        .nodes
        .iter()
        .map(|n| GraphNode {
            id: n.id.as_str(),
            deps: &n.deps,
            node_type: n.node_type,
        })
        .collect();
    let snapshot = GraphSnapshot { nodes, edges };
    let path = algo_log_path(log_dir, iter, "planned_graph.json");
    if let Ok(pretty) = serde_json::to_string_pretty(&snapshot) {
        let _ = std::fs::write(path, pretty);
    }
}

pub fn run_graph_algorithms(graph: &dag::TaskGraph, log_dir: &Path, iter: u32) {
    let signals = compute_graph_signals(graph);
    let mut id_to_index: HashMap<String, usize> = HashMap::new();
    let mut index_to_id: Vec<String> = Vec::new();
    for n in &graph.nodes {
        let idx = index_to_id.len();
        id_to_index.insert(n.id.clone(), idx);
        index_to_id.push(n.id.clone());
    }
    let mut adj = AdjList::new(graph.nodes.len());
    for n in &graph.nodes {
        let to = match id_to_index.get(&n.id) {
            Some(v) => *v,
            None => continue,
        };
        for dep in &n.deps {
            if let Some(from) = id_to_index.get(dep) {
                adj.add_edge(*from, to);
            }
        }
    }

    let csr = adj.to_csr();
    let levels = algorithms::graph::gpu::bfs_gpu(&csr, 0);
    let snapshot = serde_json::json!({
        "algorithm": "bfs_gpu",
        "source": 0,
        "levels": levels,
        "index_to_id": index_to_id,
        "signals": signals.to_json(&index_to_id),
    });
    let path = algo_log_path(log_dir, iter, "graph_algorithms.json");
    if let Ok(pretty) = serde_json::to_string_pretty(&snapshot) {
        let _ = std::fs::write(path, pretty);
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GraphSignals {
    pub roots: Vec<usize>,
    pub topo_order: Vec<usize>,
    pub sccs: Vec<Vec<usize>>,
    pub unreachable: Vec<usize>,
    pub has_cycle: bool,
}

impl GraphSignals {
    pub fn to_json(&self, index_to_id: &[String]) -> serde_json::Value {
        let to_ids = |idxs: &[usize]| idxs.iter().filter_map(|i| index_to_id.get(*i)).cloned().collect::<Vec<_>>();
        let scc_ids = self.sccs.iter().map(|comp| to_ids(comp)).collect::<Vec<_>>();
        serde_json::json!({
            "roots": to_ids(&self.roots),
            "topo_order": to_ids(&self.topo_order),
            "sccs": scc_ids,
            "unreachable": to_ids(&self.unreachable),
            "has_cycle": self.has_cycle
        })
    }
}

pub fn compute_graph_signals(graph: &dag::TaskGraph) -> GraphSignals {
    let n = graph.nodes.len();
    let mut id_to_index: HashMap<&str, usize> = HashMap::new();
    for (idx, node) in graph.nodes.iter().enumerate() {
        id_to_index.insert(node.id.as_str(), idx);
    }
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut indegree = vec![0usize; n];
    for node in &graph.nodes {
        let to = match id_to_index.get(node.id.as_str()) {
            Some(v) => *v,
            None => continue,
        };
        for dep in &node.deps {
            if let Some(from) = id_to_index.get(dep.as_str()) {
                adj[*from].push(to);
                indegree[to] += 1;
            }
        }
    }
    let roots: Vec<usize> = indegree
        .iter()
        .enumerate()
        .filter_map(|(i, &d)| if d == 0 { Some(i) } else { None })
        .collect();
    let topo_order = algorithms::graph::topological_sort::topological_sort(&adj);
    let has_cycle = topo_order.len() != n;
    let sccs = algorithms::graph::scc::kosaraju_scc(&adj)
        .into_iter()
        .filter(|c| c.len() > 1)
        .collect::<Vec<_>>();

    let reach = reachability_mask(&adj, &roots);

    let unreachable = reach
        .iter()
        .enumerate()
        .filter_map(|(i, &ok)| if ok { None } else { Some(i) })
        .collect::<Vec<_>>();
    GraphSignals {
        roots,
        topo_order,
        sccs,
        unreachable,
        has_cycle,
    }
}

fn reachability_mask(adj: &[Vec<usize>], roots: &[usize]) -> Vec<bool> {
    let csr = Csr::from_adj(adj);
    algorithms::graph::reachability::reachability_gpu(&csr, roots)
}

pub fn planner_signals_for_graph(graph: &dag::TaskGraph) -> String {
    let signals = compute_graph_signals(graph);
    let ids: Vec<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
    let to_id = |i: usize| ids.get(i).copied().unwrap_or("<unknown>");
    let roots = signals.roots.iter().map(|&i| to_id(i)).collect::<Vec<_>>().join(", ");
    let unreachable = signals.unreachable.iter().map(|&i| to_id(i)).collect::<Vec<_>>().join(", ");
    let topo = signals.topo_order.iter().map(|&i| to_id(i)).collect::<Vec<_>>().join(", ");
    let sccs = signals
        .sccs
        .iter()
        .map(|comp| comp.iter().map(|&i| to_id(i)).collect::<Vec<_>>().join(" -> "))
        .collect::<Vec<_>>()
        .join(" | ");
    format!(
        "roots=[{}]; unreachable=[{}]; topo_order=[{}]; sccs=[{}]; has_cycle={}",
        roots, unreachable, topo, sccs, signals.has_cycle
    )
}

pub fn enforce_linking_constraints(graph: &dag::TaskGraph) -> Result<(), String> {
    // Linker may intentionally leave nodes unconnected. Only ensure no self-cycle at this layer.
    for n in &graph.nodes {
        if n.deps.iter().any(|d| d == &n.id) {
            return Err(format!("self-cycle for node {}", n.id));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FeatureVector {
    pub nodes: usize,
    pub edges: usize,
    pub depth: usize,
    pub scc_count: usize,
    pub failure_rate: f64,
    pub reward_trend: f64,
}

impl FeatureVector {
    pub fn to_vec(&self) -> Vec<f64> {
        vec![
            self.nodes as f64,
            self.edges as f64,
            self.depth as f64,
            self.scc_count as f64,
            self.failure_rate,
            self.reward_trend,
        ]
    }

    pub fn with_reward_history(mut self, rewards: &[f64]) -> Self {
        if rewards.len() >= 2 {
            self.reward_trend = rewards[rewards.len() - 1] - rewards[0];
        }
        self
    }
}

pub fn graph_features(graph: &dag::TaskGraph) -> FeatureVector {
    let signals = compute_graph_signals(graph);
    let nodes = graph.nodes.len();
    let edges = graph.nodes.iter().map(|n| n.deps.len()).sum();
    let depth = compute_max_depth(graph);
    let failed = graph.nodes.iter().filter(|n| n.status == dag::Status::Failed).count();
    let failure_rate = if nodes == 0 { 0.0 } else { failed as f64 / nodes as f64 };
    FeatureVector {
        nodes,
        edges,
        depth,
        scc_count: signals.sccs.len(),
        failure_rate,
        reward_trend: 0.0,
    }
}

pub fn node_utility(graph: &dag::TaskGraph, node_id: &str, iter: u64) -> f64 {
    let node = match graph.nodes.iter().find(|n| n.id == node_id) {
        Some(n) => n,
        None => return 0.0,
    };
    let dependents = graph.nodes.iter()
        .filter(|n| n.deps.iter().any(|d| d == node_id))
        .count();
    let completion_value = if node.status == dag::Status::Completed && node.error.is_none() { 1.0 } else { 0.0 };
    let age = node.completed_iter.map(|t| iter.saturating_sub(t)).unwrap_or(0) as f64;
    0.6 * dependents as f64 + 0.3 * completion_value - 0.1 * age
}

pub fn graph_signature(graph: &dag::TaskGraph) -> String {
    let mut nodes = graph.nodes.iter().map(|n| {
        let mut caps: Vec<Capability> = n.required_capabilities.clone();
        caps.sort_by_key(|c| format!("{:?}", c));
        (n.id.clone(), format!("{:?}", n.node_type), caps)
    }).collect::<Vec<_>>();
    nodes.sort_by(|a, b| a.0.cmp(&b.0));

    let mut edges = Vec::new();
    for n in &graph.nodes {
        for dep in &n.deps {
            edges.push((dep.clone(), n.id.clone()));
        }
    }
    edges.sort();

    let mut hasher = Fnv64::new();
    hasher.write(b"nodes");
    for (id, node_type, caps) in nodes {
        hasher.write(id.as_bytes());
        hasher.write(node_type.as_bytes());
        for cap in caps {
            hasher.write(format!("{:?}", cap).as_bytes());
        }
    }
    hasher.write(b"edges");
    for (from, to) in edges {
        hasher.write(from.as_bytes());
        hasher.write(to.as_bytes());
    }
    format!("{:016x}", hasher.finish())
}

fn compute_max_depth(graph: &dag::TaskGraph) -> usize {
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

struct Fnv64 {
    state: u64,
}

impl Fnv64 {
    fn new() -> Self {
        Self { state: 0xcbf29ce484222325 }
    }

    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.state ^= *b as u64;
            self.state = self.state.wrapping_mul(0x100000001b3);
        }
    }

    fn finish(&self) -> u64 {
        self.state
    }
}
