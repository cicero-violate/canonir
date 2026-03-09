use anyhow::{anyhow, Result};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
struct NodeRow {
    id: u32,
    kind: String,
    symbol: String,
    file_id: Option<u32>,
    line: Option<u32>,
}

#[derive(Debug, Clone)]
struct EdgeRow {
    src: u32,
    dst: u32,
    kind: String,
}

#[derive(Serialize)]
struct BranchComplexityEntry {
    symbol: String,
    file: String,
    line: Option<u32>,
    branch_count: usize,
    duplicate_block_count: usize,
    score: usize,
}

#[derive(Serialize)]
struct CallgraphCentralityEntry {
    symbol: String,
    file: String,
    caller_count: usize,
    callee_count: usize,
    centrality_score: usize,
}

#[derive(Serialize)]
struct DeadCodeEntry {
    symbol: String,
    file: String,
    line: Option<u32>,
    reason: String,
}

#[derive(Serialize)]
struct DependencyCycleEntry {
    cycle_id: usize,
    nodes: Vec<String>,
    files: Vec<String>,
    cycle_length: usize,
}

#[derive(Serialize)]
struct StructuralHotspotEntry {
    symbol: String,
    file: String,
    line: Option<u32>,
    branch_count: usize,
    duplicate_blocks: usize,
    callers: Vec<String>,
    score: usize,
}

#[derive(Serialize)]
struct DataflowFanoutEntry {
    symbol: String,
    file: String,
    line: Option<u32>,
    outgoing_edges: usize,
    mutation_edges: usize,
    io_edges: usize,
}

pub fn generate_reports(output_dir: &Path, out_dir: &Path) -> Result<()> {
    let nodes = read_nodes_csv(output_dir.join("nodes.csv"))?;
    let edges = read_edges_csv(output_dir.join("edges.csv"))?;
    let cfg = read_cfg_csv(output_dir.join("cfg.csv"))?;
    let callgraph = read_callgraph_csv(output_dir.join("callgraph.csv"))?;
    let files = read_files_txt(output_dir.join("files.txt"))?;
    let _symbols_json = fs::read_to_string(output_dir.join("symbols.json"))
        .map_err(|e| anyhow!("failed to read symbols.json: {e}"))?;

    let reports_dir = out_dir.join("reports");
    fs::create_dir_all(&reports_dir)?;

    let node_map: HashMap<u32, NodeRow> = nodes.iter().map(|n| (n.id, n.clone())).collect();

    let mut file_map: HashMap<u32, String> = HashMap::new();
    for (idx, path) in files.iter().enumerate() {
        file_map.insert(idx as u32, path.clone());
    }

    let cfg_out = build_cfg_out(&cfg);
    let cfg_in = build_cfg_in(&cfg);

    let block_owner = build_block_owner(&nodes, &edges);
    let block_effect_sig = build_block_effect_signatures(&edges, &node_map);

    let branch_report = build_branch_complexity(
        &nodes,
        &node_map,
        &file_map,
        &cfg_out,
        &cfg_in,
        &block_effect_sig,
    );
    write_report(&reports_dir.join("branch_complexity_report.json"), &branch_report)?;

    let callgraph_report = build_callgraph_centrality(&callgraph, &node_map, &file_map);
    write_report(&reports_dir.join("callgraph_centrality_report.json"), &callgraph_report)?;

    let dead_report = build_dead_code(
        &nodes,
        &node_map,
        &file_map,
        &edges,
        &cfg_out,
        &cfg_in,
        &callgraph,
        &block_owner,
    );
    write_report(&reports_dir.join("dead_code_report.json"), &dead_report)?;

    let cycle_report = build_dependency_cycles(&callgraph, &node_map, &file_map);
    write_report(&reports_dir.join("dependency_cycle_report.json"), &cycle_report)?;

    let structural_report = build_structural_hotspots(
        &nodes,
        &node_map,
        &file_map,
        &callgraph,
        &cfg_out,
        &cfg_in,
        &block_owner,
        &block_effect_sig,
    );
    write_report(&reports_dir.join("structural_hotspots_report.json"), &structural_report)?;

    let dataflow_report = build_dataflow_fanout(
        &nodes,
        &node_map,
        &file_map,
        &edges,
        &block_owner,
    );
    write_report(&reports_dir.join("dataflow_fanout_report.json"), &dataflow_report)?;

    Ok(())
}

