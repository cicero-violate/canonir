use crate::smt::reachability::SmtReachabilityEntry;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
pub struct RepairSurfaceSmtEntry {
    pub node_id: u32,
    pub smt_reachable: bool,
    pub rank_smt: usize,
}

pub fn build_repair_surface_smt(repair_surface: &Value, reachability: &[SmtReachabilityEntry]) -> Value {
    let mut reach_map: BTreeMap<u32, bool> = BTreeMap::new();
    for entry in reachability {
        reach_map.insert(entry.node_id, entry.smt_reachable);
    }

    let top_k = repair_surface.get("top_k").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let mut out = Vec::new();
    for entry in top_k {
        let node_id = entry.get("node_id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        if node_id == 0 {
            continue;
        }
        let error_count = entry.get("error_count").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let smt_reachable = reach_map.get(&node_id).copied().unwrap_or(false);
        let rank_smt = if smt_reachable { error_count } else { 0 };
        out.push(RepairSurfaceSmtEntry { node_id, smt_reachable, rank_smt });
    }
    serde_json::json!({
        "top_k": out,
        "count": out.len()
    })
}
