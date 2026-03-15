pub use crate::endpoint_worker::{llm_worker_new_tabs, TabManagerHandle};
use crate::llm::{
    llm_client_call_agent_json_with_retry_allow_mismatch,
    llm_client_call_agent_raw_with_retry_allow_mismatch,
};
use crate::ws_server::WsBridge;
use anyhow::Result;
use serde_json::Value;

pub async fn module_call_llm_raw_with_retry_allow_mismatch(
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    stateful: bool,
    prompt: &str,
    role_schema: &str,
    phase: &str,
    node_id: Option<&str>,
    tabs: &TabManagerHandle,
    max_tabs: usize,
    tab_cooldown_ms: u64,
    max_retries: u32,
    delay_secs: u64,
) -> Result<String> {
    llm_client_call_agent_raw_with_retry_allow_mismatch(
        bridge,
        endpoint_id,
        url,
        stateful,
        prompt,
        role_schema,
        phase,
        node_id,
        tabs,
        max_tabs,
        tab_cooldown_ms,
        max_retries,
        delay_secs,
    )
    .await
}

pub async fn module_call_llm_json_with_retry_allow_mismatch(
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    stateful: bool,
    prompt: &str,
    role_schema: &str,
    phase: &str,
    node_id: Option<&str>,
    tabs: &TabManagerHandle,
    max_tabs: usize,
    tab_cooldown_ms: u64,
    max_retries: u32,
    delay_secs: u64,
) -> Result<Value> {
    llm_client_call_agent_json_with_retry_allow_mismatch(
        bridge,
        endpoint_id,
        url,
        stateful,
        prompt,
        role_schema,
        phase,
        node_id,
        tabs,
        max_tabs,
        tab_cooldown_ms,
        max_retries,
        delay_secs,
    )
    .await
}