fn write_report<T: Serialize>(path: &Path, data: &T) -> Result<()> {
    let file = fs::File::create(path)?;
    serde_json::to_writer_pretty(file, data)?;
    Ok(())
}

fn read_nodes_csv(path: PathBuf) -> Result<Vec<NodeRow>> {
    let content = fs::read_to_string(path)?;
    let mut out = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(7, ',').collect();
        if parts.len() < 7 {
            continue;
        }
        let id: u32 = parts[0].parse().unwrap_or(0);
        let kind = parts[1].to_string();
        let symbol = parts[2].to_string();
        let file_id = parts[3].parse::<u32>().ok();
        let line = parts[4].parse::<u32>().ok();
        out.push(NodeRow { id, kind, symbol, file_id, line });
    }
    Ok(out)
}

fn read_edges_csv(path: PathBuf) -> Result<Vec<EdgeRow>> {
    let content = fs::read_to_string(path)?;
    let mut out = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(3, ',').collect();
        if parts.len() < 3 {
            continue;
        }
        let src: u32 = parts[0].parse().unwrap_or(0);
        let dst: u32 = parts[1].parse().unwrap_or(0);
        let kind = parts[2].to_string();
        out.push(EdgeRow { src, dst, kind });
    }
    Ok(out)
}

fn read_cfg_csv(path: PathBuf) -> Result<Vec<EdgeRow>> {
    let content = fs::read_to_string(path)?;
    let mut out = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(3, ',').collect();
        if parts.len() < 3 {
            continue;
        }
        let src: u32 = parts[0].parse().unwrap_or(0);
        let dst: u32 = parts[1].parse().unwrap_or(0);
        let kind = parts[2].to_string();
        out.push(EdgeRow { src, dst, kind });
    }
    Ok(out)
}

fn read_callgraph_csv(path: PathBuf) -> Result<Vec<(u32, u32)>> {
    let content = fs::read_to_string(path)?;
    let mut out = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(2, ',').collect();
        if parts.len() < 2 {
            continue;
        }
        let src: u32 = parts[0].parse().unwrap_or(0);
        let dst: u32 = parts[1].parse().unwrap_or(0);
        out.push((src, dst));
    }
    Ok(out)
}

fn read_files_txt(path: PathBuf) -> Result<Vec<String>> {
    let content = fs::read_to_string(path)?;
    let mut files = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 2 {
            continue;
        }
        let id = parts[0].parse::<usize>().unwrap_or(usize::MAX);
        if id == usize::MAX {
            continue;
        }
        let path = parts[1..].join(",");
        if files.len() <= id {
            files.resize(id + 1, String::new());
        }
        files[id] = path;
    }
    Ok(files)
}

fn build_cfg_out(cfg: &[EdgeRow]) -> HashMap<u32, Vec<u32>> {
    let mut out = HashMap::new();
    for e in cfg {
        out.entry(e.src).or_insert_with(Vec::new).push(e.dst);
    }
    out
}

fn build_cfg_in(cfg: &[EdgeRow]) -> HashMap<u32, usize> {
    let mut inn = HashMap::new();
    for e in cfg {
        *inn.entry(e.dst).or_insert(0) += 1;
    }
    inn
}

fn build_block_owner(nodes: &[NodeRow], edges: &[EdgeRow]) -> HashMap<u32, u32> {
    let node_kind: HashMap<u32, &str> = nodes.iter().map(|n| (n.id, n.kind.as_str())).collect();
    let mut out = HashMap::new();
    for e in edges {
        if e.kind != "HAS_BLOCK" {
            continue;
        }
        let sk = node_kind.get(&e.src).copied().unwrap_or("");
        let dk = node_kind.get(&e.dst).copied().unwrap_or("");
        if (sk == "FUNCTION" || sk == "METHOD") && dk == "BASIC_BLOCK" {
            out.insert(e.dst, e.src);
        }
    }
    out
}

