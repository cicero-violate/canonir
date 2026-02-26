use crate::solver::csr_to_adj;
use algorithms::graph::scc::kosaraju_scc;
use anyhow::Result;
use canon::CanonIR;

pub fn solve(ir: &mut CanonIR) -> Result<()> {
    let v = ir.type_graph.vertex_count();
    if v == 0 {
        return Ok(());
    }

    let adj = csr_to_adj(&ir.type_graph);
    let sccs = kosaraju_scc(&adj);
    for scc in sccs.iter().filter(|s| s.len() > 1) {
        let _ = scc;
    }
    Ok(())
}
