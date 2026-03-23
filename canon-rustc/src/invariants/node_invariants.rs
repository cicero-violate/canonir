use anyhow::{bail, Result};
use canon_ir::ir::CanonIR;
use std::collections::HashSet;

pub fn validate_unique_ids(ir: &CanonIR) -> Result<()> {
    let mut seen: HashSet<u32> = HashSet::with_capacity(ir.nodes.len());
    let mut labels: HashSet<String> = HashSet::with_capacity(ir.nodes.len());
    for node in &ir.nodes {
        let id = node.id.0;
        if !seen.insert(id) {
            bail!("Invariant violation: duplicate CanonId detected id={id}");
        }
        let label = canonical_label(ir, &node.kind, id);
        if !label.is_empty() && !labels.insert(label) {
            bail!("Invariant violation: duplicate node symbol detected id={id}");
        }
    }
    Ok(())
}

fn canonical_label(ir: &CanonIR, kind: &canon_ir::node::CanonNodeKind, fallback_id: u32) -> String {
    use canon_ir::node::CanonNodeKind::*;
    let base = match kind {
        Crate { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        Module { path_id, .. } => ir.lookup_path(*path_id).to_string(),
        Struct { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        Enum { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        Trait { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        AssocType { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        AssocConst { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        Impl { .. } => "impl".to_string(),
        Fn { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        FnSig { .. } => "fn_sig".to_string(),
        Type { .. } => "type".to_string(),
        Field { name_id, .. } => name_id.map(|id| ir.lookup_name(id).to_string()).unwrap_or_else(|| "field".to_string()),
        Param { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        GenericParam { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        WherePred { .. } => "where_pred".to_string(),
        Variant { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        Attr { path_id, .. } => ir.lookup_path(*path_id).to_string(),
        Lifetime { name_id } => ir.lookup_name(*name_id).to_string(),
        Const { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        Static { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        Use { path_id, .. } => ir.lookup_path(*path_id).to_string(),
        ExternCrate { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        TypeAlias { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        TypeRef { name_id } => ir.lookup_name(*name_id).to_string(),
        MacroCall { path_id, .. } => ir.lookup_path(*path_id).to_string(),
        PathRef { path_id } => ir.lookup_path(*path_id).to_string(),
        Body { .. } => "body".to_string(),
        BasicBlock { .. } => "bb".to_string(),
        MatchArm { .. } => "match_arm".to_string(),
        Pattern { .. } => "pattern".to_string(),
        VisPath { path_id, .. } => ir.lookup_path(*path_id).to_string(),
        Local { name_id, .. } => ir.lookup_name(*name_id).to_string(),
    };
    if base.is_empty() {
        format!("node#{}", fallback_id)
    } else {
        format!("{base}#{}", fallback_id)
    }
}
