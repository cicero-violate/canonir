use super::capability::{capability_model_assert_class_disjoint, PipelineCapability};
use super::dag::{ExecutionGraph, ExecutionNode, NodeStatus};
use super::decompose::DecomposeTaskSpec;
use crate::tlog;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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
    let new_nodes_specs = update.new_nodes.clone();
    let new_edges_specs = update.new_edges.clone();
    let retract_specs = update.retract_nodes.clone();
    let rewrite_specs = update.rewrite_nodes.clone();
    tlog::emit(
        "graph_patch",
        serde_json::json!({
            "new_nodes": new_nodes_specs.iter().map(|n| n.id.clone()).collect::<Vec<_>>(),
            "new_edges": new_edges_specs.iter().map(|e| serde_json::json!({"from": e.from, "to": e.to})).collect::<Vec<_>>(),
            "retract_nodes": retract_specs.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
            "rewrite_nodes": rewrite_specs.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
        }),
    );
    let retract_ids: HashSet<String> = update
        .retract_nodes
        .into_iter()
        .filter_map(|spec| graph.nodes.iter().find(|n| n.id == spec.id).filter(|n| matches!(n.status, NodeStatus::Pending | NodeStatus::Failed)).map(|_| spec.id))
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
            if matches!(node.status, NodeStatus::Pending | NodeStatus::Failed) {
                let caps: HashSet<_> = spec.new_capabilities.iter().copied().collect();
                capability_model_assert_class_disjoint(&caps).map_err(|e| anyhow::anyhow!(e))?;
                node.description = spec.new_description;
                node.required_capabilities = spec.new_capabilities;
                node.status = NodeStatus::Pending;
                node.error = None;
                node.result = None;
                node.readonly_fail_count = 0;
                node.repair_attempts = 0;
            }
        }
    }
    let existing: HashSet<String> = graph.nodes.iter().map(|n| n.id.clone()).collect();
    for spec in &new_nodes_specs {
        if !existing.contains(&spec.id) {
            tlog::emit(
                "task_created",
                serde_json::json!({
                    "id": spec.id,
                    "description": spec.description,
                    "deps": spec.deps,
                    "node_type": spec.node_type,
                    "capabilities": spec.required_capabilities,
                    "priority": spec.priority,
                    "budget": spec.budget,
                }),
            );
        }
    }
    graph.nodes.extend(update.new_nodes.into_iter().filter(|s| !existing.contains(&s.id)).map(|spec| ExecutionNode {
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
    }));
    let id_to_idx: HashMap<String, usize> = graph.nodes.iter().enumerate().map(|(i, n)| (n.id.clone(), i)).collect();
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
