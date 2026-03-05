use rustc_middle::mir;
use rustc_middle::ty::TyCtxt;
use std::collections::{BTreeSet, HashMap};

use crate::capture::mir::guard as mir_guard;
use crate::capture::mir::ops as mir_ops;
use crate::capture::mir::resolver::LocalNameResolver;
use crate::capture::mir::util as mir_util;
use crate::types::Stmt;
use algorithms::control_flow::cfg_pattern::{compute_back_edges, detect_iterator_loops};
use std::collections::HashSet;

pub(crate) struct SwitchAnalysis {
    pub(crate) switch_sources: BTreeSet<usize>,
    pub(crate) switchint_arm_blocks: BTreeSet<usize>,
    pub(crate) switch_arm_writes_ret: BTreeSet<usize>,
    pub(crate) switch_arm_returns: BTreeSet<usize>,
    pub(crate) switch_source_writes_ret: HashMap<usize, bool>,
    pub(crate) iterator_switches: HashMap<usize, usize>,
    pub(crate) iterator_body_blocks: HashSet<usize>,
}

pub(crate) fn analyze_switch_structure(body: &mir::Body<'_>) -> SwitchAnalysis {
    let mut all_switch_sources: BTreeSet<usize> = BTreeSet::new();
    let mut switch_succs_by_source: HashMap<usize, BTreeSet<usize>> = HashMap::new();
    let mut preds: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); body.basic_blocks.len()];
    let mut succs: Vec<Vec<usize>> = vec![Vec::new(); body.basic_blocks.len()];
    for (idx, bb) in body.basic_blocks.iter_enumerated() {
        if bb.is_cleanup {
            continue;
        }
        let Some(term) = &bb.terminator else {
            continue;
        };
        for succ in term.successors() {
            if body.basic_blocks[succ].is_cleanup {
                continue;
            }
            succs[idx.as_usize()].push(succ.as_usize());
            preds[succ.as_usize()].insert(idx.as_usize());
        }
        if matches!(term.kind, mir::TerminatorKind::SwitchInt { .. }) {
            let src = idx.as_usize();
            all_switch_sources.insert(src);
            let succ_set = switch_succs_by_source.entry(src).or_default();
            for succ in term.successors() {
                if body.basic_blocks[succ].is_cleanup {
                    continue;
                }
                succ_set.insert(succ.as_usize());
            }
        }
    }

    let back_edges = compute_back_edges(&succs);
    let mut iterator_switches: HashMap<usize, usize> = HashMap::new();
    let mut iterator_body_blocks: HashSet<usize> = HashSet::new();
    for pattern in detect_iterator_loops(&succs, &back_edges) {
        iterator_switches.insert(pattern.switch_block, pattern.body_entry);
        iterator_body_blocks.extend(pattern.body_blocks.into_iter());
    }

    let mut switch_sources: BTreeSet<usize> = BTreeSet::new();
    for src in &all_switch_sources {
        let Some(succs0) = switch_succs_by_source.get(src) else {
            continue;
        };
        let mut region: BTreeSet<usize> = BTreeSet::new();
        let mut stack: Vec<usize> = succs0.iter().copied().collect();
        while let Some(cur) = stack.pop() {
            if !region.insert(cur) {
                continue;
            }
            for next in &succs[cur] {
                if !region.contains(next) {
                    stack.push(*next);
                }
            }
        }
        if region_has_cycle(&region, &succs) {
            switch_sources.insert(*src);
        }
    }

    let mut direct_switch_succ: BTreeSet<usize> = BTreeSet::new();
    for src in &switch_sources {
        if let Some(succ_set) = switch_succs_by_source.get(src) {
            direct_switch_succ.extend(succ_set.iter().copied());
        }
    }

    let mut switch_reachable: BTreeSet<usize> = direct_switch_succ.clone();
    let mut frontier: Vec<usize> = direct_switch_succ.iter().copied().collect();
    while let Some(cur) = frontier.pop() {
        for sidx in &succs[cur] {
            let sidx = *sidx;
            if switch_reachable.insert(sidx) {
                frontier.push(sidx);
            }
        }
    }

    let mut switchint_arm_blocks: BTreeSet<usize> = BTreeSet::new();
    let mut changed = true;
    while changed {
        changed = false;
        for bb_idx in 0..body.basic_blocks.len() {
            if switchint_arm_blocks.contains(&bb_idx) || !switch_reachable.contains(&bb_idx) {
                continue;
            }
            let incoming = &preds[bb_idx];
            if incoming.is_empty() {
                continue;
            }
            let exclusively_switch_reachable = incoming.iter().all(|p| switch_sources.contains(p) || switchint_arm_blocks.contains(p) || direct_switch_succ.contains(p));
            if exclusively_switch_reachable {
                switchint_arm_blocks.insert(bb_idx);
                changed = true;
            }
        }
    }
    switchint_arm_blocks.retain(|bb| !iterator_body_blocks.contains(bb));

    let bb_writes_ret: Vec<bool> = body.basic_blocks.iter().map(mir_util::bb_writes_return_place).collect();
    let mut switch_source_writes_ret: HashMap<usize, bool> = HashMap::new();
    let mut switch_arm_writes_ret: BTreeSet<usize> = BTreeSet::new();
    let mut switch_arm_returns: BTreeSet<usize> = BTreeSet::new();
    for arm in &switchint_arm_blocks {
        let arm_bb = mir::BasicBlock::from_usize(*arm);
        if bb_writes_ret.get(*arm).copied().unwrap_or(false) {
            switch_arm_writes_ret.insert(*arm);
        }
        if matches!(body.basic_blocks[arm_bb].terminator.as_ref().map(|t| &t.kind), Some(mir::TerminatorKind::Return)) {
            switch_arm_returns.insert(*arm);
        }
    }
    for src in &switch_sources {
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        let mut stack: Vec<usize> = succs[*src].clone();
        let mut writes_ret = false;
        while let Some(cur) = stack.pop() {
            if !switch_reachable.contains(&cur) || !seen.insert(cur) {
                continue;
            }
            if bb_writes_ret.get(cur).copied().unwrap_or(false) {
                writes_ret = true;
                break;
            }
            for next in &succs[cur] {
                if switch_reachable.contains(next) {
                    stack.push(*next);
                }
            }
        }
        switch_source_writes_ret.insert(*src, writes_ret);
    }

    SwitchAnalysis { switch_sources, switchint_arm_blocks, switch_arm_writes_ret, switch_arm_returns, switch_source_writes_ret, iterator_switches, iterator_body_blocks }
}

