use crate::merge::{ContextMerger, FileWriteTracker, WorkspaceDirtyTracker};
use crate::scheduler::{DependencyTracker, Scheduler};
use canon_event::{EventEmitterHandle, LoopActed, LoopObserved, LoopPlanned, ToolResult};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Clone, Default)]
pub struct PendingPlan {
    pub tick: u64,
    pub request_id: String,
    pub dispatched_at_tick: u64,
    pub goal_text: Option<String>,
    pub trace_id: String,
    pub execution_id: String,
    pub span_id: String,
    pub plan_id: String,
    pub plan_tool_call_id: String,
}

#[derive(Clone)]
pub struct PendingAct {
    pub tick: u64,
    pub action_kind: String,
    pub tool_kind: String,
    pub request_id: String,
    pub tool_call_id: String,
    pub node_id: String,
    pub started_at: Instant,
    pub trace_id: Option<String>,
    pub execution_id: Option<String>,
    pub parent_span_id: Option<String>,
    pub plan_id: Option<String>,
    pub plan_step_id: Option<String>,
    pub action_id: Option<String>,
    pub artifact_n: u32,
    pub llm_request_id: Option<String>,
}

#[derive(Clone, Default)]
pub struct BatchStatus {
    pub artifact_n: u32,
    pub planned: usize,
    pub dispatched: usize,
    pub completed_ok: usize,
    pub completed_fail: usize,
}

#[derive(Clone, Copy)]
pub enum DestructiveCmdPolicy {
    Allow,
    Warn,
    Block,
}

impl Default for DestructiveCmdPolicy {
    fn default() -> Self {
        DestructiveCmdPolicy::Block
    }
}

impl DestructiveCmdPolicy {
    pub fn from_env() -> Self {
        match std::env::var("CANON_DESTRUCTIVE_CMD_POLICY").unwrap_or_else(|_| "block".to_string()).to_ascii_lowercase().as_str() {
            "allow" => Self::Allow,
            "warn" => Self::Warn,
            _ => Self::Block,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Warn => "warn",
            Self::Block => "block",
        }
    }
}

pub struct LoopContext {
    pub workspace: PathBuf,
    pub tlog_path: PathBuf,
    pub emitter: Option<EventEmitterHandle>,

    // Observe
    pub goal_text: Option<String>,
    pub recent_compiler_errors: Vec<serde_json::Value>,
    pub error_count: usize,
    pub warning_count: usize,

    // Plan
    pub pending_plan: Option<PendingPlan>,
    pub last_llm_signals: Option<serde_json::Value>,
    pub last_observed: Option<LoopObserved>,
    pub last_observed_tick: Option<u64>,
    pub last_handled_observed_hash: Option<u64>,
    pub last_planned_observed_tick: Option<u64>,
    pub last_emitted_plan_hash: Option<u64>,
    pub last_done_goal: Option<String>,
    pub batch_acted: Vec<LoopActed>,
    pub batch_tool_results: Vec<ToolResult>,
    pub last_prompted_goal: Option<String>,
    // System prompt caching — tracks which static system prompt the executor has cached.
    pub last_system_prompt_id: Option<u64>,
    // Context-base caching — tracks the slow-changing context section (GOAL + workspace tree).
    pub last_context_base_id: Option<u64>,
    pub last_delta_hash: Option<u64>,
    pub last_route_rationale: Option<String>,
    pub last_route_confidence: Option<f64>,
    pub last_route_rationale_non_empty: Option<String>,
    pub last_route_confidence_non_empty: Option<f64>,

    // Act
    pub scheduler: Scheduler,
    pub dep_tracker: DependencyTracker,
    pub pending_act: Option<PendingAct>,
    pub artifact_dir: PathBuf,
    pub artifact_counter: u32,
    pub active_batch_llm_request_id: Option<String>,
    pub queued_artifact_index: HashMap<String, u32>,
    pub act_batch_tracker: HashMap<String, BatchStatus>,
    pub last_act_reconcile: Option<Instant>,
    pub destructive_cmd_policy: DestructiveCmdPolicy,
    pub file_write_tracker: FileWriteTracker,
    pub write_paths_by_action: HashMap<String, Vec<PathBuf>>,
    pub dirty_tracker: WorkspaceDirtyTracker,
    pub context_merger: ContextMerger,

    // Decompose
    pub pending_decompose_request_id: Option<String>,
    pub agent_id: Option<String>,

    // Verify
    pub last_verify_trace_id: Option<String>,
    pub last_verify_execution_id: Option<String>,
    pub last_act_span_id: Option<String>,
    pub last_acted: Option<LoopActed>,
    pub last_verified_action_key: Option<String>,

