use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, Seek, SeekFrom};
use std::path::Path;

use crate::graph::graph_builder::module_prefixes;
use crate::replay::tlog_reader::parse_tlog_event;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GraphCache {
    pub last_offset: u64,
    pub module_files: BTreeMap<String, String>,
    pub type_nodes: BTreeMap<String, TypeNodeCache>,
    pub type_edges: BTreeSet<TypeEdgeCache>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeNodeCache {
    pub kind: String,
    pub file: String,
    pub line: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct TypeEdgeCache {
    pub src: String,
    pub dst: String,
    pub rel: String,
}

pub fn update_graph_cache(tlog_path: &Path, reports_dir: &Path) -> Result<GraphCache> {
    fs::create_dir_all(reports_dir)?;
    let cache_path = reports_dir.join(".graph_cache.json");
    let mut cache = if cache_path.exists() {
        let data = fs::read_to_string(&cache_path)?;
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        GraphCache::default()
    };

    let mut file = fs::File::open(tlog_path)?;
    let metadata_len = file.metadata()?.len();
    if cache.last_offset > metadata_len {
        cache.last_offset = 0;
    }
    file.seek(SeekFrom::Start(cache.last_offset))?;
    let reader = std::io::BufReader::new(file);

    for raw_line in reader.lines() {
        let raw_line = raw_line?;
        let mut line = raw_line.as_str();
        loop {
            if let Some(idx) = line.find("{\"t\":\"SESSION\"") {
                if idx > 0 {
                    let (prefix, suffix) = line.split_at(idx);
                    if let Some(record) = parse_tlog_event(prefix) {
                        apply_cache_event(record, &mut cache);
                    }
                    line = suffix;
                    continue;
                }
            }
            if let Some(record) = parse_tlog_event(line) {
                apply_cache_event(record, &mut cache);
            }
            break;
        }
    }

    cache.last_offset = metadata_len;
    fs::write(&cache_path, serde_json::to_string(&cache)?)?;
    Ok(cache)
}

fn apply_cache_event(value: Value, cache: &mut GraphCache) {
    let Some(tag) = value.get("t").and_then(|v| v.as_str()) else {
        return;
    };
    match tag {
        "SESSION" => {
            cache.module_files.clear();
            cache.type_nodes.clear();
            cache.type_edges.clear();
        }
        "N" | "NODE" | "NODE_UPDATE" => {
            let sym = value.get("sym").and_then(|v| v.as_str()).unwrap_or("");
            let kind = value.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            let file = value.get("file").and_then(|v| v.as_str()).unwrap_or("");
            let line = value.get("line").and_then(|v| v.as_u64()).map(|v| v as u32);
            if sym.is_empty() && kind != "MODULE" {
                return;
            }

            if kind == "MODULE" {
                if sym.is_empty() {
                    cache.module_files.insert("".to_string(), file.to_string());
                } else {
                    cache.module_files.insert(sym.to_string(), file.to_string());
                    for module_sym in module_prefixes(sym) {
                        cache.module_files.entry(module_sym).or_insert_with(|| file.to_string());
                    }
                }
            }

            let type_kinds = ["STRUCT", "ENUM", "TRAIT", "IMPL", "TYPE"];
            if type_kinds.contains(&kind) && !sym.is_empty() {
                cache.type_nodes.insert(sym.to_string(), TypeNodeCache {
                    kind: kind.to_string(),
                    file: file.to_string(),
                    line,
                });
            }
        }
        "NODE_REMOVE" => {
            let sym = value.get("sym").and_then(|v| v.as_str()).unwrap_or("");
            if sym.is_empty() {
                return;
            }
            cache.module_files.remove(sym);
            cache.type_nodes.remove(sym);
            cache.type_edges.retain(|edge| edge.src != sym && edge.dst != sym);
        }
        "E" | "EDGE" => {
            let rel_kinds = ["HAS_FIELD", "HAS_METHOD", "IMPLEMENTS", "FOR_TYPE", "USES_TYPE", "BOUNDS"];
            let kind = value.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            if !rel_kinds.contains(&kind) {
                return;
            }
            let src_sym = value.get("src").and_then(|v| v.as_str()).unwrap_or("");
            let dst_sym = value.get("dst").and_then(|v| v.as_str()).unwrap_or("");
            if src_sym.is_empty() || dst_sym.is_empty() {
                return;
            }
            cache.type_edges.insert(TypeEdgeCache {
                src: src_sym.to_string(),
                dst: dst_sym.to_string(),
                rel: kind.to_string(),
            });
        }
        "EDGE_REMOVE" => {
            let rel_kinds = ["HAS_FIELD", "HAS_METHOD", "IMPLEMENTS", "FOR_TYPE", "USES_TYPE", "BOUNDS"];
            let kind = value.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            if !rel_kinds.contains(&kind) {
                return;
            }
            let src_sym = value.get("src").and_then(|v| v.as_str()).unwrap_or("");
            let dst_sym = value.get("dst").and_then(|v| v.as_str()).unwrap_or("");
            if src_sym.is_empty() || dst_sym.is_empty() {
                return;
            }
            cache.type_edges.remove(&TypeEdgeCache {
                src: src_sym.to_string(),
                dst: dst_sym.to_string(),
                rel: kind.to_string(),
            });
        }
        _ => {}
    }
}