fn build_block_effect_signatures(edges: &[EdgeRow], node_map: &HashMap<u32, NodeRow>) -> HashMap<u32, Vec<String>> {
    let mut effects: HashMap<u32, Vec<String>> = HashMap::new();
    let ignore = ["FLOW", "UNWIND", "HAS_BLOCK"];
    for e in edges {
        if ignore.contains(&e.kind.as_str()) {
            continue;
        }
        if node_map.get(&e.src).map(|n| n.kind.as_str()) == Some("BASIC_BLOCK") {
            effects.entry(e.src).or_default().push(e.kind.clone());
        }
        if node_map.get(&e.dst).map(|n| n.kind.as_str()) == Some("BASIC_BLOCK") {
            effects.entry(e.dst).or_default().push(e.kind.clone());
        }
    }
    for v in effects.values_mut() {
        v.sort();
    }
    effects
}

fn trace_path(start: u32, cfg_out: &HashMap<u32, Vec<u32>>, cfg_in: &HashMap<u32, usize>) -> Vec<u32> {
    let mut path = vec![start];
    let mut current = start;
    let mut depth = 0usize;
    while depth < 50 {
        let outs = cfg_out.get(&current).map(|v| v.as_slice()).unwrap_or(&[]);
        if outs.len() != 1 {
            break;
        }
        let next = outs[0];
        if path.contains(&next) {
            break;
        }
        path.push(next);
        if *cfg_in.get(&next).unwrap_or(&0) > 1 {
            break;
        }
        current = next;
        depth += 1;
    }
    path
}

fn build_branch_complexity(
    nodes: &[NodeRow],
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
    cfg_out: &HashMap<u32, Vec<u32>>,
    cfg_in: &HashMap<u32, usize>,
    block_effect_sig: &HashMap<u32, Vec<String>>,
) -> Vec<BranchComplexityEntry> {
    let mut out = Vec::new();
    for node in nodes {
        if node.kind != "BASIC_BLOCK" {
            continue;
        }
        let outs = cfg_out.get(&node.id).map(|v| v.len()).unwrap_or(0);
        if outs < 2 {
            continue;
        }
        let branch_paths: Vec<Vec<u32>> = cfg_out
            .get(&node.id)
            .unwrap_or(&Vec::new())
            .iter()
            .map(|dst| trace_path(*dst, cfg_out, cfg_in))
            .collect();
        let mut dup_blocks = 0usize;
        let mut seq_counts: BTreeMap<Vec<u32>, usize> = BTreeMap::new();
        for p in &branch_paths {
            *seq_counts.entry(p.clone()).or_insert(0) += 1;
        }
        for (seq, count) in seq_counts.iter() {
            if *count > 1 && seq.len() > dup_blocks {
                dup_blocks = seq.len();
            }
        }
        if dup_blocks == 0 {
            let mut eff_counts: BTreeMap<Vec<Vec<String>>, usize> = BTreeMap::new();
            for p in &branch_paths {
                let eff: Vec<Vec<String>> = p.iter().map(|b| block_effect_sig.get(b).cloned().unwrap_or_default()).collect();
                *eff_counts.entry(eff).or_insert(0) += 1;
            }
            for (seq, count) in eff_counts.iter() {
                if *count > 1 && seq.len() > dup_blocks {
                    dup_blocks = seq.len();
                }
            }
        }
        let file = node.file_id.and_then(|id| file_map.get(&id).cloned()).unwrap_or_default();
        let score = outs * dup_blocks;
        let symbol = node_map.get(&node.id).map(|n| n.symbol.clone()).unwrap_or_default();
        out.push(BranchComplexityEntry {
            symbol,
            file,
            line: node.line,
            branch_count: outs,
            duplicate_block_count: dup_blocks,
            score,
        });
    }
    out.sort_by(|a, b| b.score.cmp(&a.score));
    out
}