    // Reward
    pub errors_before: usize,
    pub stagnant_ticks: u32,
    pub last_action_kind: String,
    pub last_action_success: bool,
    pub halted: bool,
    pub last_reward_trace_id: Option<String>,
    pub last_reward_execution_id: Option<String>,
    pub last_reward_verify_span_id: Option<String>,
    pub goodness: Option<f32>,
    pub delta_g: Option<f32>,
    pub last_observed_error_count: u64,
    pub last_observed_goal_hash: u64,
    pub last_observed_facts_hash: u64,
    pub current_tick: u64,
}

impl LoopContext {
    pub fn new(workspace: PathBuf, tlog_path: PathBuf) -> Self {
        Self {
            workspace: workspace.clone(),
            tlog_path,
            emitter: None,
            goal_text: None,
            recent_compiler_errors: Vec::new(),
            error_count: 0,
            warning_count: 0,
            pending_plan: None,
            last_llm_signals: None,
            last_observed: None,
            last_observed_tick: None,
            last_handled_observed_hash: None,
            last_planned_observed_tick: None,
            last_emitted_plan_hash: None,
            last_done_goal: None,
            batch_acted: Vec::new(),
            batch_tool_results: Vec::new(),
            last_prompted_goal: None,
            last_system_prompt_id: None,
            last_context_base_id: None,
            last_delta_hash: None,
            last_route_rationale: None,
            last_route_confidence: None,
            last_route_rationale_non_empty: None,
            last_route_confidence_non_empty: None,
            scheduler: Scheduler::new(),
            dep_tracker: DependencyTracker::default(),
            pending_act: None,
            artifact_dir: default_artifact_dir(&workspace),
            artifact_counter: next_tool_artifact_counter(&default_artifact_dir(&workspace)),
            active_batch_llm_request_id: None,
            queued_artifact_index: HashMap::new(),
            act_batch_tracker: HashMap::new(),
            last_act_reconcile: None,
            destructive_cmd_policy: DestructiveCmdPolicy::from_env(),
            file_write_tracker: FileWriteTracker::default(),
            write_paths_by_action: HashMap::new(),
            dirty_tracker: WorkspaceDirtyTracker::default(),
            context_merger: ContextMerger::default(),
            pending_decompose_request_id: None,
            agent_id: None,
            last_verify_trace_id: None,
            last_verify_execution_id: None,
            last_act_span_id: None,
            last_acted: None,
            last_verified_action_key: None,
            errors_before: 0,
            stagnant_ticks: 0,
            last_action_kind: String::new(),
            last_action_success: true,
            halted: false,
            last_reward_trace_id: None,
            last_reward_execution_id: None,
            last_reward_verify_span_id: None,
            goodness: None,
            delta_g: None,
            last_observed_error_count: 0,
            last_observed_goal_hash: 0,
            last_observed_facts_hash: 0,
            current_tick: 0,
        }
    }
}

