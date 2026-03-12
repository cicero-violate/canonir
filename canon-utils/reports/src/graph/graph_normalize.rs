use std::collections::HashMap;

use crate::graph::graph_types::{EdgeRow, NodeRow};

pub fn normalize_graph(
    mut nodes: Vec<NodeRow>,
    mut edges: Vec<EdgeRow>,
    files: Vec<String>,
) -> (Vec<NodeRow>, Vec<EdgeRow>, Vec<String>) {
    let mut indexed_files: Vec<(u32, String)> = files
        .into_iter()
        .enumerate()
        .map(|(idx, path)| (idx as u32, path))
        .collect();
    indexed_files.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));

    let mut file_id_map: HashMap<u32, u32> = HashMap::new();
    let mut normalized_files: Vec<String> = Vec::with_capacity(indexed_files.len());
    for (new_idx, (old_idx, path)) in indexed_files.into_iter().enumerate() {
        file_id_map.insert(old_idx, new_idx as u32);
        normalized_files.push(path);
    }

    for node in &mut nodes {
        if let Some(old_id) = node.file_id {
            if let Some(&new_id) = file_id_map.get(&old_id) {
                node.file_id = Some(new_id);
            }
        }
    }

    nodes.sort_by_key(|n| n.id);
    edges.sort_by(|a, b| {
        a.src
            .cmp(&b.src)
            .then_with(|| a.dst.cmp(&b.dst))
            .then_with(|| a.kind.cmp(&b.kind))
    });

    (nodes, edges, normalized_files)
}


