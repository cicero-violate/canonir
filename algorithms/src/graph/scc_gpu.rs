//! GPU-accelerated SCC via forward/reverse reachability.
//! Uses GPU reachability kernel for each node (O(V*(V+E))).

use super::csr::Csr;
use super::reachability::reachability_tc_gpu;

#[cfg(feature = "cuda")]
pub fn scc_gpu(csr: &Csr) -> Vec<Vec<usize>> {
    let v = csr.vertex_count();
    if v == 0 {
        return Vec::new();
    }
    let mut rev_adj: Vec<Vec<usize>> = vec![Vec::new(); v];
    for u in 0..v {
        let start = csr.row_ptr[u] as usize;
        let end = csr.row_ptr[u + 1] as usize;
        for &to in &csr.col_idx[start..end] {
            rev_adj[to as usize].push(u);
        }
    }
    let rev = Csr::from_adj(&rev_adj);
    let w = (v + 63) / 64;
    let to = reachability_tc_gpu(csr, v);
    let from = reachability_tc_gpu(&rev, v);
    let mut assigned_words = vec![0u64; w];
    let mut sccs = Vec::new();
    for i in 0..v {
        let word = i >> 6;
        let bit = i & 63;
        if (assigned_words[word] >> bit) & 1 != 0 {
            continue;
        }
        let base = i * w;
        let mut comp = Vec::new();
        for wi in 0..w {
            let mut bits = to[base + wi] & from[base + wi] & !assigned_words[wi];
            while bits != 0 {
                let tz = bits.trailing_zeros() as usize;
                let j = wi * 64 + tz;
                if j < v {
                    assigned_words[wi] |= 1u64 << tz;
                    comp.push(j);
                }
                bits &= bits - 1;
            }
        }
        if !comp.is_empty() {
            sccs.push(comp);
        }
    }
    sccs
}
