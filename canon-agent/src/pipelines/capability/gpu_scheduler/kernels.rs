use super::layout::{GpuGraph, is_completed, is_ready_candidate};

#[cfg(feature = "cuda")]
use algorithms::graph::scheduler_gpu;
#[cfg(feature = "cuda")]
use algorithms::sorting::gpu as sorting_gpu;

#[cfg(feature = "cuda")]
pub fn compute_ready(graph: &GpuGraph) -> Vec<u8> {
    let status = graph.status.clone();
    let deps_offset = graph.deps_offset.iter().map(|&v| v as i32).collect::<Vec<_>>();
    let deps_flat = graph.deps_flat.iter().map(|&v| v as i32).collect::<Vec<_>>();
    let (ready, _ready_count, _completed) = scheduler_gpu::ready_mask_gpu(&status, &deps_offset, &deps_flat);
    ready
}

#[cfg(not(feature = "cuda"))]
pub fn compute_ready(graph: &GpuGraph) -> Vec<u8> {
    let mut ready = vec![0u8; graph.node_count as usize];
    for i in 0..graph.node_count as usize {
        if !is_ready_candidate(graph.status[i]) {
            continue;
        }
        let start = graph.deps_offset[i] as usize;
        let end = graph.deps_offset[i + 1] as usize;
        let mut ok = true;
        for dep_idx in &graph.deps_flat[start..end] {
            let dep = *dep_idx as usize;
            if !is_completed(graph.status[dep]) {
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
pub fn priority_sort(ready_mask: &[u8], priority: &[u16]) -> Vec<usize> {
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
    let mut indices: Vec<usize> = ready_mask.iter()
        .enumerate()
        .filter_map(|(i, &r)| if r == 1 { Some(i) } else { None })
        .collect();
    indices.sort_by(|a, b| {
        let pa = priority.get(*a).copied().unwrap_or(0);
        let pb = priority.get(*b).copied().unwrap_or(0);
        pb.cmp(&pa).then_with(|| a.cmp(b))
    });
    indices
}

pub fn deadlock_check(graph: &GpuGraph) -> bool {
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
        let completed = graph.status.iter().filter(|&&s| is_completed(s)).count();
        ready_sum == 0 && completed < graph.status.len()
    }
}
