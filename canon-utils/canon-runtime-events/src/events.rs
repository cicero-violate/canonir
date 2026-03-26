use canon_types::{EventDelta, RustcEvent, RustcState};
use std::sync::Arc;

use canon_proc_macros::{canon_event_enum, canon_event_struct};

pub const EVENT_SCHEMA_VERSION: &str = "1";

// ---------------------------------------------------------------------------
// Edit events
// ---------------------------------------------------------------------------

canon_event_struct!(
    #[impl_shape]
    RenameSymbol {
        project: String,
        #[input] old: String,
        #[output] new: String,
    }
);

canon_event_struct!(
    #[impl_shape]
    MoveSymbol {
        project: String,
        #[input] symbol: String,
        #[output] module: String,
    }
);

canon_event_struct!(
    #[impl_shape]
    DeleteSymbol {
        project: String,
        #[input] symbol: String,
        #[output] success: bool,
    }
);

canon_event_struct!(
    #[impl_shape]
    RenameModule {
        project: String,
        #[input] old: String,
        #[output] new: String,
    }
);

canon_event_struct!(
    #[impl_shape]
    RenameDir {
        project: String,
        #[input] old: std::path::PathBuf,
        #[output] new: std::path::PathBuf,
    }
);

canon_event_struct!(
    #[impl_shape]
    InlineModule {
        project: String,
        #[input] module: String,
        #[output] success: bool,
    }
);

canon_event_struct!(
    #[impl_shape]
    ExtractModule {
        project: String,
        #[input] symbol: String,
        #[output] module: String,
    }
);

canon_event_enum!(#[derive(serde::Serialize, serde::Deserialize)]
EditEvent {
    RenameSymbol(RenameSymbol),
    MoveSymbol(MoveSymbol),
    DeleteSymbol(DeleteSymbol),
    RenameModule(RenameModule),
    RenameDir(RenameDir),
    InlineModule(InlineModule),
    ExtractModule(ExtractModule),
});

// ---------------------------------------------------------------------------
// EventMask (for rustc consumers)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventMask(pub(crate) u16);

impl EventMask {
    pub const NONE: EventMask = EventMask(0);
    pub const NODE_DEFINED: EventMask = EventMask(1 << 0);
    pub const EDGE_DEFINED: EventMask = EventMask(1 << 1);
    pub const FILE_SEEN: EventMask = EventMask(1 << 2);
    pub const NODE_UPDATED: EventMask = EventMask(1 << 3);
    pub const NODE_REMOVED: EventMask = EventMask(1 << 4);
    pub const EDGE_REMOVED: EventMask = EventMask(1 << 5);
    pub const PANIC_CAPTURED: EventMask = EventMask(1 << 6);
    pub const WARNING_CAPTURED: EventMask = EventMask(1 << 7);
    pub const SESSION_START: EventMask = EventMask(1 << 8);
    pub const COMPILATION_UNIT_FINISHED: EventMask = EventMask(1 << 9);
    pub const INVARIANT_VIOLATION: EventMask = EventMask(1 << 10);
    pub const CALLSITE_OBSERVED: EventMask = EventMask(1 << 11);
    pub const SYMBOL_DEFINED: EventMask = EventMask(1 << 12);
    pub const SPAN_DEFINED: EventMask = EventMask(1 << 13);
    pub const ALL: EventMask = EventMask(
        Self::NODE_DEFINED.0
            | Self::EDGE_DEFINED.0
            | Self::FILE_SEEN.0
            | Self::NODE_UPDATED.0
            | Self::NODE_REMOVED.0
            | Self::EDGE_REMOVED.0
            | Self::PANIC_CAPTURED.0
            | Self::WARNING_CAPTURED.0
            | Self::SESSION_START.0
            | Self::COMPILATION_UNIT_FINISHED.0
            | Self::INVARIANT_VIOLATION.0
            | Self::CALLSITE_OBSERVED.0
            | Self::SYMBOL_DEFINED.0
            | Self::SPAN_DEFINED.0,
    );

