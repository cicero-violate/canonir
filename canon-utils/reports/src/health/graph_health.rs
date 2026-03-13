use anyhow::Result;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;

use crate::graph::graph_types::{EdgeRow, NodeRow};
use crate::health::system_health::current_timestamp;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GraphHealthReport {
    pub node_count: usize,
    pub edge_count: usize,
    pub node_kind_counts: BTreeMap<String, usize>,
    pub edge_histogram: BTreeMap<String, usize>,
    pub callsite_nodes: usize,
    pub call_edges: usize,
    pub callsite_coverage: f64,
    pub callgraph_ratio: f64,
    pub orphan_nodes: usize,
    pub module_owner_coverage: f64,
    pub graph_hash: u64,
    pub prev_node_count: Option<usize>,
    pub prev_edge_count: Option<usize>,
    pub graph_drift: f64,
    pub generated_at: String,
}

pub fn write_graph_health_report(
    _graph_dir: &Path,
    reports_dir: &Path,
    nodes: &[NodeRow],
    edges: &[EdgeRow],
    files: &[String],
    cfg: &[EdgeRow],
    callgraph: &[(u32, u32)],
) -> Result<()> {
    fs::create_dir_all(reports_dir)?;
    let mut node_kind_counts: BTreeMap<String, usize> = BTreeMap::new();
    for n in nodes {
        *node_kind_counts.entry(n.kind.clone()).or_insert(0) += 1;
    }
    let mut edge_histogram: BTreeMap<String, usize> = BTreeMap::new();
    for e in edges {
        *edge_histogram.entry(e.kind.clone()).or_insert(0) += 1;
    }
    let callsite_nodes = nodes.iter().filter(|n| n.kind == "CALL_SITE").count();
    let call_edges = edges.iter().filter(|e| e.kind == "CALL").count();
    let callsite_coverage = if callsite_nodes == 0 {
        0.0
    } else {
        call_edges as f64 / callsite_nodes as f64
    };
    let mut incoming: HashSet<u32> = HashSet::new();
    let mut outgoing: HashSet<u32> = HashSet::new();
    for e in edges {
        outgoing.insert(e.src);
        incoming.insert(e.dst);
    }
    let orphan_nodes = nodes
        .iter()
        .filter(|n| !incoming.contains(&n.id) && !outgoing.contains(&n.id))
        .count();

    let mut id_to_kind: HashMap<u32, &str> = HashMap::new();
    for n in nodes {
        id_to_kind.insert(n.id, n.kind.as_str());
    }
    let mut owned: HashSet<u32> = HashSet::new();
    for e in edges {
        if e.kind != "CONTAINS" {
            continue;
        }
        if id_to_kind.get(&e.src).copied() == Some("MODULE") {
            owned.insert(e.dst);
        }
    }
    let eligible = nodes.iter().filter(|n| n.kind != "MODULE").count();
    let module_owner_coverage = if eligible == 0 {
        1.0
    } else {
        owned.len() as f64 / eligible as f64
    };

    let graph_hash = hash_graph_signature(nodes, edges, files);
    let report_path = reports_dir.join("graph_health.json");
    let (prev_node_count, prev_edge_count, graph_drift) = if report_path.exists() {
        let prev: GraphHealthReport = serde_json::from_str(&fs::read_to_string(&report_path)?)
            .unwrap_or(GraphHealthReport {
                node_count: 0,
                edge_count: 0,
                node_kind_counts: BTreeMap::new(),
                edge_histogram: BTreeMap::new(),
                callsite_nodes: 0,
                call_edges: 0,
                callsite_coverage: 0.0,
                callgraph_ratio: 0.0,
                orphan_nodes: 0,
                module_owner_coverage: 0.0,
                graph_hash: 0,
                prev_node_count: None,
                prev_edge_count: None,
                graph_drift: 0.0,
                generated_at: String::new(),
            });
        let prev_total = (prev.node_count + prev.edge_count).max(1) as f64;
        let drift = ((nodes.len() as i64 - prev.node_count as i64).abs() as f64
            + (edges.len() as i64 - prev.edge_count as i64).abs() as f64)
            / prev_total;
        (Some(prev.node_count), Some(prev.edge_count), drift)
    } else {
        (None, None, 0.0)
    };

    let callgraph_ratio = if cfg.is_empty() {
        0.0
    } else {
        callgraph.len() as f64 / cfg.len() as f64
    };

    let report = GraphHealthReport {
        node_count: nodes.len(),
        edge_count: edges.len(),
        node_kind_counts,
        edge_histogram,
        callsite_nodes,
        call_edges,
        callsite_coverage,
        callgraph_ratio,
        orphan_nodes,
        module_owner_coverage,
        graph_hash,
        prev_node_count,
        prev_edge_count,
        graph_drift,
        generated_at: current_timestamp().to_string(),
    };
    fs::write(report_path, serde_json::to_string_pretty(&report)?)?;
    Ok(())
}

pub fn hash_graph_signature(nodes: &[NodeRow], edges: &[EdgeRow], files: &[String]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut node_keys: Vec<(u32, &str, &str, Option<u32>)> = nodes
        .iter()
        .map(|n| (n.id, n.kind.as_str(), n.symbol.as_str(), n.file_id))
        .collect();
    node_keys.sort_by(|a, b| a.0.cmp(&b.0));
    node_keys.hash(&mut hasher);
    let mut edge_keys: Vec<(u32, u32, &str)> = edges
        .iter()
        .map(|e| (e.src, e.dst, e.kind.as_str()))
        .collect();
    edge_keys.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)).then_with(|| a.2.cmp(&b.2)));
    edge_keys.hash(&mut hasher);
    files.hash(&mut hasher);
    hasher.finish()
}
