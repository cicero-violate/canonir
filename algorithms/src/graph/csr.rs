//! Compressed Sparse Row (CSR) graph representation.
//!
//! Variables:
//!   V        = number of vertices
//!   E        = number of edges
//!   row_ptr  = prefix-sum of out-degrees, length V+1
//!   col_idx  = concatenated neighbour lists,  length E
//!
//! Equations:
//!   row_ptr[0] = 0
//!   row_ptr[v+1] = row_ptr[v] + out_degree(v)
//!   neighbours(v) = col_idx[ row_ptr[v] .. row_ptr[v+1] ]
//!
//! Conversion from adjacency list:
//!   row_ptr[v] = sum_{u=0}^{v-1} |adj[u]|
//!   col_idx    = adj[0] ++ adj[1] ++ ... ++ adj[V-1]   (concatenation)

pub struct Csr {
    pub row_ptr: Vec<i32>, // length V+1
    pub col_idx: Vec<i32>, // length E
}

impl Csr {
    /// Build CSR from an adjacency list (each inner vec is a neighbour list).
    pub fn from_adj(adj: &[Vec<usize>]) -> Self {
        let mut row_ptr = Vec::with_capacity(adj.len() + 1);
        let mut col_idx = Vec::new();

        row_ptr.push(0i32);

        for neighbours in adj {
            for &v in neighbours {
                col_idx.push(v as i32);
            }
            row_ptr.push(col_idx.len() as i32);
        }

        Self { row_ptr, col_idx }
    }

    pub fn vertex_count(&self) -> usize {
        self.row_ptr.len() - 1
    }

    pub fn edge_count(&self) -> usize {
        self.col_idx.len()
    }

    /// Iterator over neighbours of vertex v.
    pub fn neighbours(&self, v: usize) -> &[i32] {
        let start = self.row_ptr[v] as usize;
        let end = self.row_ptr[v + 1] as usize;
        &self.col_idx[start..end]
    }
}