    pub fn contains(self, other: EventMask) -> bool {
        (self.0 & other.0) == other.0
    }

    pub fn for_event(event: &RustcEvent) -> EventMask {
        match event {
            RustcEvent::NodeDefined(_) => Self::NODE_DEFINED,
            RustcEvent::NodeUpdated(_) => Self::NODE_UPDATED,
            RustcEvent::NodeRemoved(_) => Self::NODE_REMOVED,
            RustcEvent::EdgeDefined(_) => Self::EDGE_DEFINED,
            RustcEvent::EdgeRemoved(_) => Self::EDGE_REMOVED,
            RustcEvent::FileSeen(_) => Self::FILE_SEEN,
            RustcEvent::CallsiteObserved(_) => Self::CALLSITE_OBSERVED,
            RustcEvent::SymbolDefined(_) => Self::SYMBOL_DEFINED,
            RustcEvent::SpanDefined(_) => Self::SPAN_DEFINED,
            RustcEvent::PanicCaptured(_) => Self::PANIC_CAPTURED,
            RustcEvent::WarningCaptured(_) => Self::WARNING_CAPTURED,
            RustcEvent::SessionStart(_) => Self::SESSION_START,
            RustcEvent::CompilationUnitFinished(_) => Self::COMPILATION_UNIT_FINISHED,
            RustcEvent::InvariantViolation(_) => Self::INVARIANT_VIOLATION,
        }
    }
}

