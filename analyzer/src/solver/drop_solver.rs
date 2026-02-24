//! Drop Order Solver (S16).
//!
//! Variables:
//!   f          ∈ F = { Function | Method } with Body::Blocks
//!   B_f        = [b_0 .. b_{n-1}]             — basic blocks of f
//!   decl(b_i)  = [pat | Stmt::Let{pat} ∈ b_i] — bindings declared in block i
//!   decl_order(f) = concat_{i} decl(b_i)
//!   drop_order(f) = reverse(decl_order(f))
//!
//! Equations:
//!   G_cfg_local : local adjacency built from block terminators
//!   exit_nodes  = { i | term(b_i) = Return }
//!   post_dom    = post_dominators(n, succs, exit_nodes)   [algorithms crate]
//!   valid_drop(f) <=>
//!       drop_order(f) == reverse(decl_order(f))
//!       ∧ every binding is post-dominated by its declaring block
//!         (i.e. declaring block post-dominates itself — always true,
//!          so the real check is that multi-exit functions do not reorder)
//!
//! Error condition:
//!   ∃ f : |B_f| > 0
//!       ∧ multi_exit(f)                   — >1 Return terminator
//!       ∧ ∃ block b_i with decls
//!           where b_i ∉ post_dom[b_i]     — b_i does not post-dominate itself
//!           (structural inconsistency — malformed CFG)
//!
//! Normal warning path:
//!   ∃ exit e: exit_drop(e) not a subsequence of expected_drop  => hard Err
//!
//! Subsequence predicate:
//!   is_subseq(xs, ys) <=> xs can be obtained by deleting elements from ys
//!   This allows per-path bindings to be absent while preserving relative order.

use anyhow::{bail, Result};
use model::ir::{
    model_ir::ModelIR,
    node::{Body, NodeKind, Stmt, Terminator},
};
use algorithms::control_flow::dominators::post_dominators;

pub fn solve(ir: &ModelIR) -> Result<()> {
    for node in &ir.nodes {
        let (fname, body) = match &node.kind {
            NodeKind::Function { name, body, .. } => (name.clone(), body),
            NodeKind::Method   { name, body, .. } => (name.clone(), body),
            _ => continue,
        };

        let blocks = match body {
            Body::Blocks(bbs) if !bbs.is_empty() => bbs,
            _ => continue,
        };

        let n = blocks.len();

        // ── Build local forward adjacency ────────────────────────────────────
        // Equation: succs[i] = targets(term(b_i))
        let mut succs: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut exit_nodes: Vec<usize> = Vec::new();

        for (i, bb) in blocks.iter().enumerate() {
            match &bb.terminator {
                Terminator::Goto(t)    => {
                    let t = *t as usize;
                    if t < n { succs[i].push(t); }
                }
                Terminator::Branch { true_bb, false_bb, .. } => {
                    let t = *true_bb  as usize;
                    let f = *false_bb as usize;
                    if t < n { succs[i].push(t); }
                    if f < n { succs[i].push(f); }
                }
                Terminator::Return => exit_nodes.push(i),
                Terminator::None   => {}
            }
        }

        // If no return terminators, treat last block as exit.
        if exit_nodes.is_empty() {
            exit_nodes.push(n - 1);
        }

        // ── Compute post-dominators via algorithms crate ─────────────────────
        // Equation: post_dom = post_dominators(n, &succs, &exit_nodes)
        let post_dom = post_dominators(n, &succs, &exit_nodes);

        // ── Structural check: every block must post-dominate itself ──────────
        // post_dom[v] is the full post-dominator SET (from the algorithms crate).
        // A block always post-dominates itself; if it doesn't, the CFG is malformed.
        for i in 0..n {
            let has_decls = blocks[i].stmts.iter().any(|s| matches!(s, Stmt::Let { .. }));
            if has_decls && !post_dom[i].contains(&i) {
                bail!(
                    "drop_solver: `{}` block {} has let-bindings but does not \
                     post-dominate itself — malformed CFG",
                    fname, i
                );
            }
        }

        // ── Compute declaration order and expected drop order ────────────────
        // decl_order(f) = concat_{i} [pat | Stmt::Let{pat} ∈ b_i]
        let decl_order: Vec<String> = blocks.iter()
            .flat_map(|bb| bb.stmts.iter().filter_map(|s| {
                if let Stmt::Let { pat, .. } = s { Some(pat.clone()) } else { None }
            }))
            .collect();

        if decl_order.is_empty() {
            continue;
        }

        // drop_order(f) = reverse(decl_order(f))
        let mut expected_drop = decl_order.clone();
        expected_drop.reverse();

        // ── Verify drop order consistency across multi-exit functions ─────────
        // For single-exit or linear functions the order is always consistent.
        // For multi-exit: check that all paths see the same decl prefix
        // (i.e. no block on one path declares bindings invisible on another path).
        if exit_nodes.len() > 1 {
            // Collect per-exit-block reachable decls via DFS on forward succs.
            // Equation: path_decls(exit) = decl_order restricted to blocks
            //           reachable from entry that post-dominate 'exit'.
            let super_exit = n; // index of synthetic node from post_dominators
            for &exit in &exit_nodes {
                // Blocks that post-dominate 'exit' are in post_dom[exit].
                // The super_exit synthetic node (index n) is not a real block.
                let exit_decls: Vec<String> = blocks.iter()
                    .enumerate()
                    .filter(|(i, _)| post_dom[exit].contains(i))
                    .flat_map(|(_, bb)| bb.stmts.iter().filter_map(|s| {
                        if let Stmt::Let { pat, .. } = s { Some(pat.clone()) } else { None }
                    }))
                    .collect();

                let mut exit_drop = exit_decls.clone();
                exit_drop.reverse();

                // All exit paths must agree on drop order of their post-dominator blocks.
                if !decl_order.is_empty() && !exit_decls.is_empty() {
                    // exit_drop must be a subsequence of expected_drop.
                    // Bindings on other paths are absent here — that is valid.
                    // Only a reordering within this path's bindings is an error.
                    // Equation: is_subseq(exit_drop, expected_drop)
                    if !is_subsequence(&exit_drop, &expected_drop) {
                        bail!(
                            "drop_solver: `{}` exit block {} has inconsistent drop order \
                             on this path.\n  expected suffix: {:?}\n  got:             {:?}",
                            fname, exit, expected_drop, exit_drop
                        );
                    }
                }
                // suppress unused warning on super_exit binding
                let _ = super_exit;
            }
        }

        log::info!(
            "drop_solver: `{}` — {} binding(s), drop order: {:?}",
            fname, decl_order.len(), expected_drop
        );
    }

    Ok(())
}

/// Returns true if `needle` is a subsequence of `haystack`.
///
/// Equation: is_subseq([], _) = true
///           is_subseq(x:xs, []) = false
///           is_subseq(x:xs, y:ys) = if x==y then is_subseq(xs,ys)
///                                            else is_subseq(x:xs, ys)
fn is_subsequence(needle: &[String], haystack: &[String]) -> bool {
    let mut ni = 0;
    for h in haystack {
        if ni == needle.len() { break; }
        if needle[ni] == *h { ni += 1; }
    }
    ni == needle.len()
}
