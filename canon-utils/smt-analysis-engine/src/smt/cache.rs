use crate::loader::{AnalysisGraph, EdgeKind, NodeKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub result: String,
    pub model: Option<Value>,
    pub graph_hash: String,
    pub timestamp: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CacheFile {
    entries: HashMap<String, CacheEntry>,
}

pub struct ProofCache {
    path: PathBuf,
    entries: HashMap<String, CacheEntry>,
    dirty: bool,
}

impl ProofCache {
    pub fn new(path: PathBuf, clear: bool) -> Self {
        if clear {
            let _ = fs::remove_file(&path);
        }
        let entries = if !clear {
            load_cache(&path)
        } else {
            HashMap::new()
        };
        Self {
            path,
            entries,
            dirty: false,
        }
    }

    pub fn get(&self, key: &str, graph_hash: &str) -> Option<CacheEntry> {
        self.entries.get(key).and_then(|entry| {
            if entry.graph_hash == graph_hash {
                Some(entry.clone())
            } else {
                None
            }
        })
    }

    pub fn insert(&mut self, key: String, entry: CacheEntry) {
        self.entries.insert(key, entry);
        self.dirty = true;
    }
}

impl Drop for ProofCache {
    fn drop(&mut self) {
        if !self.dirty {
            return;
        }
        let payload = CacheFile {
            entries: self.entries.clone(),
        };
        if let Ok(text) = serde_json::to_string_pretty(&payload) {
            let _ = fs::write(&self.path, text);
        }
    }
}

fn load_cache(path: &Path) -> HashMap<String, CacheEntry> {
    let Ok(text) = fs::read_to_string(path) else {
        return HashMap::new();
    };
    serde_json::from_str::<CacheFile>(&text)
        .map(|f| f.entries)
        .unwrap_or_default()
}

pub fn reachability_key(fn_id: u32, err_id: u32, graph_hash: &str) -> String {
    hash_parts(&[&format!("reach:{fn_id}:{err_id}"), graph_hash])
}

pub fn invariant_key(predicate: &str, graph_hash: &str) -> String {
    hash_parts(&[&format!("inv:{predicate}"), graph_hash])
}

pub fn equivalence_key(a: u32, b: u32, graph_hash: &str) -> String {
    hash_parts(&[&format!("eq:{a}:{b}"), graph_hash])
}

pub fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn function_graph_hash(graph: &AnalysisGraph, fn_id: u32) -> String {
    let blocks = function_blocks(graph, fn_id);
    if blocks.is_empty() {
        return hash_parts(&[&format!("fn:{fn_id}")]);
    }
    let mut edges: Vec<String> = Vec::new();
    let block_set: HashSet<u32> = blocks.iter().copied().collect();
    for e in &graph.edges {
        match e.kind {
            EdgeKind::Flow => {
                if block_set.contains(&e.src) && block_set.contains(&e.dst) {
                    edges.push(format!("flow:{}->{}", e.src, e.dst));
                }
            }
            EdgeKind::ErrorToBlock => {
                if block_set.contains(&e.dst) {
                    edges.push(format!("err:{}->{}", e.src, e.dst));
                }
            }
            _ => {}
        }
    }
    edges.sort();
    let mut parts = Vec::new();
    parts.push(format!("fn:{fn_id}"));
    for b in blocks {
        parts.push(format!("bb:{b}"));
    }
    for e in edges {
        parts.push(e);
    }
    hash_parts(&parts.iter().map(|s| s.as_str()).collect::<Vec<_>>())
}

pub fn equivalence_graph_hash(graph: &AnalysisGraph, a: u32, b: u32) -> String {
    let ha = function_graph_hash(graph, a);
    let hb = function_graph_hash(graph, b);
    hash_parts(&[&ha, &hb])
}

pub fn invariant_graph_hash(graph: &AnalysisGraph, candidate: &Value) -> String {
    if let Some(fid) = candidate.get("function_id").and_then(|v| v.as_u64()) {
        return function_graph_hash(graph, fid as u32);
    }
    if let Some(bid) = candidate.get("block_id").and_then(|v| v.as_u64()) {
        if let Some(fid) = function_for_block(graph, bid as u32) {
            return function_graph_hash(graph, fid);
        }
    }
    hash_parts(&[&format!("graph:{}", graph.nodes.len())])
}

fn function_blocks(graph: &AnalysisGraph, fn_id: u32) -> Vec<u32> {
    let mut blocks = Vec::new();
    for e in &graph.edges {
        if e.kind == EdgeKind::HasBlock && e.src == fn_id {
            if let Some(node) = graph.id_to_index.get(&e.dst).and_then(|&i| graph.nodes.get(i)) {
                if node.kind == NodeKind::BasicBlock {
                    blocks.push(node.id);
                }
            }
        }
    }
    blocks.sort();
    blocks
}

fn function_for_block(graph: &AnalysisGraph, block_id: u32) -> Option<u32> {
    for e in &graph.edges {
        if e.kind == EdgeKind::HasBlock && e.dst == block_id {
            return Some(e.src);
        }
    }
    None
}

fn hash_parts(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update(&[0u8]);
    }
    let digest = hasher.finalize();
    hex::encode(digest)
}