impl std::ops::BitOr for EventMask {
    type Output = EventMask;
    fn bitor(self, rhs: EventMask) -> EventMask {
        EventMask(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for EventMask {
    fn bitor_assign(&mut self, rhs: EventMask) {
        self.0 |= rhs.0;
    }
}

pub trait RustcEventConsumer: Send + Sync {
    fn mask(&self) -> EventMask;
    fn on_event(&mut self, delta: &EventDelta, state: &RustcState);
}

// ---------------------------------------------------------------------------
// Code event (wraps rustc output — manual CanonPayloadShape)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Code {
    pub delta: canon_types::EventDelta,
    pub state: canon_types::RustcState,
}

impl Default for Code {
    fn default() -> Self {
        Self { delta: canon_types::EventDelta::default(), state: canon_types::RustcState::default() }
    }
}

impl crate::CanonPayloadShape for Code {
    fn payload_input(&self) -> serde_json::Value {
        serde_json::json!({ "tick": self.delta.tick, "id": self.delta.id })
    }
    fn payload_output(&self) -> serde_json::Value {
        serde_json::to_value(&self.delta.event).unwrap_or_else(|_| serde_json::json!({}))
    }
    fn payload_delta(&self) -> serde_json::Value {
        serde_json::json!({ "graph_version": self.state.graph_version })
    }
    fn payload_data(&self) -> serde_json::Value {
        serde_json::json!({
            "delta": serde_json::to_value(&self.delta).unwrap_or_default(),
            "state": serde_json::to_value(&self.state).unwrap_or_default(),
        })
    }
}

// ---------------------------------------------------------------------------
// Runtime / loop events
// ---------------------------------------------------------------------------

canon_event_struct!(
    #[impl_shape]
    DebugEvent {
        #[input] source: String,
        #[input] kind: String,
        #[output] payload: serde_json::Value,
    }
);

canon_event_struct!(
    #[impl_shape]
    ErrorOccurred {
        #[input] kind: String,
        #[input] source: String,
        #[input] message: String,
        #[serde(default)]
        severity: String,
        #[serde(default)]
        context: serde_json::Value,
        #[serde(default)]
        trace_id: Option<String>,
        #[serde(default)]
        error_id: Option<String>,
        #[output] captured: bool,
    }
);

pub fn new_error_occurred(
    kind: impl Into<String>,
    source: impl Into<String>,
    message: impl Into<String>,
    severity: impl Into<String>,
    context: serde_json::Value,
    trace_id: Option<String>,
) -> ErrorOccurred {
    ErrorOccurred {
        kind: kind.into(),
        source: source.into(),
        message: message.into(),
        severity: severity.into(),
        context,
        trace_id,
        error_id: Some(uuid::Uuid::new_v4().to_string()),
        captured: true,
    }
}

canon_event_struct!(
    #[impl_shape]
    Tick {
        #[input] tick: u64,
        #[output] emitted: bool,
    }
);

canon_event_struct!(
    #[impl_shape]
    LoopObserved {
        #[input] tick: u64,
        #[output] error_count: usize,
        #[output] warning_count: usize,
        #[delta] compiler_errors: Vec<serde_json::Value>,
        #[input] goal_text: Option<String>,
        #[serde(default)]
        workspace_facts: Vec<String>,
    }
);

canon_event_struct!(
    #[impl_shape]
    LoopPlanned {
        #[input] tick: u64,
        #[input] action_kind: String,
        #[input] action_payload: serde_json::Value,
        #[output] reason: String,
        llm_request_id: Option<String>,
        #[serde(default)]
        #[delta] signals: Option<serde_json::Value>,
        #[serde(default)]
        trace_id: Option<String>,
        #[serde(default)]
        execution_id: Option<String>,
        #[serde(default)]
        span_id: Option<String>,
        #[serde(default)]
        parent_span_id: Option<String>,
        #[serde(default)]
        plan_id: Option<String>,
        #[serde(default)]
        plan_step_id: Option<String>,
        #[serde(default)]
        action_id: Option<String>,
        #[serde(default)]
        depends_on: Vec<String>,
    }
);

canon_event_struct!(
    #[impl_shape]
    LoopActed {
        #[input] tick: u64,
        #[input] action_kind: String,
        #[input] capability_request_id: String,
        #[serde(default)]
        tool_call_id: Option<String>,
        #[serde(default)]
        tool_result_id: Option<String>,
        #[output] stdout: String,
        #[delta] stderr: String,
        #[output] exit_code: Option<i32>,
        duration_ms: u64,
        #[output] success: bool,
        #[serde(default)]
        trace_id: Option<String>,
        #[serde(default)]
        execution_id: Option<String>,
        #[serde(default)]
        span_id: Option<String>,
        #[serde(default)]
        parent_span_id: Option<String>,
        #[serde(default)]
        plan_id: Option<String>,
        #[serde(default)]
        plan_step_id: Option<String>,
        #[serde(default)]
        action_id: Option<String>,
    }
);

canon_event_struct!(
    #[impl_shape]
    LoopVerified {
        #[input] tick: u64,
        #[output] passed: bool,
        #[output] compiler_clean: bool,
        tlog_clean: bool,
        #[delta] error_count: usize,
        #[delta] diagnostics: Vec<String>,
        #[serde(default)]
        trace_id: Option<String>,
        #[serde(default)]
        execution_id: Option<String>,
        #[serde(default)]
        span_id: Option<String>,
        #[serde(default)]
        parent_span_id: Option<String>,
    }
);

canon_event_struct!(
    #[impl_shape]
    LoopRewarded {
        #[input] tick: u64,
        #[input] errors_before: usize,
        #[delta] errors_after: usize,
        stagnant_ticks: u32,
        #[output] halt: bool,
        #[serde(default)]
        goodness: f32,
        #[output] reward: f32,
        #[serde(default)]
        #[delta] delta_g: f32,
        #[serde(default)]
        trace_id: Option<String>,
        #[serde(default)]
        execution_id: Option<String>,
        #[serde(default)]
        span_id: Option<String>,
        #[serde(default)]
        parent_span_id: Option<String>,
    }
);

canon_event_struct!(
    #[impl_shape]
    GoodnessSnapshot {
        #[input] tick: u64,
        #[output] g: f32,
        #[delta] delta_g: f32,
        metrics: serde_json::Value,
    }
);

// ---------------------------------------------------------------------------
// Routing events
// ---------------------------------------------------------------------------

canon_event_struct!(
    #[impl_shape]
    RouteTick {
        #[input] tick: u64,
        #[output] emitted: bool,
    }
);

canon_event_struct!(
    #[impl_shape]
    RouteSelected {
        #[input] tick: u64,
        #[input] suggested_route: String,
        #[input] prompt: String,
        #[output] approved_route: String,
        #[output] rationale: String,
        #[serde(default)]
        confidence: Option<f32>,
        gate_note: String,
        #[serde(default)]
        gate_rules_fired: Vec<String>,
        #[delta] gate_changed: bool,
        gate_should_stop: bool,
        model_json: String,
    }
);

// ---------------------------------------------------------------------------
// State update events (no natural input — payload IS the new state)
// ---------------------------------------------------------------------------

canon_event_struct!(
    #[no_input]
    #[impl_shape]
    RuntimeStateUpdated {
        #[output] payload: serde_json::Value,
    }
);

canon_event_struct!(
    #[no_input]
    #[impl_shape]
    PolicyBaselineUpdated {
        #[output] payload: serde_json::Value,
    }
);

canon_event_struct!(
    #[no_input]
    #[impl_shape]
    GoalSelected {
        #[output] payload: serde_json::Value,
    }
);

canon_event_struct!(
    #[no_input]
    #[impl_shape]
    SystemConfigLoaded {
        #[output] payload: serde_json::Value,
    }
);

canon_event_struct!(
    #[no_input]
    #[impl_shape]
    AgentRegistered {
        #[output] payload: serde_json::Value,
    }
);

canon_event_struct!(
    #[no_input]
    #[impl_shape]
    PromptLoaded {
        #[output] payload: serde_json::Value,
    }
);

// ---------------------------------------------------------------------------
// Tool events
// ---------------------------------------------------------------------------

canon_event_struct!(
    #[impl_shape]
    ToolCall {
        node_id: String,
        #[input] tool_call_id: String,
        request_id: String,
        #[input] kind: String,
        #[input] payload: serde_json::Value,
        #[output] accepted: bool,
    }
);

canon_event_struct!(
    #[impl_shape]
    ToolResult {
        node_id: String,
        #[input] tool_call_id: String,
        tool_result_id: String,
        request_id: String,
        #[input] kind: String,
        #[output] output: serde_json::Value,
        #[output] success: bool,
    }
);

// Emitted once when all pending tool results for a batch have landed.
canon_event_struct!(
    #[impl_shape]
    ToolBatchSettled {
        #[input] tick: u64,
        #[input] result_count: u32,
        #[output] any_failed: bool,
    }
);

// ---------------------------------------------------------------------------
// Goal graph events
// ---------------------------------------------------------------------------

canon_event_struct!(
    #[impl_shape]
    GoalNodeCreated {
        #[input] node_id: String,
        #[input] description: String,
        #[input] deps: Vec<String>,
        #[delta] caps: Vec<String>,
        node_type: String,
        priority: u8,
        #[serde(default)]
        budget: Option<u32>,
        #[output] created: bool,
    }
);

canon_event_struct!(
    #[impl_shape]
    GoalNodeRetracted {
        #[input] node_id: String,
        #[output] retracted: bool,
    }
);

canon_event_struct!(
    #[impl_shape]
    GoalNodeRewritten {
        #[input] node_id: String,
        #[output] new_description: String,
        #[delta] new_caps: Vec<String>,
    }
);

canon_event_struct!(
    #[impl_shape]
    GoalEdgeDefined {
        #[input] from_node_id: String,
        #[input] to_node_id: String,
        #[output] created: bool,
    }
);

canon_event_struct!(
    #[impl_shape]
    GoalGraphCheckpointed {
        #[input] tlog_seq: u64,
        #[output] checkpointed: bool,
    }
);

// ---------------------------------------------------------------------------
// Capability events
// ---------------------------------------------------------------------------

canon_event_struct!(
    #[impl_shape]
    CapabilityInvoked {
        #[input] capability_id: String,
        #[input] capability: &'static str,
        #[input] node_id: String,
        #[output] invoked: bool,
    }
);

canon_event_struct!(
    #[impl_shape]
    CapabilityResolved {
        #[input] capability_id: String,
        #[output] success: bool,
        duration_ms: u64,
    }
);

// ---------------------------------------------------------------------------
// Cargo capability events
// ---------------------------------------------------------------------------

canon_event_struct!(
    #[impl_shape]
    CargoBuild {
        #[input] request_id: String,
        #[input] crate_name: String,
        #[output] queued: bool,
    }
);

canon_event_struct!(
    #[impl_shape]
    CargoRun {
        #[input] request_id: String,
        #[input] crate_name: String,
        #[serde(default)]
        bin: Option<String>,
        #[serde(default)]
        args: Vec<String>,
        #[output] queued: bool,
    }
);

canon_event_struct!(
    #[impl_shape]
    CargoCheck {
        #[input] request_id: String,
        #[input] crate_name: String,
        #[output] queued: bool,
    }
);

canon_event_enum!(#[derive(serde::Serialize, serde::Deserialize)] CargoEvent {
    Build(CargoBuild),
    Run(CargoRun),
    Check(CargoCheck),
});

// ---------------------------------------------------------------------------
// File capability events
// ---------------------------------------------------------------------------

canon_event_struct!(
    #[impl_shape]
    FileRead {
        #[input] request_id: String,
        #[input] path: String,
        #[output] queued: bool,
    }
);

canon_event_struct!(
    #[impl_shape]
    FileWrite {
        #[input] request_id: String,
        #[input] path: String,
        #[input] content: String,
        #[output] queued: bool,
    }
);

canon_event_struct!(
    #[impl_shape]
    FilePatch {
        #[input] request_id: String,
        #[input] path: String,
        #[input] old: String,
        #[output] new: String,
        #[output] queued: bool,
    }
);

canon_event_enum!(#[derive(serde::Serialize, serde::Deserialize)] FileEvent {
    Read(FileRead),
    Write(FileWrite),
    Patch(FilePatch),
});

