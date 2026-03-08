use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use super::capability::{capability_model_assert_class_disjoint, PipelineCapability};
use super::dag::{NodeStatus, ExecutionGraph, ExecutionNode};
use super::decompose::DecomposeTaskSpec;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerUpdateEdgeSpec {
    pub from: String,
    pub to: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerUpdateRetractSpec {
    pub id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerUpdateRewriteSpec {
    pub id: String,
    pub new_description: String,
    pub new_capabilities: Vec<PipelineCapability>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphPatch {
    #[serde(default)]
    pub new_nodes: Vec<DecomposeTaskSpec>,
    #[serde(default)]
    pub new_edges: Vec<PlannerUpdateEdgeSpec>,
    #[serde(default)]
    pub retract_nodes: Vec<PlannerUpdateRetractSpec>,
    #[serde(default)]
    pub rewrite_nodes: Vec<PlannerUpdateRewriteSpec>,
}
pub fn apply_graph_patch(graph: &mut ExecutionGraph, update: GraphPatch) -> Result<()> {
    let retract_ids: HashSet<String> = update
        .retract_nodes
        .into_iter()
        .filter_map(|spec| {
            graph
                .nodes
                .iter()
                .find(|n| n.id == spec.id)
                .filter(|n| matches!(n.status, NodeStatus::Pending | NodeStatus::Failed))
                .map(|_| spec.id)
        })
        .collect();
    if !retract_ids.is_empty() {
        graph.nodes.retain(|n| !retract_ids.contains(&n.id));
        for node in &mut graph.nodes {
            node.deps.retain(|d| !retract_ids.contains(d));
        }
        graph.rebuild_index();
    }
    for spec in update.rewrite_nodes {
        if let Some(node) = graph.get_node_mut(&spec.id) {
            if node.status == NodeStatus::Pending {
                let caps: HashSet<_> = spec.new_capabilities.iter().copied().collect();
                capability_model_assert_class_disjoint(&caps)
                    .map_err(|e| anyhow::anyhow!(e))?;
                node.description = spec.new_description;
                node.required_capabilities = spec.new_capabilities;
            }
        }
    }
    let existing: HashSet<String> = graph.nodes.iter().map(|n| n.id.clone()).collect();
    graph
        .nodes
        .extend(
            update
                .new_nodes
                .into_iter()
                .filter(|s| !existing.contains(&s.id))
                .map(|spec| ExecutionNode {
                    id: spec.id,
                    description: spec.description,
                    status: NodeStatus::Pending,
                    deps: spec.deps,
                    required_capabilities: spec.required_capabilities,
                    node_type: spec.node_type,
                    priority: spec.priority,
                    budget: spec.budget,
                    reasoning_trace: spec.reasoning_trace,
                    result: None,
                    error: None,
                    readonly_fail_count: 0,
                    repair_attempts: 0,
                    completed_iter: None,
                }),
        );
    let id_to_idx: HashMap<String, usize> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.clone(), i))
        .collect();
    for edge in update.new_edges {
        if let Some(&to_idx) = id_to_idx.get(&edge.to) {
            let deps = &mut graph.nodes[to_idx].deps;
            if !deps.contains(&edge.from) {
                deps.push(edge.from);
            }
        }
    }
    Ok(())
}
