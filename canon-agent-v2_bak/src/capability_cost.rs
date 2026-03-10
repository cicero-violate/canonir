use super::capability::PipelineCapability;
use super::dag::ExecutionNode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
const COST_PATH: &str = "/workspace/ai_sandbox/canon/agent_logs/capability_costs.json";
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CapabilityCost {
    pub latency_avg: f64,
    pub failure_rate: f64,
    pub samples: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CapabilityCostCapabilityCostTable {
    pub costs: HashMap<PipelineCapability, CapabilityCost>,
}
impl CapabilityCostCapabilityCostTable {
    pub fn snapshot_store_load() -> Self {
        std::fs::read_to_string(COST_PATH).ok().and_then(|s| serde_json::from_str::<CapabilityCostCapabilityCostTable>(&s).ok()).unwrap_or_default()
    }
    pub fn snapshot_store_save(&self) {
        if let Ok(pretty) = serde_json::to_string_pretty(self) {
            let _ = std::fs::create_dir_all(Path::new(COST_PATH).parent().unwrap_or(Path::new(".")));
            let _ = std::fs::write(COST_PATH, pretty);
        }
    }
    pub fn update(&mut self, cap: PipelineCapability, latency_ms: f64, success: bool, decay: f64) {
        let decay = decay.clamp(0.0, 1.0);
        let entry = self.costs.entry(cap).or_default();
        let failure = if success { 0.0 } else { 1.0 };
        if entry.samples == 0 {
            entry.latency_avg = latency_ms;
            entry.failure_rate = failure;
        } else {
            entry.latency_avg = (1.0 - decay) * entry.latency_avg + decay * latency_ms;
            entry.failure_rate = (1.0 - decay) * entry.failure_rate + decay * failure;
        }
        entry.samples = entry.samples.saturating_add(1);
    }
    pub fn node_cost(&self, caps: &[PipelineCapability], latency_weight: f64, failure_weight: f64) -> f64 {
        caps.iter().map(|c| self.costs.get(c)).flatten().map(|c| latency_weight * c.latency_avg + failure_weight * c.failure_rate).sum()
    }
    pub fn avg_latency(&self) -> f64 {
        let mut total = 0.0;
        let mut count = 0u64;
        for c in self.costs.values() {
            if c.samples > 0 {
                total += c.latency_avg;
                count += 1;
            }
        }
        if count == 0 {
            0.0
        } else {
            total / count as f64
        }
    }
    pub fn avg_failure(&self) -> f64 {
        let mut total = 0.0;
        let mut count = 0u64;
        for c in self.costs.values() {
            if c.samples > 0 {
                total += c.failure_rate;
                count += 1;
            }
        }
        if count == 0 {
            0.0
        } else {
            total / count as f64
        }
    }
    pub fn summary(&self, max_entries: usize, latency_weight: f64, failure_weight: f64) -> String {
        let mut entries: Vec<(PipelineCapability, f64, f64, f64)> = self
            .costs
            .iter()
            .map(|(cap, cost)| {
                let score = latency_weight * cost.latency_avg + failure_weight * cost.failure_rate;
                (*cap, cost.latency_avg, cost.failure_rate, score)
            })
            .collect();
        entries.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
        entries.truncate(max_entries);
        let mut lines = Vec::new();
        for (cap, lat, fail, score) in entries {
            lines.push(format!("{:?}: latency_ms={:.1} fail_rate={:.2} cost={:.2}", cap, lat, fail, score));
        }
        lines.join("\n")
    }
}
pub fn capability_cost_apply_node_cost_update(
    table: &mut CapabilityCostCapabilityCostTable, node: &ExecutionNode, latency_ms: f64, success: bool, decay: f64, latency_weight: f64, failure_weight: f64,
) -> f64 {
    for cap in &node.required_capabilities {
        table.update(*cap, latency_ms, success, decay);
    }
    table.node_cost(&node.required_capabilities, latency_weight, failure_weight)
}