fn default_artifact_dir(_workspace: &PathBuf) -> PathBuf {
    let default_dir = "/workspace/ai_sandbox/canon/canon-utils/state/reports_out/llm";
    std::env::var("CANON_LLM_LOG_DIR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(default_dir))
}

fn next_tool_artifact_counter(log_dir: &std::path::Path) -> u32 {
    let mut max_seen: Option<u32> = None;
    let Ok(entries) = std::fs::read_dir(log_dir) else {
        return 0;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else {
            continue;
        };
        let Some((n, _suffix, _ts)) = parse_artifact_name(&name) else {
            continue;
        };
        max_seen = Some(max_seen.map_or(n, |m| m.max(n)));
    }
    max_seen.map_or(0, |m| m.saturating_add(1))
}

fn parse_artifact_name(name: &str) -> Option<(u32, String, Option<u64>)> {
    let mut parts = name.splitn(3, '_');
    let first = parts.next()?;
    let second = parts.next()?;
    let third = parts.next()?;

    if let Ok(n) = first.parse::<u32>() {
        return Some((n, format!("{}_{}", second, third), None));
    }
    let ts = first.parse::<u64>().ok()?;
    let n = second.parse::<u32>().ok()?;
    Some((n, third.to_string(), Some(ts)))
}

// ----------------- batch tracking helpers (ported) -----------------
impl LoopContext {
    pub fn artifact_index_for_plan(&mut self, planned: &LoopPlanned) -> u32 {
        if let Some(request_id) = planned.llm_request_id.as_deref() {
            if let Some(n) = find_request_index_by_request_id(&self.artifact_dir, request_id) {
                if let Some(cache_key) = plan_cache_key(planned) {
                    self.queued_artifact_index.insert(cache_key, n);
                }
                return n;
            }
        }
        if let Some(cache_key) = plan_cache_key(planned) {
            if let Some(n) = self.queued_artifact_index.get(&cache_key) {
                return *n;
            }
            let n = self.next_tool_artifact_n();
            self.queued_artifact_index.insert(cache_key, n);
            return n;
        }
        self.next_tool_artifact_n()
    }

    pub fn clear_cached_artifact_index_for_plan(&mut self, planned: &LoopPlanned) {
        if let Some(cache_key) = plan_cache_key(planned) {
            self.queued_artifact_index.remove(&cache_key);
        }
    }

    pub fn mark_batch_planned(&mut self, planned: &LoopPlanned, artifact_n: u32) {
        let Some(llm_request_id) = planned.llm_request_id.clone() else {
            return;
        };
        let snapshot = {
            let status = self.act_batch_tracker.entry(llm_request_id.clone()).or_insert_with(|| BatchStatus { artifact_n, ..BatchStatus::default() });
            if status.artifact_n == 0 {
                status.artifact_n = artifact_n;
            }
            status.planned = status.planned.saturating_add(1);
            status.clone()
        };
        write_batch_status_artifact(&self.artifact_dir, snapshot.artifact_n, &llm_request_id, "in_progress", &snapshot);
    }

    pub fn mark_batch_dispatched(&mut self, planned: &LoopPlanned) {
        let Some(llm_request_id) = planned.llm_request_id.as_deref() else {
            return;
        };
        let Some(status) = self.act_batch_tracker.get_mut(llm_request_id) else {
            return;
        };
        status.dispatched = status.dispatched.saturating_add(1);
        let snapshot = status.clone();
        write_batch_status_artifact(&self.artifact_dir, snapshot.artifact_n, llm_request_id, "in_progress", &snapshot);
    }

    pub fn mark_batch_inline_completion(&mut self, planned: &LoopPlanned, success: bool) {
        self.mark_batch_completion(planned.llm_request_id.as_deref(), success);
    }

    pub fn mark_batch_completion(&mut self, llm_request_id: Option<&str>, success: bool) {
        let Some(llm_request_id) = llm_request_id else {
            return;
        };
        let mut should_remove = false;
        let Some(status) = self.act_batch_tracker.get_mut(llm_request_id) else {
            return;
        };
        if success {
            status.completed_ok = status.completed_ok.saturating_add(1);
        } else {
            status.completed_fail = status.completed_fail.saturating_add(1);
        }
        let finished = status.completed_ok.saturating_add(status.completed_fail) >= status.planned;
        let status_label = if finished {
            if status.completed_fail == 0 {
                "completed"
            } else {
                "failed_partial"
            }
        } else {
            "in_progress"
        };
        let snapshot = status.clone();
        write_batch_status_artifact(&self.artifact_dir, snapshot.artifact_n, llm_request_id, status_label, &snapshot);
        if finished {
            should_remove = true;
        }
        if should_remove {
            self.act_batch_tracker.remove(llm_request_id);
        }
    }

    fn next_tool_artifact_n(&mut self) -> u32 {
        let n = self.artifact_counter;
        self.artifact_counter = self.artifact_counter.saturating_add(1);
        n
    }
}

fn plan_cache_key(planned: &LoopPlanned) -> Option<String> {
    planned.action_id.as_ref().map(|id| format!("action:{id}")).or_else(|| planned.plan_step_id.as_ref().map(|id| format!("step:{id}")))
}

fn find_request_index_by_request_id(log_dir: &std::path::Path, request_id: &str) -> Option<u32> {
    let entries = std::fs::read_dir(log_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_str()?;
        let Some((n, suffix, _ts)) = parse_artifact_name(name) else {
            continue;
        };
        if suffix != "request.json" {
            continue;
        }
        let raw = std::fs::read_to_string(entry.path()).ok()?;
        let v = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
        if v.get("request_id").and_then(|x| x.as_str()) == Some(request_id) {
            return Some(n);
        }
    }
    None
}

fn write_batch_status_artifact(log_dir: &std::path::Path, artifact_n: u32, llm_request_id: &str, status: &str, batch: &BatchStatus) {
    let _ = std::fs::create_dir_all(log_dir);
    let path = artifact_path_for(log_dir, artifact_n, "batch_status");
    let value = serde_json::json!({
        "n": artifact_n,
        "llm_request_id": llm_request_id,
        "status": status,
        "planned": batch.planned,
        "dispatched": batch.dispatched,
        "completed_ok": batch.completed_ok,
        "completed_fail": batch.completed_fail,
        "updated_ms": now_ms_u64(),
    });
    let serialized = serde_json::to_string_pretty(&value).unwrap_or_default();
    write_atomic(&path, &serialized);
}

// utility share with act stage
fn now_ms_u64() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn artifact_path_for(log_dir: &std::path::Path, artifact_n: u32, suffix: &str) -> std::path::PathBuf {
    log_dir.join(format!("{:09}_{}.json", artifact_n, suffix))
}

fn write_atomic(path: &std::path::Path, content: &str) {
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, content).is_ok() {
        let _ = std::fs::rename(tmp, path);
    }
}
