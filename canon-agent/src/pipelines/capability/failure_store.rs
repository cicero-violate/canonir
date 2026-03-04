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
