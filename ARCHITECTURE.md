Comprehensive Analysis: Canon WebSocket Server, LLM Dispatch, and Tab Management System

Based on my thorough exploration of the codebase, here's a complete technical breakdown of how the Chromium WebSocket server, LLM dispatch, and tab management works:

---
1. CHROMIUM WEBSOCKET SERVER ARCHITECTURE

Location: /workspace/ai_sandbox/canon/canon-utils/canon-llm-runtime/src/ws_server.rs (521 lines)

Core Design:
- Single shared WS connection from Chrome extension (background.js) to Rust runtime
- Port: 127.0.0.1:9100 (configurable via CANON_LLM_BRIDGE_ADDR)
- Protocol: JSON-based frames with tabId tagging for multiplexing

Message Types:

Extension → Rust (inbound):
TAB_OPENED       { tabId, url, reqId? }
TAB_CLOSED       { tabId }
TAB_READY        { tabId, url, reqId? }
INBOUND_MESSAGE  { tabId, payload (streamed response) }
NEW_CHAT_DONE    { tabId }
TEMP_CHAT_DONE   { tabId }
PING             (keepalive)

Rust → Extension (outbound):
OPEN_TAB         { url, reqId? }
TURN             { tabId, text, turnId? }
OUTBOUND_SUBMIT  { tabId, payload (prompt injection) }
CLOSE_TAB        { tabId }
NEW_CHAT         { tabId }
TEMP_CHAT        { tabId }

Server State (lines 77-107):
- out_tx: outbound channel to extension WS
- tab_assemblers: tabId → FrameAssembler (buffering streamed responses)
- pending: tabId → oneshot waiting for completed response
- pending_turn_id: tabId → expected turnId (for filtering chunks)
- pending_open: reqId → oneshot waiting for TAB_OPENED confirmation
- live_tabs: set of active tabIds
- turn_replay_queue: buffers TURN frames while WS is disconnected
- Frame dumping to ./frames/inbound.jsonl and ./frames/assembled.jsonl

Key APIs (lines 158-275):
- open_fresh_tab_with_url() - async, waits for extension to confirm via TAB_OPENED
- send_turn() - sends prompt, buffers response chunks, reassembles, returns full text
- wait_for_connection() - blocks until extension WS connects
- new_chat(), temp_chat() - reset chat state
- wait_new_chat(), wait_temp_chat() - wait for completion signals

Inbound Handler (lines 370-520):
- TAB_READY gates Rust TURN dispatch (comment line 2): "TAB_READY (not TAB_OPENED) gates Rust TURN dispatch"
- Turn filtering by pending_turn_id (lines 452-460)
- Assembler-driven frame buffering with sequence tracking
- Automatic re-injection on startup (lines 236-242)

---
2. CHROMIUM EXTENSION ARCHITECTURE

Location: /workspace/ai_sandbox/canon/canon-chromium-extension/

Background Script (background.js, 246 lines):
- Lines 4: WS connects to ws://127.0.0.1:9100
- Lines 6-15: Tracks pendingOpenReqIds, tabOriginalUrls, pendingNewChatNavigations
- Lines 79-132: chrome.runtime.onMessage listener
  - INBOUND_MESSAGE (lines 83-101) - captures streamed response chunks from page
  - CONTENT_READY (lines 104-116) - signals tab content is ready (TAB_READY sent)
  - NEW_CHAT_DONE, TEMP_CHAT_DONE (lines 118-128) - completion signals
- Lines 136-225: handleRustMessage dispatcher
  - OPEN_TAB: creates tab, tracks reqId, sets auto-discard=false
  - TURN: activates window, sets tab active, sends OUTBOUND_SUBMIT (lines 198-224)
  - NEW_CHAT: navigates to original URL for custom GPTs (lines 170-181)
  - TEMP_CHAT, CLOSE_TAB: simple pass-through
- Lines 228-233: Tab lifecycle cleanup (onRemoved)
- Line 245: Initial connect() call

Content Script (content.js, 96 lines):
- Lines 29-36: Injects bridge, posts BRIDGE_READY to page
- Lines 40-67: Page → content listener
  - INBOUND_MESSAGE with turn_id patching (lines 46-58)
  - NEW_CHAT_DONE, TEMP_CHAT_DONE signal forwarding
