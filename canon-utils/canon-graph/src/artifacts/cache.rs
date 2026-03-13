use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, Seek, SeekFrom};
use std::path::Path;

use crate::graph::graph_builder::module_prefixes;
use canon_tlog_replay::{extract_kernel_event, parse_any_event, read_any_events_from_path, AnyEvent};
use canon_types::KernelEvent;

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

    if tlog_path.is_dir() {
        for event in read_any_events_from_path(tlog_path)? {
            if let AnyEvent::Canon(canon) = event {
                if let Some(kernel) = extract_kernel_event(&canon) {
                    apply_cache_event(kernel, &mut cache);
                }
            }
        }
    } else {
        let mut file = fs::File::open(tlog_path)?;
        let metadata_len = file.metadata()?.len();
        if cache.last_offset > metadata_len {
            cache.last_offset = 0;
        }
        file.seek(SeekFrom::Start(cache.last_offset))?;
        let reader = std::io::BufReader::new(file);

        for raw_line in reader.lines() {
            let raw_line = raw_line?;
            if let Some(event) = parse_any_event(&raw_line) {
                if let AnyEvent::Canon(canon) = event {
                    if let Some(kernel) = extract_kernel_event(&canon) {
                        apply_cache_event(kernel, &mut cache);
                    }
                }
            }
        }
        cache.last_offset = metadata_len;
    }
    fs::write(&cache_path, serde_json::to_string(&cache)?)?;
    Ok(cache)
}

fn apply_cache_event(event: KernelEvent, cache: &mut GraphCache) {
    match event {
        KernelEvent::SessionStart { .. } => {
            cache.module_files.clear();
            cache.type_nodes.clear();
            cache.type_edges.clear();
        }
        KernelEvent::NodeDefined { symbol, kind, file, line, .. }
        | KernelEvent::NodeUpdated { symbol, kind, file, line, .. } => {
            let sym = symbol.as_str();
            let kind = kind.as_str();
            let file = file.as_str();
            let line = Some(line).filter(|v| *v > 0);
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
        KernelEvent::NodeRemoved { symbol } => {
            let sym = symbol.as_str();
            if sym.is_empty() {
                return;
            }
            cache.module_files.remove(sym);
            cache.type_nodes.remove(sym);
            cache.type_edges.retain(|edge| edge.src != sym && edge.dst != sym);
        }
        KernelEvent::EdgeDefined { src, dst, kind } => {
            let rel_kinds = ["HAS_FIELD", "HAS_METHOD", "IMPLEMENTS", "FOR_TYPE", "USES_TYPE", "BOUNDS"];
            let kind = kind.as_str();
            if !rel_kinds.contains(&kind) {
                return;
            }
            let src_sym = src.as_str();
            let dst_sym = dst.as_str();
            if src_sym.is_empty() || dst_sym.is_empty() {
                return;
            }
            cache.type_edges.insert(TypeEdgeCache {
                src: src_sym.to_string(),
                dst: dst_sym.to_string(),
                rel: kind.to_string(),
            });
        }
        KernelEvent::EdgeRemoved { src, dst, kind } => {
            let rel_kinds = ["HAS_FIELD", "HAS_METHOD", "IMPLEMENTS", "FOR_TYPE", "USES_TYPE", "BOUNDS"];
            let kind = kind.as_str();
            if !rel_kinds.contains(&kind) {
                return;
            }
            let src_sym = src.as_str();
            let dst_sym = dst.as_str();
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
