use canon::ir::CanonIR;
use canon::node::{CanonId, CanonNodeKind, CfgOp};
use std::collections::HashSet;

use crate::emit::types::render_type_id;

pub fn emit_body(ir: &CanonIR, body_id: CanonId, param_names: &[String], pad: &str) -> String {
    let CanonNodeKind::Body { blocks } = &ir.node(body_id).kind else {
        return String::new();
    };
    let mut out = String::new();
    let mut declared: HashSet<String> = param_names.iter().cloned().collect();
    let mut suppressed: HashSet<String> = HashSet::new();
    for bb in blocks {
        let CanonNodeKind::BasicBlock { ops, .. } = &ir.node(*bb).kind else {
            continue;
        };
        for op in ops {
            let rendered = render_op(ir, op, &mut declared, &mut suppressed);
            if rendered.is_empty() {
                continue;
            }
            out.push_str(pad);
            out.push_str(&rendered);
            if !out.ends_with('\n') {
                out.push('\n');
            }
        }
    }
    out
}

fn render_op(ir: &CanonIR, op: &CfgOp, declared: &mut HashSet<String>, suppressed: &mut HashSet<String>) -> String {
    match op {
        CfgOp::Let { lhs, ty, rhs } => {
            let lhs_name = local_name(ir, *lhs);
            declared.insert(lhs_name.clone());
            let rhs_expr = rhs.map(|r| local_name(ir, r)).unwrap_or_else(|| "Default::default()".into());
            format!("let {}: {} = {};", lhs_name, render_type_id(ir, *ty), rhs_expr)
        }
        CfgOp::Assign { lhs, rhs } => {
            let lhs_name = local_name(ir, *lhs);
            let rhs_name = local_name(ir, *rhs);
            if rhs_name == "__canon_suppressed__" {
                bind_or_assign(&lhs_name, "Default::default()".to_string(), declared)
            } else if suppressed.contains(&rhs_name) {
                bind_or_assign(&lhs_name, "Default::default()".to_string(), declared)
            } else if rhs_name == "__canon_call_gap__" {
                bind_or_assign(&lhs_name, "Default::default()".to_string(), declared)
            } else if rhs_name == "__canon_switch_gap__" {
                bind_or_assign(&lhs_name, "Default::default()".to_string(), declared)
            } else {
                bind_or_assign(&lhs_name, rhs_name, declared)
            }
        }
        CfgOp::Return(v) => match v {
            Some(v) => {
                let name = local_name(ir, *v);
                if suppressed.contains(&name) {
                    "return Default::default();".to_string()
                } else {
                    format!("return {};", name)
                }
            }
            None => "return;".into(),
        },
        CfgOp::Call { func, args, dest } => {
            let fname = callable_name(ir, *func);
            let arg_names = args.iter().map(|a| local_name(ir, *a)).collect::<Vec<_>>();
            let args = arg_names.join(", ");
            match dest {
                Some(d) => {
                    let dest_name = local_name(ir, *d);
                    if arg_names.iter().any(|a| suppressed.contains(a)) {
                        suppressed.insert(dest_name.clone());
                        render_suppressed_binding(&dest_name, declared)
                    } else {
                        bind_or_assign(&dest_name, format!("{}({})", fname, args), declared)
                    }
                }
                None => format!("{}({});", fname, args),
            }
        }
        CfgOp::FieldAccess { base, field, dest } => {
            let base_name = local_name(ir, *base);
            let expr = format!("{}.{}", base_name, ir.lookup_name(*field));
            match dest {
                Some(d) => {
                    let dest_name = local_name(ir, *d);
                    if suppressed.contains(&base_name) {
                        suppressed.insert(dest_name.clone());
                        render_suppressed_binding(&dest_name, declared)
                    } else {
                        bind_or_assign(&dest_name, expr, declared)
                    }
                }
                None => format!("{};", expr),
            }
        }
        CfgOp::MethodCall { receiver, method, args, dest } => {
            let receiver_name = local_name(ir, *receiver);
            let arg_names = args.iter().map(|a| local_name(ir, *a)).collect::<Vec<_>>();
            let args = arg_names.join(", ");
            let expr = format!("{}.{}({})", receiver_name, ir.lookup_name(*method), args);
            match dest {
                Some(d) => {
                    let dest_name = local_name(ir, *d);
                    if suppressed.contains(&receiver_name) || arg_names.iter().any(|a| suppressed.contains(a)) {
                        suppressed.insert(dest_name.clone());
                        render_suppressed_binding(&dest_name, declared)
                    } else {
                        bind_or_assign(&dest_name, expr, declared)
                    }
                }
                None => format!("{};", expr),
            }
        }
        CfgOp::Index { base, idx, dest } => {
            let expr = format!("{}[{}]", local_name(ir, *base), local_name(ir, *idx));
            match dest {
                Some(d) => bind_or_assign(&local_name(ir, *d), expr, declared),
                None => format!("{};", expr),
            }
        }
        CfgOp::Closure { .. } => "// closure".into(),
        CfgOp::StructLit { ty, fields, dest } => {
            let ctor = render_type_id(ir, *ty);
            let expr = if fields.is_empty() {
                ctor
            } else if fields.iter().all(|(name, _)| ir.lookup_name(*name).chars().all(|c| c.is_ascii_digit())) {
                let mut values: Vec<(usize, String)> = fields
                    .iter()
                    .map(|(name, val)| {
                        let idx = ir.lookup_name(*name).parse::<usize>().unwrap_or(usize::MAX);
                        (idx, local_name(ir, *val))
                    })
                    .collect();
                values.sort_by_key(|(idx, _)| *idx);
                format!("{}({})", ctor, values.into_iter().map(|(_, v)| v).collect::<Vec<_>>().join(", "))
            } else {
                let fields = fields.iter().map(|(name, val)| format!("{}: {}", ir.lookup_name(*name), local_name(ir, *val))).collect::<Vec<_>>().join(", ");
                format!("{} {{ {} }}", ctor, fields)
            };
            match dest {
                Some(d) => {
                    let dest_name = local_name(ir, *d);
                    if fields.iter().map(|(_, val)| local_name(ir, *val)).any(|v| suppressed.contains(&v)) {
                        suppressed.insert(dest_name.clone());
                        render_suppressed_binding(&dest_name, declared)
                    } else {
                        bind_or_assign(&dest_name, expr, declared)
                    }
                }
                None => format!("{};", expr),
            }
        }
        CfgOp::Match { dest } => match dest {
            Some(d) => {
                let name = local_name(ir, *d);
                bind_or_assign(&name, "Default::default()".to_string(), declared)
            }
            None => "// match".into(),
        },
        CfgOp::Branch { .. } => String::new(),
        CfgOp::Goto(_) => String::new(),
        CfgOp::Unreachable => String::new(),
        CfgOp::Expr(v) => format!("{};", local_name(ir, *v)),
    }
}