- Lines 71-94: Content → page listener
  - OUTBOUND_SUBMIT: postMessage to page with turn_id (lines 72-79)
  - NEW_CHAT, TEMP_CHAT: postMessage to page

Injected Page Bridge (inject.js, lines 100+):
- WebSocket hook (lines 21-33): intercepts all WS messages, emits inbound chunks
- Fetch hook (lines 36-93): intercepts ChatGPT/Gemini API calls
  - Targets: /backend-api/f/conversation, /backend-api/calpico
  - Stream reads (lines 66-93) emit chunks as INBOUND_MESSAGE
  - Line 82: detects [DONE] marker
- Globals for prompt injection: __pendingPromptInjection, __promptInjectionMode, __currentTurnId

---
3. LLM DISPATCH AND ROUTING SYSTEM

Location: /workspace/ai_sandbox/canon/canon-utils/canon-llm-runtime/src/

Flow: Event → CapabilityExecutor → LlmCall event → LLM worker thread → endpoint selection → tab assignment → WS TURN → response

Endpoint Resolution (endpoint_worker.rs lines 48-126):

The system uses hierarchical endpoint selection (lines 110-126):

let selected = if let Some(aid) = agent_id.as_deref() {
    // 1. Match by endpoint ID exactly
    config.llm_endpoints.iter().find(|e| e.id == aid)
        .or_else(|| config.llm_endpoints.iter().find(|e| e.url.contains(aid)))
        // 2. Fallback: treat agent_id as role name
        .or_else(|| config.llm_endpoints.iter().find(|e| e.role.as_deref() == Some(aid)))
} else if let Some(role_name) = role.as_deref() {
    // 3. Match by role
    config.llm_endpoints.iter().find(|e| e.role.as_deref() == Some(role_name))
        .or_else(|| if role_name == "router" { config.llm_endpoints.iter().find(|e| e.role.as_deref() == Some("planner")) } else { None })
} else {
    // 4. Default to first endpoint
    config.llm_endpoints.first()
};

LlmCall Event Structure (canon-runtime-events/src/events.rs, lines 856-893):
LlmCall {
    request_id: String,
    prompt: String,               // Dynamic delta
    role: Option<String>,         // "planner", "router", "exec", "analyst", etc.
    agent_id: Option<String>,     // Override role or endpoint ID
    dispatched: bool,             // Output field
    system: Option<String>,       // Static system prompt (first call only)
    system_prompt_id: Option<String>,  // Cache key for system
    context_base: Option<String>,      // Slow-changing context
    context_base_id: Option<String>,   // Cache key for context
    prompt_base_id: Option<String>,    // Hash for tracing
    prev_prompt_id: Option<String>,    // Causal chain
}

RequestDispatch Event (canon-runtime-events/src/events.rs, lines 898-910):
RequestDispatch {
    dispatch_id: String,
    parent_request_id: String,
    agent_id: String,
    task_prompt: String,
    task_kind: String,
    deps: Vec<String>,
    workspace_scope: Option<String>,
    dispatched: bool,
}

---
4. ROLE/ENDPOINT SYSTEM

Configuration Loading (canon-llm-runtime/src/config.rs, lines 364-447):

Loaded from /workspace/ai_sandbox/canon/canon-agent-prompts/capability_config.toml

Endpoint Definition (lines 294-305):
pub struct LlmEndpoint {
    pub id: String,              // Unique ID (e.g., "planner_chatgpt_group")
    pub url: String,             // ChatGPT/Gemini URL
    pub role_markdown: String,   // Builtin role prompt ID
    pub role: Option<String>,    // "planner", "exec", "analyst", etc.
    pub stateful: bool,          // Session persistence (true = cache sys+base)
    pub max_tabs: usize,         // Concurrent tabs allowed
}

Role Configuration (lines 289-293):
pub struct RoleConfig {
    pub weights: HashMap<String, u32>,  // Endpoint selection weights
    pub burst: Option<usize>,           // Max concurrent requests
}

Example from capability_config.toml:
[llm.endpoints.planner]
id = "planner_chatgpt_group"
url = "https://chatgpt.com/gg/69c897e5a6448198a36a18b58f83de07"
role = "planner"
stateful = true
max_tabs = 1