// ---------------------------------------------------------------------------
// Bash capability
// ---------------------------------------------------------------------------

canon_event_struct!(
    #[impl_shape]
    BashInvoke {
        #[input] request_id: String,
        #[input] cmd: String,
        #[serde(default)]
        cwd: Option<String>,
        #[output] queued: bool,
    }
);

// ---------------------------------------------------------------------------
// LLM capability
// ---------------------------------------------------------------------------

canon_event_struct!(
    #[impl_shape]
    LlmCall {
        #[input] request_id: String,
        /// Dynamic context only (goal, workspace state, errors, recent actions/results).
        /// The static system instructions live in `system` and are cached by `system_prompt_id`.
        #[input] prompt: String,
        #[serde(default)]
        role: Option<String>,
        #[serde(default)]
        agent_id: Option<String>,
        #[output] dispatched: bool,
        /// Static system instructions — tools, workflow, safety rules, output format.
        /// Sent only on the first call or after a session reset; None on subsequent calls.
        /// The executor worker caches this by `system_prompt_id` and prepends it for the LLM.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        system: Option<String>,
        /// Hash of the static system prompt. Used as cache key in the executor worker.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        system_prompt_id: Option<String>,
        /// Slow-changing context (GOAL, workspace tree, facts, search hints, sub-agents).
        /// Sent only when changed from the previous call; worker caches by `context_base_id`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context_base: Option<String>,
        /// Hash of `context_base`. Always set when context caching is in use so the worker
        /// can reconstruct the full prompt for stateless endpoints.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context_base_id: Option<String>,
        /// Hash of the base context used for this call (for causal chain tracing).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt_base_id: Option<String>,
        /// Base id from the previous LLM call — causal chain.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prev_prompt_id: Option<String>,
    }
);

