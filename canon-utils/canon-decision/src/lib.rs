use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RouteKind {
    Scan,
    Shape,
    Execute,
    Validate,
    Conclude,
}

impl RouteKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RouteKind::Scan => "scan",
            RouteKind::Shape => "shape",
            RouteKind::Execute => "execute",
            RouteKind::Validate => "validate",
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
    pub last_tool_result: Option<Value>,
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
- scan: gather more context; do not plan or execute changes.\n\
- shape: plan the work; produce planned actions for later execution.\n\
- execute: run the planned actions. Never pick execute if there are zero planned/queued actions — pick shape instead.\n\
- validate: verify recent actions or state after execution.\n\
- conclude: finish when the goal is satisfied.";

    let recent =
        if input.journal.is_empty() { "(none)".to_string() } else { input.journal.iter().rev().take(8).map(|line| format!("- [{}] {}", line.lane, line.summary)).collect::<Vec<_>>().join("\n") };
    let last_tool_result = input
        .last_tool_result
        .as_ref()
        .map(|value| {
            let mut text = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
            if text.len() > 2000 {
                text.truncate(2000);
                text.push_str("\n...<truncated>");
            }
            text
        })
        .unwrap_or_else(|| "(none)".to_string());

    format!(
        "You are the runtime route selector. Pick exactly one next route.\n\n\
Mission:\n{mission}\n\n\
Snapshot:\n{snapshot}\n\n\
Last Tool Result:\n{last_tool_result}\n\n\
Recent Journal:\n{recent}\n\n\
Allowed Routes: {routes}\n\n\
Route Descriptions:\n{route_descriptions}\n\n\
Hard rule: If there is no queued/planned work, do NOT pick \"execute\" — pick \"shape\" to create a plan first.\n\n\
Return exactly one JSON object in one fenced ```json code block with schema:\n\
{{\n  \"route\": \"scan|shape|execute|validate|conclude\",\n  \"rationale\": \"short reason\",\n  \"confidence\": 0.0,\n  \"signals\": {{}}\n}}\n\
No prose outside the code block.",
        mission = mission_summary,
        snapshot = input.snapshot,
        last_tool_result = last_tool_result,
        recent = recent,
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