[llm.endpoints.exec_chatgpt_a]
id = "exec_chatgpt_a"
url = "https://chatgpt.com/gg/699c50e06bc881a3aa5ac1866bf15679"
role = "exec"
stateful = true
max_tabs = 1

[llm.roles.analyst.weights]
analyst_chatgpt = 100

[llm.roles.analyst]
burst = 1

Key Roles in the system:
- planner - Main orchestration (lines 59-65 in config)
- router - Route selection (fallback to planner, lines 67-73)
- exec - Action execution (multiple: exec_chatgpt_a…f, lines 113-150)
- analyst - Error analysis (lines 83-89)
- goal_gen - Goal generation (lines 75-81)
- harness_repair - Test failure repair (lines 91-97)
- harness_eventlog - Event log analysis (lines 99-105)

---
5. TAB MANAGEMENT SYSTEM

Location: /workspace/ai_sandbox/canon/canon-utils/canon-llm-runtime/src/tab_management.rs

TabSlotTable (lines 5-13):
pub struct TabSlotTable {
    pub owner: HashMap<String, u32>,         // endpoint_id → tabId
    pub meta: HashMap<u32, TabSlotMeta>,     // tabId → metadata
}

TabSlotMeta (lines 14-20):
pub struct TabSlotMeta {
    pub last_sent_ms: Option<u128>,          // When TURN was sent
    pub last_response_ms: Option<u128>,      // When response arrived
    pub in_flight: bool,                     // Active request in progress
    pub cooldown_until_ms: Option<u128>,     // Rate limiting
}

Tab Lifecycle (lines 22-42):
1. tab_manager_get_or_open_tab() - tries to reuse owner tab, else opens fresh
2. tab_manager_set_tab_id() - registers endpoint→tabId mapping
3. tab_manager_mark_tab_sent() - records outgoing TURN
4. tab_manager_mark_tab_response() - records incoming response
5. tab_manager_mark_tab_in_flight() - lock/unlock
6. tab_manager_mark_tab_cooldown() - rate-limit window
7. tab_manager_drop_tab() - cleanup on failure

Stateful vs Stateless:
- Stateful endpoints (e.g., planner): reuse same tab (line 23), send role_schema only on first turn (endpoint_worker.rs lines 78-83)
- Stateless endpoints (e.g., exec): NEW_CHAT after every TURN (lines 127-148), reconstruct full prompt each time

---
6. EVENT SYSTEM AND FLOW

Location: /workspace/ai_sandbox/canon/canon-utils/canon-runtime-events/src/events.rs

RuntimeEvent Master Enum (lines 991-1041):
Primary event types include:
- Loop Stage Events: LoopObserved, LoopPlanned, LoopActed, LoopVerified, LoopRewarded
- Routing: RouteSelected, RouteTick
- LLM Capability: Llm(LlmCall) - line 1011
- Capability Lifecycle: CapabilityInvoked (line 1039), CapabilityResolved (line 1040), CapabilityCompleted (line 1020), CapabilityFailed (line 1021)
- Sub-agent: RequestDispatch (line 1012), SubTaskResult (line 1013)
- Analysis: Analysis(AnalysisEvent)
- Tools: ToolCall, ToolResult, ToolBatchSettled

CapabilityCompleted (lines 1238-1244):
CapabilityCompleted {
    request_id: String,
    capability: &'static str,
    result: CapabilityResult,    // Process(ProcessResult) | Llm(LlmResult) | Empty
}

LlmResult (lines 1194-1198):
pub struct LlmResult {
    pub success: bool,
    pub duration_ms: u64,
    pub response: serde_json::Value,
}

EventEmitter Trait (lines 1109-1129):
- emit_with_parents(event, parents_vec, file, line) - only allowed path
- emit_located() - forbidden (panics)
- Parent IDs track causal chains

EventOutcome (lines 1150-1171):
- Emit { event, file, line }
- EmitMany { events, file, line }
- NoOp("reason")
- Error { event, file, line }

---
7. REQUEST ID TRACKING AND RESPONSE ROUTING