fn build_callgraph_centrality(
    callgraph: &[(u32, u32)],
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
) -> Vec<CallgraphCentralityEntry> {
    let mut callers: HashMap<u32, BTreeSet<u32>> = HashMap::new();
    let mut callees: HashMap<u32, BTreeSet<u32>> = HashMap::new();
    for (s, d) in callgraph {
        callers.entry(*d).or_default().insert(*s);
        callees.entry(*s).or_default().insert(*d);
    }
    let mut out = Vec::new();
    let mut node_ids: BTreeSet<u32> = BTreeSet::new();
    for (s, d) in callgraph {
        node_ids.insert(*s);
        node_ids.insert(*d);
    }
    for id in node_ids {
        let node = node_map.get(&id);
        let symbol = node.map(|n| n.symbol.clone()).unwrap_or_default();
        let file = node
            .and_then(|n| n.file_id)
            .and_then(|id| file_map.get(&id).cloned())
            .unwrap_or_default();
        let caller_count = callers.get(&id).map(|s| s.len()).unwrap_or(0);
        let callee_count = callees.get(&id).map(|s| s.len()).unwrap_or(0);
        let centrality_score = caller_count + callee_count;
        out.push(CallgraphCentralityEntry { symbol, file, caller_count, callee_count, centrality_score });
    }
    out.sort_by(|a, b| b.centrality_score.cmp(&a.centrality_score));
    out
}

