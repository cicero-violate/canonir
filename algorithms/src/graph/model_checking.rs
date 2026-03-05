//! GPU model checking: verify invariant over reachable states.
//!
//! `invariant_mask[v]` = 1 if state v satisfies invariant, else 0.

use super::csr::Csr;

#[cfg(not(feature = "cuda"))]
compile_error!("graph::model_checking requires feature \"cuda\" (GPU-only module)");

#[cfg(feature = "cuda")]
unsafe extern "C" {
    fn gpu_model_check(row_ptr: *const i32, col_idx: *const i32, v: i32, e: i32, roots: *const i32, root_count: i32, invariant_mask: *const u8) -> i32;
}

/// Returns true iff all reachable states satisfy the invariant.
#[cfg(feature = "cuda")]
pub fn model_check_gpu(csr: &Csr, roots: &[usize], invariant_mask: &[u8]) -> bool {
    let roots_i32: Vec<i32> = roots.iter().map(|&r| r as i32).collect();
    let ok =
        unsafe { gpu_model_check(csr.row_ptr.as_ptr(), csr.col_idx.as_ptr(), csr.vertex_count() as i32, csr.edge_count() as i32, roots_i32.as_ptr(), roots_i32.len() as i32, invariant_mask.as_ptr()) };
    ok != 0
}
