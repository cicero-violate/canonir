use std::collections::{HashMap, HashSet, VecDeque};

use algorithms::graph::csr::Csr;
#[cfg(feature = "cuda")]
use algorithms::graph::reachability::reachability_gpu;

use canon_graph::graph::graph_types::{CodeGraphEdge, CodeGraphNode};
use crate::analysis::callgraph::find_callgraph_roots;
use crate::DeadCodeEntry;

pub fn detect_dead_code(
    nodes: &[CodeGraphNode],
    node_map: &HashMap<u32, CodeGraphNode>,
    file_map: &HashMap<u32, String>,
    edges: &[CodeGraphEdge],
    cfg_out: &HashMap<u32, Vec<u32>>,
    cfg_in: &HashMap<u32, usize>,
    callgraph: &[(u32, u32)],
    block_owner: &HashMap<u32, u32>,
) -> Vec<DeadCodeEntry> {
    let mut fn_nodes: HashSet<u32> = HashSet::new();
    let mut blocks: HashSet<u32> = HashSet::new();
    for n in nodes {
        if n.kind == "FUNCTION" || n.kind == "METHOD" {
            fn_nodes.insert(n.id);
        } else if n.kind == "BASIC_BLOCK" {
            blocks.insert(n.id);
        }
    }

    let mut call_adj: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut call_in: HashMap<u32, usize> = HashMap::new();
    for (s, d) in callgraph {
        if fn_nodes.contains(s) && fn_nodes.contains(d) {
            call_adj.entry(*s).or_default().push(*d);
            *call_in.entry(*d).or_insert(0) += 1;
        }
    }

    let mut entrypoints: Vec<u32> = fn_nodes.iter().copied().filter(|f| *call_in.get(f).unwrap_or(&0) == 0).collect();
    for f in &fn_nodes {
        if let Some(sym) = node_map.get(f).map(|n| n.symbol.as_str()) {
            if sym.ends_with("::main::fn") || sym == "main::fn" {
                entrypoints.push(*f);
            }
        }
    }

    let mut reachable_fns = HashSet::new();
    let mut stack: Vec<u32> = entrypoints;
    while let Some(f) = stack.pop() {
        if !reachable_fns.insert(f) {
            continue;
        }
        if let Some(next) = call_adj.get(&f) {
            for n in next {
                if !reachable_fns.contains(n) {
                    stack.push(*n);
                }
            }
        }
    }

    let mut fn_to_blocks: HashMap<u32, Vec<u32>> = HashMap::new();
    for e in edges {
        if e.kind != "HAS_BLOCK" {
            continue;
        }
        if node_map.get(&e.src).map(|n| n.kind.as_str()) == Some("FUNCTION")
            || node_map.get(&e.src).map(|n| n.kind.as_str()) == Some("METHOD")
        {
            if node_map.get(&e.dst).map(|n| n.kind.as_str()) == Some("BASIC_BLOCK") {
                fn_to_blocks.entry(e.src).or_default().push(e.dst);
            }
        }
    }

    let mut reachable_blocks: HashSet<u32> = HashSet::new();
    for f in &reachable_fns {
        let blocks = fn_to_blocks.get(f).cloned().unwrap_or_default();
        if blocks.is_empty() {
            continue;
        }
        let entries: Vec<u32> = blocks
            .iter()
            .copied()
            .filter(|b| cfg_in.get(b).copied().unwrap_or(0) == 0)
            .collect();
        let mut queue: VecDeque<u32> = if entries.is_empty() { VecDeque::from(vec![blocks[0]]) } else { VecDeque::from(entries) };
        let mut seen: HashSet<u32> = HashSet::new();
        while let Some(b) = queue.pop_front() {
            if !seen.insert(b) {
                continue;
            }
            reachable_blocks.insert(b);
            if let Some(outs) = cfg_out.get(&b) {
                for dst in outs {
                    if block_owner.get(dst).copied() == Some(*f) {
                        queue.push_back(*dst);
                    }
                }
            }
        }
    }

    let mut out = Vec::new();
    for f in fn_nodes {
        if !reachable_fns.contains(&f) {
            let node = node_map.get(&f);
            let symbol = node.map(|n| n.symbol.clone()).unwrap_or_default();
            let file = node
                .and_then(|n| n.file_id)
                .and_then(|id| file_map.get(&id).cloned())
                .unwrap_or_default();
            let line = node.and_then(|n| n.line);
            out.push(DeadCodeEntry { symbol, file, line, reason: "unreachable function".to_string() });
        }
    }
    for b in blocks {
        if !reachable_blocks.contains(&b) {
            let node = node_map.get(&b);
            let symbol = node.map(|n| n.symbol.clone()).unwrap_or_default();
            let file = node
                .and_then(|n| n.file_id)
                .and_then(|id| file_map.get(&id).cloned())
                .unwrap_or_default();
            let line = node.and_then(|n| n.line);
            out.push(DeadCodeEntry { symbol, file, line, reason: "unreachable basic block".to_string() });
        }
    }
    out
}

pub fn detect_dead_code_gpu(
    nodes: &[CodeGraphNode],
    node_map: &HashMap<u32, CodeGraphNode>,
    file_map: &HashMap<u32, String>,
    edges: &[CodeGraphEdge],
    cfg_out: &HashMap<u32, Vec<u32>>,
    cfg_in: &HashMap<u32, usize>,
    callgraph: &[(u32, u32)],
    block_owner: &HashMap<u32, u32>,
    _cg_csr: &Csr,
    _cg_id_to_local: &[u32],
    cg_local_to_id: &[u32],
) -> Vec<DeadCodeEntry> {
    #[cfg(feature = "cuda")]
    let reachable_callgraph = {
        let roots = find_callgraph_roots(callgraph);
        let roots_local: Vec<usize> = roots
            .iter()
            .filter_map(|id| cg_local_to_id.iter().position(|x| x == id))
            .collect();
        let visited = reachability_gpu(_cg_csr, &roots_local);
        visited
            .into_iter()
            .enumerate()
            .filter_map(|(idx, ok)| if ok { Some(idx as u32) } else { None })
            .collect::<Vec<u32>>()
    };

    #[cfg(not(feature = "cuda"))]
    let reachable_callgraph = {
        let roots = find_callgraph_roots(callgraph);
        let adj = build_callgraph_adj(callgraph);
        let mut reachable = HashSet::new();
        for root in roots {
            dfs_callgraph(&adj, root, &mut reachable);
        }
        reachable
            .into_iter()
            .filter_map(|id| cg_local_to_id.iter().position(|x| x == &id).map(|v| v as u32))
            .collect::<Vec<u32>>()
    };

    let reachable_callgraph_ids: HashSet<u32> = reachable_callgraph
        .into_iter()
        .filter_map(|local| cg_local_to_id.get(local as usize).copied())
        .collect();

    detect_dead_code(
        nodes,
        node_map,
        file_map,
        edges,
        cfg_out,
        cfg_in,
        callgraph,
        block_owner,
    )
    .into_iter()
    .filter(|entry| {
        node_map
            .iter()
            .find(|(_, n)| n.symbol == entry.symbol)
            .map(|(id, _)| !reachable_callgraph_ids.contains(id))
            .unwrap_or(true)
    })
    .collect()
}