canon_event_struct!(
    #[impl_shape]
    RequestDispatch {
        dispatch_id: String,
        #[input] parent_request_id: String,
        agent_id: String,
        #[input] task_prompt: String,
        task_kind: String,
        #[serde(default)]
        #[delta] deps: Vec<String>,
        #[serde(default)]
        workspace_scope: Option<String>,
        #[output] dispatched: bool,
    }
);

canon_event_struct!(
    #[impl_shape]
    SubTaskResult {
        #[input] dispatch_id: String,
        #[input] agent_id: String,
        parent_request_id: String,
        #[output] success: bool,
        #[output] output: serde_json::Value,
        #[delta] actions_taken: Vec<String>,
        #[serde(default)]
        error: Option<String>,
    }
);

// ---------------------------------------------------------------------------
// Analysis capability
// ---------------------------------------------------------------------------

canon_event_struct!(
    #[impl_shape]
    AnalysisRun {
        #[input] request_id: String,
        #[input] crate_name: String,
        #[serde(default)]
        batch_id: Option<String>,
        #[output] queued: bool,
    }
);

canon_event_struct!(
    #[impl_shape]
    AnalysisWorkspace {
        #[input] request_id: String,
        #[output] queued: bool,
    }
);

canon_event_enum!(#[derive(serde::Serialize, serde::Deserialize)] AnalysisEvent {
    Run(AnalysisRun),
    Workspace(AnalysisWorkspace),
});

