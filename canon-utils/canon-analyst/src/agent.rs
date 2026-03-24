use anyhow::Result;
use canon_llm::config::CapabilityConfig;
use canon_llm::endpoint_worker::{llm_worker_new_tabs, llm_worker_send_request};

use crate::python;

const ANALYST_ENDPOINT_ID: &str = "analyst_chatgpt";
const ANALYST_ROLE: &str = "analyst";
const MAX_TURNS: usize = 6;

const SYSTEM_PROMPT: &str = r#"You are a systems analyst for the Canon multi-agent Rust runtime.

You have read-only access to the system's event log (tlog) via Python.
Use Python code blocks to query and analyse the log. When you have enough
information, write a clear diagnosis and actionable recommendations.

## Tools available to you

To run Python code, output a fenced block exactly like this:

```python
import json, os
tlog = os.environ["CANON_TLOG"]
# ... your analysis code ...
print(result)
```

Rules:
- One Python block per turn. Wait for the result before continuing.
- When you have enough information, write your final analysis with NO python block.
- Keep code focused: extract specific metrics, counts, timings, or error patterns.
- The tlog is newline-delimited JSON. Each line is one event.
- Useful event kinds: LoopObserved, RouteSelected, LoopPlanned, LoopActed,
  LoopVerified, LoopRewarded, CapabilityCompleted, CapabilityFailed, ErrorOccurred.
"#;

/// Run the analyst agent loop.
/// Sends the question + tlog summary to the analyst LLM, executes any Python
/// it emits, feeds results back, and repeats until the LLM stops emitting code.
pub async fn run(question: &str, tlog_summary: &str, tlog_path: &str) -> Result<()> {
    let config = CapabilityConfig::snapshot_store_load()?;

    let endpoint = config
        .llm_endpoints
        .iter()
        .find(|e| e.id == ANALYST_ENDPOINT_ID || e.role.as_deref() == Some(ANALYST_ROLE))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "analyst endpoint '{}' not found in capability_config.toml. \
                 Add [llm.endpoints.analyst] with role=\"analyst\".",
                ANALYST_ENDPOINT_ID
            )
        })?;

    let bridge_addr = std::env::var("CANON_LLM_BRIDGE_ADDR").unwrap_or_else(|_| "127.0.0.1:9100".to_string());
    let addr: std::net::SocketAddr = bridge_addr.parse().unwrap_or_else(|_| "127.0.0.1:9100".parse().unwrap());
    let ws_emitter: std::sync::Arc<std::sync::OnceLock<canon_event::EventEmitterHandle>> = std::sync::Arc::new(std::sync::OnceLock::new());
    let bridge = canon_llm::ws_server::spawn(addr, config.response_timeout_secs, ws_emitter);
    let tabs = llm_worker_new_tabs();

    // Build the initial prompt.
    let mut conversation: Vec<String> = Vec::new();
    let first_prompt = format!(
        "{SYSTEM_PROMPT}\n\n---\n\n## Question\n{question}\n\n---\n\n{tlog_summary}"
    );
    conversation.push(first_prompt.clone());

    println!("=== Canon Analyst ===");
    println!("Question: {question}\n");

    let mut turn = 0;
    loop {
        turn += 1;
        if turn > MAX_TURNS {
            eprintln!("[analyst] max turns ({MAX_TURNS}) reached");
            break;
        }

        let prompt = conversation.join("\n\n---\n\n");
        let raw = llm_worker_send_request(
            &bridge,
            &endpoint.id,
            &endpoint.url,
            endpoint.stateful,
            &prompt,
            "",              // role_schema embedded in prompt
            None,            // node_id
            None,            // cache_key
            false,           // bust_cache
            true,            // allow_req_id_mismatch
            "analyst",       // phase
            &tabs,
            endpoint.max_tabs,
            config.tab_cooldown_ms,
        )
        .await?;

        println!("--- Turn {turn} ---\n{raw}\n");

        // Extract Python block if present.
        match extract_python_block(&raw) {
            Some(code) => {
                println!("[running python...]\n");
                let result = python::run(&code, tlog_path)?;
                let block = result.to_context_block();
                println!("[python result]\n{block}");
                // Feed result back as next turn context.
                conversation.push(raw);
                conversation.push(format!("## Python result\n```\n{block}\n```"));
            }
            None => {
                // No python block = final analysis. Done.
                break;
            }
        }
    }

    Ok(())
}

/// Extract the content of the first ```python ... ``` block from a response.
fn extract_python_block(text: &str) -> Option<String> {
    let start = text.find("```python")?;
    let after = &text[start + 9..]; // skip "```python"
    let end = after.find("```")?;
    let code = after[..end].trim().to_string();
    if code.is_empty() { None } else { Some(code) }
}
