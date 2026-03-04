use algorithms::graph::csr::Csr;
use anyhow::Result;
use canon::id::NodeId;
use canon::node::CanonId;
use canon::CanonIR;

pub mod borrow_solver;
pub mod call_solver;
pub mod cfg_solver;
pub mod const_solver;
pub mod cycle_diag_solver;
pub mod dep_solver;
pub mod drop_solver;
pub mod exhaustiveness_solver;
pub mod generic_solver;
pub(crate) mod gpu_algorithms;
pub mod impl_solver;
pub mod invariant_solver;
pub mod liveness_solver;
pub mod macro_solver;
pub mod module_solver;
pub mod name_solver;
pub mod provenance_solver;
pub mod return_solver;
pub mod stability_solver;
pub mod trait_solver;
pub mod type_solver;
pub mod unsafe_solver;
pub mod use_solver;
pub mod visibility_solver;

pub fn solve(ir: &mut CanonIR) -> Result<()> {
    invariant_solver::solve(ir)?;
    module_solver::solve(ir)?;
    use_solver::solve(ir)?;
    name_solver::solve(ir)?;
    dep_solver::solve(ir)?;
    type_solver::solve(ir)?;
    call_solver::solve(ir)?;
    cfg_solver::solve(ir)?;
    visibility_solver::solve(ir)?;
    impl_solver::solve(ir)?;
    trait_solver::solve(ir)?;
    generic_solver::solve(ir)?;
    provenance_solver::solve(ir)?;
    cycle_diag_solver::solve(ir)?;
    liveness_solver::solve(ir)?;
    return_solver::solve(ir)?;
    stability_solver::solve(ir)?;
    borrow_solver::solve(ir)?;
    const_solver::solve(ir)?;
    macro_solver::solve(ir)?;
    exhaustiveness_solver::solve(ir)?;
    drop_solver::solve(ir)?;
    unsafe_solver::solve(ir)?;
    Ok(())
}

pub(crate) fn to_node_id(id: CanonId) -> NodeId {
    NodeId(id.0)
}

pub(crate) fn to_canon_id(id: NodeId) -> CanonId {
    CanonId(id.0)
}

pub(crate) fn csr_to_adj<ND, ED>(graph: &canon::csr_graph::CsrGraph<ND, ED>) -> Vec<Vec<usize>> {
    let v = graph.vertex_count();
    (0..v).map(|i| graph.neighbours(NodeId(i as u32)).map(|(dst, _)| dst.index()).collect()).collect()
}

pub(crate) fn graph_to_csr<ND, ED>(graph: &canon::csr_graph::CsrGraph<ND, ED>) -> Csr {
    let row_ptr: Vec<i32> = graph.row_ptr.iter().map(|&x| x as i32).collect();
    let col_idx: Vec<i32> = graph.col_idx.iter().map(|&x| x as i32).collect();
    Csr { row_ptr, col_idx }
}

pub(crate) fn global_csr_to_adj(ir: &CanonIR) -> Vec<Vec<usize>> {
    graph_csr_to_adj(&ir.graph_csr)
}

pub(crate) fn global_csr_rev_to_adj(ir: &CanonIR) -> Vec<Vec<usize>> {
    graph_csr_to_adj(&ir.graph_csr_rev)
}

fn graph_csr_to_adj(csr: &canon::ir::CanonCsr) -> Vec<Vec<usize>> {
    let row_ptr: Vec<i32> = csr.row_ptr.iter().map(|&x| x as i32).collect();
    let col_idx: Vec<i32> = csr.col_idx.iter().map(|&x| x as i32).collect();
    let graph = Csr { row_ptr, col_idx };
    (0..graph.vertex_count()).map(|i| graph.neighbours(i).iter().map(|&dst| dst as usize).collect()).collect()
}
