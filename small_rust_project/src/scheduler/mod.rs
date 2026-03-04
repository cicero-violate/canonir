pub mod frontier;
pub mod ordering;

use crate::dag::{NodeId, Status, TaskGraph};
use frontier::FrontierQueue;
use ordering::deterministic_ready_order;

/// Resolve the next frontier queue from the graph state.
pub fn build_frontier(graph: &TaskGraph) -> FrontierQueue {
    let ready = graph.ready_nodes();
    let ordered = deterministic_ready_order(ready);
    FrontierQueue::from_nodes(ordered)
}
