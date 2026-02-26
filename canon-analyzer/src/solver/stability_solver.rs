use anyhow::Result;
use canon::node::CanonNodeKind;
use canon::CanonIR;

fn node_sort_key(ir: &CanonIR, kind: &CanonNodeKind) -> (&'static str, String) {
    match kind {
        CanonNodeKind::Crate { name_id, .. } => ("0_crate", ir.lookup_name(*name_id).to_string()),
        CanonNodeKind::Module { path_id, .. } => ("1_module", ir.lookup_path(*path_id).to_string()),
        CanonNodeKind::Use { path_id, .. } => ("2_use", ir.lookup_path(*path_id).to_string()),
        CanonNodeKind::Struct { name_id, .. } => ("3_struct", ir.lookup_name(*name_id).to_string()),
        CanonNodeKind::Trait { name_id, .. } => ("4_trait", ir.lookup_name(*name_id).to_string()),
        CanonNodeKind::TypeAlias { name_id, .. } => ("5_tyalias", ir.lookup_name(*name_id).to_string()),
        CanonNodeKind::Impl { for_ty, for_trait, .. } => ("6_impl", format!("{}__{}", for_ty.0, for_trait.map(|t| t.0).unwrap_or(0))),
        CanonNodeKind::Fn { name_id, .. } => ("7_fn", ir.lookup_name(*name_id).to_string()),
        CanonNodeKind::Enum { name_id, .. } => ("3_enum", ir.lookup_name(*name_id).to_string()),
        CanonNodeKind::Const { name_id, .. } => ("5b_const", ir.lookup_name(*name_id).to_string()),
        CanonNodeKind::Static { name_id, .. } => ("5c_static", ir.lookup_name(*name_id).to_string()),
        CanonNodeKind::MacroCall { path_id, .. } => ("9b_macro", ir.lookup_path(*path_id).to_string()),
        CanonNodeKind::TypeRef { name_id } => ("9_tyref", ir.lookup_name(*name_id).to_string()),
        CanonNodeKind::ExternCrate { name_id, .. } => ("1b_extern", ir.lookup_name(*name_id).to_string()),
        CanonNodeKind::Lifetime { name_id } => ("0b_lifetime", ir.lookup_name(*name_id).to_string()),
        _ => ("z_other", format!("{:?}", kind)),
    }
}

pub fn solve(ir: &mut CanonIR) -> Result<()> {
    if ir.emit_order.is_empty() {
        return Ok(());
    }

    let keyed: std::collections::HashMap<u32, (&'static str, String)> = ir.emit_order.iter().filter_map(|id| ir.nodes.get(id.0 as usize).map(|n| (id.0, node_sort_key(ir, &n.kind)))).collect();

    ir.emit_order.sort_by(|&a, &b| keyed.get(&a.0).cmp(&keyed.get(&b.0)));

    Ok(())
}
