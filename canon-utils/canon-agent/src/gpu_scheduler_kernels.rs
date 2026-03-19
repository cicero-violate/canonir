use crate::gpu_scheduler_layout::{
    gpu_scheduler_layout_is_completed, gpu_scheduler_layout_is_ready_candidate, GpuScheduleGraph,
};
#[cfg(feature = "cuda")]
use algorithms::graph::csr::Csr;
#[cfg(feature = "cuda")]
use algorithms::graph::scheduler_gpu;
#[cfg(feature = "cuda")]
use algorithms::graph::{depth_gpu, reachability, scc_gpu, topological_sort_gpu};
#[cfg(not(feature = "cuda"))]
use algorithms::graph::{scc, topological_sort};
#[cfg(feature = "cuda")]
use algorithms::sorting::gpu as sorting_gpu;
#[cfg(feature = "cuda")]
pub fn graph_cpu_kernels_compute_ready(graph: &GpuScheduleGraph) -> Vec<u8> {
    let status = graph.status.clone();
    let deps_offset = graph.deps_offset.iter().map(|&v| v as i32).collect::<Vec<_>>();
    let deps_flat = graph.deps_flat.iter().map(|&v| v as i32).collect::<Vec<_>>();
    let (ready, _ready_count, _completed) = scheduler_gpu::ready_mask_gpu(&status, &deps_offset, &deps_flat);
    ready
}
#[cfg(not(feature = "cuda"))]
pub fn compute_ready(graph: &GpuScheduleGraph) -> Vec<u8> {
    let mut ready = vec![0u8; graph.node_count as usize];
    for i in 0..graph.node_count as usize {
        if !gpu_scheduler_layout_is_ready_candidate(graph.status[i]) {
            continue;
        }
        let start = graph.deps_offset[i] as usize;
        let end = graph.deps_offset[i + 1] as usize;
        let mut ok = true;
        for dep_idx in &graph.deps_flat[start..end] {
            let dep = *dep_idx as usize;
            if !gpu_scheduler_layout_is_completed(graph.status[dep]) {
                ok = false;
                break;
            }
        }
        if ok {
            ready[i] = 1;
        }
    }
    ready
}
#[cfg(feature = "cuda")]
pub fn graph_cpu_kernels_priority_sort(ready_mask: &[u8], priority: &[u16]) -> Vec<usize> {
    let mut keys = scheduler_gpu::pack_ready_priority(ready_mask, priority);
    sorting_gpu::bitonic_sort_gpu(&mut keys);
    let mut indices = Vec::new();
    for key in keys.into_iter().rev() {
        let idx = (key & 0xFFFF_FFFF) as usize;
        if key < 0 {
            continue;
        }
        indices.push(idx);
    }
    indices
}
#[cfg(not(feature = "cuda"))]
pub fn priority_sort(ready_mask: &[u8], priority: &[u16]) -> Vec<usize> {
    let mut indices: Vec<usize> = ready_mask.iter().enumerate().filter_map(|(i, &r)| if r == 1 { Some(i) } else { None }).collect();
    indices.sort_by(|a, b| {
        let pa = priority.get(*a).copied().unwrap_or(0);
        let pb = priority.get(*b).copied().unwrap_or(0);
        pb.cmp(&pa).then_with(|| a.cmp(b))
    });
    indices
}
pub fn graph_cpu_kernels_deadlock_check(graph: &GpuScheduleGraph) -> bool {
    #[cfg(feature = "cuda")]
    {
        let deps_offset = graph.deps_offset.iter().map(|&v| v as i32).collect::<Vec<_>>();
        let deps_flat = graph.deps_flat.iter().map(|&v| v as i32).collect::<Vec<_>>();
        return scheduler_gpu::deadlock_gpu(&graph.status, &deps_offset, &deps_flat);
    }
    #[cfg(not(feature = "cuda"))]
    {
        let ready_mask = compute_ready(graph);
        let ready_sum = ready_mask.iter().map(|v| *v as u64).sum::<u64>();
        let completed = graph
            .status
            .iter()
            .filter(|&&s| gpu_scheduler_layout_is_completed(s))
            .count();
        ready_sum == 0 && completed < graph.status.len()
    }
}
pub fn graph_cpu_kernels_compute_topo_order(adj: &[Vec<usize>]) -> Vec<usize> {
    #[cfg(feature = "cuda")]
    {
        let csr = Csr::from_adj(adj);
        return topological_sort_gpu::topological_sort_gpu(&csr);
    }
    #[cfg(not(feature = "cuda"))]
    {
        return topological_sort::topological_sort(adj);
    }
}
pub fn graph_cpu_kernels_compute_roots(adj: &[Vec<usize>]) -> Vec<usize> {
    #[cfg(feature = "cuda")]
    {
        let csr = Csr::from_adj(adj);
        let indegree = topological_sort_gpu::indegree_gpu(&csr);
        return indegree.iter().enumerate().filter_map(|(i, &d)| (d == 0).then_some(i)).collect();
    }
    #[cfg(not(feature = "cuda"))]
    {
        let mut indegree = vec![0usize; adj.len()];
        for edges in adj {
            for &v in edges {
                if v < indegree.len() {
                    indegree[v] += 1;
                }
            }
        }
        return indegree.iter().enumerate().filter_map(|(i, &d)| (d == 0).then_some(i)).collect();
    }
}
pub fn graph_cpu_kernels_compute_scc(adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    #[cfg(feature = "cuda")]
    {
        let csr = Csr::from_adj(adj);
        return scc_gpu::scc_gpu(&csr);
    }
    #[cfg(not(feature = "cuda"))]
    {
        return scc::kosaraju_scc(adj);
    }
}
pub fn graph_cpu_kernels_compute_reachability(adj: &[Vec<usize>], roots: &[usize]) -> Vec<bool> {
    #[cfg(feature = "cuda")]
    {
        let csr = Csr::from_adj(adj);
        return reachability::reachability_gpu(&csr, roots);
    }
    #[cfg(not(feature = "cuda"))]
    {
        let mut visited = vec![false; adj.len()];
        let mut stack = Vec::new();
        for &r in roots {
            if r < adj.len() {
                stack.push(r);
            }
        }
        while let Some(u) = stack.pop() {
            if visited[u] {
                continue;
            }
            visited[u] = true;
            for &v in &adj[u] {
                if v < adj.len() && !visited[v] {
                    stack.push(v);
                }
            }
        }
        return visited;
    }
}
pub fn graph_cpu_kernels_compute_depth(adj: &[Vec<usize>]) -> Vec<i32> {
    #[cfg(feature = "cuda")]
    {
        let csr = Csr::from_adj(adj);
        return depth_gpu::longest_path_depth_gpu(&csr);
    }
    #[cfg(not(feature = "cuda"))]
    {
        let topo = topological_sort::topological_sort(adj);
        let mut depth = vec![0i32; adj.len()];
        for &u in &topo {
            for &v in &adj[u] {
                let next = depth[u] + 1;
                if next > depth[v] {
                    depth[v] = next;
                }
            }
        }
        return depth;
    }
}