Location: /workspace/ai_sandbox/canon/canon-utils/canon-llm-runtime/src/response_router.rs (13 lines)

static ROUTES: Lazy<Mutex<HashMap<u64, String>>> = Lazy::new(|| Mutex::new(HashMap::new()));

pub async fn response_router_register(req_id: u64, node_id: &str) {
    routes.insert(req_id, node_id.to_string());
}

pub async fn response_router_resolve(req_id: u64) -> Option<String> {
    routes.remove(&req_id)
}

Flow (endpoint_worker.rs lines 62-63):
1. When LlmWorkItem arrives, register: response_router_register(req.req_id, node_id)
2. After response received: response_router_resolve(req_id) retrieves the node_id
3. This maps the response back to the requesting goal graph node

---
8. REQUEST ID IN RESPONSES

Prompt Injection (ws_server.rs lines 178-209):
- Line 180: req_id = self.next_req_id.fetch_add(1, Ordering::Relaxed)
- Line 193: Frame includes "turnId": turn_id (separate counter)
- Line 180: Injected as turn_id in payload to page

Response Validation (endpoint_worker.rs lines 119-125):
if !llm_worker_response_matches_req_id(&raw, req_id) {
    if !allow_req_id_mismatch {
        return Err(anyhow::anyhow!("req_id mismatch"));
    }
}

Request ID in LLM Execution (canon-exec/src/exec/llm.rs lines 180, 196):
let prompt_with_request_id = format!("{{\"request_id\":\"{}\"}}\n{}", request_id, full_prompt);
// Later logged with args: json!({ "prompt": prompt_with_request_id, "role": role })

---
9. RUNTIME SUPERVISOR AND BOOTSTRAP

Location: /workspace/ai_sandbox/canon/canon-utils/canon-runtime-supervisor/src/

binary_supervisor.rs (40 lines):
pub fn run_binary_supervisor(binary_path: &Path) {
    // Watches binary modification time
    // On change: kills old process, spawns new one with args:
    //   --tlog /workspace/ai_sandbox/canon/state/event_log/event.tlog.d
    // Sleep interval: 200ms
}

supervisor.rs (lines 1-9):
- Runs canon-runtime binary via run_binary_supervisor()
- Binary path: /workspace/ai_sandbox/canon/target/debug/canon-runtime

Event Runtime Bootstrap (canon-runtime/src/bin/event_runtime.rs):
- Lines 180-200: Parses --tlog argument (required)
- Acquires lock at /workspace/ai_sandbox/canon/state/event_log/event.tlog.d.lock
- Loads consumers: LoopStageExecutor, RouteExecutor, CapabilityExecutor, etc.
- Replay mode: loads prior events, replays to catch up
- Watch mode: monitors tlog for new events

---
10. MULTI-AGENT AND MULTI-ROLE CONCURRENCY

Location: /workspace/ai_sandbox/canon/canon-utils/canon-runtime/src/consumers/

Dispatch Consumer (dispatch_consumer.rs, lines 222-250):
- Manages exec endpoint assignment via round-robin (lines 238-250)
- Spawns sub-agent EventRuntime in separate thread (lines 155-216)
- Sub-agent loop monitors LoopObserved (seeded with task_prompt), runs to halt or timeout

Agent Registry (agent_registry.rs, lines 16-76):
- Tracks agent statuses: Idle, Busy { dispatch_id }, Failed { reason }
- available_agents(role) filters by role and Idle status
- Sub-agents emit AgentRegistered event with card payload

Concurrency Limits:
1. max_concurrency (capability_config.toml line 11): 4 - global concurrent capability requests
2. max_tabs per endpoint (config.rs line 304): e.g., max_tabs = 1 for planner (single stateful session)
3. tab_cooldown_ms (capability_config.toml line 57): 4000 - cooldown between TURNs on same tab
4. burst (role config, line 292): Max concurrent requests for a role (e.g., analyst burst=1)
5. SUB_AGENT_TIMEOUT_SECS (dispatch_consumer.rs line 19): 300 - sub-agent deadline

---
11. CAPABILITY_CONFIG.TOML LOADING AND RUNTIME PARSING

Location: /workspace/ai_sandbox/canon/canon-agent-prompts/capability_config.toml

