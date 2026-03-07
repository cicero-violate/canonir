use super::super::dag::ExecutionGraph;
use super::kernels::{
    graph_cpu_kernels_compute_ready, graph_cpu_kernels_deadlock_check,
    graph_cpu_kernels_priority_sort,
};
use super::layout::gpu_scheduler_layout_from_task_graph;
pub struct GpuScheduler;
impl GpuScheduler {
    pub fn schedule(graph: &ExecutionGraph) -> Vec<String> {
        let (gpu, index) = gpu_scheduler_layout_from_task_graph(graph);
        let ready = graph_cpu_kernels_compute_ready(&gpu);
        let order = graph_cpu_kernels_priority_sort(&ready, &gpu.priority);
        order.into_iter().filter_map(|idx| index.index_to_id.get(idx).cloned()).collect()
    }
    pub fn detect_deadlock(graph: &ExecutionGraph) -> bool {
        let (gpu, _index) = gpu_scheduler_layout_from_task_graph(graph);
        graph_cpu_kernels_deadlock_check(&gpu)
    }
}
