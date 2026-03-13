use anyhow::{anyhow, Result};
use crate::symbol_index::SymbolIndex;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct GraphEdgeRecord {
    pub src: String,
    pub dst: String,
    pub kind: String,
}

pub struct AnalysisSession {
    pub module_files: HashMap<String, PathBuf>,
    pub file_modules: HashMap<PathBuf, Vec<String>>,
    pub files: HashSet<PathBuf>,
    pub uses_crate_prefix: bool,
    edges: Vec<GraphEdgeRecord>,
}

impl AnalysisSession {
    pub fn load(project_root: &Path) -> Result<Self> {
        let index = SymbolIndex::build(project_root)?;
        if index.module_files().is_empty() {
            return Err(anyhow!(
                "reports has no module mapping for {}",
                project_root.display()
            ));
        }
        let reports_dir = reports_out_dir(project_root)?;
        let (edges, files) = load_edges(&reports_dir)?;
        Ok(Self {
            module_files: index.module_files().clone(),
            file_modules: index.file_modules().clone(),
            files: files,
            uses_crate_prefix: index.uses_crate_prefix(),
            edges,
        })
    }

    pub fn edges_by_kind(&self, edge_kind: &str) -> Result<Vec<GraphEdgeRecord>> {
        Ok(self
            .edges
            .iter()
            .filter(|e| e.kind == edge_kind)
            .cloned()
            .collect())
    }

    pub fn callers_of(&self, sym: &str) -> Result<Vec<GraphEdgeRecord>> {
        Ok(self
            .edges
            .iter()
            .filter(|e| e.kind == "CALL" && e.dst == sym)
            .cloned()
            .collect())
    }
}

fn reports_out_dir(project_root: &Path) -> Result<PathBuf> {
    if let Ok(out) = std::env::var("CANON_REPORTS_OUT") {
        return Ok(PathBuf::from(out));
    }
    Ok(project_root.join("state/reports_out/kernel"))
}

fn load_edges(reports_dir: &Path) -> Result<(Vec<GraphEdgeRecord>, HashSet<PathBuf>)> {
    let nodes_path = reports_dir.join("nodes.csv");
    let edges_path = reports_dir.join("edges.csv");
    let files_path = reports_dir.join("files.txt");
    if !nodes_path.exists() || !edges_path.exists() || !files_path.exists() {
        return Err(anyhow!(
            "reports artifacts not found in {}; run canon_reports first",
            reports_dir.display()
        ));
    }

    let files = read_files_txt(&files_path)?;
    let nodes = read_nodes_csv(&nodes_path)?;
    let id_to_symbol: HashMap<u32, String> = nodes
        .into_iter()
        .map(|n| (n.id, n.symbol))
        .collect();
    let edges = read_edges_csv(&edges_path, &id_to_symbol)?;
    Ok((edges, files))
}

#[derive(Debug)]
struct NodeRow {
    id: u32,
    symbol: String,
}

fn read_nodes_csv(path: &Path) -> Result<Vec<NodeRow>> {
    let mut rdr = csv::ReaderBuilder::new().from_path(path)?;
    let headers = rdr
        .headers()
        .map(|h| h.iter().map(|s| s.to_string()).collect::<Vec<_>>())?;
    let id_idx = headers.iter().position(|h| h == "id").unwrap_or(0);
    let symbol_idx = headers.iter().position(|h| h == "symbol").unwrap_or(2);
    let mut out = Vec::new();
    for rec in rdr.records() {
        let rec = rec?;
        let id = rec.get(id_idx).unwrap_or("0").parse::<u32>().unwrap_or(0);
        let symbol = rec.get(symbol_idx).unwrap_or("").to_string();
        if symbol.is_empty() {
            continue;
        }
        out.push(NodeRow { id, symbol });
    }
    Ok(out)
}

fn read_edges_csv(path: &Path, id_to_symbol: &HashMap<u32, String>) -> Result<Vec<GraphEdgeRecord>> {
    let mut rdr = csv::ReaderBuilder::new().from_path(path)?;
    let headers = rdr
        .headers()
        .map(|h| h.iter().map(|s| s.to_string()).collect::<Vec<_>>())?;
    let src_idx = headers.iter().position(|h| h == "src").unwrap_or(0);
    let dst_idx = headers.iter().position(|h| h == "dst").unwrap_or(1);
    let kind_idx = headers.iter().position(|h| h == "kind").unwrap_or(2);
    let mut out = Vec::new();
    for rec in rdr.records() {
        let rec = rec?;
        let src_id = rec.get(src_idx).unwrap_or("0").parse::<u32>().unwrap_or(0);
        let dst_id = rec.get(dst_idx).unwrap_or("0").parse::<u32>().unwrap_or(0);
        let kind = rec.get(kind_idx).unwrap_or("").to_string();
        let Some(src) = id_to_symbol.get(&src_id) else { continue };
        let Some(dst) = id_to_symbol.get(&dst_id) else { continue };
        out.push(GraphEdgeRecord {
            src: src.clone(),
            dst: dst.clone(),
            kind,
        });
    }
    Ok(out)
}

fn read_files_txt(path: &Path) -> Result<HashSet<PathBuf>> {
    let content = std::fs::read_to_string(path)?;
    let mut out = HashSet::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        out.insert(PathBuf::from(trimmed));
    }
    Ok(out)
}
