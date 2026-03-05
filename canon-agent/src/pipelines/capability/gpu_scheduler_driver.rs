use super::super::dag::TaskGraph;
use super::kernels::{compute_ready, deadlock_check, priority_sort};
use super::layout::from_task_graph;

pub struct GpuScheduler;

impl GpuScheduler {
    pub fn schedule(graph: &TaskGraph) -> Vec<String> {
        let (gpu, index) = from_task_graph(graph);
        let ready = compute_ready(&gpu);
        let order = priority_sort(&ready, &gpu.priority);
        order.into_iter().filter_map(|idx| index.index_to_id.get(idx).cloned()).collect()
    }

    pub fn detect_deadlock(graph: &TaskGraph) -> bool {
        let (gpu, _index) = from_task_graph(graph);
        deadlock_check(&gpu)
    }
}
