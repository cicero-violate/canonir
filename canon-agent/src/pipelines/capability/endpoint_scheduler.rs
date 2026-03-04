use std::collections::HashMap;

use super::config::CapabilityConfig;

#[derive(Clone)]
pub struct EndpointCtx {
    pub id: String,
    pub url: String,
    pub max_tabs: usize,
}

pub fn role_burst(config: &CapabilityConfig, role: &str) -> usize {
    let role_cfg = config.role_config(role);
    role_cfg.burst.unwrap_or_else(|| config.max_concurrency.max(1))
}

pub async fn select_endpoints_for_role(
    config: &CapabilityConfig,
    role_rr: &tokio::sync::Mutex<HashMap<String, usize>>,
    role: &str,
    burst: usize,
) -> Vec<EndpointCtx> {
    let role_cfg = config.role_config(role);
    let mut weights: Vec<(usize, u32)> = Vec::new();
    let mut total = 0u32;
    for (idx, ep) in config.llm_endpoints.iter().enumerate() {
        if let Some(ep_role) = ep.role.as_deref() {
            if ep_role != role {
                continue;
            }
        }
        let w = role_cfg.weights.get(&ep.id).copied().unwrap_or(0);
        if w > 0 {
            weights.push((idx, w));
            total += w;
        }
    }
    let use_default_weights = total == 0;
    if use_default_weights {
        for (idx, _ep) in config.llm_endpoints.iter().enumerate() {
            if let Some(ep_role) = config.llm_endpoints[idx].role.as_deref() {
                if ep_role != role {
                    continue;
                }
            }
            weights.push((idx, 1));
            total += 1;
        }
    }
    if weights.is_empty() {
        return Vec::new();
    }

    let mut selected = Vec::with_capacity(burst.max(1));
    for _ in 0..burst.max(1) {
        let idx = {
            let mut rr = role_rr.lock().await;
            let entry = rr.entry(role.to_string()).or_insert(0);
            let sel = *entry % (total as usize);
            *entry = entry.wrapping_add(1);
            sel
        };
        let chosen = weights
            .iter()
            .scan(0usize, |acc, &(ep_idx, w)| { *acc += w as usize; Some((*acc, ep_idx)) })
            .find_map(|(acc, ep_idx)| (idx < acc).then_some(ep_idx))
            .unwrap_or(weights[0].0);
        let ep = &config.llm_endpoints[chosen];
        selected.push(EndpointCtx {
            id: ep.id.clone(),
            url: ep.url.clone(),
            max_tabs: ep.max_tabs,
        });
    }
    selected
}
