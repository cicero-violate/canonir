use crate::solver::csr_to_adj;
use algorithms::graph::reachability::is_acyclic;
use anyhow::{bail, Result};
use canon::node::CanonNodeKind;
use canon::CanonIR;
use canon::edge::EdgeKind;
use canon::id::NodeId;

pub fn solve(ir: &CanonIR) -> Result<()> {
    let v = ir.nodes.len();

    let graphs: &[(&str, &dyn Fn() -> Vec<Vec<usize>>)] = &[
        ("name", &|| csr_to_adj(&ir.name_graph)),
        ("type", &|| csr_to_adj(&ir.type_graph)),
        ("call", &|| csr_to_adj(&ir.call_graph)),
        ("module", &|| csr_to_adj(&ir.module_graph)),
        ("cfg", &|| csr_to_adj(&ir.cfg_graph)),
        ("region", &|| csr_to_adj(&ir.region_graph)),
        ("value", &|| csr_to_adj(&ir.value_graph)),
        ("macro", &|| csr_to_adj(&ir.macro_graph)),
    ];
    for (name, g) in graphs {
        let adj = g();
        for (src, neighbours) in adj.iter().enumerate() {
            for &dst in neighbours {
                if src >= v || dst >= v {
                    bail!("invariant_solver: dangling edge in {}_graph: {} -> {} (|V|={})", name, src, dst, v);
                }
            }
        }
    }

    let name_v = ir.name_graph.vertex_count();
    for src_idx in 0..name_v {
        let src_id = NodeId(src_idx as u32);
        for (dst_id, edge) in ir.name_graph.neighbours(src_id) {
            if *edge != EdgeKind::Renames {
                continue;
            }
            let dst_idx = dst_id.index();
            if src_idx >= v || dst_idx >= v {
                continue;
            }
            let src_kind = &ir.nodes[src_idx].kind;
            let dst_kind = &ir.nodes[dst_idx].kind;
            let src_tag = node_kind_tag(src_kind);
            let dst_tag = node_kind_tag(dst_kind);
            let allowed_cross_kind = matches!(src_kind, CanonNodeKind::Use { .. } | CanonNodeKind::ExternCrate { .. });
            if src_tag != dst_tag && !allowed_cross_kind {
                bail!("invariant_solver: illegal Renames edge {} ({}) -> {} ({}): kind mismatch", src_idx, src_tag, dst_idx, dst_tag);
            }
            if allowed_cross_kind && !is_name_bearing(dst_kind) {
                bail!("invariant_solver: illegal Renames edge {} ({}) -> {} ({}): destination is not name-bearing", src_idx, src_tag, dst_idx, dst_tag);
            }
        }
    }

    for (idx, node) in ir.nodes.iter().enumerate() {
        if let CanonNodeKind::Impl { for_ty, .. } = &node.kind {
            let ok = matches!(
                ir.nodes.get(for_ty.0 as usize).map(|n| &n.kind),
                Some(CanonNodeKind::Type { .. }) | Some(CanonNodeKind::Struct { .. }) | Some(CanonNodeKind::Enum { .. }) | Some(CanonNodeKind::Trait { .. }) | Some(CanonNodeKind::TypeAlias { .. })
            );
            if !ok {
                bail!("invariant_solver: Impl node {} references unknown target {:?}", idx, for_ty);
            }
        }
    }

    let mod_v = ir.module_graph.vertex_count();
    if mod_v > 0 {
        let adj = csr_to_adj(&ir.module_graph);
        if !is_acyclic(&adj) {
            bail!("invariant_solver: module_graph contains a cycle");
        }
    }

    Ok(())
}

fn node_kind_tag(kind: &CanonNodeKind) -> &'static str {
    match kind {
        CanonNodeKind::Crate { .. } => "Crate",
        CanonNodeKind::Module { .. } => "Module",
        CanonNodeKind::Struct { .. } => "Struct",
        CanonNodeKind::Enum { .. } => "Enum",
        CanonNodeKind::Trait { .. } => "Trait",
        CanonNodeKind::Impl { .. } => "Impl",
        CanonNodeKind::Fn { .. } => "Fn",
        CanonNodeKind::FnSig { .. } => "FnSig",
        CanonNodeKind::Type { .. } => "Type",
        CanonNodeKind::Field { .. } => "Field",
        CanonNodeKind::Param { .. } => "Param",
        CanonNodeKind::GenericParam { .. } => "GenericParam",
        CanonNodeKind::WherePred { .. } => "WherePred",
        CanonNodeKind::Variant { .. } => "Variant",
        CanonNodeKind::Attr { .. } => "Attr",
        CanonNodeKind::Lifetime { .. } => "Lifetime",
        CanonNodeKind::Const { .. } => "Const",
        CanonNodeKind::Static { .. } => "Static",
        CanonNodeKind::Use { .. } => "Use",
        CanonNodeKind::ExternCrate { .. } => "ExternCrate",
        CanonNodeKind::TypeAlias { .. } => "TypeAlias",
        CanonNodeKind::TypeRef { .. } => "TypeRef",
        CanonNodeKind::MacroCall { .. } => "MacroCall",
        CanonNodeKind::Body { .. } => "Body",
        CanonNodeKind::BasicBlock { .. } => "BasicBlock",
        CanonNodeKind::Local { .. } => "Local",
    }
}

fn is_name_bearing(kind: &CanonNodeKind) -> bool {
    matches!(
        kind,
        CanonNodeKind::Struct { .. }
            | CanonNodeKind::Enum { .. }
            | CanonNodeKind::Trait { .. }
            | CanonNodeKind::Fn { .. }
            | CanonNodeKind::TypeAlias { .. }
            | CanonNodeKind::TypeRef { .. }
            | CanonNodeKind::Const { .. }
            | CanonNodeKind::Static { .. }
            | CanonNodeKind::ExternCrate { .. }
            | CanonNodeKind::Lifetime { .. }
            | CanonNodeKind::GenericParam { .. }
            | CanonNodeKind::Param { .. }
            | CanonNodeKind::Variant { .. }
            | CanonNodeKind::Module { .. }
            | CanonNodeKind::Use { .. }
    )
}
