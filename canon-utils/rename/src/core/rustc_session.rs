use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::project_editor_helpers::split_module_segments;

#[derive(Clone, Debug)]
pub struct SpanRange {
    pub lo: usize,
    pub hi: usize,
}

pub struct RustcSession {
    span_index: HashMap<String, HashMap<PathBuf, Vec<SpanRange>>>,
    symbol_kinds: HashMap<String, String>,
    symbol_catalog: Vec<(String, String)>,
    pub normalized_sources: HashMap<PathBuf, String>,
}

impl RustcSession {
    pub fn build(project_root: &Path) -> Result<Self> {
        let analysis_dir = project_root.join("analysis");
        let spans_bin = analysis_dir.join("spans.bin");
        let files_txt = analysis_dir.join("files.txt");
        let nodes_csv = analysis_dir.join("nodes.csv");
        let symbols_json = analysis_dir.join("symbols.json");
        let (mut span_index, symbol_kinds, normalized_sources) = if spans_bin.exists()
            && files_txt.exists()
            && nodes_csv.exists()
            && symbols_json.exists()
        {
            load_spans_from_upg(&spans_bin, &files_txt, &nodes_csv, &symbols_json)?
        } else {
            return Err(anyhow!(
                "missing span data; expected spans.bin+files.txt+nodes.csv+symbols.json in {}",
                analysis_dir.display()
            ));
        };

        if symbol_kinds.is_empty() {
            return Err(anyhow!("span collector produced no output"));
        }

        let mut symbol_catalog: Vec<(String, String)> = symbol_kinds.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        symbol_catalog.sort_by(|a, b| a.0.cmp(&b.0));

        for per_file in span_index.values_mut() {
            for spans in per_file.values_mut() {
                spans.sort_by(|a, b| a.lo.cmp(&b.lo));
                spans.dedup_by(|a, b| a.lo == b.lo && a.hi == b.hi);
            }
        }

        Ok(Self { span_index, symbol_kinds, symbol_catalog, normalized_sources })
    }

    pub fn spans_for(&self, symbol_id: &str) -> Option<&HashMap<PathBuf, Vec<SpanRange>>> {
        self.span_index.get(symbol_id)
    }

    pub fn normalized_source(&self, path: &PathBuf) -> Option<&String> {
        self.normalized_sources.get(path)
    }

    pub fn symbol_catalog(&self) -> Vec<(String, String)> {
        self.symbol_catalog.clone()
    }

    pub fn symbol_ids(&self) -> Vec<String> {
        self.symbol_catalog
            .iter()
            .map(|(id, _)| split_module_segments(id).join("::"))
            .collect()
    }

    pub fn symbol_kind(&self, symbol_id: &str) -> Option<&str> {
        self.symbol_kinds.get(symbol_id).map(|value| value.as_str())
    }
}

fn load_spans_from_upg(
    spans_bin: &Path,
    files_txt: &Path,
    nodes_csv: &Path,
    symbols_json: &Path,
) -> Result<(HashMap<String, HashMap<PathBuf, Vec<SpanRange>>>, HashMap<String, String>, HashMap<PathBuf, String>)> {
    let files = load_files_txt(files_txt)?;
    let symbols_by_id = load_symbols_from_nodes(nodes_csv)?;
    let span_records = load_spans_bin(spans_bin)?;
    let mut spans_by_node: Vec<(u32, u32, u32)> = vec![(u32::MAX, 0, 0); symbols_by_id.len()];
    let mut seen = 0usize;
    for (node_id, file_id, lo, hi) in span_records {
        let idx = node_id as usize;
        if idx >= spans_by_node.len() {
            return Err(anyhow!(
                "spans.bin node id {} out of range (nodes.csv count {})",
                node_id,
                symbols_by_id.len()
            ));
        }
        if spans_by_node[idx].0 == u32::MAX {
            seen += 1;
        }
        spans_by_node[idx] = (file_id, lo, hi);
    }
    if seen != symbols_by_id.len() {
        return Err(anyhow!(
            "spans.bin record count {} does not match nodes.csv count {}",
            seen,
            symbols_by_id.len()
        ));
    }

    let symbol_kinds = load_symbol_kinds(symbols_json)?;
    let mut span_index: HashMap<String, HashMap<PathBuf, Vec<SpanRange>>> = HashMap::new();

    for (idx, (file_id, lo, hi)) in spans_by_node.into_iter().enumerate() {
        if file_id == u32::MAX || (lo == 0 && hi == 0) {
            continue;
        }
        let symbol = symbols_by_id.get(idx).map(|s| s.as_str()).unwrap_or("");
        if symbol.is_empty() {
            continue;
        }
        let file = match files.get(file_id as usize) {
            Some(path) => PathBuf::from(path),
            None => continue,
        };
        span_index
            .entry(symbol.to_string())
            .or_default()
            .entry(file)
            .or_default()
            .push(SpanRange { lo: lo as usize, hi: hi as usize });
    }

    Ok((span_index, symbol_kinds, HashMap::new()))
}

