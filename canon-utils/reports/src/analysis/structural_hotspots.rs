use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use algorithms::graph::csr::Csr;
#[cfg(feature = "cuda")]
use algorithms::graph::reachability::reachability_batched_gpu;

use crate::graph::graph_types::NodeRow;
#[cfg(feature = "cuda")]
use crate::analysis::callgraph::find_callgraph_roots_from_edges;
use crate::reports::{BranchComplexityEntry, BranchPressureEntry, StructuralHotspotEntry, MergeCandidateEntry, ReachabilityEntry, PathRedundancyEntry};

pub fn build_structural_hotspots(
    nodes: &[NodeRow],
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
    callgraph: &[(u32, u32)],
    cfg_out: &HashMap<u32, Vec<u32>>,
    cfg_in: &HashMap<u32, usize>,
    block_owner: &HashMap<u32, u32>,
    block_effect_sig: &HashMap<u32, Vec<String>>,
) -> Vec<StructuralHotspotEntry> {
    let mut callers: HashMap<u32, BTreeSet<u32>> = HashMap::new();
    for (s, d) in callgraph {
        callers.entry(*d).or_default().insert(*s);
    }

    let mut branch_entries = build_branch_complexity_with_ids(nodes, node_map, file_map, cfg_out, cfg_in, block_effect_sig);
    let mut per_fn: HashMap<u32, (usize, usize)> = HashMap::new(); // fn -> (branch_count, dup_blocks)
    for (block_id, entry) in branch_entries.drain(..) {
        if let Some(fid) = block_owner.get(&block_id) {
            let e = per_fn.entry(*fid).or_insert((0, 0));
            e.0 += entry.branch_count;
            e.1 += entry.duplicate_block_count;
        }
    }

    let mut out = Vec::new();
    for (fid, (branch_count, dup_blocks)) in per_fn {
        let node = node_map.get(&fid);
        let symbol = node.map(|n| n.symbol.clone()).unwrap_or_default();
        let file = node
            .and_then(|n| n.file_id)
            .and_then(|id| file_map.get(&id).cloned())
            .unwrap_or_default();
        let line = node.and_then(|n| n.line);
        let caller_syms: Vec<String> = callers
            .get(&fid)
            .map(|s| s.iter().filter_map(|id| node_map.get(id).map(|n| n.symbol.clone())).collect())
            .unwrap_or_else(Vec::new);
        let score = branch_count * dup_blocks.max(1) * caller_syms.len().max(1);
        out.push(StructuralHotspotEntry {
            symbol,
            file,
            line,
            branch_count,
            duplicate_blocks: dup_blocks,
            callers: caller_syms,
            score,
        });
    }
    out.sort_by(|a, b| b.score.cmp(&a.score));
    out
}

fn build_branch_complexity_with_ids(
    nodes: &[NodeRow],
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
    cfg_out: &HashMap<u32, Vec<u32>>,
    _cfg_in: &HashMap<u32, usize>,
    block_effect_sig: &HashMap<u32, Vec<String>>,
) -> Vec<(u32, BranchComplexityEntry)> {
    let _ = node_map;
    let mut sig_counts: HashMap<Vec<String>, usize> = HashMap::new();
    for sig in block_effect_sig.values() {
        *sig_counts.entry(sig.clone()).or_insert(0) += 1;
    }

    let mut out = Vec::new();
    for node in nodes {
        if node.kind != "BASIC_BLOCK" {
            continue;
        }
        let outs = cfg_out.get(&node.id).map(|v| v.len()).unwrap_or(0);
        let dup = block_effect_sig
            .get(&node.id)
            .and_then(|sig| sig_counts.get(sig).copied())
            .unwrap_or(1)
            .saturating_sub(1);
        let symbol = node.symbol.clone();
        let file = node
            .file_id
            .and_then(|id| file_map.get(&id).cloned())
            .unwrap_or_default();
        let line = node.line;
        let score = outs * dup.max(1);
        out.push((
            node.id,
            BranchComplexityEntry {
                symbol,
                file,
                line,
                branch_count: outs,
                duplicate_block_count: dup,
                score,
            },
        ));
    }
    out
}

