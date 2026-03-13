use std::collections::HashMap;

use canon_graph::graph::graph_types::{EdgeRow, NodeRow};

pub fn build_cfg_out(cfg: &[EdgeRow]) -> HashMap<u32, Vec<u32>> {
    let mut out = HashMap::new();
    for e in cfg {
        out.entry(e.src).or_insert_with(Vec::new).push(e.dst);
    }
    out
}

pub fn build_cfg_in(cfg: &[EdgeRow]) -> HashMap<u32, usize> {
    let mut inn = HashMap::new();
    for e in cfg {
        *inn.entry(e.dst).or_insert(0) += 1;
    }
    inn
}

pub fn trace_path(start: u32, cfg_out: &HashMap<u32, Vec<u32>>, cfg_in: &HashMap<u32, usize>) -> Vec<u32> {
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

pub fn build_block_owner(nodes: &[NodeRow], edges: &[EdgeRow]) -> HashMap<u32, u32> {
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

pub fn build_block_effect_signatures(edges: &[EdgeRow], node_map: &HashMap<u32, NodeRow>) -> HashMap<u32, Vec<String>> {
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

pub fn extract_cfg_edges(nodes: &[NodeRow], edges: &[EdgeRow]) -> Vec<EdgeRow> {
    let id_to_kind: HashMap<u32, &str> = nodes.iter().map(|n| (n.id, n.kind.as_str())).collect();
    let mut out = Vec::new();
    for edge in edges {
        if edge.kind != "FLOW" && edge.kind != "UNWIND" && edge.kind != "RETURN" && edge.kind != "BRANCH" {
            continue;
        }
        let src_kind = id_to_kind.get(&edge.src);
        let dst_kind = id_to_kind.get(&edge.dst);
        if src_kind == Some(&"BASIC_BLOCK") {
            if edge.kind == "RETURN" {
                out.push(edge.clone());
                continue;
            }
            if dst_kind == Some(&"BASIC_BLOCK") {
                out.push(edge.clone());
            }
        }
    }
    out
}