Load Path (config.rs line 5):
const CAPABILITY_CONFIG_TOML: &str = "/workspace/ai_sandbox/canon/canon-agent-prompts/capability_config.toml";

Where Loaded:
1. LLM Worker Init (canon-exec/src/exec/llm.rs line 63):
let config = CapabilityConfig::snapshot_store_load()?;
2. Dispatch Consumer (dispatch_consumer.rs line 16):
load_exec_endpoint_ids() → CapabilityConfig::snapshot_store_load()
3. Endpoint Worker (endpoint_worker.rs) - passed to worker threads

Parsing (config.rs lines 364-447):
- Calls CapabilityConfig::snapshot_store_load()
- Deserializes TOML via serde
- Extracts endpoints from map, finds planner endpoint
- Returns CapabilityConfig struct with all settings

Runtime Usage:
- Planner endpoint retrieved (line 461): pub fn planner_endpoint(&self) -> Result<&LlmEndpoint>
- Endpoints filtered by role (exec endpoints via load_exec_endpoint_ids)
- Tab cooldown applied (tab_management.rs line 215)
- Response timeout used (line 274)

---
12. STATE MACHINE AND SESSION STATE TRACKING

Tab Session State (tab_management.rs lines 14-20):
in_flight: bool            → TURN sent, waiting for response
last_sent_ms: Option       → Timestamp of last TURN
last_response_ms: Option   → Timestamp of last response
cooldown_until_ms: Option  → Rate-limit expiry

Endpoint Worker State (endpoint_worker.rs lines 32-46):
tabs_with_role_sent: HashSet<u32>  → Which tabs got system prompt (stateful optimization)
cache: HashMap<u64, String>        → Response cache by prompt hash
seen_hashes: HashSet<u64>          → Dedup detection

LLM Worker System State (canon-exec/src/exec/llm.rs lines 87-90):
system_cache: HashMap<String, String>      → Cached static system prompts
context_base_cache: HashMap<String, String> → Cached slow-changing context
llm_call_counter: u32                       → Request numbering for logs

Sub-Agent Session State (dispatch_consumer.rs lines 155-216):
halted: Arc<AtomicBool>              → Loop halt signal
actions_taken: Arc<Mutex<Vec<String>>> → Collected action IDs for SubTaskResult

Agent Registry State (agent_registry.rs lines 16-76):
agents: HashMap<String, AgentCard>   → Known agents with status

---
13. KEY ARCHITECTURAL INSIGHTS

One-to-Many Message Routing:
- Single WS connection carries all tabIds (multiplexed)
- Request ID embedded in injected prompt for echo-back validation
- Response routing via request_id → node_id map

Stateful vs Stateless Endpoints:
- Stateful (planner): Reuses tab, sends role_schema once, keeps context in LLM memory
- Stateless (exec): Fresh NEW_CHAT each turn, reconstructs full prompt from cache

Prompt Caching Hierarchy:
- Tier 1: System prompt (static, cached by system_prompt_id)
- Tier 2: Context base (slow-changing, cached by context_base_id)
- Tier 3: Delta prompt (always fresh per request)

Concurrency Control:
- Per-endpoint tab limits (max_tabs)
- Per-role burst limits
- Global max_concurrency cap
- Tab cooldown rate-limiting

Event Flow (harness → browser → response):
1. LlmCall event emitted (from LoopPlanned or other source)
2. CapabilityExecutor spawns async thread
3. LLM worker thread picks endpoint by (agent_id | role | default)
4. Gets or opens tab via WsBridge.open_fresh_tab_with_url()
5. Sends TURN via WsBridge.send_turn() with injected request_id
6. Page echoes request_id in response chunks
7. FrameAssembler collects chunks, assembles full response
8. Response returned to worker, matched against expected request_id
9. For stateless endpoints: NEW_CHAT issued, tab reset
10. CapabilityCompleted event emitted with LlmResult
11. SubTaskResult collected (if sub-agent)
12. Response routed back to requesting node via request_id map

---
14. KEY FILE LOCATIONS SUMMARY

