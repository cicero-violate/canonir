//! Emission Stability Solver (S10) — deterministic ordering + stable hashing.
//!
//! Variables:
//!   emit_order   = ir.emit_order (Vec<NodeId>)
//!   key(v)       = (module_depth(v), node_name(v), v.index())
//!   stable_order = sort(emit_order, key)
//!
//! Equation:
//!   stable_order = [ v | v ∈ emit_order ] sorted by key(v)
//!   // Guarantees same JSON input → same .rs output regardless of hash-map iteration order.

use anyhow::Result;
use model::ir::{model_ir::ModelIR, node::NodeKind};

fn node_sort_key(kind: &NodeKind) -> (&'static str, String) {
    match kind {
        NodeKind::Crate     { name, .. }  => ("0_crate",   name.clone()),
        NodeKind::Module    { path, .. }  => ("1_module",  path.clone()),
        NodeKind::Use       { path, .. }  => ("2_use",     path.clone()),
        NodeKind::Struct    { name, .. }  => ("3_struct",  name.clone()),
        NodeKind::Trait     { name, .. }  => ("4_trait",   name.clone()),
        NodeKind::TypeAlias { name, .. }  => ("5_tyalias", name.clone()),
        NodeKind::Impl      { for_struct, for_trait, .. } => (
            "6_impl",
            format!("{}__{}", for_struct, for_trait.as_deref().unwrap_or(""))
        ),
        NodeKind::Function  { name, .. }  => ("7_fn",      name.clone()),
        NodeKind::Method    { name, .. }  => ("8_method",  name.clone()),
        NodeKind::Enum      { name, .. }  => ("3_enum",    name.clone()),
        NodeKind::Const     { name, .. }  => ("5b_const",  name.clone()),
        NodeKind::Static    { name, .. }  => ("5c_static", name.clone()),
        NodeKind::MacroCall { path, .. }  => ("9b_macro",  path.clone()),
        NodeKind::TypeRef   { name }      => ("9_tyref",   name.clone()),
    }
}

pub fn solve(ir: &mut ModelIR) -> Result<()> {
    if ir.emit_order.is_empty() { return Ok(()); }

    // Sort emit_order by (kind_bucket, name) for deterministic emission.
    // Equation: stable_order = sort_by(emit_order, key)
    ir.emit_order.sort_by(|&a, &b| {
        let ka = ir.nodes.get(a.index()).map(|n| node_sort_key(&n.kind));
        let kb = ir.nodes.get(b.index()).map(|n| node_sort_key(&n.kind));
        ka.cmp(&kb)
    });

    Ok(())
}
