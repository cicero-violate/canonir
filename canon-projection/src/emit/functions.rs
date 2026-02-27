use canon::ir::CanonIR;
use canon::node::{flags, CanonId, CanonNodeKind, CfgOp, NameId};

use crate::emit::body::emit_body;
use crate::emit::fmt::vis_token;
use crate::emit::types::render_type_id;

pub fn emit_trait(ir: &CanonIR, name_id: NameId, methods: &[CanonId], flags_u: u32, pad: &str) -> String {
    let vis = vis_token(flags_u);
    let unsafe_kw = if (flags_u & flags::UNSAFE) != 0 { "unsafe " } else { "" };
    let mut out = format!("{}{}{}trait {} {{\n", pad, vis, unsafe_kw, ir.lookup_name(name_id));
    for m in methods {
        if let CanonNodeKind::Fn { name_id, sig_id, flags, .. } = &ir.node(*m).kind {
            let async_kw = if (*flags & flags::ASYNC) != 0 { "async " } else { "" };
            let unsafe_kw = if (*flags & flags::UNSAFE) != 0 { "unsafe " } else { "" };
            let (params, ret) = sig_parts(ir, *sig_id);
            let gens = sig_generics(ir, *sig_id);
            out.push_str(&format!("{}    {}{}fn {}{}({}) -> {};\n", pad, async_kw, unsafe_kw, ir.lookup_name(*name_id), gens, params, ret));
        }
    }
    out.push_str(&format!("{}}}\n", pad));
    out
}

pub fn emit_fn(ir: &CanonIR, name_id: NameId, sig_id: CanonId, body: Option<CanonId>, flags_u: u32, pad: &str) -> String {
    let vis = vis_token(flags_u);
    let unsafe_kw = if (flags_u & flags::UNSAFE) != 0 { "unsafe " } else { "" };
    let async_kw = if (flags_u & flags::ASYNC) != 0 { "async " } else { "" };
    let (params, ret) = sig_parts(ir, sig_id);
    let gens = sig_generics(ir, sig_id);

    if let Some(body_id) = body {
        let mut out = format!("{}{}{}{}fn {}{}({}) -> {} {{\n", pad, vis, unsafe_kw, async_kw, ir.lookup_name(name_id), gens, params, ret);
        let param_names = collect_param_names(ir, sig_id);
        out.push_str(&emit_body(ir, body_id, &param_names, &format!("{}    ", pad)));
        if ret.trim() != "()" && !has_structural_return_op(ir, body_id) {
            panic!("Invariant violation: non-unit function missing structural return op");
        }
        out.push_str(&format!("{}}}\n", pad));
        out
    } else {
        format!("{}{}{}{}fn {}{}({}) -> {};\n", pad, vis, unsafe_kw, async_kw, ir.lookup_name(name_id), gens, params, ret)
    }
}

fn has_structural_return_op(ir: &CanonIR, body_id: CanonId) -> bool {
    let CanonNodeKind::Body { blocks } = &ir.node(body_id).kind else {
        return false;
    };
    for bb in blocks {
        let CanonNodeKind::BasicBlock { ops, .. } = &ir.node(*bb).kind else {
            continue;
        };
        for op in ops {
            match op {
                CfgOp::Return(Some(_)) => return true,
                CfgOp::Match { dest: Some(_) } => return true,
                CfgOp::Assign { lhs, .. }
                | CfgOp::Call { dest: Some(lhs), .. }
                | CfgOp::FieldAccess { dest: Some(lhs), .. }
                | CfgOp::MethodCall { dest: Some(lhs), .. }
                | CfgOp::StructLit { dest: Some(lhs), .. } => {
                    if local_is_ret(ir, *lhs) {
                        return true;
                    }
                }
                _ => {}
            }
        }
    }
    false
}

fn local_is_ret(ir: &CanonIR, id: CanonId) -> bool {
    match &ir.node(id).kind {
        CanonNodeKind::Local { name_id, .. } => ir.lookup_name(*name_id) == "__ret",
        _ => false,
    }
}

fn collect_param_names(ir: &CanonIR, sig_id: CanonId) -> Vec<String> {
    let CanonNodeKind::FnSig { params, .. } = &ir.node(sig_id).kind else {
        return Vec::new();
    };
    params
        .iter()
        .filter_map(|p| match &ir.node(*p).kind {
            CanonNodeKind::Param { name_id, .. } => Some(ir.lookup_name(*name_id).to_string()),
            _ => None,
        })
        .collect()
}

pub fn sig_parts(ir: &CanonIR, sig_id: CanonId) -> (String, String) {
    match &ir.node(sig_id).kind {
        CanonNodeKind::FnSig { params, ret, .. } => {
            let params = params
                .iter()
                .filter_map(|p| match &ir.node(*p).kind {
                    CanonNodeKind::Param { name_id, ty, flags } => {
                        let name = ir.lookup_name(*name_id);
                        if name == "self" {
                            let ty_str = render_type_id(ir, *ty);
                            let rendered = match ty_str.as_str() {
                                "&Self" | "& Self" => "&self".to_string(),
                                "&mut Self" | "&mut  Self" => "&mut self".to_string(),
                                _ if ty_str.starts_with("&mut ") => "&mut self".to_string(),
                                _ if ty_str.starts_with('&') => "&self".to_string(),
                                _ => format!("self: {}", ty_str),
                            };
                            Some(rendered)
                        } else {
                            let mut_kw = if (*flags & flags::MUT) != 0 { "mut " } else { "" };
                            Some(format!("{}{}: {}", mut_kw, name, render_type_id(ir, *ty)))
                        }
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(", ");
            (params, render_type_id(ir, *ret))
        }
        _ => (String::new(), "()".into()),
    }
}

fn sig_generics(ir: &CanonIR, sig_id: CanonId) -> String {
    let CanonNodeKind::FnSig { generics, .. } = &ir.node(sig_id).kind else {
        return String::new();
    };
    if generics.is_empty() {
        return String::new();
    }
    let items: Vec<String> = generics
        .iter()
        .filter_map(|g| match &ir.node(*g).kind {
            CanonNodeKind::GenericParam { name_id, bounds, is_lifetime, .. } => {
                let n = ir.lookup_name(*name_id);
                let base = if *is_lifetime && !n.starts_with('\'') { format!("'{}", n) } else { n.to_string() };
                if bounds.is_empty() {
                    Some(base)
                } else {
                    let bs = bounds
                        .iter()
                        .filter_map(|b| match &ir.node(*b).kind {
                            CanonNodeKind::TypeRef { name_id } => Some(ir.lookup_name(*name_id).to_string()),
                            CanonNodeKind::Trait { name_id, .. } => Some(ir.lookup_name(*name_id).to_string()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" + ");
                    if bs.is_empty() {
                        Some(base)
                    } else {
                        Some(format!("{}: {}", base, bs))
                    }
                }
            }
            _ => None,
        })
        .collect();
    if items.is_empty() {
        String::new()
    } else {
        format!("<{}>", items.join(", "))
    }
}
