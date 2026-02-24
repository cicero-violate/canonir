//! FFI bridge to control_flow/dominators.cu and control_flow/dataflow.cu
//! via unified libgpu.a.

use crate::graph::csr::Csr;

#[cfg(feature = "cuda")]
unsafe extern "C" {
    fn gpu_dominators(pred_row: *const i32, pred_col: *const i32, n: i32, entry: i32, words: i32, dom_out: *mut u64);

    fn gpu_reaching_definitions(pred_row: *const i32, pred_col: *const i32, b: i32, words: i32, r#gen: *const u64, kill: *const u64, out: *mut u64);
}

/// Compute dominators using GPU.
#[cfg(feature = "cuda")]
pub fn dominators_gpu(pred_csr: &Csr, entry: usize, node_count: usize) -> Vec<u64> {
    let words = (node_count + 63) / 64;
    let mut dom = vec![0u64; node_count * words];
    unsafe {
        gpu_dominators(pred_csr.row_ptr.as_ptr(), pred_csr.col_idx.as_ptr(), node_count as i32, entry as i32, words as i32, dom.as_mut_ptr());
    }
    dom
}

/// Reaching definitions using GPU (bitset form).
#[cfg(feature = "cuda")]
pub fn reaching_definitions_gpu(pred_csr: &Csr, block_count: usize, def_count: usize, r#gen: &[u64], kill: &[u64]) -> Vec<u64> {
    let words = (def_count + 63) / 64;
    let mut out = vec![0u64; block_count * words];
    unsafe {
        gpu_reaching_definitions(pred_csr.row_ptr.as_ptr(), pred_csr.col_idx.as_ptr(), block_count as i32, words as i32, r#gen.as_ptr(), kill.as_ptr(), out.as_mut_ptr());
    }
    out
}
