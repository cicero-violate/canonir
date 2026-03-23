use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::Metrics;

pub struct MetricsStorage {
    metrics_path: PathBuf,
    goodness_path: PathBuf,
}

impl MetricsStorage {
    pub fn new(root: &Path) -> Self {
        Self {
            metrics_path: root.join("metrics.log"),
            goodness_path: root.join("goodness.log"),
        }
    }

    pub fn append_metrics(&self, tick: u64, m: &Metrics) {
        if let Ok(mut f) = open_append(&self.metrics_path) {
            let _ = writeln!(f, "{}", serde_json::json!({ "tick": tick, "metrics": m }));
        }
    }

    pub fn append_goodness(&self, tick: u64, g: f32, delta: f32) {
        if let Ok(mut f) = open_append(&self.goodness_path) {
            let _ = writeln!(f, "{}", serde_json::json!({ "tick": tick, "g": g, "delta_g": delta, "ts_ms": now_ms() }));
        }
    }
}

fn open_append(path: &Path) -> std::io::Result<std::fs::File> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    OpenOptions::new().create(true).append(true).open(path)
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

