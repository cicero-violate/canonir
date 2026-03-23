use rustc_middle::mir;
use std::collections::{BTreeSet, HashMap};

use crate::capture::pipeline::mir::resolver::LocalNameResolver;
use crate::capture::types::Stmt;
use algorithms::control_flow::cfg_pattern::{compute_back_edges, detect_iterator_loops};
use std::collections::HashSet;

pub(crate) struct SwitchAnalysis {
    pub(crate) switch_sources: BTreeSet<usize>,
    pub(crate) switchint_arm_blocks: BTreeSet<usize>,
    pub(crate) iterator_switches: HashMap<usize, usize>,
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

    let _ = (succs, switch_reachable);

    SwitchAnalysis { switch_sources, switchint_arm_blocks, iterator_switches }
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

pub(crate) fn collect_suppressed_dest_sentinels(
    body: &mir::Body<'_>, resolver: &LocalNameResolver, switch_analysis: &SwitchAnalysis, defined: &mut HashSet<String>, suppressed_sentinel_names: &mut HashSet<String>,
) -> Vec<Stmt> {
    // Suppressed destination sentinels are forbidden by invariant.
    // Do not collect or inject any synthetic sentinel statements.
    let _ = (body, resolver, switch_analysis, defined, suppressed_sentinel_names);
    Vec::new()
}
