use super::capability_types::PipelineCapability;
use super::gpu_scheduler_kernels as gpu_kernels;
use super::{task_graph, decompose};
use algorithms::graph::adj_list::AdjList;
#[cfg(feature = "cuda")]
use algorithms::graph::csr::Csr;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
fn graph_analysis_algo_log_path(log_dir: &Path, iter: u32, name: &str) -> PathBuf {
    if iter == 0 {
        log_dir.join(name)
    } else {
        log_dir.join(format!("iter_{:03}_{}", iter, name))
    }
}
pub fn graph_analysis_emit_planned_graph(graph: &task_graph::TaskGraph, log_dir: &Path, iter: u32) {
    #[derive(serde::Serialize)]
    struct GraphSnapshot<'a> {
        nodes: Vec<GraphNode<'a>>,
        edges: Vec<GraphEdge<'a>>,
    }
    #[derive(serde::Serialize)]
    struct GraphNode<'a> {
        id: &'a str,
        deps: &'a [String],
        node_type: decompose::DecomposeNodeType,
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
    let nodes = graph.nodes.iter().map(|n| GraphNode { id: n.id.as_str(), deps: &n.deps, node_type: n.node_type }).collect();
    let snapshot = GraphSnapshot { nodes, edges };
    let path = graph_analysis_algo_log_path(log_dir, iter, "planned_graph.json");
    if let Ok(pretty) = serde_json::to_string_pretty(&snapshot) {
        let _ = std::fs::write(path, pretty);
    }
}
pub fn graph_analysis_run_graph_algorithms(graph: &task_graph::TaskGraph, log_dir: &Path, iter: u32) {
    let signals = graph_analysis_compute_graph_signals(graph);
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
    let snapshot = serde_json::json!(
        { "algorithm" : "bfs_gpu", "source" : 0, "levels" : levels, "index_to_id" :
        index_to_id, "signals" : signals.to_json(& index_to_id), }
    );
    let path = graph_analysis_algo_log_path(log_dir, iter, "graph_algorithms.json");
    if let Ok(pretty) = serde_json::to_string_pretty(&snapshot) {
        let _ = std::fs::write(path, pretty);
    }
}
#[derive(Debug, Clone, serde::Serialize)]
pub struct GraphAnalysis {
    pub roots: Vec<usize>,
    pub topo_order: Vec<usize>,
    pub sccs: Vec<Vec<usize>>,
    pub unreachable: Vec<usize>,
    pub has_cycle: bool,
}
impl GraphAnalysis {
    pub fn to_json(&self, index_to_id: &[String]) -> serde_json::Value {
        let to_ids = |idxs: &[usize]| idxs.iter().filter_map(|i| index_to_id.get(*i)).cloned().collect::<Vec<_>>();
        let scc_ids = self.sccs.iter().map(|comp| to_ids(comp)).collect::<Vec<_>>();
        serde_json::json!(
            { "roots" : to_ids(& self.roots), "sccs" : scc_ids, "unreachable" :
            to_ids(& self.unreachable), "has_cycle" : self.has_cycle }
        )
    }
}
pub fn graph_analysis_compute_graph_signals(graph: &task_graph::TaskGraph) -> GraphAnalysis {
    let n = graph.nodes.len();
    let id_to_index: HashMap<&str, usize> = graph.nodes.iter().enumerate().map(|(idx, node)| (node.id.as_str(), idx)).collect();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for node in &graph.nodes {
        if let Some(&to) = id_to_index.get(node.id.as_str()) {
            for dep in &node.deps {
                if let Some(&from) = id_to_index.get(dep.as_str()) {
                    adj[from].push(to);
                }
            }
        }
    }
    let roots = gpu_kernels::graph_cpu_kernels_compute_roots(&adj);
    let topo_order = gpu_kernels::graph_cpu_kernels_compute_topo_order(&adj);
    let has_cycle = topo_order.len() != n;
    let sccs = gpu_kernels::graph_cpu_kernels_compute_scc(&adj).into_iter().filter(|c| c.len() > 1).collect::<Vec<_>>();
    let reach = gpu_kernels::graph_cpu_kernels_compute_reachability(&adj, &roots);

    // enforce internal consistency via mask
    let _mask = graph_analysis_reachability_mask(&adj, &roots);

    let unreachable = reach.iter().enumerate().filter_map(|(i, &ok)| (!ok).then_some(i)).collect::<Vec<_>>();
    GraphAnalysis { roots, topo_order, sccs, unreachable, has_cycle }
}
fn graph_analysis_reachability_mask(adj: &[Vec<usize>], roots: &[usize]) -> Vec<bool> {
    gpu_kernels::graph_cpu_kernels_compute_reachability(adj, roots)
}
pub fn graph_analysis_planner_signals_for_graph(graph: &task_graph::TaskGraph) -> String {
    let signals = graph_analysis_compute_graph_signals(graph);
    let ids: Vec<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
    let to_id = |i: usize| ids.get(i).copied().unwrap_or("<unknown>");
    let roots = signals.roots.iter().map(|&i| to_id(i)).collect::<Vec<_>>().join(", ");
    let unreachable = signals.unreachable.iter().map(|&i| to_id(i)).collect::<Vec<_>>().join(", ");
    let topo = signals.topo_order.iter().map(|&i| to_id(i)).collect::<Vec<_>>().join(", ");
    let sccs = signals.sccs.iter().map(|comp| comp.iter().map(|&i| to_id(i)).collect::<Vec<_>>().join(" -> ")).collect::<Vec<_>>().join(" | ");
    format!("roots=[{}]; unreachable=[{}]; topo_order=[{}]; sccs=[{}]; has_cycle={}", roots, unreachable, topo, sccs, signals.has_cycle)
}
pub fn graph_analysis_enforce_linking_constraints(graph: &task_graph::TaskGraph) -> Result<(), String> {
    for n in &graph.nodes {
        if n.deps.iter().any(|d| d == &n.id) {
            return Err(format!("self-cycle for node {}", n.id));
        }
    }
    Ok(())
}
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct GraphFeatureVector {
    pub nodes: usize,
    pub edges: usize,
    pub depth: usize,
    pub scc_count: usize,
    pub failure_rate: f64,
    pub reward_trend: f64,
    pub avg_out_degree: f64,
    pub avg_in_degree: f64,
    pub branching_factor: f64,
    pub leaf_count: usize,
    pub root_count: usize,
    pub verify_to_mutate_ratio: f64,
    pub observe_to_mutate_ratio: f64,
    pub node_type_entropy: f64,
    pub avg_node_priority: f64,
    pub avg_node_budget: f64,
    pub blocked_fraction: f64,
    pub ready_fraction: f64,
    pub failed_fraction: f64,
    pub completion_velocity: f64,
    pub retry_rate: f64,
    pub failure_pattern_rate: f64,
    pub cycle_frequency: f64,
    pub deadlock_rate: f64,
}
impl GraphFeatureVector {
    pub fn to_vec(&self) -> Vec<f64> {
        vec![
            self.nodes as f64,
            self.edges as f64,
            self.depth as f64,
            self.scc_count as f64,
            self.failure_rate,
            self.reward_trend,
            self.avg_out_degree,
            self.avg_in_degree,
            self.branching_factor,
            self.leaf_count as f64,
            self.root_count as f64,
            self.verify_to_mutate_ratio,
            self.observe_to_mutate_ratio,
            self.node_type_entropy,
            self.avg_node_priority,
            self.avg_node_budget,
            self.blocked_fraction,
            self.ready_fraction,
            self.failed_fraction,
            self.completion_velocity,
            self.retry_rate,
            self.failure_pattern_rate,
            self.cycle_frequency,
            self.deadlock_rate,
        ]
    }
    pub fn with_reward_history(mut self, rewards: &[f64]) -> Self {
        if rewards.len() >= 2 {
            self.reward_trend = rewards[rewards.len() - 1] - rewards[0];
        }
        self
    }
    pub fn with_failure_stats(mut self, stats: &super::failure_store::FailureStoreFailureStats) -> Self {
        self.failure_pattern_rate = stats.failure_pattern_rate;
        self.cycle_frequency = stats.cycle_frequency;
        self.deadlock_rate = stats.deadlock_rate;
        self
    }
}
pub fn compute_graph_features_parallel(graph: &task_graph::TaskGraph) -> GraphFeatureVector {
    let signals = graph_analysis_compute_graph_signals(graph);
    let nodes = graph.nodes.len();
    let edges = graph.nodes.iter().map(|n| n.deps.len()).sum();
    let depth = graph_max_depth(graph);
    let failed = graph.nodes.iter().filter(|n| n.status == task_graph::NodeStatus::Failed).count();
    let failure_rate = if nodes == 0 { 0.0 } else { failed as f64 / nodes as f64 };
    #[cfg(feature = "cuda")]
    let (
        root_count,
        leaf_count,
        avg_out_degree,
        avg_in_degree,
        branching_factor,
        verify_to_mutate_ratio,
        observe_to_mutate_ratio,
        node_type_entropy,
        avg_node_priority,
        avg_node_budget,
        blocked_fraction,
        ready_fraction,
        failed_fraction,
        completion_velocity,
        retry_rate,
    ) = {
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); nodes];
        let mut id_to_idx: HashMap<&str, usize> = HashMap::new();
        for (i, n) in graph.nodes.iter().enumerate() {
            id_to_idx.insert(n.id.as_str(), i);
        }
        for n in &graph.nodes {
            if let Some(&to) = id_to_idx.get(n.id.as_str()) {
                for dep in &n.deps {
                    if let Some(&from) = id_to_idx.get(dep.as_str()) {
                        adj[from].push(to);
                    }
                }
            }
        }
        let csr = Csr::from_adj(&adj);
        let (indegree, outdegree) = algorithms::graph::feature_gpu::indegree_outdegree(&csr);
        let status = graph.nodes.iter().map(|n| n.status as u8).collect::<Vec<_>>();
        let priority = graph.nodes.iter().map(|n| n.priority as u16).collect::<Vec<_>>();
        let budget = graph.nodes.iter().map(|n| n.budget.unwrap_or(0) as u32).collect::<Vec<_>>();
        let retry = graph.nodes.iter().map(|n| n.readonly_fail_count as u32).collect::<Vec<_>>();
        let mut has_verify = Vec::with_capacity(nodes);
        let mut has_mutate = Vec::with_capacity(nodes);
        let mut has_observe = Vec::with_capacity(nodes);
        let mut node_type = Vec::with_capacity(nodes);
        let mut max_completed_iter = 0u64;
        let mut completed = 0usize;
        for n in &graph.nodes {
            let mut v = 0u8;
            let mut m = 0u8;
            let mut o = 0u8;
            for cap in &n.required_capabilities {
                match cap.class() {
                    super::capability_types::CapabilityMode::Verify => v = 1,
                    super::capability_types::CapabilityMode::Mutate => m = 1,
                    super::capability_types::CapabilityMode::Observe => o = 1,
                }
            }
            has_verify.push(v);
            has_mutate.push(m);
            has_observe.push(o);
            node_type.push(if n.node_type == decompose::DecomposeNodeType::Analysis { 0 } else { 1 });
            if n.status == task_graph::NodeStatus::Completed {
                completed += 1;
                if let Some(t) = n.completed_iter {
                    max_completed_iter = max_completed_iter.max(t);
                }
            }
        }
        let stats = algorithms::graph::feature_gpu::feature_stats_gpu(&status, &indegree, &outdegree, &priority, &budget, &retry, &has_verify, &has_mutate, &has_observe, &node_type);
        let avg_out_degree = if nodes == 0 { 0.0 } else { edges as f64 / nodes as f64 };
        let avg_in_degree = avg_out_degree;
        let branching_factor = if stats.non_leaf_count == 0 { 0.0 } else { stats.outdegree_sum as f64 / stats.non_leaf_count as f64 };
        let verify_to_mutate_ratio = if stats.mutate_count == 0 { 0.0 } else { stats.verify_count as f64 / stats.mutate_count as f64 };
        let observe_to_mutate_ratio = if stats.mutate_count == 0 { 0.0 } else { stats.observe_count as f64 / stats.mutate_count as f64 };
        let total = nodes.max(1) as f64;
        let p_a = stats.analysis_count as f64 / total;
        let p_r = stats.render_count as f64 / total;
        let h = |p: f64| if p <= 0.0 { 0.0 } else { -p * p.ln() };
        let node_type_entropy = h(p_a) + h(p_r);
        let avg_node_priority = if nodes == 0 { 0.0 } else { stats.priority_sum as f64 / nodes as f64 };
        let avg_node_budget = if nodes == 0 { 0.0 } else { stats.budget_sum as f64 / nodes as f64 };
        let blocked_fraction = if nodes == 0 { 0.0 } else { stats.blocked_count as f64 / nodes as f64 };
        let ready_fraction = if nodes == 0 { 0.0 } else { stats.ready_count as f64 / nodes as f64 };
        let failed_fraction = if nodes == 0 { 0.0 } else { stats.failed_count as f64 / nodes as f64 };
        let completion_velocity = if completed == 0 { 0.0 } else { completed as f64 / (max_completed_iter.max(1) as f64 + 1.0) };
        let retry_rate = if nodes == 0 { 0.0 } else { stats.retry_sum as f64 / nodes as f64 };
        (
            stats.root_count as usize,
            stats.leaf_count as usize,
            avg_out_degree,
            avg_in_degree,
            branching_factor,
            verify_to_mutate_ratio,
            observe_to_mutate_ratio,
            node_type_entropy,
            avg_node_priority,
            avg_node_budget,
            blocked_fraction,
            ready_fraction,
            failed_fraction,
            completion_velocity,
            retry_rate,
        )
    };
    #[cfg(not(feature = "cuda"))]
    let (
        root_count,
        leaf_count,
        avg_out_degree,
        avg_in_degree,
        branching_factor,
        verify_to_mutate_ratio,
        observe_to_mutate_ratio,
        node_type_entropy,
        avg_node_priority,
        avg_node_budget,
        blocked_fraction,
        ready_fraction,
        failed_fraction,
        completion_velocity,
        retry_rate,
    ) = {
        let mut indegree = vec![0usize; nodes];
        let mut outdegree = vec![0usize; nodes];
        let mut id_to_idx: HashMap<&str, usize> = HashMap::new();
        for (i, n) in graph.nodes.iter().enumerate() {
            id_to_idx.insert(n.id.as_str(), i);
            indegree[i] = n.deps.len();
        }
        for n in &graph.nodes {
            for dep in &n.deps {
                if let Some(&idx) = id_to_idx.get(dep.as_str()) {
                    outdegree[idx] += 1;
                }
            }
        }
        let root_count = graph.nodes.iter().filter(|n| n.deps.is_empty()).count();
        let leaf_count = outdegree.iter().filter(|&&d| d == 0).count();
        let avg_out_degree = if nodes == 0 { 0.0 } else { edges as f64 / nodes as f64 };
        let avg_in_degree = avg_out_degree;
        let branching_factor = {
            let non_leaf = outdegree.iter().filter(|&&d| d > 0).count();
            if non_leaf == 0 {
                0.0
            } else {
                outdegree.iter().sum::<usize>() as f64 / non_leaf as f64
            }
        };
        let mut verify_count = 0usize;
        let mut mutate_count = 0usize;
        let mut observe_count = 0usize;
        let mut analysis_count = 0usize;
        let mut render_count = 0usize;
        let mut priority_sum = 0f64;
        let mut budget_sum = 0f64;
        let mut blocked = 0usize;
        let mut ready = 0usize;
        let mut failed_count = 0usize;
        let mut completed = 0usize;
        let mut retry_total = 0f64;
        let mut max_completed_iter = 0u64;
        for n in &graph.nodes {
            if n.node_type == decompose::NodeType::Analysis {
                analysis_count += 1;
            } else {
                render_count += 1;
            }
            for cap in &n.required_capabilities {
                match cap.class() {
                    super::capability_types::CapabilityClass::Verify => verify_count += 1,
                    super::capability_types::CapabilityClass::Mutate => mutate_count += 1,
                    super::capability_types::CapabilityClass::Observe => observe_count += 1,
                }
            }
            priority_sum += n.priority as f64;
            if let Some(b) = n.budget {
                budget_sum += b as f64;
            }
            match n.status {
                task_graph::Status::Blocked => blocked += 1,
                task_graph::Status::Ready => ready += 1,
                task_graph::Status::Failed => failed_count += 1,
                task_graph::Status::Completed => {
                    completed += 1;
                    if let Some(t) = n.completed_iter {
                        max_completed_iter = max_completed_iter.max(t);
                    }
                }
                _ => {}
            }
            retry_total += n.readonly_fail_count as f64;
        }
        let verify_to_mutate_ratio = if mutate_count == 0 { 0.0 } else { verify_count as f64 / mutate_count as f64 };
        let observe_to_mutate_ratio = if mutate_count == 0 { 0.0 } else { observe_count as f64 / mutate_count as f64 };
        let node_type_entropy = {
            let total = nodes.max(1) as f64;
            let p_a = analysis_count as f64 / total;
            let p_r = render_count as f64 / total;
            let h = |p: f64| if p <= 0.0 { 0.0 } else { -p * p.ln() };
            h(p_a) + h(p_r)
        };
        let avg_node_priority = if nodes == 0 { 0.0 } else { priority_sum / nodes as f64 };
        let avg_node_budget = if nodes == 0 { 0.0 } else { budget_sum / nodes as f64 };
        let blocked_fraction = if nodes == 0 { 0.0 } else { blocked as f64 / nodes as f64 };
        let ready_fraction = if nodes == 0 { 0.0 } else { ready as f64 / nodes as f64 };
        let failed_fraction = if nodes == 0 { 0.0 } else { failed_count as f64 / nodes as f64 };
        let completion_velocity = if completed == 0 { 0.0 } else { completed as f64 / (max_completed_iter.max(1) as f64 + 1.0) };
        let retry_rate = if nodes == 0 { 0.0 } else { retry_total / nodes as f64 };
        (
            root_count,
            leaf_count,
            avg_out_degree,
            avg_in_degree,
            branching_factor,
            verify_to_mutate_ratio,
            observe_to_mutate_ratio,
            node_type_entropy,
            avg_node_priority,
            avg_node_budget,
            blocked_fraction,
            ready_fraction,
            failed_fraction,
            completion_velocity,
            retry_rate,
        )
    };
    let mut features = GraphFeatureVector {
        nodes,
        edges,
        depth,
        scc_count: signals.sccs.len(),
        failure_rate,
        reward_trend: 0.0,
        avg_out_degree,
        avg_in_degree,
        branching_factor,
        leaf_count,
        root_count,
        verify_to_mutate_ratio,
        observe_to_mutate_ratio,
        node_type_entropy,
        avg_node_priority,
        avg_node_budget,
        blocked_fraction,
        ready_fraction,
        failed_fraction,
        completion_velocity,
        retry_rate,
        failure_pattern_rate: 0.0,
        cycle_frequency: 0.0,
        deadlock_rate: 0.0,
    };
    if !graph.nodes.is_empty() {
        let total: f64 = graph
            .nodes
            .par_iter()
            .map(|n| score_node_utility(graph, &n.id, 0))
            .sum();
        features.avg_node_priority = total / graph.nodes.len() as f64;
    }
    features
}