// ---------------------------------------------------------------------------
// Enum Default impls — explicit construction (Default no longer derived)
// ---------------------------------------------------------------------------

impl Default for EditEvent {
    fn default() -> Self {
        Self::RenameSymbol(RenameSymbol { project: String::new(), old: String::new(), new: String::new() })
    }
}

impl Default for CargoEvent {
    fn default() -> Self {
        Self::Build(CargoBuild { request_id: String::new(), crate_name: String::new(), queued: false })
    }
}

impl Default for FileEvent {
    fn default() -> Self {
        Self::Read(FileRead { request_id: String::new(), path: String::new(), queued: false })
    }
}

impl Default for AnalysisEvent {
    fn default() -> Self {
        Self::Run(AnalysisRun { request_id: String::new(), crate_name: String::new(), batch_id: None, queued: false })
    }
}

// ---------------------------------------------------------------------------
// RuntimeEvent master enum
// ---------------------------------------------------------------------------

canon_event_enum!(RuntimeEvent {
    Code(Code),
    Debug(DebugEvent),
    ErrorOccurred(ErrorOccurred),
    Edit(EditEvent),
    Tick(Tick),
    LoopObserved(LoopObserved),
    LoopPlanned(LoopPlanned),
    LoopActed(LoopActed),
    LoopVerified(LoopVerified),
    LoopRewarded(LoopRewarded),
    GoodnessSnapshot(GoodnessSnapshot),
    RouteTick(RouteTick),
    RouteSelected(RouteSelected),
    Cargo(CargoEvent),
    File(FileEvent),
    Bash(BashInvoke),
    Llm(LlmCall),
    RequestDispatch(RequestDispatch),
    SubTaskResult(SubTaskResult),
    Analysis(AnalysisEvent),
    RuntimeStateUpdated(RuntimeStateUpdated),
    NodeReady(NodeReady),
    NodeStarted(NodeStarted),
    NodeCompleted(NodeCompleted),
    NodeFailed(NodeFailed),
    CapabilityCompleted(CapabilityCompleted),
    CapabilityFailed(CapabilityFailed),
    PolicyBaselineUpdated(PolicyBaselineUpdated),
    GoalSelected(GoalSelected),
    SystemConfigLoaded(SystemConfigLoaded),
    AgentRegistered(AgentRegistered),
    PromptLoaded(PromptLoaded),
    ToolCall(ToolCall),
    ToolResult(ToolResult),
    ToolBatchSettled(ToolBatchSettled),
    GoalNodeCreated(GoalNodeCreated),
    GoalNodeRetracted(GoalNodeRetracted),
    GoalNodeRewritten(GoalNodeRewritten),
    GoalEdgeDefined(GoalEdgeDefined),
    GoalGraphCheckpointed(GoalGraphCheckpointed),
    CapabilityInvoked(CapabilityInvoked),
    CapabilityResolved(CapabilityResolved),
});