fn load_files_txt(path: &Path) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(path)?;
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

fn load_symbols_from_nodes(path: &Path) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(path)?;
    let header = content.lines().next().unwrap_or_default();
    let has_file_id = header.contains("file_id");
    let mut symbols: Vec<String> = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        if has_file_id {
            if parts.len() < 7 {
                continue;
            }
            let id = parts[0].parse::<usize>().unwrap_or(usize::MAX);
            if id == usize::MAX {
                continue;
            }
            let symbol = normalize_node_symbol(parts[2..parts.len() - 4].join(","));
            if symbols.len() <= id {
                symbols.resize(id + 1, String::new());
            }
            symbols[id] = symbol;
        } else {
            if parts.len() < 6 {
                continue;
            }
            let id = parts[0].parse::<usize>().unwrap_or(usize::MAX);
            if id == usize::MAX {
                continue;
            }
            let symbol = normalize_node_symbol(parts[2..parts.len() - 3].join(","));
            if symbols.len() <= id {
                symbols.resize(id + 1, String::new());
            }
            symbols[id] = symbol;
        }
    }
    Ok(symbols)
}

fn normalize_node_symbol(raw: String) -> String {
    let sym = raw.trim().to_string();
    if sym.is_empty() {
        return "crate::".to_string();
    }
    if sym.starts_with("crate::") {
        return sym;
    }
    if sym.starts_with('<') || sym.starts_with("<<") {
        return sym;
    }
    format!("crate::{sym}")
}

fn load_spans_bin(path: &Path) -> Result<Vec<(u32, u32, u32, u32)>> {
    let bytes = std::fs::read(path)?;
    if bytes.len() % 16 != 0 {
        return Err(anyhow!("invalid spans.bin length {}", bytes.len()));
    }
    let mut out = Vec::with_capacity(bytes.len() / 16);
    for chunk in bytes.chunks_exact(16) {
        let node_id = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let file_id = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
        let lo = u32::from_le_bytes([chunk[8], chunk[9], chunk[10], chunk[11]]);
        let hi = u32::from_le_bytes([chunk[12], chunk[13], chunk[14], chunk[15]]);
        out.push((node_id, file_id, lo, hi));
    }
    Ok(out)
}

fn load_symbol_kinds(path: &Path) -> Result<HashMap<String, String>> {
    let content = std::fs::read_to_string(path)?;
    let value: Value = serde_json::from_str(&content)?;
    let mut kinds = HashMap::new();
    let Some(items) = value.as_array() else {
        return Ok(kinds);
    };
    for item in items {
        let Some(symbol_id) = item.get("symbol_id").and_then(|v| v.as_str()) else { continue; };
        let kind = item.get("kind").and_then(|v| v.as_str()).unwrap_or("unknown");
        kinds.insert(symbol_id.to_string(), kind.to_string());
    }
    Ok(kinds)
}
