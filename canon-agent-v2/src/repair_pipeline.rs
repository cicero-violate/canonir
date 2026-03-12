use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairRecommendation {
    pub kind: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairPipelineReport {
    pub recommendations: Vec<RepairRecommendation>,
    pub generated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SystemHealthReport {
    graph_drift: f64,
    callgraph_ratio: f64,
    orphan_nodes: usize,
    tlog_growth_rate: f64,
    tlog_ok: bool,
    generated_at: String,
}

pub fn run_repair_pipeline() {
    let health_path = Path::new("/workspace/ai_sandbox/canon/kernel/graph/reports/system_health.json");
    let data = match std::fs::read_to_string(health_path) {
        Ok(data) => data,
        Err(_) => return,
    };
    let health: SystemHealthReport = match serde_json::from_str(&data) {
        Ok(h) => h,
        Err(_) => return,
    };
    let mut recs = Vec::new();
    if health.orphan_nodes > 0 {
        recs.push(RepairRecommendation {
            kind: "fix_module_ownership".to_string(),
            reason: format!("orphan_nodes={}", health.orphan_nodes),
        });
    }
    if health.callgraph_ratio < 0.05 {
        recs.push(RepairRecommendation {
            kind: "boost_callgraph_edges".to_string(),
            reason: format!("callgraph_ratio={:.4}", health.callgraph_ratio),
        });
    }
    if !health.tlog_ok {
        recs.push(RepairRecommendation {
            kind: "validate_tlog_integrity".to_string(),
            reason: "tlog integrity check failed".to_string(),
        });
    }
    if health.graph_drift > 0.2 {
        recs.push(RepairRecommendation {
            kind: "stabilize_graph_capture".to_string(),
            reason: format!("graph_drift={:.4}", health.graph_drift),
        });
    }
    if health.tlog_growth_rate > 0.25 {
        recs.push(RepairRecommendation {
            kind: "tlog_growth_watch".to_string(),
            reason: format!("tlog_growth_rate={:.4}", health.tlog_growth_rate),
        });
    }
    let report = RepairPipelineReport {
        recommendations: recs,
        generated_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    };
    let out_path = Path::new("/workspace/ai_sandbox/canon/agent_logs/repair_pipeline.json");
    if let Ok(payload) = serde_json::to_string_pretty(&report) {
        let _ = std::fs::create_dir_all(out_path.parent().unwrap_or(Path::new(".")));
        let _ = std::fs::write(out_path, payload);
    }
}