fn callable_name(ir: &CanonIR, id: CanonId) -> String {
    match &ir.node(id).kind {
        CanonNodeKind::Fn { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        CanonNodeKind::Local { name_id, .. } => {
            let raw = ir.lookup_name(*name_id);
            resolve_local_callable_path(ir, raw).unwrap_or_else(|| raw.to_string())
        }
        _ => format!("node_{}", id.0),
    }
}

fn resolve_local_callable_path(ir: &CanonIR, raw: &str) -> Option<String> {
    let tail = raw.strip_prefix("crate::")?;
    if tail.contains("::") {
        return Some(raw.to_string());
    }
    let target_name = tail;
    let target_fn = ir.nodes.iter().find(|n| matches!(&n.kind, CanonNodeKind::Fn { name_id, .. } if ir.lookup_name(*name_id) == target_name))?;

    for node in &ir.nodes {
        let CanonNodeKind::Module { path_id, .. } = &node.kind else {
            continue;
        };
        let src = canon::id::NodeId(node.id.0);
        let has_contains = ir.module_graph.neighbours(src).any(|(dst, edge)| matches!(edge, canon::edge::EdgeKind::Contains) && dst.0 == target_fn.id.0);
        if has_contains {
            let module_path = ir.lookup_path(*path_id);
            if module_path == "crate" {
                return Some(format!("crate::{target_name}"));
            }
            return Some(format!("{module_path}::{target_name}"));
        }
    }
    Some(raw.to_string())
}

fn local_name(ir: &CanonIR, id: CanonId) -> String {
    match &ir.node(id).kind {
        CanonNodeKind::Local { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        CanonNodeKind::Param { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        CanonNodeKind::Const { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        CanonNodeKind::Static { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        _ => format!("v{}", id.0),
    }
}

fn bind_or_assign(name: &str, expr: String, declared: &mut HashSet<String>) -> String {
    if declared.contains(name) {
        format!("{name} = {expr};")
    } else {
        declared.insert(name.to_string());
        format!("let mut {name} = {expr};")
    }
}

fn render_suppressed_binding(name: &str, declared: &mut HashSet<String>) -> String {
    // Suppressed bindings are no longer allowed to materialize as panics.
    // Always lower them structurally to a deterministic default value.
    bind_or_assign(name, "Default::default()".to_string(), declared)
}
