use anyhow::Result;
use canon::node::{CanonNodeKind, CfgOp};
use canon::CanonIR;

pub fn solve(ir: &CanonIR) -> Result<()> {
    let enums: Vec<(String, Vec<String>)> = ir
        .nodes
        .iter()
        .filter_map(|n| {
            if let CanonNodeKind::Enum { name_id, variants, .. } = &n.kind {
                let vnames: Vec<String> =
                    variants.iter().filter_map(|id| if let CanonNodeKind::Variant { name_id, .. } = &ir.node(*id).kind { Some(ir.lookup_name(*name_id).to_string()) } else { None }).collect();
                Some((ir.lookup_name(*name_id).to_string(), vnames))
            } else {
                None
            }
        })
        .collect();

    if enums.is_empty() {
        return Ok(());
    }

    let mut body_text = String::new();
    for node in &ir.nodes {
        if let CanonNodeKind::Fn { body: Some(body_id), .. } = &node.kind {
            collect_body_text(ir, *body_id, &mut body_text);
        }
    }

    for (enum_name, variants) in &enums {
        if variants.is_empty() {
            continue;
        }
        if body_text.contains("_ =>") || body_text.contains("match _") {
            log::info!("exhaustiveness_solver: enum `{}` - wildcard arm present, assumed covered", enum_name);
            continue;
        }
        let uncovered: Vec<&str> = variants.iter().filter(|v| !body_text.contains(v.as_str())).map(|v| v.as_str()).collect();
        if !uncovered.is_empty() {
            log::warn!("exhaustiveness_solver: enum `{}` may have uncovered variants: {:?}", enum_name, uncovered);
        } else {
            log::info!("exhaustiveness_solver: enum `{}` - all {} variant(s) referenced", enum_name, variants.len());
        }
    }

    Ok(())
}

fn collect_body_text(ir: &CanonIR, body_id: canon::node::CanonId, out: &mut String) {
    let CanonNodeKind::Body { blocks } = &ir.node(body_id).kind else {
        return;
    };

    for bb_id in blocks {
        let CanonNodeKind::BasicBlock { ops, .. } = &ir.node(*bb_id).kind else {
            continue;
        };
        for op in ops {
            match op {
                CfgOp::Expr(local_id) => {
                    if let CanonNodeKind::Local { name_id, .. } = &ir.node(*local_id).kind {
                        out.push_str(ir.lookup_name(*name_id));
                        out.push('\n');
                    }
                }
                _ => {}
            }
        }
    }
}
