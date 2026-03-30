use anyhow::{anyhow, Result};
use canon_analysis::{graph_import_bindings, load_latest_workspace_graph_artifact};
use canon_ir::CanonNodeKind;
use canon_types::{ReportLayout, SpanRange};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
//
pub struct SymbolIndex {
    span_index: HashMap<String, HashMap<PathBuf, Vec<SpanRange>>>,
    symbol_kinds: HashMap<String, String>,
    symbol_catalog: Vec<(String, String)>,
    alias_targets: HashMap<String, String>,
    pub normalized_sources: HashMap<PathBuf, String>,
    tlog_offset: u64,
    module_files: HashMap<String, PathBuf>,
    file_modules: HashMap<PathBuf, Vec<String>>,
    files: HashSet<PathBuf>,
    uses_crate_prefix: bool,
}

impl SymbolIndex {
    pub fn from_reports(out_dir: &Path) -> Result<Self> {
        let graph_dir = if out_dir.file_name().and_then(|s| s.to_str()) == Some("graph") { out_dir.to_path_buf() } else { ReportLayout::from_crate_root(out_dir.to_path_buf()).graph_dir() };
        let symbols_path = graph_dir.join("symbols.json");
        let spans_path = graph_dir.join("symbol_spans.jsonl");
        if !symbols_path.exists() || !spans_path.exists() {
            return Err(anyhow!("reports artifacts not found in {}; run reports generation first", out_dir.display()));
        }
        let tlog_offset = symbols_path.metadata().map(|m| m.len()).unwrap_or(0).saturating_add(spans_path.metadata().map(|m| m.len()).unwrap_or(0));
        let reader = BufReader::new(File::open(&spans_path)?);
        let mut span_index: HashMap<String, HashMap<PathBuf, Vec<SpanRange>>> = HashMap::new();
        let symbol_kinds: HashMap<String, String> = load_symbol_kinds(&symbols_path)?;
        let mut module_files: HashMap<String, PathBuf> = HashMap::new();
        let mut file_modules: HashMap<PathBuf, Vec<String>> = HashMap::new();
        let mut files: HashSet<PathBuf> = HashSet::new();

        for line in reader.lines() {
            let line = line?;
            apply_span_line(&line, &mut span_index, &mut files)?;
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

        Ok(Self { span_index, symbol_kinds, symbol_catalog, alias_targets: HashMap::new(), normalized_sources: HashMap::new(), tlog_offset, module_files, file_modules, files, uses_crate_prefix })
    }

    pub fn build(project_root: &Path) -> Result<Self> {
        if let Ok(index) = Self::from_graph_artifact(project_root) {
            return Ok(index);
        }
        let out_dir = reports_out_dir(project_root)?;
        Self::from_reports(&out_dir)
    }

    pub fn is_stale(&self, out_dir: &Path) -> bool {
        if self.tlog_offset == 0 {
            return false;
        }
        let graph_dir = if out_dir.file_name().and_then(|s| s.to_str()) == Some("graph") { out_dir.to_path_buf() } else { ReportLayout::from_crate_root(out_dir.to_path_buf()).graph_dir() };
        let symbols_path = graph_dir.join("symbols.json");
        let spans_path = graph_dir.join("symbol_spans.jsonl");
        let current = symbols_path.metadata().map(|m| m.len()).unwrap_or(0).saturating_add(spans_path.metadata().map(|m| m.len()).unwrap_or(0));
        current != self.tlog_offset
    }

    pub fn refresh_from_reports(&mut self, out_dir: &Path) -> Result<bool> {
        let graph_dir = if out_dir.file_name().and_then(|s| s.to_str()) == Some("graph") { out_dir.to_path_buf() } else { ReportLayout::from_crate_root(out_dir.to_path_buf()).graph_dir() };
        let symbols_path = graph_dir.join("symbols.json");
        let spans_path = graph_dir.join("symbol_spans.jsonl");
        let new_len = symbols_path.metadata().map(|m| m.len()).unwrap_or(0).saturating_add(spans_path.metadata().map(|m| m.len()).unwrap_or(0));
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
            apply_span_line(&line, &mut self.span_index, &mut self.files)?;
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
        let canonical = self.resolve_symbol_id(symbol_id);
        self.span_index.get(canonical.as_str())
    }

    pub fn normalized_source(&self, path: &PathBuf) -> Option<&String> {
        self.normalized_sources.get(path)
    }

    pub fn symbol_catalog(&self) -> Vec<(String, String)> {
        self.symbol_catalog.clone()
    }

    pub fn symbol_ids(&self) -> Vec<String> {
        self.symbol_catalog.iter().map(|(id, _)| split_module_segments(id).join("::")).collect()
    }

    pub fn symbol_kind(&self, symbol_id: &str) -> Option<&str> {
        self.symbol_kinds.get(symbol_id).map(|value| value.as_str())
    }

    pub fn contains(&self, symbol_id: &str) -> bool {
        self.symbol_kinds.contains_key(symbol_id) || self.alias_targets.contains_key(symbol_id)
    }

    pub fn resolve_symbol_id(&self, symbol_id: &str) -> String {
        self.alias_targets.get(symbol_id).cloned().unwrap_or_else(|| symbol_id.to_string())
    }

    pub fn alias_targets(&self) -> &HashMap<String, String> {
        &self.alias_targets
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

    pub fn validate_invariants(&self) -> Result<()> {
        for (symbol_id, _) in &self.symbol_catalog {
            if !self.symbol_kinds.contains_key(symbol_id) {
                return Err(anyhow!("index invariant: catalog symbol missing kind: {symbol_id}"));
            }
        }
        let mut seen = HashSet::new();
        for (symbol_id, _) in &self.symbol_catalog {
            if !seen.insert(symbol_id) {
                return Err(anyhow!("index invariant: duplicate canonical symbol id: {symbol_id}"));
            }
        }
        for symbol_id in self.span_index.keys() {
            if !self.symbol_kinds.contains_key(symbol_id) {
                return Err(anyhow!("index invariant: spans reference unresolved symbol: {symbol_id}"));
            }
        }
        for (symbol_id, kind) in &self.symbol_kinds {
            if kind != "module" && kind != "MODULE" && !self.span_index.contains_key(symbol_id) {
                return Err(anyhow!("index invariant: reachable symbol missing reference coverage: {symbol_id}"));
            }
        }
        for (alias, target) in &self.alias_targets {
            if !self.symbol_kinds.contains_key(target) {
                return Err(anyhow!("index invariant: alias target missing from catalog: {alias} -> {target}"));
            }
        }
        Ok(())
    }
}

impl SymbolIndex {
    fn from_graph_artifact(project_root: &Path) -> Result<Self> {
        let (_summary, ir) = load_latest_workspace_graph_artifact(project_root)?;
        let mut span_index: HashMap<String, HashMap<PathBuf, Vec<SpanRange>>> = HashMap::new();
        let mut symbol_kinds = HashMap::new();
        let mut symbol_catalog = Vec::new();
        let mut alias_targets = HashMap::new();
        let mut module_files = HashMap::new();
        let mut file_modules: HashMap<PathBuf, Vec<String>> = HashMap::new();
        let mut files = HashSet::new();
        let mut normalized_sources = HashMap::new();
        let source_root = project_root.join("src");

        for entry in WalkDir::new(project_root).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            if path.components().any(|component| component.as_os_str() == "target") {
                continue;
            }
            let path_buf = path.to_path_buf();
            files.insert(path_buf.clone());
            if let Ok(source) = std::fs::read_to_string(&path_buf) {
                normalized_sources.insert(path_buf.clone(), source);
            }
            let module_path = module_path_from_file_guess(project_root, &source_root, &path_buf)?;
            module_files.insert(module_path.clone(), path_buf.clone());
            file_modules.entry(path_buf).or_default().push(module_path);
        }

        let module_membership = graph_module_membership(&ir);
        for node in &ir.nodes {
            let Some((name, kind)) = graph_symbol_identity(&ir, &node.kind) else {
                continue;
            };
            let module_path = module_membership.get(&node.id.0).cloned().unwrap_or_else(|| "crate".to_string());
            let symbol_id = format!("{module_path}::{name}");
            if symbol_kinds.contains_key(&symbol_id) {
                return Err(anyhow!("graph index invariant: duplicate canonical symbol id: {symbol_id}"));
            }
            symbol_kinds.insert(symbol_id.clone(), kind.clone());
            symbol_catalog.push((symbol_id, kind));
        }
        symbol_catalog.sort_by(|a, b| a.0.cmp(&b.0));
        build_source_spans(&symbol_catalog, &normalized_sources, &mut span_index);
        for binding in graph_import_bindings(project_root)? {
            if symbol_kinds.contains_key(&binding.target_path) {
                alias_targets.insert(binding.visible_path, binding.target_path);
            }
        }
        dedup_spans(&mut span_index);

        Ok(Self { span_index, symbol_kinds, symbol_catalog, alias_targets, normalized_sources, tlog_offset: 0, module_files, file_modules, files, uses_crate_prefix: true })
    }
}

fn build_source_spans(symbol_catalog: &[(String, String)], normalized_sources: &HashMap<PathBuf, String>, span_index: &mut HashMap<String, HashMap<PathBuf, Vec<SpanRange>>>) {
    for (symbol_id, kind) in symbol_catalog {
        if kind == "module" || kind == "MODULE" {
            continue;
        }
        let ident = symbol_id.rsplit("::").next().unwrap_or(symbol_id.as_str());
        for (path, source) in normalized_sources {
            let mut offset = 0usize;
            while let Some(found) = source[offset..].find(ident) {
                let lo = offset + found;
                let hi = lo + ident.len();
                let left_ok = lo == 0 || !source[..lo].chars().next_back().is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_');
                let right_ok = hi == source.len() || !source[hi..].chars().next().is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_');
                if left_ok && right_ok {
                    span_index.entry(symbol_id.clone()).or_default().entry(path.clone()).or_default().push(SpanRange { lo: lo as u32, hi: hi as u32 });
                }
                offset = hi;
            }
        }
    }
}

fn module_path_from_file_guess(project_root: &Path, source_root: &Path, file: &Path) -> Result<String> {
    let root = if file.starts_with(source_root) { source_root } else { project_root };
    let rel = file.strip_prefix(root).unwrap_or(file);
    let mut components: Vec<String> = rel.components().filter_map(|c| c.as_os_str().to_str().map(|s| s.to_string())).collect();
    if components.is_empty() {
        return Err(anyhow!("cannot derive module path for {}", file.display()));
    }
    let filename = components.pop().unwrap();
    let module_segments = if filename == "lib.rs" || filename == "main.rs" || filename == "mod.rs" {
        components
    } else {
        let stem = filename.trim_end_matches(".rs").to_string();
        let mut segs = components;
        segs.push(stem);
        segs
    };
    let mut path = String::from("crate");
    for segment in module_segments {
        if !segment.is_empty() {
            path.push_str("::");
            path.push_str(&segment);
        }
    }
    Ok(path)
}

fn graph_module_membership(ir: &canon_ir::CanonIR) -> HashMap<u32, String> {
    use canon_ir::NodeId;
    let mut membership = HashMap::new();
    for node in &ir.nodes {
        let CanonNodeKind::Module { path_id, .. } = &node.kind else {
            continue;
        };
        let module_path = ir.lookup_path(*path_id).to_string();
        for (dst, _) in ir.module_graph.neighbours(NodeId(node.id.0)) {
            membership.entry(dst.0).or_insert_with(|| module_path.clone());
        }
    }
    membership
}

fn graph_symbol_identity(ir: &canon_ir::CanonIR, kind: &CanonNodeKind) -> Option<(String, String)> {
    match kind {
        CanonNodeKind::Struct { name_id, .. } => Some((ir.lookup_name(*name_id).to_string(), "struct".to_string())),
        CanonNodeKind::Enum { name_id, .. } => Some((ir.lookup_name(*name_id).to_string(), "enum".to_string())),
        CanonNodeKind::Trait { name_id, .. } => Some((ir.lookup_name(*name_id).to_string(), "trait".to_string())),
        CanonNodeKind::AssocType { name_id, .. } => Some((ir.lookup_name(*name_id).to_string(), "type".to_string())),
        CanonNodeKind::AssocConst { name_id, .. } => Some((ir.lookup_name(*name_id).to_string(), "const".to_string())),
        CanonNodeKind::Fn { name_id, .. } => Some((ir.lookup_name(*name_id).to_string(), "fn".to_string())),
        CanonNodeKind::Module { path_id, .. } => Some((ir.lookup_path(*path_id).to_string(), "module".to_string())),
        _ => None,
    }
}

fn split_module_segments(path: &str) -> Vec<&str> {
    path.split("::").filter(|s| !s.is_empty()).collect()
}

fn apply_span_line(line: &str, span_index: &mut HashMap<String, HashMap<PathBuf, Vec<SpanRange>>>, files: &mut HashSet<PathBuf>) -> Result<()> {
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
    span_index.entry(sym.to_string()).or_default().entry(pb.clone()).or_default().push(SpanRange { lo, hi });
    Ok(())
}

fn reports_out_dir(project_root: &Path) -> Result<PathBuf> {
    if let Ok(out) = std::env::var("CANON_REPORTS_OUT") {
        return Ok(PathBuf::from(out));
    }
    Ok(project_root.join("state/reports_out/crates/unknown"))
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
    let mut catalog: Vec<(String, String)> = symbol_kinds.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
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
