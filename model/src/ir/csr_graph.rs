//! Generic CSR graph with typed node and edge data.
//!
//! Variables:
//!   V        : usize          — vertex count
//!   E        : usize          — edge count
//!   row_ptr  : Vec<u32>       — prefix sums of out-degrees, length V+1
//!   col_idx  : Vec<u32>       — destination node indices,   length E
//!   edge_data: Vec<ED>        — per-edge payload,           length E
//!   node_data: Vec<ND>        — per-node payload,           length V
//!
//! Invariants:
//!   row_ptr[0] = 0
//!   row_ptr[v+1] = row_ptr[v] + out_degree(v)
//!   neighbours(v) = col_idx[ row_ptr[v] .. row_ptr[v+1] ]
//!   edge_data[row_ptr[v]..row_ptr[v+1]] aligned with col_idx

use crate::ir::node::NodeId;
use serde::{Deserialize, Serialize};

/// A directed CSR graph carrying per-node and per-edge payloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsrGraph<ND, ED> {
    /// Per-node payload, indexed by NodeId.
    pub node_data: Vec<ND>,
    /// Prefix-sum of out-degrees.  Length = V + 1.
    pub row_ptr: Vec<u32>,
    /// Destination node indices.  Length = E.
    pub col_idx: Vec<u32>,
    /// Per-edge payload aligned with col_idx.  Length = E.
    pub edge_data: Vec<ED>,
}

impl<ND, ED> CsrGraph<ND, ED> {
    /// Create an empty graph with no nodes.
    pub fn empty() -> Self {
        Self { node_data: Vec::new(), row_ptr: vec![0], col_idx: Vec::new(), edge_data: Vec::new() }
    }

    pub fn vertex_count(&self) -> usize {
        self.node_data.len()
    }
    pub fn edge_count(&self) -> usize {
        self.col_idx.len()
    }

    /// Out-neighbours of vertex v as (dst NodeId, &edge_data) pairs.
    pub fn neighbours(&self, v: NodeId) -> impl Iterator<Item = (NodeId, &ED)> {
        let i = v.index();
        let start = self.row_ptr[i] as usize;
        let end = self.row_ptr[i + 1] as usize;
        (start..end).map(move |e| (NodeId(self.col_idx[e]), &self.edge_data[e]))
    }

    /// Build from a sorted edge list: (src, dst, edge_data).
    /// node_data must already be sized to V.
    /// Edges must be sorted by src ascending (stable).
    pub fn from_edges(node_data: Vec<ND>, mut edges: Vec<(u32, u32, ED)>) -> Self {
        let v = node_data.len();
        edges.sort_by_key(|e| e.0);

        let mut row_ptr = vec![0u32; v + 1];
        for (src, _, _) in &edges {
            row_ptr[*src as usize + 1] += 1;
        }
        // prefix sum
        for i in 1..=v {
            row_ptr[i] += row_ptr[i - 1];
        }

        let col_idx: Vec<u32> = edges.iter().map(|e| e.1).collect();
        let edge_data: Vec<ED> = edges.into_iter().map(|e| e.2).collect();

        Self { node_data, row_ptr, col_idx, edge_data }
    }
}

impl<ND, ED> Default for CsrGraph<ND, ED> {
    fn default() -> Self { Self::empty() }
}