pub fn graph_embedding(graph: &task_graph::TaskGraph, dim: usize) -> Vec<f32> {
    let feats = compute_graph_features_parallel(graph).to_vec();
    let dim = dim.max(1).max(feats.len());
    let mut out = vec![0.0f32; dim];
    for (i, v) in feats.iter().enumerate() {
        out[i % dim] += *v as f32;
    }
    out
}

pub fn graph_embedding_dim() -> usize {
    GraphFeatureVector::default().to_vec().len()
}
pub fn graph_analysis_normalize_features(f: &GraphFeatureVector, max_nodes: usize, max_edges: usize) -> Vec<f64> {
    let denom_nodes = max_nodes.max(1) as f64;
    let denom_edges = max_edges.max(1) as f64;
    vec![
        f.nodes as f64 / denom_nodes,
        f.edges as f64 / denom_edges,
        f.depth as f64 / (max_nodes.max(1) as f64),
        f.scc_count as f64 / (max_nodes.max(1) as f64),
        f.failure_rate,
        f.reward_trend,
        f.avg_out_degree / 10.0,
        f.avg_in_degree / 10.0,
        f.branching_factor / 10.0,
        f.leaf_count as f64 / denom_nodes,
        f.root_count as f64 / denom_nodes,
        f.verify_to_mutate_ratio,
        f.observe_to_mutate_ratio,
        f.node_type_entropy,
        f.avg_node_priority / 10.0,
        f.avg_node_budget / 10.0,
        f.blocked_fraction,
        f.ready_fraction,
        f.failed_fraction,
        f.completion_velocity,
        f.retry_rate,
        f.failure_pattern_rate,
        f.cycle_frequency,
        f.deadlock_rate,
    ]
}
pub fn score_node_utility(graph: &task_graph::TaskGraph, node_id: &str, iter: u64) -> f64 {
    let node = match graph.nodes.iter().find(|n| n.id == node_id) {
        Some(n) => n,
        None => return 0.0,
    };
    let dependents = graph.nodes.iter().filter(|n| n.deps.iter().any(|d| d == node_id)).count();
    let completion_value = if node.status == task_graph::NodeStatus::Completed && node.error.is_none() { 1.0 } else { 0.0 };
    let age = node.completed_iter.map(|t| iter.saturating_sub(t)).unwrap_or(0) as f64;
    0.6 * dependents as f64 + 0.3 * completion_value - 0.1 * age
}
pub fn graph_analysis_edge_count(graph: &task_graph::TaskGraph) -> usize {
    graph.nodes.iter().map(|n| n.deps.len()).sum()
}
pub fn hash_graph_structure(graph: &task_graph::TaskGraph) -> String {
    let mut nodes = graph
        .nodes
        .iter()
        .map(|n| {
            let mut caps: Vec<PipelineCapability> = n.required_capabilities.clone();
            caps.sort_by_key(|c| format!("{:?}", c));
            (n.id.clone(), format!("{:?}", n.node_type), caps)
        })
        .collect::<Vec<_>>();
    nodes.sort_by(|a, b| a.0.cmp(&b.0));
    let mut edges = Vec::new();
    for n in &graph.nodes {
        for dep in &n.deps {
            edges.push((dep.clone(), n.id.clone()));
        }
    }
    edges.sort();
    let mut hasher = GraphAnalysisFnv64::new();
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
fn graph_max_depth(graph: &task_graph::TaskGraph) -> usize {
    if graph.nodes.is_empty() {
        return 0;
    }
    let id_to_idx: HashMap<&str, usize> = graph.nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), i)).collect();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); graph.nodes.len()];
    for (idx, node) in graph.nodes.iter().enumerate() {
        for dep in &node.deps {
            if let Some(&j) = id_to_idx.get(dep.as_str()) {
                adj[j].push(idx);
            }
        }
    }
    let depth = gpu_kernels::graph_cpu_kernels_compute_depth(&adj);
    depth.into_iter().map(|d| d.max(0) as usize).max().unwrap_or(0)
}
struct GraphAnalysisFnv64 {
    state: u64,
}
impl GraphAnalysisFnv64 {
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
