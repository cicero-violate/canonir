use crate::dag::{GoalGraph, NodeStatus};
use std::collections::HashMap;
#[derive(Debug, Clone)]
pub struct GpuScheduleGraph {
    pub node_count: u32,
    pub status: Vec<u8>,
    pub priority: Vec<u16>,
    pub deps_offset: Vec<u32>,
    pub deps_flat: Vec<u32>,
}
pub struct GpuGraphIndex {
    pub id_to_index: HashMap<String, usize>,
    pub index_to_id: Vec<String>,
}
pub fn gpu_scheduler_layout_from_task_graph(graph: &GoalGraph) -> (GpuScheduleGraph, GpuGraphIndex) {
    let mut id_to_index = HashMap::new();
    let mut index_to_id = Vec::new();
    for (idx, node) in graph.nodes.iter().enumerate() {
        id_to_index.insert(node.id.clone(), idx);
        index_to_id.push(node.id.clone());
    }
    let mut deps_offset = Vec::with_capacity(graph.nodes.len() + 1);
    let mut deps_flat = Vec::new();
    deps_offset.push(0);
    for node in &graph.nodes {
        for dep in &node.deps {
            if let Some(&idx) = id_to_index.get(dep) {
                deps_flat.push(idx as u32);
            }
        }
        deps_offset.push(deps_flat.len() as u32);
    }
    let status = graph.nodes.iter().map(|n| n.status as u8).collect::<Vec<_>>();
    let priority = graph.nodes.iter().map(|n| n.priority as u16).collect::<Vec<_>>();
    (GpuScheduleGraph { node_count: graph.nodes.len() as u32, status, priority, deps_offset, deps_flat }, GpuGraphIndex { id_to_index, index_to_id })
}
pub fn gpu_scheduler_layout_is_completed(status: u8) -> bool {
    status == NodeStatus::Completed as u8
}
pub fn gpu_scheduler_layout_is_ready_candidate(status: u8) -> bool {
    status == NodeStatus::Pending as u8 || status == NodeStatus::Ready as u8
}
