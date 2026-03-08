use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

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
        let spans_path = project_root.join("analysis").join("spans.jsonl");
        if !spans_path.exists() {
            return Err(anyhow!(
                "missing spans.jsonl at {}; run cargo check to generate analysis/",
                spans_path.display()
            ));
        }

        let (mut span_index, symbol_kinds, normalized_sources, saw_done) =
            load_spans_from_file(&spans_path)?;
        if symbol_kinds.is_empty() {
            return Err(anyhow!("span collector produced no output"));
        }

        if !saw_done {
            return Err(anyhow!("span collector did not finish writing spans"));
        }

        let mut symbol_catalog: Vec<(String, String)> = symbol_kinds
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        symbol_catalog.sort_by(|a, b| a.0.cmp(&b.0));

        for per_file in span_index.values_mut() {
            for spans in per_file.values_mut() {
                spans.sort_by(|a, b| a.lo.cmp(&b.lo));
                spans.dedup_by(|a, b| a.lo == b.lo && a.hi == b.hi);
            }
        }

        Ok(Self {
            span_index,
            symbol_kinds,
            symbol_catalog,
            normalized_sources,
        })
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
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub fn symbol_kind(&self, symbol_id: &str) -> Option<&str> {
        self.symbol_kinds
            .get(symbol_id)
            .map(|value| value.as_str())
    }
}

fn load_spans_from_file(
    path: &Path,
) -> Result<(
    HashMap<String, HashMap<PathBuf, Vec<SpanRange>>>,
    HashMap<String, String>,
    HashMap<PathBuf, String>,
    bool,
)> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let mut span_index: HashMap<String, HashMap<PathBuf, Vec<SpanRange>>> = HashMap::new();
    let mut symbol_kinds: HashMap<String, String> = HashMap::new();
    let mut normalized_sources: HashMap<PathBuf, String> = HashMap::new();
    let mut saw_done = false;

    while reader.read_line(&mut line)? > 0 {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            line.clear();
            continue;
        }
        let value: Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(_) => {
                line.clear();
                continue;
            }
        };
        if let Some(kind) = value.get("type").and_then(|v| v.as_str()) {
            if kind == "done" {
                saw_done = true;
            } else if kind == "source" {
                if let (Some(file), Some(src)) = (
                    value.get("file").and_then(|v| v.as_str()),
                    value.get("src").and_then(|v| v.as_str()),
                ) {
                    normalized_sources.insert(PathBuf::from(file), src.to_string());
                }
            }
            line.clear();
            continue;
        }
        let symbol_id = match value.get("symbol_id").and_then(|v| v.as_str()) {
            Some(value) => value,
            None => {
                line.clear();
                continue;
            }
        };
        let kind = value
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let file = match value.get("file").and_then(|v| v.as_str()) {
            Some(value) => value,
            None => {
                line.clear();
                continue;
            }
        };
        let lo = value.get("lo").and_then(|v| v.as_u64()).unwrap_or(0);
        let hi = value.get("hi").and_then(|v| v.as_u64()).unwrap_or(0);

        symbol_kinds
            .entry(symbol_id.to_string())
            .or_insert_with(|| kind.to_string());
        span_index
            .entry(symbol_id.to_string())
            .or_default()
            .entry(PathBuf::from(file))
            .or_default()
            .push(SpanRange {
                lo: lo as usize,
                hi: hi as usize,
            });

        line.clear();
    }

    Ok((span_index, symbol_kinds, normalized_sources, saw_done))
}