// ---------------------------------------------------------------------------
// Current dispatch ID — set by the event bus in the consumer thread just
// before calling on_event, so consumers can read the canonical ID of the
// triggering event and use it as a parent when emitting new events.
// ---------------------------------------------------------------------------

/// Returns a static string naming the variant of a `RuntimeEvent`.
/// Used for diagnostics and logging.
pub fn event_kind_str(event: &RuntimeEvent) -> &'static str {
    match event {
        RuntimeEvent::LoopObserved(_) => "loop_observed",
        RuntimeEvent::LoopPlanned(_) => "loop_planned",
        RuntimeEvent::LoopActed(_) => "loop_acted",
        RuntimeEvent::LoopVerified(_) => "loop_verified",
        RuntimeEvent::LoopRewarded(_) => "loop_rewarded",
        RuntimeEvent::GoodnessSnapshot(_) => "goodness_snapshot",
        RuntimeEvent::RouteTick(_) => "route_tick",
        RuntimeEvent::RouteSelected(_) => "route_selected",
        RuntimeEvent::CapabilityInvoked(_) => "capability_invoked",
        RuntimeEvent::CapabilityResolved(_) => "capability_resolved",
        RuntimeEvent::CapabilityCompleted(_) => "capability_completed",
        RuntimeEvent::CapabilityFailed(_) => "capability_failed",
        RuntimeEvent::ErrorOccurred(_) => "error_occurred",
        RuntimeEvent::Debug(_) => "debug",
        RuntimeEvent::PromptLoaded(_) => "prompt_loaded",
        RuntimeEvent::RuntimeStateUpdated(_) => "runtime_state_updated",
        RuntimeEvent::ToolCall(_) => "tool_call",
        RuntimeEvent::ToolResult(_) => "tool_result",
        RuntimeEvent::ToolBatchSettled(_) => "tool_batch_settled",
        RuntimeEvent::GoalNodeCreated(_) => "goal_node_created",
        RuntimeEvent::GoalNodeRetracted(_) => "goal_node_retracted",
        RuntimeEvent::GoalNodeRewritten(_) => "goal_node_rewritten",
        RuntimeEvent::GoalEdgeDefined(_) => "goal_edge_defined",
        RuntimeEvent::GoalGraphCheckpointed(_) => "goal_graph_checkpointed",
        RuntimeEvent::GoalSelected(_) => "goal_selected",
        RuntimeEvent::AgentRegistered(_) => "agent_registered",
        RuntimeEvent::RequestDispatch(_) => "request_dispatch",
        RuntimeEvent::SubTaskResult(_) => "sub_task_result",
        RuntimeEvent::Tick(_) => "tick",
        RuntimeEvent::Code(_) => "code",
        RuntimeEvent::Edit(_) => "edit",
        RuntimeEvent::Llm(_) => "llm",
        RuntimeEvent::Cargo(_) => "cargo",
        RuntimeEvent::File(_) => "file",
        RuntimeEvent::Bash(_) => "bash",
        RuntimeEvent::Analysis(_) => "analysis",
        RuntimeEvent::PolicyBaselineUpdated(_) => "policy_baseline_updated",
        RuntimeEvent::SystemConfigLoaded(_) => "system_config_loaded",
        RuntimeEvent::NodeReady(_) => "node_ready",
        RuntimeEvent::NodeStarted(_) => "node_started",
        RuntimeEvent::NodeCompleted(_) => "node_completed",
        RuntimeEvent::NodeFailed(_) => "node_failed",
    }
}

// ---------------------------------------------------------------------------
// EventEmitter trait
// ---------------------------------------------------------------------------

pub trait EventEmitter: Send + Sync {
    /// Forbidden: always panics. Use `emit_with_parents` instead.
    fn emit_located(&self, _event: RuntimeEvent, _file: &'static str, _line: u32) {
        panic!("emit_located forbidden — use emit_with_parents");
    }

