use anyhow::Result;
use serde_json::Value;

/// Read the tlog and return a compact summary: event counts by kind plus the
/// last N lines verbatim so the LLM has both a macro view and recent detail.
pub fn summarise(path: &str) -> Result<String> {
    let raw = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = raw.lines().collect();

    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut last_verified: Option<String> = None;
    let mut last_planned: Option<String> = None;
    let mut last_rewarded: Option<String> = None;

    for line in &lines {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let kind = v["kind"].as_str().unwrap_or("unknown").to_string();
        *counts.entry(kind.clone()).or_insert(0) += 1;
        match kind.as_str() {
            "LoopVerified" => last_verified = Some(line.to_string()),
            "LoopPlanned"  => last_planned  = Some(line.to_string()),
            "LoopRewarded" => last_rewarded  = Some(line.to_string()),
            _ => {}
        }
    }

    let total = lines.len();
    let mut out = format!("## Tlog summary ({total} events)\n\n### Event counts\n");
    for (k, n) in &counts {
        out.push_str(&format!("- {k}: {n}\n"));
    }

    out.push_str("\n### Most recent key events\n");
    for (label, ev) in [
        ("LoopVerified", &last_verified),
        ("LoopPlanned",  &last_planned),
        ("LoopRewarded", &last_rewarded),
    ] {
        match ev {
            Some(s) => out.push_str(&format!("\n**{label}**\n```json\n{s}\n```\n")),
            None    => out.push_str(&format!("\n**{label}**: (none)\n")),
        }
    }

    // Include the last 40 raw lines for recency context.
    out.push_str("\n### Last 40 raw events\n```\n");
    for line in lines.iter().rev().take(40).rev() {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("```\n");

    Ok(out)
}
