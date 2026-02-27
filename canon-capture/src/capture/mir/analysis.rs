use rustc_middle::mir;
use std::collections::{BTreeSet, HashMap};

use crate::capture::mir::util as mir_util;

pub(crate) struct SwitchAnalysis {
    pub(crate) switch_sources: BTreeSet<usize>,
    pub(crate) switchint_arm_blocks: BTreeSet<usize>,
    pub(crate) switch_source_writes_ret: HashMap<usize, bool>,
}

pub(crate) fn analyze_switch_structure(body: &mir::Body<'_>) -> SwitchAnalysis {
    let mut switch_sources: BTreeSet<usize> = BTreeSet::new();
    let mut direct_switch_succ: BTreeSet<usize> = BTreeSet::new();
    let mut preds: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); body.basic_blocks.len()];
    let mut succs: Vec<Vec<usize>> = vec![Vec::new(); body.basic_blocks.len()];
    for (idx, bb) in body.basic_blocks.iter_enumerated() {
        let Some(term) = &bb.terminator else {
            continue;
        };
        for succ in term.successors() {
            succs[idx.as_usize()].push(succ.as_usize());
            preds[succ.as_usize()].insert(idx.as_usize());
        }
        if matches!(term.kind, mir::TerminatorKind::SwitchInt { .. }) {
            switch_sources.insert(idx.as_usize());
            for succ in term.successors() {
                direct_switch_succ.insert(succ.as_usize());
            }
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
            let exclusively_switch_reachable = incoming.iter().all(|p| {
                switch_sources.contains(p)
                    || switchint_arm_blocks.contains(p)
                    || direct_switch_succ.contains(p)
            });
            if exclusively_switch_reachable {
                switchint_arm_blocks.insert(bb_idx);
                changed = true;
            }
        }
    }

    let bb_writes_ret: Vec<bool> = body
        .basic_blocks
        .iter()
        .map(mir_util::bb_writes_return_place)
        .collect();
    let mut switch_source_writes_ret: HashMap<usize, bool> = HashMap::new();
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

    SwitchAnalysis {
        switch_sources,
        switchint_arm_blocks,
        switch_source_writes_ret,
    }
}