fn region_has_cycle(region: &BTreeSet<usize>, succs: &[Vec<usize>]) -> bool {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Color {
        White,
        Gray,
        Black,
    }

    fn dfs(cur: usize, region: &BTreeSet<usize>, succs: &[Vec<usize>], colors: &mut [Color]) -> bool {
        colors[cur] = Color::Gray;
        for &next in &succs[cur] {
            if !region.contains(&next) {
                continue;
            }
            match colors[next] {
                Color::Gray => return true,
                Color::White => {
                    if dfs(next, region, succs, colors) {
                        return true;
                    }
                }
                Color::Black => {}
            }
        }
        colors[cur] = Color::Black;
        false
    }

    let mut colors = vec![Color::White; succs.len()];
    for &node in region {
        if colors[node] == Color::White && dfs(node, region, succs, &mut colors) {
            return true;
        }
    }
    false
}

pub(crate) fn compute_call_feed_locals<'tcx>(tcx: TyCtxt<'tcx>, body: &mir::Body<'tcx>, resolver: &LocalNameResolver) -> HashSet<String> {
    let local_use_counts = mir_util::count_local_uses(body);
    let mut filtered_arg_locals: HashSet<u32> = HashSet::new();
    for bb in body.basic_blocks.iter() {
        let Some(term_ref) = &bb.terminator else {
            continue;
        };
        let mir::TerminatorKind::Call { func, args, .. } = &term_ref.kind else {
            continue;
        };
        if !mir_ops::filtered_internal_call_target(tcx, func, resolver) {
            continue;
        }
        for arg in args {
            if let mir::Operand::Copy(place) | mir::Operand::Move(place) = &arg.node {
                filtered_arg_locals.insert(place.local.as_u32());
            }
        }
    }
    let mut call_feed_locals: HashSet<String> = HashSet::new();
    for local_u32 in filtered_arg_locals {
        if local_use_counts.get(&local_u32).copied().unwrap_or(0) != 1 {
            continue;
        }
        let local = mir::Local::from_u32(local_u32);
        if let Some(name) = resolver.label_local(local) {
            call_feed_locals.insert(name);
        }
    }
    call_feed_locals
}

pub(crate) fn collect_suppressed_dest_sentinels(
    body: &mir::Body<'_>, resolver: &LocalNameResolver, switch_analysis: &SwitchAnalysis, defined: &mut HashSet<String>, suppressed_sentinel_names: &mut HashSet<String>,
) -> Vec<Stmt> {
    // Suppressed destination sentinels are forbidden by invariant.
    // Do not collect or inject any synthetic sentinel statements.
    let _ = (body, resolver, switch_analysis, defined, suppressed_sentinel_names);
    Vec::new()
}