pub fn build_branch_complexity(
    nodes: &[NodeRow],
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
    cfg_out: &HashMap<u32, Vec<u32>>,
    cfg_in: &HashMap<u32, usize>,
    block_effect_sig: &HashMap<u32, Vec<String>>,
) -> Vec<BranchComplexityEntry> {
    let mut out = Vec::new();
    for (_block_id, entry) in build_branch_complexity_with_ids(nodes, node_map, file_map, cfg_out, cfg_in, block_effect_sig) {
        out.push(entry);
    }
    out.sort_by(|a, b| b.score.cmp(&a.score));
    out
}

pub fn build_branch_pressure(
    block_owner: &HashMap<u32, u32>,
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
    cfg_out: &HashMap<u32, Vec<u32>>,
) -> Vec<BranchPressureEntry> {
    let mut per_fn: HashMap<u32, (usize, usize)> = HashMap::new();
    for (block, outs) in cfg_out {
        let Some(owner) = block_owner.get(block).copied() else {
            continue;
        };
        let branch_nodes = if outs.len() > 1 { 1 } else { 0 };
        let pressure = outs.len().saturating_sub(1);
        let entry = per_fn.entry(owner).or_insert((0, 0));
        entry.0 += branch_nodes;
        entry.1 += pressure;
    }

    per_fn
        .into_iter()
        .filter_map(|(fn_id, (branches, pressure))| {
            let node = node_map.get(&fn_id)?;
            let file = node
                .file_id
                .and_then(|id| file_map.get(&id))
                .cloned()
                .unwrap_or_default();
            Some(BranchPressureEntry {
                symbol: node.symbol.clone(),
                file,
                line: node.line,
                branch_nodes: branches,
                branch_pressure: pressure,
            })
        })
        .collect()
}

pub fn build_merge_candidates(
    cfg_out: &HashMap<u32, Vec<u32>>,
    block_owner: &HashMap<u32, u32>,
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
) -> Vec<MergeCandidateEntry> {
    let mut out: Vec<MergeCandidateEntry> = Vec::new();
    for (block, outs) in cfg_out {
        if outs.len() < 2 {
            continue;
        }
        let mut groups: HashMap<BTreeSet<u32>, Vec<u32>> = HashMap::new();
        for succ in outs {
            let succ_outs = cfg_out.get(succ).cloned().unwrap_or_default();
            let key: BTreeSet<u32> = succ_outs.into_iter().collect();
            groups.entry(key).or_default().push(*succ);
        }
        for (key, group) in groups {
            if group.len() < 2 {
                continue;
            }
            let fn_id = block_owner.get(block).copied().unwrap_or_default();
            let fn_symbol = node_map
                .get(&fn_id)
                .map(|n| n.symbol.clone())
                .unwrap_or_default();
            let _fn_file = node_map
                .get(&fn_id)
                .and_then(|n| n.file_id)
                .and_then(|id| file_map.get(&id))
                .cloned()
                .unwrap_or_default();
            out.push(MergeCandidateEntry {
                function: fn_symbol,
                branch_block: *block,
                successors: key.into_iter().collect(),
                candidate_blocks: group,
            });
        }
    }
    out
}

pub fn build_path_redundancy(
    cfg_out: &HashMap<u32, Vec<u32>>,
    block_owner: &HashMap<u32, u32>,
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
) -> Vec<PathRedundancyEntry> {
    let mut blocks_by_fn: HashMap<u32, Vec<u32>> = HashMap::new();
    for (block, fn_id) in block_owner {
        blocks_by_fn.entry(*fn_id).or_default().push(*block);
    }
    let mut entries = Vec::new();
    for (fn_id, blocks) in blocks_by_fn {
        let mut total = 0usize;
        let mut unique: HashSet<(u32, u32, u32)> = HashSet::new();
        for block in &blocks {
            if let Some(outs) = cfg_out.get(block) {
                for dst in outs {
                    let nexts = cfg_out.get(dst).cloned().unwrap_or_default();
                    if nexts.is_empty() {
                        total += 1;
                        unique.insert((*block, *dst, *dst));
                    } else {
                        for next in nexts {
                            total += 1;
                            unique.insert((*block, *dst, next));
                        }
                    }
                }
            }
        }
        let unique_count = unique.len();
        let ratio = if total == 0 { 0.0 } else { unique_count as f64 / total as f64 };
        if let Some(node) = node_map.get(&fn_id) {
            let file = node
                .file_id
                .and_then(|id| file_map.get(&id))
                .cloned()
                .unwrap_or_default();
            entries.push(PathRedundancyEntry {
                symbol: node.symbol.clone(),
                file,
                line: node.line,
                paths_total: total,
                paths_unique: unique_count,
                redundancy_ratio: ratio,
            });
        }
    }
    entries
}

