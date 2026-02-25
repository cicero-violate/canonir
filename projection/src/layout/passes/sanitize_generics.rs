use model::ir::node::{GenericParam, NodeKind, Param};

use super::{LayoutCtx, LayoutPass};
use crate::layout::{ItemPlan, ModuleDeclPlan, Plan};

pub struct SanitizeGenerics;

impl LayoutPass for SanitizeGenerics {
    fn run(&self, plan: &mut Plan, ctx: &LayoutCtx) {
        for file in &mut plan.files {
            sanitize_items(&mut file.items, ctx);
        }
    }
}

fn sanitize_items(items: &mut [ItemPlan], ctx: &LayoutCtx) {
    for item in items.iter_mut() {
        match item {
            ItemPlan::Leaf(kind) => sanitize_kind(kind, ctx),
            ItemPlan::Impl(imp) => {
                for m in &mut imp.methods {
                    sanitize_kind(m, ctx);
                }
            }
            ItemPlan::Module(ModuleDeclPlan { items: child, .. }) => sanitize_items(child, ctx),
            _ => {}
        }
    }
}

fn sanitize_kind(kind: &mut NodeKind, ctx: &LayoutCtx) {
    match kind {
        NodeKind::Function { generics, params, ret, .. } | NodeKind::Method { generics, params, ret, .. } => {
            *generics = sanitize_generics(generics, params, ret, &ctx.defined_types);
        }
        _ => {}
    }
}

fn sanitize_generics(generics: &[GenericParam], params: &[Param], ret: &str, defined: &std::collections::HashSet<String>) -> Vec<GenericParam> {
    let mut gs: Vec<GenericParam> = generics
        .iter()
        .map(|g| {
            let mut g2 = g.clone();
            if !g2.is_lifetime {
                g2.default_ty = None;
            }
            g2
        })
        .collect();
    let mut present: std::collections::HashSet<String> = gs.iter().map(|g| g.name.clone()).collect();
    let mut inferred: Vec<GenericParam> = Vec::new();
    for ty in params.iter().map(|p| p.ty.as_str()).chain(std::iter::once(ret)) {
        if ty.trim_start().starts_with("impl ") || ty.trim_start().starts_with("dyn ") {
            continue;
        }
        if let Some(id) = infer_type_param(ty, defined) {
            if present.insert(id.clone()) {
                inferred.push(GenericParam { name: id, bounds: Vec::new(), is_lifetime: false, default_ty: None });
            }
        }
    }
    gs.extend(inferred);
    gs
}

fn infer_type_param(ty: &str, defined: &std::collections::HashSet<String>) -> Option<String> {
    let mut t = ty.trim();
    if t.contains("::") {
        return None;
    }
    if let Some(rest) = t.strip_prefix('&') {
        t = rest.trim();
    }
    if let Some(rest) = t.strip_prefix('\'') {
        t = rest.trim();
    }
    if let Some(rest) = t.strip_prefix("mut ") {
        t = rest.trim();
    }
    if let Some(rest) = t.strip_prefix("dyn ") {
        t = rest.trim();
    }
    if let Some(rest) = t.strip_prefix("impl ") {
        t = rest.trim();
    }
    let end = t.find(|c: char| matches!(c, ':' | '<' | ' ' | '(' | '[' | ',' | '>' | ')')).unwrap_or_else(|| t.len());
    let ident = &t[..end];
    if ident.is_empty() || !ident.chars().next().unwrap().is_ascii_uppercase() {
        return None;
    }
    let stop = ["Self", "Box", "Vec", "Option", "Result", "String", "Cow", "Path", "PathBuf", "HashMap", "HashSet", "BTreeMap", "BTreeSet", "Rc", "Arc", "Ordering", "Ok", "Err"];
    if stop.contains(&ident) || defined.contains(ident) {
        return None;
    }
    Some(ident.to_string())
}
