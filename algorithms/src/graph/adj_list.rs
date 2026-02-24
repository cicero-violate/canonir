//! Adjacency list graph representation.
//!
//! Variables:
//!   V       = number of vertices
//!   E       = number of directed edges
//!   adj[u]  = Vec<usize> of out-neighbours of vertex u
//!
//! Equations:
//!   add_edge(u, v):  adj[u].push(v),  E += 1
//!   out_degree(u)  = |adj[u]|
//!   in_degree(v)   = |{ u | v in adj[u] }|   (O(V+E) to compute)
//!   add_undirected(u,v): add_edge(u,v) + add_edge(v,u),  E += 2
//!
//! Conversion to CSR (see graph::csr):
//!   row_ptr[v] = sum_{u=0}^{v-1} out_degree(u)
//!   col_idx    = adj[0] ++ adj[1] ++ ... ++ adj[V-1]

use super::csr::Csr;

pub struct AdjList {
    pub adj: Vec<Vec<usize>>,
}

impl AdjList {
    /// Create an empty graph with `v` vertices and no edges.
    pub fn new(v: usize) -> Self {
        Self { adj: vec![Vec::new(); v] }
    }

    pub fn vertex_count(&self) -> usize {
        self.adj.len()
    }

    pub fn edge_count(&self) -> usize {
        self.adj.iter().map(|n| n.len()).sum()
    }

    /// Add a directed edge u -> v.
    pub fn add_edge(&mut self, u: usize, v: usize) {
        self.adj[u].push(v);
    }

    /// Add undirected edge (both directions).
    pub fn add_undirected(&mut self, u: usize, v: usize) {
        self.adj[u].push(v);
        self.adj[v].push(u);
    }

    pub fn neighbours(&self, u: usize) -> &[usize] {
        &self.adj[u]
    }

    /// Convert to CSR — O(V+E).
    pub fn to_csr(&self) -> Csr {
        Csr::from_adj(&self.adj)
    }

    /// Borrow inner slice for algorithms expecting &[Vec<usize>].
    pub fn as_slice(&self) -> &[Vec<usize>] {
        &self.adj
    }
}
