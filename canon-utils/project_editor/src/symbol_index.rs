use anyhow::{anyhow, Result};
use canon_types::SpanRange;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
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
    pub fn from_reports(out_dir: &Path) -> Result<Self> {
        let symbols_path = out_dir.join("symbols.json");
        let spans_path = out_dir.join("symbol_spans.jsonl");
        if !symbols_path.exists() || !spans_path.exists() {
            return Err(anyhow!(
                "reports artifacts not found in {}; run canon_reports first",
                out_dir.display()
            ));
        }
        let tlog_offset = symbols_path
            .metadata()
            .map(|m| m.len())
            .unwrap_or(0)
            .saturating_add(spans_path.metadata().map(|m| m.len()).unwrap_or(0));
        let reader = BufReader::new(File::open(&spans_path)?);
        let mut span_index: HashMap<String, HashMap<PathBuf, Vec<SpanRange>>> = HashMap::new();
        let symbol_kinds: HashMap<String, String> = load_symbol_kinds(&symbols_path)?;
        let mut module_files: HashMap<String, PathBuf> = HashMap::new();
        let mut file_modules: HashMap<PathBuf, Vec<String>> = HashMap::new();
        let mut files: HashSet<PathBuf> = HashSet::new();

        for line in reader.lines() {
            let line = line?;
            apply_span_line(
                &line,
                &mut span_index,
                &mut files,
            )?;
        }

        for (symbol_id, kind) in &symbol_kinds {
            if kind != "MODULE" {
                continue;
            }
            if let Some(file_map) = span_index.get(symbol_id) {
                if let Some((file, _)) = file_map.iter().next() {
                    module_files.insert(symbol_id.clone(), file.clone());
                    file_modules.entry(file.clone()).or_default().push(symbol_id.clone());
                }
            }
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
        let out_dir = reports_out_dir(project_root)?;
        Self::from_reports(&out_dir)
    }

    pub fn is_stale(&self, out_dir: &Path) -> bool {
        if self.tlog_offset == 0 {
            return false;
        }
        let symbols_path = out_dir.join("symbols.json");
        let spans_path = out_dir.join("symbol_spans.jsonl");
        let current = symbols_path
            .metadata()
            .map(|m| m.len())
            .unwrap_or(0)
            .saturating_add(spans_path.metadata().map(|m| m.len()).unwrap_or(0));
        current != self.tlog_offset
    }

    pub fn refresh_from_reports(&mut self, out_dir: &Path) -> Result<bool> {
        let symbols_path = out_dir.join("symbols.json");
        let spans_path = out_dir.join("symbol_spans.jsonl");
        let new_len = symbols_path
            .metadata()
            .map(|m| m.len())
            .unwrap_or(0)
            .saturating_add(spans_path.metadata().map(|m| m.len()).unwrap_or(0));
        if new_len == self.tlog_offset {
            return Ok(false);
        }

        let reader = BufReader::new(File::open(&spans_path)?);
        self.span_index.clear();
        self.symbol_kinds = load_symbol_kinds(&symbols_path)?;
        self.module_files.clear();
        self.file_modules.clear();
        self.files.clear();
        for line in reader.lines() {
            let line = line?;
            apply_span_line(
                &line,
                &mut self.span_index,
                &mut self.files,
            )?;
        }
        for (symbol_id, kind) in &self.symbol_kinds {
            if kind != "MODULE" {
                continue;
            }
            if let Some(file_map) = self.span_index.get(symbol_id) {
                if let Some((file, _)) = file_map.iter().next() {
                    self.module_files.insert(symbol_id.clone(), file.clone());
                    self.file_modules.entry(file.clone()).or_default().push(symbol_id.clone());
                }
            }
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

fn apply_span_line(
    line: &str,
    span_index: &mut HashMap<String, HashMap<PathBuf, Vec<SpanRange>>>,
    files: &mut HashSet<PathBuf>,
) -> Result<()> {
    if line.trim().is_empty() {
        return Ok(());
    }
    let value: serde_json::Value = serde_json::from_str(line)?;
    let sym = value.get("symbol_id").and_then(|v| v.as_str()).unwrap_or("");
    let file = value.get("file").and_then(|v| v.as_str()).unwrap_or("");
    let lo = value.get("lo").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let hi = value.get("hi").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    if sym.is_empty() || file.is_empty() {
        return Ok(());
    }
    let pb = PathBuf::from(file);
    files.insert(pb.clone());
    span_index
        .entry(sym.to_string())
        .or_default()
        .entry(pb.clone())
        .or_default()
        .push(SpanRange { lo, hi });
    Ok(())
}

fn reports_out_dir(project_root: &Path) -> Result<PathBuf> {
    if let Ok(out) = std::env::var("CANON_REPORTS_OUT") {
        return Ok(PathBuf::from(out));
    }
    Ok(project_root.join("state/reports_out/kernel"))
}

fn load_symbol_kinds(path: &Path) -> Result<HashMap<String, String>> {
    let data = std::fs::read_to_string(path)?;
    let value: serde_json::Value = serde_json::from_str(&data)?;
    let mut out = HashMap::new();
    if let Some(obj) = value.as_object() {
        for (k, v) in obj {
            if let Some(kind) = v.as_str() {
                out.insert(k.clone(), kind.to_string());
            }
        }
        return Ok(out);
    }
    if let Some(arr) = value.as_array() {
        for item in arr {
            let Some(symbol_id) = item.get("symbol_id").and_then(|v| v.as_str()) else { continue };
            let Some(kind) = item.get("kind").and_then(|v| v.as_str()) else { continue };
            out.insert(symbol_id.to_string(), kind.to_string());
        }
    }
    Ok(out)
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