fn build_reachability_report(
    cfg_out: &HashMap<u32, Vec<u32>>,
    block_owner: &HashMap<u32, u32>,
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
) -> Vec<ReachabilityEntry> {
    let mut blocks_by_fn: HashMap<u32, Vec<u32>> = HashMap::new();
    for (block, fn_id) in block_owner {
        blocks_by_fn.entry(*fn_id).or_default().push(*block);
    }
    let mut entries = Vec::new();
    for (fn_id, blocks) in blocks_by_fn {
        let mut incoming: HashMap<u32, usize> = HashMap::new();
        for block in &blocks {
            if let Some(outs) = cfg_out.get(block) {
                for dst in outs {
                    *incoming.entry(*dst).or_insert(0) += 1;
                }
            }
        }
        let mut roots: Vec<u32> = blocks
            .iter()
            .copied()
            .filter(|b| !incoming.contains_key(b))
            .collect();
        if roots.is_empty() {
            roots.extend(blocks.iter().copied().take(1));
        }
        let mut visited: HashSet<u32> = HashSet::new();
        let mut queue: VecDeque<u32> = VecDeque::new();
        for root in roots {
            visited.insert(root);
            queue.push_back(root);
        }
        while let Some(current) = queue.pop_front() {
            if let Some(outs) = cfg_out.get(&current) {
                for dst in outs {
                    if visited.insert(*dst) {
                        queue.push_back(*dst);
                    }
                }
            }
        }
        let total = blocks.len();
        let reachable = visited.len();
        let ratio = if total == 0 { 0.0 } else { reachable as f64 / total as f64 };
        if let Some(node) = node_map.get(&fn_id) {
            let file = node
                .file_id
                .and_then(|id| file_map.get(&id))
                .cloned()
                .unwrap_or_default();
            entries.push(ReachabilityEntry {
                symbol: node.symbol.clone(),
                file,
                line: node.line,
                reachable_blocks: reachable,
                total_blocks: total,
                reachable_ratio: ratio,
            });
        }
    }
    entries
}

pub fn build_reachability_report_gpu(
    cfg_out: &HashMap<u32, Vec<u32>>,
    block_owner: &HashMap<u32, u32>,
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
    _cg_csr: &Csr,
    _cg_id_to_local: &[u32],
    cg_local_to_id: &[u32],
) -> Vec<ReachabilityEntry> {
    #[cfg(feature = "cuda")]
    let reachable_callgraph = {
        let roots = find_callgraph_roots_from_edges(cg_local_to_id);
        let batches = reachability_batched_gpu(_cg_csr, &roots);
        let mut reachable = Vec::new();
        for row in batches {
            for (idx, ok) in row.iter().enumerate() {
                if *ok {
                    reachable.push(idx as u32);
                }
            }
        }
        reachable
    };

    #[cfg(not(feature = "cuda"))]
    let reachable_callgraph: Vec<u32> = Vec::new();

    let reachable_callgraph_ids: HashSet<u32> = reachable_callgraph
        .into_iter()
        .filter_map(|local| cg_local_to_id.get(local as usize).copied())
        .collect();

    let base = build_reachability_report(cfg_out, block_owner, node_map, file_map);
    if reachable_callgraph_ids.is_empty() {
        return base;
    }

    base.into_iter()
        .filter(|entry| {
            node_map
                .iter()
                .find(|(_, n)| n.symbol == entry.symbol)
                .map(|(id, _)| !reachable_callgraph_ids.contains(id))
                .unwrap_or(true)
        })
        .collect()
}
