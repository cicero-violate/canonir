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
    Decompose,
}

impl RouteKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RouteKind::Observe => "observe",
            RouteKind::Plan => "plan",
            RouteKind::Act => "act",
            RouteKind::Verify => "verify",
            RouteKind::Conclude => "conclude",
            RouteKind::Decompose => "decompose",
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
    pub semantic_context: String,
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

    // Only describe routes that are actually available.
    let route_descriptions = input
        .open_routes
        .iter()
        .map(|r| match r {
            RouteKind::Observe => "- observe: gather context; do not plan or execute.",
            RouteKind::Plan => "- plan: ask the LLM to plan the next action.",
            RouteKind::Act => "- act: execute the queued planned actions. Only when planned_pending>0.",
            RouteKind::Verify => "- verify: run cargo check after execution.",
            RouteKind::Conclude => "- conclude: terminate. Only when finish_ready=true.",
            RouteKind::Decompose => "- decompose: split goal into parallel sub-tasks.",
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Strip internal tracking IDs; skip empty llm.plan entries.
    const STRIP_KEYS: &[&str] = &["tool_call_id", "tool_result_id", "node_id", "llm_request_id", "request_id"];
    let recent_tool_results_text = {
        let entries: Vec<String> = input
            .recent_tool_results
            .iter()
            .enumerate()
            .filter_map(|(i, value)| {
                if value.get("kind").and_then(|v| v.as_str()) == Some("llm.plan") {
                    return None;
                }
                let slim: serde_json::Map<String, serde_json::Value> =
                    value.as_object().map(|m| m.iter().filter(|(k, _)| !STRIP_KEYS.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default();
                let mut text = serde_json::to_string_pretty(&serde_json::Value::Object(slim)).unwrap_or_else(|_| value.to_string());
                if text.len() > 600 {
                    text.truncate(600);
                    text.push_str("\n...<truncated>");
                }
                Some(format!("[{}] {}", i + 1, text))
            })
            .collect();
        if entries.is_empty() {
            "(none)".to_string()
        } else {
            entries.join("\n")
        }
    };

    format!(
        "Mission: {mission}\n\n\
Snapshot:\n{snapshot}\n\n\
Semantic Context:\n{semantic_context}\n\n\
Recent Actions:\n{recent_tool_results_text}\n\n\
Routes: {routes}\n\
RULE: finish_ready=true → conclude. planned_pending=0 → do not select act.\n\n\
{route_descriptions}\n\n\
Respond with one fenced ```json block:\n\
{{\n  \"route\": \"{routes}\",\n  \"rationale\": \"...\",\n  \"confidence\": 0.0\n}}",
        mission = mission_summary,
        snapshot = input.snapshot,
        semantic_context = input.semantic_context,
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
