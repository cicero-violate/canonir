use std::collections::HashMap;
use std::path::{Path, PathBuf};

use algorithms::graph::adj_list::AdjList;
use algorithms::graph::csr::Csr;

use super::{dag, decompose};

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

#[derive(Debug, Clone)]
struct GraphSignals {
    roots: Vec<usize>,
    topo_order: Vec<usize>,
    sccs: Vec<Vec<usize>>,
    unreachable: Vec<usize>,
    has_cycle: bool,
}

impl GraphSignals {
    fn to_json(&self, index_to_id: &[String]) -> serde_json::Value {
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

fn compute_graph_signals(graph: &dag::TaskGraph) -> GraphSignals {
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
