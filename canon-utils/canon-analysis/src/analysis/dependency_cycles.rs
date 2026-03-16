use std::collections::{BTreeSet, HashMap, HashSet};

use algorithms::graph::csr::Csr;
#[cfg(feature = "cuda")]
use algorithms::graph::scc_gpu::scc_gpu;

use canon_graph::graph::graph_types::CodeNode;
use crate::DependencyCycleEntry;

pub fn compute_scc(callgraph: &[(u32, u32)]) -> Vec<Vec<u32>> {
    let mut index = 0u32;
    let mut stack: Vec<u32> = Vec::new();
    let mut indices: HashMap<u32, u32> = HashMap::new();
    let mut lowlink: HashMap<u32, u32> = HashMap::new();
    let mut on_stack: HashSet<u32> = HashSet::new();
    let mut result: Vec<Vec<u32>> = Vec::new();

    let mut nodes: HashSet<u32> = HashSet::new();
    for (src, dst) in callgraph {
        nodes.insert(*src);
        nodes.insert(*dst);
    }

    fn strongconnect(
        v: u32,
        index: &mut u32,
        stack: &mut Vec<u32>,
        indices: &mut HashMap<u32, u32>,
        lowlink: &mut HashMap<u32, u32>,
        on_stack: &mut HashSet<u32>,
        result: &mut Vec<Vec<u32>>,
        callgraph: &[(u32, u32)],
    ) {
        indices.insert(v, *index);
        lowlink.insert(v, *index);
        *index += 1;
        stack.push(v);
        on_stack.insert(v);

        for (src, dst) in callgraph {
            if *src != v {
                continue;
            }
            if !indices.contains_key(dst) {
                strongconnect(*dst, index, stack, indices, lowlink, on_stack, result, callgraph);
                let low_v = *lowlink.get(&v).unwrap();
                let low_dst = *lowlink.get(dst).unwrap();
                lowlink.insert(v, low_v.min(low_dst));
            } else if on_stack.contains(dst) {
                let low_v = *lowlink.get(&v).unwrap();
                let idx_dst = *indices.get(dst).unwrap();
                lowlink.insert(v, low_v.min(idx_dst));
            }
        }

        if indices.get(&v) == lowlink.get(&v) {
            let mut scc = Vec::new();
            loop {
                if let Some(w) = stack.pop() {
                    on_stack.remove(&w);
                    scc.push(w);
                    if w == v {
                        break;
                    }
                }
            }
            result.push(scc);
        }
    }

    for node in nodes {
        if !indices.contains_key(&node) {
            strongconnect(
                node,
                &mut index,
                &mut stack,
                &mut indices,
                &mut lowlink,
                &mut on_stack,
                &mut result,
                callgraph,
            );
        }
    }
    result
}

pub fn build_dependency_cycles(
    callgraph: &[(u32, u32)],
    node_map: &HashMap<u32, CodeNode>,
    file_map: &HashMap<u32, String>,
) -> Vec<DependencyCycleEntry> {
    let sccs = compute_scc(callgraph);
    sccs
        .into_iter()
        .enumerate()
        .filter_map(|(idx, comp)| {
            if comp.len() < 2 {
                return None;
            }
            let mut nodes = Vec::new();
            let mut unique_symbols = BTreeSet::new();
            let mut files = Vec::new();
            for id in comp.iter() {
                let node = node_map.get(id)?;
                nodes.push(node.symbol.clone());
                unique_symbols.insert(node.symbol.clone());
                if let Some(file_id) = node.file_id {
                    if let Some(path) = file_map.get(&file_id) {
                        files.push(path.clone());
                    }
                }
            }
            if unique_symbols.len() < 2 {
                return None;
            }
            Some(DependencyCycleEntry {
                cycle_id: idx + 1,
                nodes,
                files,
                cycle_length: comp.len(),
            })
        })
        .collect()
}

pub fn build_dependency_cycles_gpu(
    _callgraph: &[(u32, u32)],
    node_map: &HashMap<u32, CodeNode>,
    file_map: &HashMap<u32, String>,
    _cg_csr: &Csr,
    _cg_local_to_id: &[u32],
) -> Vec<DependencyCycleEntry> {
    #[cfg(not(feature = "cuda"))]
    {
        return build_dependency_cycles(callgraph, node_map, file_map);
    }

    #[cfg(feature = "cuda")]
    {
        let sccs = scc_gpu(_cg_csr);
        return sccs
            .into_iter()
            .enumerate()
            .filter_map(|(idx, comp)| {
                if comp.len() < 2 {
                    return None;
                }
                let mut nodes = Vec::new();
                let mut unique_symbols = BTreeSet::new();
                let mut files = Vec::new();
                for local in &comp {
                    let id = *_cg_local_to_id.get(*local as usize)?;
                    let node = node_map.get(&id)?;
                    nodes.push(node.symbol.clone());
                    unique_symbols.insert(node.symbol.clone());
                    if let Some(file_id) = node.file_id {
                        if let Some(path) = file_map.get(&file_id) {
                            files.push(path.clone());
                        }
                    }
                }
                if unique_symbols.len() < 2 {
                    return None;
                }
                Some(DependencyCycleEntry {
                    cycle_id: idx + 1,
                    nodes,
                    files,
                    cycle_length: comp.len(),
                })
            })
            .collect();
    }
}
