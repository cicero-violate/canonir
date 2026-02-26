use algorithms::control_flow::dominators::post_dominators;
use anyhow::{bail, Result};
use canon::node::{CanonId, CanonNodeKind, CfgOp};
use canon::CanonIR;

pub fn solve(ir: &CanonIR) -> Result<()> {
    for node in &ir.nodes {
        let (fname, body_id) = match &node.kind {
            CanonNodeKind::Fn { name_id, body: Some(body), .. } => (ir.lookup_name(*name_id).to_string(), *body),
            _ => continue,
        };

        let blocks = match &ir.node(body_id).kind {
            CanonNodeKind::Body { blocks } if !blocks.is_empty() => blocks,
            _ => continue,
        };

        let n = blocks.len();
        let mut succs: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut exit_nodes: Vec<usize> = Vec::new();

        for (i, bb_id) in blocks.iter().enumerate() {
            let CanonNodeKind::BasicBlock { ops, next } = &ir.node(*bb_id).kind else {
                continue;
            };

            if let Some(last) = ops.last() {
                match last {
                    CfgOp::Goto(t) => {
                        let t = *t as usize;
                        if t < n {
                            succs[i].push(t);
                        }
                    }
                    CfgOp::Branch { true_bb, false_bb, .. } => {
                        let t = *true_bb as usize;
                        let f = *false_bb as usize;
                        if t < n {
                            succs[i].push(t);
                        }
                        if f < n {
                            succs[i].push(f);
                        }
                    }
                    CfgOp::Return(_) => exit_nodes.push(i),
                    CfgOp::Unreachable => {}
                    _ => {
                        if let Some(nx) = next {
                            let nx = *nx as usize;
                            if nx < n {
                                succs[i].push(nx);
                            }
                        }
                    }
                }
            }
        }

        if exit_nodes.is_empty() {
            exit_nodes.push(n - 1);
        }

        let post_dom = post_dominators(n, &succs, &exit_nodes);

        for i in 0..n {
            let has_decls = block_decl_names(ir, blocks[i]).next().is_some();
            if has_decls && !post_dom[i].contains(&i) {
                bail!("drop_solver: `{}` block {} has let-bindings but does not post-dominate itself - malformed CFG", fname, i);
            }
        }

        let decl_order: Vec<String> = blocks.iter().flat_map(|bb_id| block_decl_names(ir, *bb_id)).collect();

        if decl_order.is_empty() {
            continue;
        }

        let mut expected_drop = decl_order.clone();
        expected_drop.reverse();

        if exit_nodes.len() > 1 {
            for &exit in &exit_nodes {
                let exit_decls: Vec<String> = blocks.iter().enumerate().filter(|(i, _)| post_dom[exit].contains(i)).flat_map(|(_, bb)| block_decl_names(ir, *bb)).collect();

                let mut exit_drop = exit_decls.clone();
                exit_drop.reverse();

                if !decl_order.is_empty() && !exit_decls.is_empty() && !is_subsequence(&exit_drop, &expected_drop) {
                    bail!("drop_solver: `{}` exit block {} has inconsistent drop order on this path. expected suffix: {:?}, got: {:?}", fname, exit, expected_drop, exit_drop);
                }
            }
        }

        log::info!("drop_solver: `{}` - {} binding(s), drop order: {:?}", fname, decl_order.len(), expected_drop);
    }

    Ok(())
}

fn block_decl_names<'a>(ir: &'a CanonIR, bb_id: CanonId) -> impl Iterator<Item = String> + 'a {
    let mut out = Vec::new();
    if let CanonNodeKind::BasicBlock { ops, .. } = &ir.node(bb_id).kind {
        for op in ops {
            if let CfgOp::Let { lhs, .. } = op {
                if let CanonNodeKind::Local { name_id, .. } = &ir.node(*lhs).kind {
                    out.push(ir.lookup_name(*name_id).to_string());
                }
            }
        }
    }
    out.into_iter()
}

fn is_subsequence(needle: &[String], haystack: &[String]) -> bool {
    let mut ni = 0;
    for h in haystack {
        if ni == needle.len() {
            break;
        }
        if needle[ni] == *h {
            ni += 1;
        }
    }
    ni == needle.len()
}
