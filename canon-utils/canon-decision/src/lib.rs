use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RouteKind {
    Observe,
    Plan,
    Act,
    Verify,
    Conclude,
}

impl RouteKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RouteKind::Observe => "observe",
            RouteKind::Plan => "plan",
            RouteKind::Act => "act",
            RouteKind::Verify => "verify",
            RouteKind::Conclude => "conclude",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalLine {
    pub lane: String,
    pub summary: String,
    #[serde(default)]
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingInput {
    pub mission: String,
    pub snapshot: String,
    #[serde(default)]
    pub recent_tool_results: Vec<Value>,
    #[serde(default)]
    pub journal: Vec<JournalLine>,
    pub open_routes: Vec<RouteKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteSelection {
    pub route: RouteKind,
    pub rationale: String,
    #[serde(default)]
    pub confidence: Option<f32>,
    #[serde(default)]
    pub signals: Value,
}

pub fn compose_routing_prompt(input: &RoutingInput) -> String {
    let routes = input.open_routes.iter().map(|r| r.as_str()).collect::<Vec<_>>().join(", ");
    // Include only the first line of the mission to avoid re-sending the full goal on every tick.
    let mission_summary = input.mission.lines().next().unwrap_or("(unknown goal)").trim();
    let route_descriptions = "\
- observe: gather more context; do not plan or execute changes.\n\
- plan: produce planned actions for later execution (calls plan::execute_trigger).\n\
- act: run the planned actions (calls act::execute_dispatch). Never pick act if there are zero queued actions — pick plan instead.\n\
- verify: run cargo check and verify state after execution (calls verify::execute).\n\
- conclude: select this when finish_ready=true in the snapshot. This terminates the loop. Only select conclude when the workspace is verified and the goal requirements are met.";

    let recent_tool_results_text = if input.recent_tool_results.is_empty() {
        "(none)".to_string()
    } else {
        input.recent_tool_results.iter().enumerate().map(|(i, value)| {
            let mut text = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
            if text.len() > 800 {
                text.truncate(800);
                text.push_str("\n...<truncated>");
            }
            format!("[{}] {}", i + 1, text)
        }).collect::<Vec<_>>().join("\n")
    };

    format!(
        "You are the runtime route selector. Pick exactly one next route.\n\n\
Mission:\n{mission}\n\n\
Snapshot:\n{snapshot}\n\n\
Recent Tool Results:\n{recent_tool_results_text}\n\n\
Allowed Routes: {routes}


ROUTING RULE: If finish_ready=true, you MUST select conclude. Do not select plan or act when finish_ready=true.

ROUTING RULE: Never choose execute if planned_pending=0 (nothing to run). Prefer shape.

Route Descriptions:
{route_descriptions}

Return exactly one JSON object in one fenced ```json code block with schema:\n\
{{\n\
  \"route\": \"observe|plan|act|verify|conclude\",\n\
  \"rationale\": \"short reason\",\n\
  \"confidence\": 0.0,\n\
  \"signals\": {{\n\
    \"goal_alignment_score\": 0.0,\n\
    \"confidence\": 0.0,\n\
    \"task_completion_likelihood\": 0.0,\n\
    \"error_likelihood\": 0.0,\n\
    \"plan_validity\": 0.0,\n\
    \"state_consistency\": 0.0,\n\
    \"action_effectiveness\": 0.0,\n\
    \"progress_score\": 0.0,\n\
    \"blocking_severity\": 0.0,\n\
    \"ambiguity_level\": 0.0,\n\
    \"context_completeness\": 0.0,\n\
    \"plan_optimality\": 0.0,\n\
    \"redundancy_level\": 0.0,\n\
    \"recovery_difficulty\": 0.0,\n\
    \"tool_reliability\": 0.0,\n\
    \"execution_risk\": 0.0,\n\
    \"verification_coverage\": 0.0,\n\
    \"change_impact\": 0.0,\n\
    \"stability_score\": 0.0,\n\
    \"iteration_efficiency\": 0.0,\n\
    \"novelty_score\": 0.0,\n\
    \"dependency_health\": 0.0,\n\
    \"resource_efficiency\": 0.0,\n\
    \"termination_readiness\": 0.0\n\
  }}\n\
}}\n\
No prose outside the code block.",
        mission = mission_summary,
        snapshot = input.snapshot,
        routes = routes,
        route_descriptions = route_descriptions,
    )
}

pub fn parse_route_selection(raw: &str, allowed: &[RouteKind]) -> Result<RouteSelection> {
    let mut last_err: Option<anyhow::Error> = None;
    for parsed in parse_json_candidates(raw) {
        match serde_json::from_value::<RouteSelection>(parsed) {
            Ok(mut selection) => {
                if selection.rationale.trim().is_empty() {
                    last_err = Some(anyhow!("route selection rationale is empty"));
                    continue;
                }

                if !allowed.contains(&selection.route) {
                    last_err = Some(anyhow!("route '{}' is not in current allowed set", selection.route.as_str()));
                    continue;
                }

                if let Some(confidence) = selection.confidence {
                    if !(0.0..=1.0).contains(&confidence) {
                        last_err = Some(anyhow!("confidence must be in [0.0, 1.0]"));
                        continue;
                    }
                }

                if selection.signals.is_null() {
                    selection.signals = Value::Object(serde_json::Map::new());
                }

                return Ok(selection);
            }
            Err(err) => {
                last_err = Some(err.into());
            }
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow!("route selection is not valid JSON")))
}

fn parse_json_candidates(raw: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let trimmed = raw.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        out.push(value);
    }
    for block in extract_fenced_blocks(trimmed) {
        if let Ok(value) = serde_json::from_str::<Value>(&block) {
            out.push(value);
        }
    }
    out
}

fn extract_fenced_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut in_fence = false;
    let mut current = String::new();

    for line in text.lines() {
        let trimmed = line.trim_start();
        if !in_fence {
            if trimmed.starts_with("```") {
                in_fence = true;
                current.clear();
            }
            continue;
        }

        if trimmed.starts_with("```") {
            blocks.push(current.trim().to_string());
            in_fence = false;
            current.clear();
            continue;
        }

        current.push_str(line);
        current.push('\n');
    }

    blocks
}
