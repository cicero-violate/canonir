//! Graph builders — Phase 1 of the analysis pipeline.
//!
//! Each sub-module constructs one CSR graph from the ModelIR node arena.
//! The builders produce raw edge lists (src, dst, EdgeKind) which
//! CsrGraph::from_edges compacts into CSR form.
//!
//! Variables:
//!   nodes    : &[Node]              — read-only node arena
//!   edges    : Vec<(u32,u32,ED)>   — raw edge list before CSR compaction
//!
//! Equation (all builders follow this pattern):
//!   for node in nodes:
//!     match node.kind -> emit (src, dst, EdgeKind) pairs
//!   CsrGraph::from_edges(node_ids, edges)

pub mod call_graph;
pub mod cfg_graph;
pub mod module_graph;
pub mod name_graph;
pub mod type_graph;
pub mod region_graph;
pub mod value_graph;
pub mod macro_graph;