┌─────────────────────┬────────────────────────────────────────────────────────────────────────────────────────────┬───────┐
│      Component      │                                            Path                                            │ Lines │
├─────────────────────┼────────────────────────────────────────────────────────────────────────────────────────────┼───────┤
│ WS Server           │ /workspace/ai_sandbox/canon/canon-utils/canon-llm-runtime/src/ws_server.rs                 │ 521   │
├─────────────────────┼────────────────────────────────────────────────────────────────────────────────────────────┼───────┤
│ Background Script   │ /workspace/ai_sandbox/canon/canon-chromium-extension/background.js                         │ 246   │
├─────────────────────┼────────────────────────────────────────────────────────────────────────────────────────────┼───────┤
│ Content Script      │ /workspace/ai_sandbox/canon/canon-chromium-extension/content.js                            │ 96    │
├─────────────────────┼────────────────────────────────────────────────────────────────────────────────────────────┼───────┤
│ Injected Bridge     │ /workspace/ai_sandbox/canon/canon-chromium-extension/inject.js                             │ 340+  │
├─────────────────────┼────────────────────────────────────────────────────────────────────────────────────────────┼───────┤
│ Config Loading      │ /workspace/ai_sandbox/canon/canon-utils/canon-llm-runtime/src/config.rs                    │ 503   │
├─────────────────────┼────────────────────────────────────────────────────────────────────────────────────────────┼───────┤
│ Tab Management      │ /workspace/ai_sandbox/canon/canon-utils/canon-llm-runtime/src/tab_management.rs            │ 100   │
├─────────────────────┼────────────────────────────────────────────────────────────────────────────────────────────┼───────┤
│ Endpoint Worker     │ /workspace/ai_sandbox/canon/canon-utils/canon-llm-runtime/src/endpoint_worker.rs           │ 240+  │
├─────────────────────┼────────────────────────────────────────────────────────────────────────────────────────────┼───────┤
│ Response Router     │ /workspace/ai_sandbox/canon/canon-utils/canon-llm-runtime/src/response_router.rs           │ 13    │
├─────────────────────┼────────────────────────────────────────────────────────────────────────────────────────────┼───────┤
│ LLM Execution       │ /workspace/ai_sandbox/canon/canon-utils/canon-exec/src/exec/llm.rs                         │ 400+  │
├─────────────────────┼────────────────────────────────────────────────────────────────────────────────────────────┼───────┤
│ Runtime Events      │ /workspace/ai_sandbox/canon/canon-utils/canon-runtime-events/src/events.rs                 │ 1300+ │
├─────────────────────┼────────────────────────────────────────────────────────────────────────────────────────────┼───────┤
│ Dispatch Consumer   │ /workspace/ai_sandbox/canon/canon-utils/canon-runtime/src/consumers/dispatch_consumer.rs   │ 280+  │
├─────────────────────┼────────────────────────────────────────────────────────────────────────────────────────────┼───────┤
│ Agent Registry      │ /workspace/ai_sandbox/canon/canon-utils/canon-runtime/src/consumers/agent_registry.rs      │ 178   │
├─────────────────────┼────────────────────────────────────────────────────────────────────────────────────────────┼───────┤
│ Capability Executor │ /workspace/ai_sandbox/canon/canon-utils/canon-runtime/src/consumers/capability_executor.rs │ 100   │
├─────────────────────┼────────────────────────────────────────────────────────────────────────────────────────────┼───────┤
│ Runtime Supervisor  │ /workspace/ai_sandbox/canon/canon-utils/canon-runtime-supervisor/src/binary_supervisor.rs  │ 40    │
├─────────────────────┼────────────────────────────────────────────────────────────────────────────────────────────┼───────┤
│ Event Runtime       │ /workspace/ai_sandbox/canon/canon-utils/canon-runtime/src/bin/event_runtime.rs             │ 400+  │
├─────────────────────┼────────────────────────────────────────────────────────────────────────────────────────────┼───────┤
│ Config TOML         │ /workspace/ai_sandbox/canon/canon-agent-prompts/capability_config.toml                     │ 150+  │
└─────────────────────┴────────────────────────────────────────────────────────────────────────────────────────────┴───────┘

This is a sophisticated event-driven system with explicit causal chains, request ID tracking, and stateful tab management designed for multi-role, multi-agent orchestration across browser automation and LLM interactions.
