//! GPU-assisted topological sort (Kahn) using CUDA kernels.

use super::csr::Csr;

#[cfg(feature = "cuda")]
unsafe extern "C" {
    fn gpu_topo_indegree(row_ptr: *const i32, col_idx: *const i32, v: i32, indegree_out: *mut i32);
    fn gpu_topo_frontier(indegree: *const i32, removed: *const u8, v: i32, frontier_out: *mut u8, count_out: *mut i32);
    fn gpu_topo_remove_frontier(
        row_ptr: *const i32,
        col_idx: *const i32,
        v: i32,
        frontier: *const u8,
        indegree: *mut i32,
        removed: *mut u8,
    );
}

#[cfg(feature = "cuda")]
pub fn indegree_gpu(csr: &Csr) -> Vec<i32> {
    let v = csr.vertex_count() as i32;
    let mut indegree = vec![0i32; csr.vertex_count()];
    unsafe {
        gpu_topo_indegree(
            csr.row_ptr.as_ptr(),
            csr.col_idx.as_ptr(),
            v,
            indegree.as_mut_ptr(),
        );
    }
    indegree
}

#[cfg(feature = "cuda")]
pub fn topological_sort_gpu(csr: &Csr) -> Vec<usize> {
    let v = csr.vertex_count() as i32;
    if v == 0 {
        return Vec::new();
    }
    let mut indegree = vec![0i32; v as usize];
    unsafe {
        gpu_topo_indegree(
            csr.row_ptr.as_ptr(),
            csr.col_idx.as_ptr(),
            v,
            indegree.as_mut_ptr(),
        );
    }
    let mut removed = vec![0u8; v as usize];
    let mut order = Vec::with_capacity(v as usize);
    loop {
        let mut frontier = vec![0u8; v as usize];
        let mut count = 0i32;
        unsafe {
            gpu_topo_frontier(
                indegree.as_ptr(),
                removed.as_ptr(),
                v,
                frontier.as_mut_ptr(),
                &mut count,
            );
        }
        if count == 0 {
            break;
        }
        for (idx, &flag) in frontier.iter().enumerate() {
            if flag == 1 {
                order.push(idx);
            }
        }
        unsafe {
            gpu_topo_remove_frontier(
                csr.row_ptr.as_ptr(),
                csr.col_idx.as_ptr(),
                v,
                frontier.as_ptr(),
                indegree.as_mut_ptr(),
                removed.as_mut_ptr(),
            );
        }
        if order.len() == v as usize {
            break;
        }
    }
    order
}
