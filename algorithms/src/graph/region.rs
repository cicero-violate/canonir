//! Region / lifetime constraint analysis.
//!
//! Variables:
//!   adj     : &[Vec<usize>]       — G_region adjacency list
//!                                   edge u->v means lifetime u outlives v ('u: 'v)
//!   sccs    : Vec<Vec<usize>>     — Kosaraju SCCs of adj
//!
//! Equations:
//!   outlives_cycles(adj) = { scc | scc ∈ kosaraju_scc(adj), |scc| > 1 }
//!
//!   A cycle in G_region means 'a: 'b ∧ 'b: 'a — a contradictory constraint.
//!   outlives_cycles returns all such conflicting groups.
//!   An empty result means the region graph is a valid DAG.

use super::scc::kosaraju_scc;

/// Returns all groups of mutually-conflicting lifetime nodes (SCC size > 1).
///
/// Equation:
///   outlives_cycles(adj) = [ scc | scc ∈ kosaraju_scc(adj), |scc| > 1 ]
pub fn outlives_cycles(adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    kosaraju_scc(adj)
        .into_iter()
        .filter(|scc| scc.len() > 1)
        .collect()
}
