use anyhow::{bail, Result};
use canon::node::{CanonId, CanonNodeKind, CfgOp, PrimTy, TypeKind};
use canon::CanonIR;
use std::collections::HashSet;

pub fn solve(ir: &CanonIR) -> Result<()> {
    let live_emit: HashSet<usize> = ir.emit_order.iter().map(|id| id.0 as usize).collect();
    for idx in live_emit {
        let Some(node) = ir.nodes.get(idx) else {
            continue;
        };
        let CanonNodeKind::Fn { name_id, sig_id, body, .. } = &node.kind else {
            continue;
        };
        if fn_sig_returns_unit(ir, *sig_id) {
            continue;
        }
        let Some(body_id) = body else {
            // Declaration-only items (e.g. trait methods without default bodies)
            // are not executable bodies and are outside return-body completeness.
            continue;
        };
        let CanonNodeKind::Body { blocks } = &ir.node(*body_id).kind else {
            bail!(
                "return_solver: non-unit function `{}` (node {}) body is not structural blocks",
                ir.lookup_name(*name_id),
                idx
            );
        };

        let mut ret_write_count = 0usize;
        let mut explicit_return_value = false;
        let mut outgoing: Vec<Vec<usize>> = vec![Vec::new(); blocks.len()];
        let mut block_returnish: Vec<bool> = vec![false; blocks.len()];

        for (bb_idx, bb_id) in blocks.iter().enumerate() {
            let CanonNodeKind::BasicBlock { ops, .. } = &ir.node(*bb_id).kind else {
                continue;
            };
            for op in ops {
                match op {
                    CfgOp::Assign { lhs, .. } => {
                        if local_is_ret(ir, *lhs) {
                            ret_write_count += 1;
                            block_returnish[bb_idx] = true;
                        }
                    }
                    CfgOp::Call { dest: Some(dest), .. }
                    | CfgOp::FieldAccess { dest: Some(dest), .. }
                    | CfgOp::MethodCall { dest: Some(dest), .. }
                    | CfgOp::StructLit { dest: Some(dest), .. } => {
                        if local_is_ret(ir, *dest) {
                            ret_write_count += 1;
                            block_returnish[bb_idx] = true;
                        }
                    }
                    CfgOp::Return(Some(_)) => {
                        explicit_return_value = true;
                        block_returnish[bb_idx] = true;
                    }
                    CfgOp::Match { dest: Some(_) } => {
                        explicit_return_value = true;
                        block_returnish[bb_idx] = true;
                    }
                    CfgOp::Unreachable => {
                        block_returnish[bb_idx] = true;
                    }
                    CfgOp::Goto(target) => {
                        let t = *target as usize;
                        if t < blocks.len() {
                            outgoing[bb_idx].push(t);
                        }
                    }
                    CfgOp::Branch { true_bb, false_bb, .. } => {
                        let t = *true_bb as usize;
                        let f = *false_bb as usize;
                        if t < blocks.len() {
                            outgoing[bb_idx].push(t);
                        }
                        if f < blocks.len() {
                            outgoing[bb_idx].push(f);
                        }
                    }
                    _ => {}
                }
            }
        }

        if !(ret_write_count == 1 || explicit_return_value) {
            bail!(
                "return_solver: non-unit function `{}` (node {}) missing structural return completeness (ret_writes={}, explicit_return_value={})",
                ir.lookup_name(*name_id),
                idx,
                ret_write_count,
                explicit_return_value
            );
        }

        let reachable = reachable_blocks(&outgoing);
        for bb_idx in reachable {
            if outgoing[bb_idx].is_empty() && !block_returnish[bb_idx] {
                bail!(
                    "return_solver: non-unit function `{}` (node {}) has terminal block {} without return-producing op",
                    ir.lookup_name(*name_id),
                    idx,
                    bb_idx
                );
            }
        }
    }
    Ok(())
}

fn fn_sig_returns_unit(ir: &CanonIR, sig_id: CanonId) -> bool {
    let CanonNodeKind::FnSig { ret, .. } = &ir.node(sig_id).kind else {
        return false;
    };
    matches!(&ir.node(*ret).kind, CanonNodeKind::Type { kind: TypeKind::Primitive(PrimTy::Unit) })
}

fn local_is_ret(ir: &CanonIR, id: CanonId) -> bool {
    match &ir.node(id).kind {
        CanonNodeKind::Local { name_id, .. } => ir.lookup_name(*name_id) == "__ret",
        _ => false,
    }
}

fn reachable_blocks(outgoing: &[Vec<usize>]) -> Vec<usize> {
    if outgoing.is_empty() {
        return Vec::new();
    }
    let mut seen = vec![false; outgoing.len()];
    let mut stack = vec![0usize];
    seen[0] = true;
    while let Some(idx) = stack.pop() {
        for &next in &outgoing[idx] {
            if !seen[next] {
                seen[next] = true;
                stack.push(next);
            }
        }
    }
    seen.into_iter().enumerate().filter_map(|(idx, ok)| ok.then_some(idx)).collect()
}
