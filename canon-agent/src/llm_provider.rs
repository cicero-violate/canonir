//! LLM Provider Adapter
//!
//! Responsibility:
//!   Transform AgentCallInput → LLM turn → structured AgentCallOutput.
//!
//! Contract:
//!   - Stateless
//!   - No disk I/O
//!   - No polling
//!   - JSON-only response contract
//!   - WebSocket transport abstracted via WsBridge
//!
//! Pipeline:
//!   Input → PromptBuilder → WsBridge → JsonExtractor → OutputAssembler

use serde_json::Value;

use super::call::{AgentCallInput, AgentCallOutput};
use super::ws_server::{WsBridge, WsBridgeError};

/// ------------------------------------------------------------------------
/// Error Domain
/// ------------------------------------------------------------------------

#[derive(Debug)]
pub enum LlmProviderError {
    Transport(WsBridgeError),
    MissingJsonFence,
    JsonDecodeFailure(String),
}

impl std::fmt::Display for LlmProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(e) => write!(f, "transport error: {e}"),
            Self::MissingJsonFence => write!(f, "no fenced json block found"),
            Self::JsonDecodeFailure(e) => write!(f, "invalid json payload: {e}"),
        }
    }
}

impl std::error::Error for LlmProviderError {}

impl From<WsBridgeError> for LlmProviderError {
    fn from(e: WsBridgeError) -> Self {
        Self::Transport(e)
    }
}

/// ------------------------------------------------------------------------
/// Public Entry Point
/// ------------------------------------------------------------------------

pub async fn call_llm(bridge: &WsBridge, input: &AgentCallInput) -> Result<AgentCallOutput, LlmProviderError> {
    let prompt = PromptBuilder::new(input).build();

    let raw_response = bridge.send_turn(Some(bridge.open_fresh_tab().await?), prompt).await.map_err(LlmProviderError::Transport)?;

    let payload = JsonExtractor::extract(&raw_response)?;

    Ok(OutputAssembler::assemble(input, payload))
}

/// ------------------------------------------------------------------------
/// Prompt Builder
/// ------------------------------------------------------------------------

struct PromptBuilder<'a> {
    input: &'a AgentCallInput,
}

impl<'a> PromptBuilder<'a> {
    fn new(input: &'a AgentCallInput) -> Self {
        Self { input }
    }

    fn build(&self) -> String {
        let stage = format!("{:?}", self.input.stage);
        let ir = self.serialize_ir();
        let predecessors = self.serialize_predecessors();
        let required = required_fields_for_stage(&stage);

        format!(
            "Capability Node Execution\n\
             ------------------------\n\
             Stage: {stage}\n\
             Node: {node}\n\
             Call: {call}\n\
             \n\
             IR Slice:\n\
             ```json\n{ir}\n```\n\
             \n\
             Predecessor Outputs:\n\
             {predecessors}\n\
             \n\
             Required Output:\n\
             Respond with ONE fenced ```json block only.\n\
             Must include fields:\n\
             {required}",
            stage = stage,
            node = self.input.node_id,
            call = self.input.call_id,
            ir = ir,
            predecessors = predecessors,
            required = required,
        )
    }

    fn serialize_ir(&self) -> String {
        serde_json::to_string_pretty(&self.input.ir_slice).unwrap_or_else(|_| "{}".into())
    }

    fn serialize_predecessors(&self) -> String {
        if self.input.predecessor_outputs.is_empty() {
            return "none".into();
        }

        self.input
            .predecessor_outputs
            .iter()
            .map(|p| format!("node={} stage={:?} payload={}", p.node_id, p.stage, serde_json::to_string(&p.payload).unwrap_or_default()))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// ------------------------------------------------------------------------
/// JSON Extraction
/// ------------------------------------------------------------------------

struct JsonExtractor;

impl JsonExtractor {
    pub fn extract(text: &str) -> Result<Value, LlmProviderError> {
        let start = Self::find_open(text)?;
        let content = &text[start..];
        let json_str = Self::slice_json(content)?;
        serde_json::from_str(json_str).map_err(|e| LlmProviderError::JsonDecodeFailure(e.to_string()))
    }

    fn find_open(text: &str) -> Result<usize, LlmProviderError> {
        text.find("```json").or_else(|| text.find("```JSON")).map(|i| i + 7).ok_or(LlmProviderError::MissingJsonFence)
    }

    fn slice_json(text: &str) -> Result<&str, LlmProviderError> {
        let end = text.find("```").ok_or(LlmProviderError::MissingJsonFence)?;
        Ok(text[..end].trim())
    }
}

/// ------------------------------------------------------------------------
/// Output Assembly
/// ------------------------------------------------------------------------

struct OutputAssembler;

impl OutputAssembler {
    fn assemble(input: &AgentCallInput, payload: Value) -> AgentCallOutput {
        let proof_id = payload.get("proof_id").and_then(|v| v.as_str()).map(str::to_owned);

        let emitted_delta_ids = payload.get("emitted_delta_ids").and_then(|v| v.as_array()).map(|arr| arr.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()).unwrap_or_default();

        AgentCallOutput { call_id: input.call_id.clone(), node_id: input.node_id.clone(), payload, proof_id, emitted_delta_ids, stage: input.stage.clone() }
    }
}

/// ------------------------------------------------------------------------
/// Stage Schema
/// ------------------------------------------------------------------------

fn required_fields_for_stage(stage: &str) -> &'static str {
    match stage {
        "Observe" => {
            r#"{
  "observation": "<string>",
  "hottest_modules": ["<module_id>"],
  "issues_found": ["<string>"]
}"#
        }

        "Learn" => {
            r#"{
  "rationale": "<string>",
  "proposed_kind": "<split_module|merge_modules|move_artifact|rename_artifact|add_edge|remove_edge>",
  "target_artifact_id": "<string>",
  "destination_id": "<string|null>"
}"#
        }

        "Decide" => {
            r#"{
  "proof_id": "<string>",
  "verification_notes": "<string>"
}"#
        }

        "Plan" => {
            r#"{
  "decision": "<accept|reject>",
  "admission_id": "<string>",
  "rationale": "<string>"
}"#
        }

        _ => {
            r#"{
  "observation": "<string>",
  "rationale": "<string>"
}"#
        }
    }
}
