use anyhow::{anyhow, Result};
use serde_json::Value;
use canon_types::SpanRange;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub struct SymbolIndex {
    span_index: HashMap<String, HashMap<PathBuf, Vec<SpanRange>>>,
    symbol_kinds: HashMap<String, String>,
    symbol_catalog: Vec<(String, String)>,
    pub normalized_sources: HashMap<PathBuf, String>,
    tlog_offset: u64,
    module_files: HashMap<String, PathBuf>,
    file_modules: HashMap<PathBuf, Vec<String>>,
    files: HashSet<PathBuf>,
    uses_crate_prefix: bool,
}

impl SymbolIndex {
    pub fn from_tlog(tlog_path: &Path) -> Result<Self> {
        let file = File::open(tlog_path)?;
        let tlog_offset = file.metadata().map(|m| m.len()).unwrap_or(0);
        let reader = BufReader::new(file);
        let mut span_index: HashMap<String, HashMap<PathBuf, Vec<SpanRange>>> = HashMap::new();
        let mut symbol_kinds: HashMap<String, String> = HashMap::new();
        let mut module_files: HashMap<String, PathBuf> = HashMap::new();
        let mut file_modules: HashMap<PathBuf, Vec<String>> = HashMap::new();
        let mut files: HashSet<PathBuf> = HashSet::new();

        for line in reader.lines() {
            let line = line?;
            apply_tlog_line(
                &line,
                &mut span_index,
                &mut symbol_kinds,
                &mut module_files,
                &mut file_modules,
                &mut files,
            )?;
        }

        let symbol_catalog = build_symbol_catalog(&symbol_kinds);
        dedup_spans(&mut span_index);
        let uses_crate_prefix = module_files.keys().any(|k| k.starts_with("crate::"));

        Ok(Self {
            span_index,
            symbol_kinds,
            symbol_catalog,
            normalized_sources: HashMap::new(),
            tlog_offset,
            module_files,
            file_modules,
            files,
            uses_crate_prefix,
        })
    }

    pub fn build(project_root: &Path) -> Result<Self> {
        let tlog_path = project_root.join("logs/graph.tlog");
        if !tlog_path.exists() {
            return Err(anyhow!(
                "graph.tlog not found at {}; run canon_kernel first",
                tlog_path.display()
            ));
        }
        Self::from_tlog(&tlog_path)
    }

    pub fn is_stale(&self, tlog_path: &Path) -> bool {
        if self.tlog_offset == 0 {
            return false;
        }
        tlog_path
            .metadata()
            .map(|m| m.len() != self.tlog_offset)
            .unwrap_or(true)
    }

    pub fn refresh_from_tlog(&mut self, tlog_path: &Path) -> Result<bool> {
        let new_len = tlog_path.metadata().map(|m| m.len()).unwrap_or(0);
        if new_len == self.tlog_offset {
            return Ok(false);
        }

        let idx_path = tlog_path.with_extension("tlog.idx");
        let last_session_offset = if idx_path.exists() {
            read_tlog_index(&idx_path)?
        } else {
            0
        };
        if last_session_offset > new_len {
            return Err(anyhow!(
                "tlog index offset {} exceeds tlog length {}",
                last_session_offset,
                new_len
            ));
        }

        let mut file = File::open(tlog_path)?;
        file.seek(SeekFrom::Start(last_session_offset))?;
        let reader = BufReader::new(file);
        self.span_index.clear();
        self.symbol_kinds.clear();
        self.module_files.clear();
        self.file_modules.clear();
        self.files.clear();
        for line in reader.lines() {
            let line = line?;
            apply_tlog_line(
                &line,
                &mut self.span_index,
                &mut self.symbol_kinds,
                &mut self.module_files,
                &mut self.file_modules,
                &mut self.files,
            )?;
        }
        self.symbol_catalog = build_symbol_catalog(&self.symbol_kinds);
        dedup_spans(&mut self.span_index);
        self.uses_crate_prefix = self.module_files.keys().any(|k| k.starts_with("crate::"));
        self.tlog_offset = new_len;
        Ok(true)
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

    pub fn module_files(&self) -> &HashMap<String, PathBuf> {
        &self.module_files
    }

    pub fn file_modules(&self) -> &HashMap<PathBuf, Vec<String>> {
        &self.file_modules
    }

    pub fn files(&self) -> &HashSet<PathBuf> {
        &self.files
    }

    pub fn uses_crate_prefix(&self) -> bool {
        self.uses_crate_prefix
    }
}

fn split_module_segments(path: &str) -> Vec<&str> {
    path.split("::").filter(|s| !s.is_empty()).collect()
}

fn apply_tlog_line(
    line: &str,
    span_index: &mut HashMap<String, HashMap<PathBuf, Vec<SpanRange>>>,
    symbol_kinds: &mut HashMap<String, String>,
    module_files: &mut HashMap<String, PathBuf>,
    file_modules: &mut HashMap<PathBuf, Vec<String>>,
    files: &mut HashSet<PathBuf>,
) -> Result<()> {
    if line.trim().is_empty() {
        return Ok(());
    }
    let value: Value = serde_json::from_str(line)?;
    let Some(tag) = value.get("t").and_then(|v| v.as_str()) else {
        return Ok(());
    };
    match tag {
        "SESSION" => {
            span_index.clear();
            symbol_kinds.clear();
            module_files.clear();
            file_modules.clear();
            files.clear();
        }
        "N" => {
            let sym = value.get("sym").and_then(|v| v.as_str()).unwrap_or("");
            let kind = value.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            let file = value.get("file").and_then(|v| v.as_str()).unwrap_or("");
            let lo = value.get("lo").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let hi = value.get("hi").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            if sym.is_empty() || file.is_empty() {
                return Ok(());
            }
            let pb = PathBuf::from(file);
            files.insert(pb.clone());
            symbol_kinds.insert(sym.to_string(), kind.to_string());
            span_index
                .entry(sym.to_string())
                .or_default()
                .entry(pb.clone())
                .or_default()
                .push(SpanRange { lo, hi });
            if kind == "MODULE" {
                module_files.insert(sym.to_string(), pb.clone());
                file_modules.entry(pb).or_default().push(sym.to_string());
            }
        }
        _ => {}
    }
    Ok(())
}

fn build_symbol_catalog(symbol_kinds: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut catalog: Vec<(String, String)> =
        symbol_kinds.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    catalog.sort_by(|a, b| a.0.cmp(&b.0));
    catalog
}

fn dedup_spans(span_index: &mut HashMap<String, HashMap<PathBuf, Vec<SpanRange>>>) {
    for per_file in span_index.values_mut() {
        for spans in per_file.values_mut() {
            spans.sort_by(|a, b| a.lo.cmp(&b.lo));
            spans.dedup_by(|a, b| a.lo == b.lo && a.hi == b.hi);
        }
    }
}

fn read_tlog_index(idx_path: &Path) -> Result<u64> {
    let content = std::fs::read_to_string(idx_path)?;
    let value: Value = serde_json::from_str(&content)?;
    Ok(value
        .get("last_session_offset")
        .and_then(|v| v.as_u64())
        .unwrap_or(0))
}
