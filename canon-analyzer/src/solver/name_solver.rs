use crate::solver::{csr_to_adj, to_node_id};
use algorithms::graph::topological_sort::topological_sort;
use anyhow::Result;
use canon::edge::EdgeKind;
use canon::node::CanonNodeKind;
use canon::CanonIR;

pub fn solve(ir: &mut CanonIR) -> Result<()> {
    let v = ir.name_graph.vertex_count();
    if v == 0 {
        return Ok(());
    }

    let adj = csr_to_adj(&ir.name_graph);
    let order = topological_sort(&adj);

    let mut renames: Vec<(usize, String)> = Vec::new();
    for src_idx in &order {
        let src_id = to_node_id(canon::node::CanonId(*src_idx as u32));
        let src_name = node_name(ir, *src_idx);
        if let Some(name) = src_name {
            for (dst_id, edge) in ir.name_graph.neighbours(src_id) {
                if *edge == EdgeKind::Renames {
                    renames.push((dst_id.index(), name.clone()));
                }
            }
        }
    }

    for (idx, new_name) in renames {
        apply_rename(ir, idx, &new_name);
    }

    Ok(())
}

fn node_name(ir: &CanonIR, idx: usize) -> Option<String> {
    let kind = &ir.nodes.get(idx)?.kind;
    match kind {
        CanonNodeKind::Struct { name_id, .. }
        | CanonNodeKind::Enum { name_id, .. }
        | CanonNodeKind::Trait { name_id, .. }
        | CanonNodeKind::Fn { name_id, .. }
        | CanonNodeKind::TypeRef { name_id }
        | CanonNodeKind::TypeAlias { name_id, .. }
        | CanonNodeKind::Const { name_id, .. }
        | CanonNodeKind::Static { name_id, .. }
        | CanonNodeKind::ExternCrate { name_id, .. }
        | CanonNodeKind::Lifetime { name_id }
        | CanonNodeKind::GenericParam { name_id, .. }
        | CanonNodeKind::Param { name_id, .. }
        | CanonNodeKind::Variant { name_id, .. } => Some(ir.lookup_name(*name_id).to_string()),
        CanonNodeKind::Use { alias, path_id, .. } => {
            if let Some(a) = alias {
                Some(ir.lookup_name(*a).to_string())
            } else {
                Some(ir.lookup_path(*path_id).to_string())
            }
        }
        _ => None,
    }
}

fn apply_rename(ir: &mut CanonIR, idx: usize, new_name: &str) {
    let new_id = ir.intern_name(new_name);
    if let Some(node) = ir.nodes.get_mut(idx) {
        match &mut node.kind {
            CanonNodeKind::Use { alias, .. } => *alias = Some(new_id),
            _ => {}
        }
    }
}
