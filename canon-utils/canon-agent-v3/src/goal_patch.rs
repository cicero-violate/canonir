use super::capability_types::{capability_model_assert_class_disjoint, PipelineCapability};
use super::goal_graph::{GoalGraph, GoalNode, NodeStatus};
use super::decompose::DecomposeTaskSpec;
use canon_event::emit_debug::info;
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
pub struct GoalGraphPatch {
    #[serde(default)]
    pub new_nodes: Vec<DecomposeTaskSpec>,
    #[serde(default)]
    pub new_edges: Vec<PlannerUpdateEdgeSpec>,
    #[serde(default)]
    pub retract_nodes: Vec<PlannerUpdateRetractSpec>,
    #[serde(default)]
    pub rewrite_nodes: Vec<PlannerUpdateRewriteSpec>,
}

#[derive(Debug, Clone)]
pub enum GoalGraphEvent {
    NodeCreated { node_id: String, description: String, deps: Vec<String>, caps: Vec<String>, node_type: String, priority: u8, budget: Option<u32> },
    NodeRetracted { node_id: String },
    NodeRewritten { node_id: String, new_description: String, new_caps: Vec<String> },
    EdgeDefined { from: String, to: String },
}

pub fn apply_graph_patch(graph: &mut GoalGraph, update: GoalGraphPatch) -> Result<Vec<GoalGraphEvent>> {
    let new_nodes_specs = update.new_nodes.clone();
    let new_edges_specs = update.new_edges.clone();
    let retract_specs = update.retract_nodes.clone();
    let rewrite_specs = update.rewrite_nodes.clone();
    info(
        "agent_planner",
        "graph_patch",
        serde_json::json!({
            "new_nodes": new_nodes_specs.iter().map(|n| n.id.clone()).collect::<Vec<_>>(),
            "new_edges": new_edges_specs.iter().map(|e| serde_json::json!({"from": e.from, "to": e.to})).collect::<Vec<_>>(),
            "retract_nodes": retract_specs.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
            "rewrite_nodes": rewrite_specs.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
        }),
    );

    let mut events: Vec<GoalGraphEvent> = Vec::new();

    let retract_ids: HashSet<String> = update
        .retract_nodes
        .into_iter()
        .filter_map(|spec| graph.nodes.iter().find(|n| n.id == spec.id).filter(|n| matches!(n.status, NodeStatus::Pending | NodeStatus::Failed)).map(|_| spec.id))
        .collect();
    if !retract_ids.is_empty() {
        for id in &retract_ids {
            events.push(GoalGraphEvent::NodeRetracted { node_id: id.clone() });
        }
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
                let new_caps: Vec<String> = spec.new_capabilities.iter().map(|c| format!("{c:?}")).collect();
                node.description = spec.new_description.clone();
                node.required_capabilities = spec.new_capabilities;
                node.status = NodeStatus::Pending;
                node.error = None;
                node.result = None;
                node.readonly_fail_count = 0;
                node.repair_attempts = 0;
                events.push(GoalGraphEvent::NodeRewritten {
                    node_id: spec.id,
                    new_description: spec.new_description,
                    new_caps,
                });
            }
        }
    }
    let existing: HashSet<String> = graph.nodes.iter().map(|n| n.id.clone()).collect();
    for spec in &new_nodes_specs {
        if !existing.contains(&spec.id) {
            info(
                "agent_planner",
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
    let new_node_specs_filtered: Vec<DecomposeTaskSpec> = update.new_nodes.into_iter().filter(|s| !existing.contains(&s.id)).collect();
    for spec in &new_node_specs_filtered {
        let caps: Vec<String> = spec.required_capabilities.iter().map(|c| format!("{c:?}")).collect();
        events.push(GoalGraphEvent::NodeCreated {
            node_id: spec.id.clone(),
            description: spec.description.clone(),
            deps: spec.deps.clone(),
            caps,
            node_type: format!("{:?}", spec.node_type),
            priority: spec.priority,
            budget: spec.budget,
        });
    }
    graph.nodes.extend(new_node_specs_filtered.into_iter().map(|spec| GoalNode {
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
                deps.push(edge.from.clone());
                events.push(GoalGraphEvent::EdgeDefined { from: edge.from, to: edge.to });
            }
        }
    }
    Ok(events)
}
