//! Pattern Exhaustiveness Solver (S13).
//!
//! Variables:
//!   E         = { v | NodeKind::Enum { variants } }
//!   arms(s)   = variant names extracted from Stmt::Raw/Expr match arms
//!   covered(e, arms) <=> has_wildcard(arms) ∨ ∀ v ∈ e.variants: v.name ∈ arms
//!
//! Equation:
//!   ∀ e ∈ E, ∀ match in IR bodies referencing e:
//!     covered(e, arms)  =>  OK
//!     ¬covered(e, arms) =>  WARN (non-exhaustive match)
//!
//! Current implementation: warns when an enum has variants but no body in the
//! IR references a wildcard or all variant names (best-effort string scan).

use anyhow::Result;
use model::ir::{
    model_ir::ModelIR,
    node::{Body, NodeKind, Stmt},
};

pub fn solve(ir: &ModelIR) -> Result<()> {
    // Collect all enum variant name sets.
    let enums: Vec<(String, Vec<String>)> = ir.nodes.iter()
        .filter_map(|n| {
            if let NodeKind::Enum { name, variants, .. } = &n.kind {
                let vnames: Vec<String> = variants.iter().map(|v| v.name.clone()).collect();
                Some((name.clone(), vnames))
            } else {
                None
            }
        })
        .collect();

    if enums.is_empty() {
        return Ok(());
    }

    // Collect all raw source strings from function/method bodies for scanning.
    let mut body_text = String::new();
    for node in &ir.nodes {
        let body = match &node.kind {
            NodeKind::Function { body, .. } | NodeKind::Method { body, .. } => Some(body),
            _ => None,
        };
        if let Some(b) = body {
            collect_body_text(b, &mut body_text);
        }
    }

    for (enum_name, variants) in &enums {
        if variants.is_empty() {
            continue;
        }
        // Best-effort: if body text contains `_` wildcard arm, assume covered.
        if body_text.contains("_ =>") || body_text.contains("_ =>\n") {
            log::info!("exhaustiveness_solver: enum `{}` — wildcard arm present, assumed covered", enum_name);
            continue;
        }
        // Check that every variant name appears somewhere in the body text.
        let uncovered: Vec<&str> = variants.iter()
            .filter(|v| !body_text.contains(v.as_str()))
            .map(|v| v.as_str())
            .collect();
        if !uncovered.is_empty() {
            log::warn!(
                "exhaustiveness_solver: enum `{}` may have uncovered variants: {:?}",
                enum_name, uncovered
            );
        } else {
            log::info!("exhaustiveness_solver: enum `{}` — all {} variant(s) referenced", enum_name, variants.len());
        }
    }

    Ok(())
}

fn collect_body_text(body: &Body, out: &mut String) {
    match body {
        Body::Raw(s) => out.push_str(s),
        Body::Blocks(bbs) => {
            for bb in bbs {
                for stmt in &bb.stmts {
                    let s = match stmt {
                        Stmt::Raw(s) | Stmt::Expr(s) => s.as_str(),
                        Stmt::Let { init: Some(e), .. } => e.as_str(),
                        Stmt::Return(Some(e)) => e.as_str(),
                        _ => "",
                    };
                    out.push_str(s);
                    out.push('\n');
                }
            }
        }
        Body::None => {}
    }
}
