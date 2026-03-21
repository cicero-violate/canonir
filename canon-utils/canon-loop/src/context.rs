use canon_event::{EventEmitterHandle, LoopActed, LoopObserved, LoopPlanned, ToolResult};
use std::collections::{HashMap, VecDeque};
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
    pub last_observed: Option<LoopObserved>,
    pub last_planned_observed_tick: Option<u64>,
    pub last_done_goal: Option<String>,
    pub batch_acted: Vec<LoopActed>,
    pub batch_tool_results: Vec<ToolResult>,
    pub last_prompted_goal: Option<String>,

    // Act
    pub act_queue: VecDeque<LoopPlanned>,
    pub pending_act: Option<PendingAct>,
    pub artifact_dir: PathBuf,
    pub artifact_counter: u32,
    pub active_batch_llm_request_id: Option<String>,
    pub queued_artifact_index: HashMap<String, u32>,
    pub act_batch_tracker: HashMap<String, BatchStatus>,
    pub last_act_reconcile: Option<Instant>,
    pub destructive_cmd_policy: DestructiveCmdPolicy,

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
            last_observed: None,
            last_planned_observed_tick: None,
            last_done_goal: None,
            batch_acted: Vec::new(),
            batch_tool_results: Vec::new(),
            last_prompted_goal: None,
            act_queue: VecDeque::new(),
            pending_act: None,
            artifact_dir: default_artifact_dir(&workspace),
            artifact_counter: 0,
            active_batch_llm_request_id: None,
            queued_artifact_index: HashMap::new(),
            act_batch_tracker: HashMap::new(),
            last_act_reconcile: None,
            destructive_cmd_policy: DestructiveCmdPolicy::default(),
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
        }
    }
}

fn default_artifact_dir(_workspace: &PathBuf) -> PathBuf {
    let default_dir = "/workspace/ai_sandbox/canon/canon-utils/state/reports_out/llm";
    std::env::var("CANON_LLM_LOG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(default_dir))
}