fn build_dead_code(
    nodes: &[NodeRow],
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
    edges: &[EdgeRow],
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

fn build_dependency_cycles(
    callgraph: &[(u32, u32)],
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
) -> Vec<DependencyCycleEntry> {
    let mut adj: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut nodes: HashSet<u32> = HashSet::new();
    for (s, d) in callgraph {
        adj.entry(*s).or_default().push(*d);
        nodes.insert(*s);
        nodes.insert(*d);
    }

    // Tarjan SCC
    let mut index = 0usize;
    let mut stack: Vec<u32> = Vec::new();
    let mut onstack: HashSet<u32> = HashSet::new();
    let mut indices: HashMap<u32, usize> = HashMap::new();
    let mut lowlink: HashMap<u32, usize> = HashMap::new();
    let mut sccs: Vec<Vec<u32>> = Vec::new();

    fn strongconnect(
        v: u32,
        index: &mut usize,
        stack: &mut Vec<u32>,
        onstack: &mut HashSet<u32>,
        indices: &mut HashMap<u32, usize>,
        lowlink: &mut HashMap<u32, usize>,
        adj: &HashMap<u32, Vec<u32>>,
        sccs: &mut Vec<Vec<u32>>,
    ) {
        indices.insert(v, *index);
        lowlink.insert(v, *index);
        *index += 1;
        stack.push(v);
        onstack.insert(v);

        if let Some(neigh) = adj.get(&v) {
            for w in neigh {
                if !indices.contains_key(w) {
                    strongconnect(*w, index, stack, onstack, indices, lowlink, adj, sccs);
                    let lw = *lowlink.get(w).unwrap_or(&0);
                    let lv = *lowlink.get(&v).unwrap_or(&0);
                    lowlink.insert(v, lv.min(lw));
                } else if onstack.contains(w) {
                    let iw = *indices.get(w).unwrap_or(&0);
                    let lv = *lowlink.get(&v).unwrap_or(&0);
                    lowlink.insert(v, lv.min(iw));
                }
            }
        }

        if lowlink.get(&v) == indices.get(&v) {
            let mut scc = Vec::new();
            loop {
                if let Some(w) = stack.pop() {
                    onstack.remove(&w);
                    scc.push(w);
                    if w == v {
                        break;
                    }
                }
            }
            sccs.push(scc);
        }
    }

    for v in nodes {
        if !indices.contains_key(&v) {
            strongconnect(v, &mut index, &mut stack, &mut onstack, &mut indices, &mut lowlink, &adj, &mut sccs);
        }
    }

    let mut out = Vec::new();
    let mut cycle_id = 0usize;
    for scc in sccs {
        let mut is_cycle = scc.len() > 1;
        if !is_cycle {
            // self-loop
            let v = scc[0];
            if let Some(neigh) = adj.get(&v) {
                if neigh.contains(&v) {
                    is_cycle = true;
                }
            }
        }
        if !is_cycle {
            continue;
        }
        let mut node_syms: Vec<String> = Vec::new();
        let mut file_set: BTreeSet<String> = BTreeSet::new();
        for n in &scc {
            if let Some(node) = node_map.get(n) {
                node_syms.push(node.symbol.clone());
                if let Some(fid) = node.file_id {
                    if let Some(path) = file_map.get(&fid) {
                        file_set.insert(path.clone());
                    }
                }
            }
        }
        node_syms.sort();
        cycle_id += 1;
        out.push(DependencyCycleEntry {
            cycle_id,
            nodes: node_syms,
            files: file_set.into_iter().collect(),
            cycle_length: scc.len(),
        });
    }
    out
}

fn build_structural_hotspots(
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

    let mut branch_entries = build_branch_complexity(nodes, node_map, file_map, cfg_out, cfg_in, block_effect_sig);
    let mut per_fn: HashMap<u32, (usize, usize)> = HashMap::new(); // fn -> (branch_count, dup_blocks)
    for entry in branch_entries.drain(..) {
        // branch symbol is block; map to function owner
        let block_id = node_map
            .iter()
            .find(|(_, n)| n.symbol == entry.symbol && n.line == entry.line)
            .map(|(id, _)| *id);
        if let Some(bid) = block_id {
            if let Some(fid) = block_owner.get(&bid) {
                let e = per_fn.entry(*fid).or_insert((0, 0));
                e.0 += entry.branch_count;
                e.1 += entry.duplicate_block_count;
            }
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

fn build_dataflow_fanout(
    nodes: &[NodeRow],
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
    edges: &[EdgeRow],
    block_owner: &HashMap<u32, u32>,
) -> Vec<DataflowFanoutEntry> {
    let mut out = Vec::new();
    let mut fn_nodes: Vec<u32> = Vec::new();
    for n in nodes {
        if n.kind == "FUNCTION" || n.kind == "METHOD" {
            fn_nodes.push(n.id);
        }
    }

    let mutation_kinds: HashSet<&str> = ["ASSIGN", "PROPAGATES", "ARG_TO_PARAM", "RETURNS"].into_iter().collect();
    let io_kinds: HashSet<&str> = ["CALL", "RETURN"].into_iter().collect();

    let mut edges_by_fn: HashMap<u32, Vec<&EdgeRow>> = HashMap::new();
    for e in edges {
        let owner = block_owner.get(&e.src).copied().or_else(|| block_owner.get(&e.dst).copied());
        if let Some(fid) = owner {
            edges_by_fn.entry(fid).or_default().push(e);
        }
    }

    for fid in fn_nodes {
        let node = node_map.get(&fid);
        let symbol = node.map(|n| n.symbol.clone()).unwrap_or_default();
        let file = node
            .and_then(|n| n.file_id)
            .and_then(|id| file_map.get(&id).cloned())
            .unwrap_or_default();
        let line = node.and_then(|n| n.line);
        let fn_edges = edges_by_fn.get(&fid).cloned().unwrap_or_default();
        let outgoing_edges = fn_edges.len();
        let mutation_edges = fn_edges.iter().filter(|e| mutation_kinds.contains(e.kind.as_str())).count();
        let io_edges = fn_edges.iter().filter(|e| io_kinds.contains(e.kind.as_str())).count();
        out.push(DataflowFanoutEntry { symbol, file, line, outgoing_edges, mutation_edges, io_edges });
    }
    out.sort_by(|a, b| b.outgoing_edges.cmp(&a.outgoing_edges));
    out
}