    /// The only allowed emission path. Pass an empty vec for genuine root events.
    fn emit_with_parents(
        &self,
        event: RuntimeEvent,
        parents: Vec<crate::EventId>,
        file: &'static str,
        line: u32,
    );

    fn emit_child(&self, event: RuntimeEvent, parents: Vec<crate::EventId>, file: &'static str, line: u32) {
        self.emit_with_parents(event, parents, file, line)
    }
}

pub type EventEmitterHandle = Arc<dyn EventEmitter>;

#[derive(Debug, Clone)]
pub struct LocatedEvent {
    pub event: RuntimeEvent,
    pub file: &'static str,
    pub line: u32,
    pub parent_ids: Vec<crate::EventId>,
}

#[derive(Debug, Clone, Copy)]
pub enum EventFilter {
    All,
    ErrorOnly,
    Code(EventMask),
    EditOnly,
    CapabilityOnly,
}

/// Every consumer must return an `EventOutcome` from `on_event`.
/// Returning `()` is a compile error — every path must declare intent.
#[derive(Debug)]
pub enum EventOutcome {
    Emit(RuntimeEvent),
    EmitMany(Vec<RuntimeEvent>),
    /// Explicit no-op — the `&'static str` is a required reason string.
    NoOp(&'static str),
    Error(RuntimeEvent),
}

pub trait EventConsumer: Send + Sync {
    fn filter(&self) -> EventFilter;
    fn on_event(&mut self, event: &RuntimeEvent, trigger_id: crate::EventId) -> EventOutcome;
    fn set_emitter(&mut self, _emitter: EventEmitterHandle) {}
}

// ---------------------------------------------------------------------------
// Capability result types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProcessResult {
    pub status: i32,
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LlmResult {
    pub success: bool,
    pub duration_ms: u64,
    pub response: serde_json::Value,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum CapabilityResult {
    Process(ProcessResult),
    Llm(LlmResult),
    Empty,
}

impl Default for CapabilityResult {
    fn default() -> Self {
        Self::Empty
    }
}

impl CapabilityResult {
    pub fn kind_str(&self) -> &'static str {
        match self {
            CapabilityResult::Process(_) => "process",
            CapabilityResult::Llm(_) => "llm.call",
            CapabilityResult::Empty => "empty",
        }
    }

    pub fn duration_ms(&self) -> Option<u64> {
        match self {
            CapabilityResult::Process(_) => None,
            CapabilityResult::Llm(llm) => Some(llm.duration_ms),
            CapabilityResult::Empty => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Node lifecycle events
// ---------------------------------------------------------------------------

canon_event_struct!(
    #[impl_shape]
    CapabilityCompleted {
        #[input] request_id: String,
        #[input] capability: &'static str,
        #[output] result: CapabilityResult,
    }
);

canon_event_struct!(
    #[impl_shape]
    CapabilityFailed {
        #[input] request_id: String,
        #[input] capability: &'static str,
        #[output] error: String,
    }
);

canon_event_struct!(
    #[impl_shape]
    NodeReady {
        #[input] node_id: String,
        #[input] capability: String,
        #[serde(default)]
        request_id: String,
        #[output] ready: bool,
    }
);

canon_event_struct!(
    #[impl_shape]
    NodeStarted {
        #[input] node_id: String,
        #[input] capability: String,
        #[serde(default)]
        request_id: String,
        #[output] started: bool,
    }
);

canon_event_struct!(
    #[impl_shape]
    NodeCompleted {
        #[input] node_id: String,
        #[input] capability: String,
        #[serde(default)]
        request_id: String,
        #[output] completed: bool,
    }
);

canon_event_struct!(
    #[impl_shape]
    NodeFailed {
        #[input] node_id: String,
        #[input] capability: String,
        #[output] error: Option<String>,
        #[serde(default)]
        request_id: String,
    }
);
