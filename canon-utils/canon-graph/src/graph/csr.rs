use algorithms::graph::csr::Csr;
use std::collections::HashMap;

use crate::artifacts_loader::{CsrGraph, Edge as GraphEdge};

pub fn build_csr_graph(node_count: usize, edges: &[GraphEdge]) -> CsrGraph {
    let mut row_ptr = vec![0u32; node_count + 1];
    for e in edges {
        if (e.src as usize) < node_count {
            row_ptr[e.src as usize + 1] += 1;
        }
    }
    for i in 1..row_ptr.len() {
        row_ptr[i] += row_ptr[i - 1];
    }
    let mut col_idx = vec![0u32; edges.len()];
    let mut cursor = row_ptr.clone();
    for e in edges {
        let src = e.src as usize;
        if src >= node_count {
            continue;
        }
        let pos = cursor[src] as usize;
        if pos < col_idx.len() {
            col_idx[pos] = e.dst;
            cursor[src] += 1;
        }
    }
    for row in 0..node_count {
        let start = row_ptr[row] as usize;
        let end = row_ptr[row + 1] as usize;
        if end > start {
            col_idx[start..end].sort_unstable();
        }
    }
    CsrGraph { row_ptr, col_idx }
}

pub fn build_callgraph_csr_graph(callgraph: &[(u32, u32)]) -> (Csr, Vec<u32>, Vec<u32>) {
    let mut id_to_local: HashMap<u32, u32> = HashMap::new();
    let mut local_to_id: Vec<u32> = Vec::new();
    let mut edges: Vec<(usize, usize)> = Vec::new();

    for (src, dst) in callgraph {
        let src_local = *id_to_local.entry(*src).or_insert_with(|| {
            let id = local_to_id.len() as u32;
            local_to_id.push(*src);
            id
        });
        let dst_local = *id_to_local.entry(*dst).or_insert_with(|| {
            let id = local_to_id.len() as u32;
            local_to_id.push(*dst);
            id
        });
        edges.push((src_local as usize, dst_local as usize));
    }

    let csr = Csr::from_edges(local_to_id.len(), &edges);

    let mut id_to_local_vec = vec![0u32; local_to_id.len()];
    for (id, local) in id_to_local {
        id_to_local_vec[local as usize] = id;
    }

    (csr, id_to_local_vec, local_to_id)
}
