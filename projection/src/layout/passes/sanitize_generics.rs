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
    let _ = (params, ret, defined);
    generics
        .iter()
        .map(|g| {
            let mut g2 = g.clone();
            if !g2.is_lifetime {
                g2.default_ty = None;
            }
            g2
        })
        .collect()
}
