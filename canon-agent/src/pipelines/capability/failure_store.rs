use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

use super::dag::TaskGraph;
use super::graph_algo::graph_signature;
use super::TEMPLATE_ROOT;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureEntry {
    pub signature: String,
    #[serde(rename = "type")]
    pub failure_type: String,
    pub node_count: usize,
    pub edge_count: usize,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureLogEntry {
    pub template_hash: String,
    pub failure_type: String,
    pub signature: String,
    pub iteration: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FailureFile {
    pub template_hash: String,
    pub failures: Vec<FailureEntry>,
}

pub struct FailureStore {
    path: PathBuf,
    log_path: PathBuf,
    data: FailureFile,
}

#[derive(Debug, Clone)]
pub struct FailureStats {
    pub total: usize,
    pub cycle: usize,
    pub deadlock: usize,
    pub failure_pattern_rate: f64,
    pub cycle_frequency: f64,
    pub deadlock_rate: f64,
}

#[derive(Debug, Clone)]
pub struct Constraint {
    pub signature: String,
    pub rule: ConstraintRule,
}

#[derive(Debug, Clone)]
pub enum ConstraintRule {
    NoCycle,
    NoUnreachable,
    CapabilityConflict,
    InvalidDependency,
    PatternRewrite { pattern: String, replacement: String },
    SignatureBan,
}

impl FailureStore {
    pub fn load(template_hash: &str) -> Self {
        let dir = Path::new(TEMPLATE_ROOT).join("failures");
        let path = dir.join(format!("{}_failures.json", template_hash));
        let log_path = dir.join("failure_log.json");
        let data = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<FailureFile>(&s).ok())
            .unwrap_or_else(|| FailureFile {
                template_hash: template_hash.to_string(),
                failures: Vec::new(),
            });
        Self { path, log_path, data }
    }

    pub fn contains(&self, signature: &str) -> bool {
        self.data.failures.iter().any(|f| f.signature == signature)
    }

    pub fn failure_count(&self) -> usize {
        self.data.failures.len()
    }

    pub fn stats(&self) -> FailureStats {
        let mut cycle = 0usize;
        let mut deadlock = 0usize;
        for f in &self.data.failures {
            match f.failure_type.as_str() {
                "cycle" => cycle += 1,
                "deadlock" => deadlock += 1,
                _ => {}
            }
        }
        let total = self.data.failures.len();
        let denom = total.max(1) as f64;
        FailureStats {
            total,
            cycle,
            deadlock,
            failure_pattern_rate: (total as f64 / 10.0).min(1.0),
            cycle_frequency: cycle as f64 / denom,
            deadlock_rate: deadlock as f64 / denom,
        }
    }

    pub fn constraints(&self, threshold: usize, max_constraints: usize) -> Vec<Constraint> {
        let mut by_type: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for f in &self.data.failures {
            *by_type.entry(f.failure_type.as_str()).or_insert(0) += 1;
        }
        let mut out = Vec::new();
        for f in &self.data.failures {
            if out.len() >= max_constraints {
                break;
            }
            if let Some(count) = by_type.get(f.failure_type.as_str()) {
                if *count < threshold {
                    continue;
                }
            }
            let rule = match f.failure_type.as_str() {
                "cycle" => ConstraintRule::NoCycle,
                "deadlock" | "blocked" => ConstraintRule::NoUnreachable,
                "invalid_authority" => ConstraintRule::CapabilityConflict,
                "dependency_order" => ConstraintRule::InvalidDependency,
                "verify_loop" => ConstraintRule::PatternRewrite { pattern: "cargo build".to_string(), replacement: "cargo check".to_string() },
                _ => ConstraintRule::SignatureBan,
            };
            out.push(Constraint { signature: f.signature.clone(), rule });
        }
        out
    }

    pub fn record(&mut self, signature: String, failure_type: &str, graph: &TaskGraph, iteration: u64) {
        if self.contains(&signature) {
            return;
        }
        let entry = FailureEntry {
            signature: signature.clone(),
            failure_type: failure_type.to_string(),
            node_count: graph.nodes.len(),
            edge_count: graph.nodes.iter().map(|n| n.deps.len()).sum(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        };
        self.data.failures.push(entry);
        self.persist();
        self.append_log(FailureLogEntry {
            template_hash: self.data.template_hash.clone(),
            failure_type: failure_type.to_string(),
            signature,
            iteration,
        });
    }

    pub fn record_graph(&mut self, failure_type: &str, graph: &TaskGraph, iteration: u64) {
        let signature = graph_signature(graph);
        self.record(signature, failure_type, graph, iteration);
    }

    fn persist(&self) {
        if let Ok(pretty) = serde_json::to_string_pretty(&self.data) {
            let _ = std::fs::create_dir_all(self.path.parent().unwrap_or(Path::new(".")));
            let _ = std::fs::write(&self.path, pretty);
        }
    }

    fn append_log(&self, entry: FailureLogEntry) {
        if let Ok(line) = serde_json::to_string(&entry) {
            let _ = std::fs::create_dir_all(self.log_path.parent().unwrap_or(Path::new(".")));
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.log_path)
                .and_then(|mut f| f.write_all(format!("{}\n", line).as_bytes()));
        }
    }
}
