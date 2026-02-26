use crate::solver::csr_to_adj;
use algorithms::graph::scc::kosaraju_scc;
use anyhow::Result;
use canon::node::CanonNodeKind;
use canon::CanonIR;

fn node_label(ir: &CanonIR, kind: &CanonNodeKind) -> String {
    match kind {
        CanonNodeKind::Struct { name_id, .. } => format!("struct {}", ir.lookup_name(*name_id)),
        CanonNodeKind::Trait { name_id, .. } => format!("trait {}", ir.lookup_name(*name_id)),
        CanonNodeKind::Fn { name_id, .. } => format!("fn {}", ir.lookup_name(*name_id)),
        CanonNodeKind::TypeAlias { name_id, .. } => format!("type {}", ir.lookup_name(*name_id)),
        CanonNodeKind::TypeRef { name_id } => format!("ref {}", ir.lookup_name(*name_id)),
        _ => "?".to_string(),
    }
}

pub fn solve(ir: &mut CanonIR) -> Result<()> {
    let v = ir.type_graph.vertex_count();
    if v == 0 {
        return Ok(());
    }

    let adj = csr_to_adj(&ir.type_graph);
    let sccs = kosaraju_scc(&adj);

    for scc in sccs.iter().filter(|s| s.len() > 1) {
        let labels: Vec<String> = scc.iter().filter_map(|&idx| ir.nodes.get(idx).map(|n| node_label(ir, &n.kind))).collect();
        let diag_label = format!("cycle:{}", labels.join(" -> "));
        log::debug!("DIAG cycle_diag_solver: type cycle detected [{}]: {}", scc.len(), &diag_label[6..]);
        let name_id = ir.intern_name(&diag_label);
        let diag_id = ir.push_node(CanonNodeKind::TypeRef { name_id });
        ir.emit_order.push(diag_id);
    }

    Ok(())
}
