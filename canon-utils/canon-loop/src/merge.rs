use canon_event::SubTaskResult;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Default, Clone)]
pub struct FileWriteTracker {
    /// path → (agent_id, action_id) with an in-flight write
    pending: HashMap<PathBuf, (String, String)>,
}

#[derive(Default, Clone)]
pub struct ContextMerger {
    pub merged_actions: Vec<MergedActionEntry>,
}

#[derive(Clone)]
pub struct MergedActionEntry {
    pub agent_id: String,
    pub action_kind: String,
    pub success: bool,
    pub stdout_summary: String,
    pub ts: u64,
}

impl ContextMerger {
    pub fn absorb(&mut self, result: &SubTaskResult, agent_id: &str) {
        let summary = serde_json::to_string(&result.output).unwrap_or_default();
        let mut entry = MergedActionEntry {
            agent_id: agent_id.to_string(),
            action_kind: "sub_task".to_string(),
            success: result.success,
            stdout_summary: if summary.len() > 256 { format!("{}...<truncated>", &summary[..256]) } else { summary },
            ts: 0,
        };
        if let Some(err) = &result.error {
            entry.stdout_summary.push_str(&format!(" error={}", err));
        }
        self.merged_actions.push(entry);
        if self.merged_actions.len() > 32 {
            let drop_n = self.merged_actions.len() - 32;
            self.merged_actions.drain(0..drop_n);
        }
    }

    pub fn prompt_section(&self) -> String {
        if self.merged_actions.is_empty() {
            return String::new();
        }
        let lines: Vec<String> = self.merged_actions.iter().rev().take(8).map(|m| format!("- agent={} success={} kind={} note={}", m.agent_id, m.success, m.action_kind, m.stdout_summary)).collect();
        format!("Recent sub-agent actions:\n{}\n", lines.join("\n"))
    }
}

#[derive(Default, Clone)]
pub struct WorkspaceDirtyTracker {
    pub dirty_by_agent: HashMap<String, Vec<String>>, // agent -> action_ids
}

impl WorkspaceDirtyTracker {
    pub fn mark_dirty(&mut self, agent: &str, action_id: Option<&str>) {
        let aid = agent.to_string();
        let entry = self.dirty_by_agent.entry(aid).or_default();
        if let Some(act) = action_id {
            entry.push(act.to_string());
        }
    }

    pub fn mark_verified(&mut self, agent: &str) {
        self.dirty_by_agent.remove(agent);
    }

    pub fn any_dirty(&self) -> bool {
        !self.dirty_by_agent.is_empty()
    }

    pub fn all_clean(&self) -> bool {
        self.dirty_by_agent.is_empty()
    }
}

impl FileWriteTracker {
    /// Returns Some(conflicting_agent, conflicting_action) if conflict detected.
    pub fn claim(&mut self, path: &Path, agent_id: &str, action_id: &str) -> Option<(String, String)> {
        let norm = path.to_path_buf();
        if let Some((other_agent, other_action)) = self.pending.get(&norm) {
            if other_agent != agent_id || other_action != action_id {
                return Some((other_agent.clone(), other_action.clone()));
            }
        }
        self.pending.insert(norm, (agent_id.to_string(), action_id.to_string()));
        None
    }

    pub fn release(&mut self, path: &Path) {
        self.pending.remove(path);
    }

    pub fn release_agent(&mut self, agent_id: &str) {
        self.pending.retain(|_, (agent, _)| agent != agent_id);
    }
}

/// Extract file paths touched by an action payload.
pub fn extract_written_paths(action_kind: &str, payload: &serde_json::Value) -> Vec<PathBuf> {
    match action_kind {
        "write_file" => payload.get("path").and_then(|v| v.as_str()).map(|s| PathBuf::from(s)).into_iter().collect(),
        "apply_patch" => extract_paths_from_patch(payload.get("patch").and_then(|v| v.as_str()).unwrap_or("")),
        _ => Vec::new(),
    }
}

fn extract_paths_from_patch(patch: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for line in patch.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("*** Add File:") {
            out.push(PathBuf::from(rest.trim()));
        } else if let Some(rest) = trimmed.strip_prefix("*** Update File:") {
            out.push(PathBuf::from(rest.trim()));
        }
    }
    out
}
