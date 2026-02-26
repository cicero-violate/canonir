use canon::ir::CanonIR;
use canon::node::{CanonId, CanonNodeKind, CfgOp};

use crate::emit::types::render_type_id;

pub fn emit_body(ir: &CanonIR, body_id: CanonId, pad: &str) -> String {
    let CanonNodeKind::Body { blocks } = &ir.node(body_id).kind else {
        return String::new();
    };
    let mut out = String::new();
    for bb in blocks {
        let CanonNodeKind::BasicBlock { ops, .. } = &ir.node(*bb).kind else {
            continue;
        };
        for op in ops {
            out.push_str(pad);
            out.push_str(&render_op(ir, op));
            if !out.ends_with('\n') {
                out.push('\n');
            }
        }
    }
    out
}

fn render_op(ir: &CanonIR, op: &CfgOp) -> String {
    match op {
        CfgOp::Let { lhs, ty, rhs } => {
            let lhs_name = local_name(ir, *lhs);
            let rhs_expr = rhs.map(|r| local_name(ir, r)).unwrap_or_else(|| "Default::default()".into());
            format!("let {}: {} = {};", lhs_name, render_type_id(ir, *ty), rhs_expr)
        }
        CfgOp::Assign { lhs, rhs } => format!("{} = {};", local_name(ir, *lhs), local_name(ir, *rhs)),
        CfgOp::Return(v) => match v {
            Some(v) => format!("return {};", local_name(ir, *v)),
            None => "return;".into(),
        },
        CfgOp::Call { func, args, dest } => {
            let fname = callable_name(ir, *func);
            let args = args.iter().map(|a| local_name(ir, *a)).collect::<Vec<_>>().join(", ");
            match dest {
                Some(d) => format!("let {} = {}({});", local_name(ir, *d), fname, args),
                None => format!("{}({});", fname, args),
            }
        }
        CfgOp::Branch { .. } => "// branch".into(),
        CfgOp::Goto(_) => "// goto".into(),
        CfgOp::Unreachable => "unreachable!();".into(),
        CfgOp::Expr(v) => format!("{};", local_name(ir, *v)),
        CfgOp::Raw(id) => ir.lookup_name(*id).to_string(),
    }
}

fn callable_name(ir: &CanonIR, id: CanonId) -> String {
    match &ir.node(id).kind {
        CanonNodeKind::Fn { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        _ => format!("node_{}", id.0),
    }
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
