use anyhow::{anyhow, bail, Context, Result};
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
    let routes = input
        .open_routes
        .iter()
        .map(|r| r.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    let recent = if input.journal.is_empty() {
        "(none)".to_string()
    } else {
        input
            .journal
            .iter()
            .rev()
            .take(8)
            .map(|line| format!("- [{}] {}", line.lane, line.summary))
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "You are the runtime route selector. Pick exactly one next route.\n\n\
Mission:\n{mission}\n\n\
Snapshot:\n{snapshot}\n\n\
Recent Journal:\n{recent}\n\n\
Allowed Routes: {routes}\n\n\
Return JSON only with schema:\n\
{{\n  \"route\": \"scan|shape|execute|validate|conclude\",\n  \"rationale\": \"short reason\",\n  \"confidence\": 0.0,\n  \"signals\": {{}}\n}}\n\
No markdown. No extra text.",
        mission = input.mission,
        snapshot = input.snapshot,
        recent = recent,
        routes = routes,
    )
}

pub fn parse_route_selection(raw: &str, allowed: &[RouteKind]) -> Result<RouteSelection> {
    let parsed = parse_json(raw).context("route selection is not valid JSON")?;
    let mut selection: RouteSelection =
        serde_json::from_value(parsed).context("route selection schema mismatch")?;

    if selection.rationale.trim().is_empty() {
        bail!("route selection rationale is empty");
    }

    if !allowed.contains(&selection.route) {
        return Err(anyhow!(
            "route '{}' is not in current allowed set",
            selection.route.as_str()
        ));
    }

    if let Some(confidence) = selection.confidence {
        if !(0.0..=1.0).contains(&confidence) {
            bail!("confidence must be in [0.0, 1.0]");
        }
    }

    if selection.signals.is_null() {
        selection.signals = Value::Object(serde_json::Map::new());
    }

    Ok(selection)
}

fn parse_json(raw: &str) -> Result<Value> {
    let trimmed = raw.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Ok(value);
    }
    if let Some(unfenced) = strip_fence(trimmed) {
        return serde_json::from_str::<Value>(unfenced).context("failed to parse fenced JSON");
    }
    serde_json::from_str::<Value>(trimmed).context("failed to parse JSON")
}

fn strip_fence(input: &str) -> Option<&str> {
    if !input.starts_with("```") {
        return None;
    }
    let rest = &input[3..];
    let rest = rest
        .strip_prefix("json")
        .or_else(|| rest.strip_prefix("JSON"))
        .unwrap_or(rest)
        .trim_start_matches(['\n', '\r', ' ']);
    let end = rest.rfind("```")?;
    Some(rest[..end].trim())
}
