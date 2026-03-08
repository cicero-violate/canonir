//! GPU feature aggregation for scheduler metrics.

use super::csr::Csr;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct FeatureStats {
    pub root_count: i32,
    pub leaf_count: i32,
    pub blocked_count: i32,
    pub ready_count: i32,
    pub failed_count: i32,
    pub completed_count: i32,
    pub verify_count: i32,
    pub mutate_count: i32,
    pub observe_count: i32,
    pub analysis_count: i32,
    pub render_count: i32,
    pub non_leaf_count: i32,
    pub priority_sum: u64,
    pub budget_sum: u64,
    pub retry_sum: u64,
    pub outdegree_sum: u64,
}

#[cfg(feature = "cuda")]
unsafe extern "C" {
    fn gpu_feature_stats(
        status: *const u8, indegree: *const i32, outdegree: *const i32, priority: *const u16, budget: *const u32, retry: *const u32, has_verify: *const u8, has_mutate: *const u8,
        has_observe: *const u8, node_type: *const u8, v: i32, out: *mut FeatureStats,
    );

    fn gpu_edge_kind_histogram(row_ptr: *const i32, edge_kind: *const u8, v: i32, e: i32, kind_count: i32, out: *mut u32);
}

#[cfg(feature = "cuda")]
pub fn feature_stats_gpu(
    status: &[u8], indegree: &[i32], outdegree: &[i32], priority: &[u16], budget: &[u32], retry: &[u32], has_verify: &[u8], has_mutate: &[u8], has_observe: &[u8], node_type: &[u8],
) -> FeatureStats {
    let v = status.len() as i32;
    let mut out = FeatureStats::default();
    unsafe {
        gpu_feature_stats(
            status.as_ptr(),
            indegree.as_ptr(),
            outdegree.as_ptr(),
            priority.as_ptr(),
            budget.as_ptr(),
            retry.as_ptr(),
            has_verify.as_ptr(),
            has_mutate.as_ptr(),
            has_observe.as_ptr(),
            node_type.as_ptr(),
            v,
            &mut out,
        );
    }
    out
}

#[cfg(feature = "cuda")]
pub fn indegree_outdegree(csr: &Csr) -> (Vec<i32>, Vec<i32>) {
    let v = csr.vertex_count();
    let mut indegree = vec![0i32; v];
    let mut outdegree = vec![0i32; v];
    for u in 0..v {
        let start = csr.row_ptr[u] as usize;
        let end = csr.row_ptr[u + 1] as usize;
        outdegree[u] = (end - start) as i32;
        for &to in &csr.col_idx[start..end] {
            indegree[to as usize] += 1;
        }
    }
    (indegree, outdegree)
}

#[cfg(feature = "cuda")]
pub fn edge_kind_histogram_gpu(csr: &Csr, edge_kind: &[u8], kind_count: usize) -> Vec<u32> {
    if csr.vertex_count() == 0 || kind_count == 0 {
        return Vec::new();
    }
    assert!(csr.vertex_count() <= i32::MAX as usize);
    assert!(csr.edge_count() <= i32::MAX as usize);
    assert_eq!(edge_kind.len(), csr.edge_count());
    let mut out = vec![0u32; csr.vertex_count() * kind_count];
    unsafe {
        gpu_edge_kind_histogram(
            csr.row_ptr.as_ptr(),
            edge_kind.as_ptr(),
            csr.vertex_count() as i32,
            csr.edge_count() as i32,
            kind_count as i32,
            out.as_mut_ptr(),
        );
    }
    out
}
