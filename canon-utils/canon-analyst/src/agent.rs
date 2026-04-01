use anyhow::Result;
use canon_llm::config::CapabilityConfig;
use canon_llm::endpoint_worker::{llm_worker_new_tabs, llm_worker_send_request};

use crate::python;

const ANALYST_ENDPOINT_ID: &str = "analyst_chatgpt";
const ANALYST_ROLE: &str = "analyst";
const MAX_TURNS: usize = 6;
const MAX_PROMPT_CHARS: usize = 100_000; // stay below the 120k hard cap in endpoint_worker

const SYSTEM_PROMPT: &str = "\
You are a systems analyst for the Canon runtime.\n\n\
Query the tlog with Python:\n\
```python\n\
import json, os\n\
tlog = os.environ[\"CANON_TLOG\"]\n\
# your code\n\
print(result)\n\
```\n\
One Python block per turn. Wait for the result before writing more.\n\
No code block = final answer.";

/// Run the analyst agent loop.
/// Sends the question to the analyst LLM, executes any Python it emits, feeds
/// results back, and repeats until the LLM stops emitting code.
pub async fn run(question: &str, tlog_path: &str) -> Result<()> {
    let config = CapabilityConfig::snapshot_store_load()?;

    let endpoint = config.llm_endpoints.iter().find(|e| e.id == ANALYST_ENDPOINT_ID || e.role.as_deref() == Some(ANALYST_ROLE)).ok_or_else(|| {
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
    let first_prompt = format!("{SYSTEM_PROMPT}\n\n{question}");
    let mut history: Vec<(String, String)> = Vec::new(); // (llm_response, python_result)
    let mut last_python_result = String::new();

    println!("=== Canon Analyst ===");
    println!("Question: {question}\n");

    let mut turn = 0;
    loop {
        turn += 1;
        if turn > MAX_TURNS {
            eprintln!("[analyst] max turns ({MAX_TURNS}) reached");
            break;
        }

        let prompt = if turn == 1 {
            first_prompt.clone()
        } else if endpoint.stateful {
            // Stateful endpoints retain the chat history; send only the latest Python result.
            last_python_result.clone()
        } else {
            build_non_stateful_prompt(&first_prompt, &history)
        };

        let raw = llm_worker_send_request(
            &bridge,
            &endpoint.id,
            &endpoint.url,
            endpoint.stateful,
            &prompt,
            "",        // role_schema embedded in prompt
            None,      // node_id
            None,      // cache_key
            false,     // bust_cache
            true,      // allow_req_id_mismatch
            "analyst", // phase
            &tabs,
            endpoint.max_tabs,
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
                last_python_result = format!("## Python result\n```\n{block}\n```");
                history.push((raw, last_python_result.clone()));
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
    if code.is_empty() {
        None
    } else {
        Some(code)
    }
}

/// Build a bounded prompt for non-stateful endpoints by replaying the
/// first prompt plus (llm, python) pairs, dropping oldest pairs to stay under the limit.
fn build_non_stateful_prompt(first: &str, history: &[(String, String)]) -> String {
    let sep = "\n\n---\n\n";
    let mut parts: Vec<&str> = vec![first];
    for (llm_turn, py_result) in history {
        parts.push(llm_turn.as_str());
        parts.push(py_result.as_str());
    }
    loop {
        let total: usize = parts.iter().map(|s| s.len() + sep.len()).sum();
        if total <= MAX_PROMPT_CHARS || parts.len() <= 1 {
            break;
        }
        if parts.len() >= 3 {
            parts.remove(1);
            parts.remove(1);
        } else {
            break;
        }
    }
    parts.join(sep)
}
