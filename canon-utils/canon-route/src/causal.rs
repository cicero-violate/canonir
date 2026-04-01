use std::collections::HashMap;

use canon_event::{LoopActed, LoopPlanned, RuntimeEvent, ToolCall, ToolResult};

#[derive(Debug, Clone)]
pub enum CausalNodeKind {
    LlmCall { request_id: String, role: Option<String>, agent_id: Option<String> },
    LoopPlanned { action_id: String, action_kind: String },
    ToolCall { tool_call_id: String, kind: String },
    ToolResult { tool_result_id: String, success: bool },
    LoopActed { action_id: Option<String>, success: bool },
    LoopVerified { passed: bool },
    // REMOVED: RequestDispatch node (synthetic dispatch eliminated)
    SubTaskResult { dispatch_id: String, agent_id: String, success: bool },
}

#[derive(Debug, Clone)]
pub struct CausalEdge {
    pub from: String,
    pub to: String,
    pub kind: &'static str,
}

#[derive(Default, Debug, Clone)]
pub struct CausalGraph {
    pub nodes: HashMap<String, CausalNodeKind>,
    pub edges: Vec<CausalEdge>,
}

impl CausalGraph {
    fn upsert_node(&mut self, key: &str, kind: CausalNodeKind) {
        self.nodes.entry(key.to_string()).or_insert(kind);
    }

    pub fn record_llm_call(&mut self, request_id: &str, role: Option<&str>, agent_id: Option<&str>) {
        self.upsert_node(request_id, CausalNodeKind::LlmCall { request_id: request_id.to_string(), role: role.map(str::to_string), agent_id: agent_id.map(str::to_string) });
    }

    pub fn record_planned(&mut self, planned: &LoopPlanned) {
        if let Some(action_id) = &planned.action_id {
            self.upsert_node(action_id, CausalNodeKind::LoopPlanned { action_id: action_id.clone(), action_kind: planned.action_kind.clone() });
            if let Some(llm_req) = &planned.llm_request_id {
                self.edges.push(CausalEdge { from: llm_req.clone(), to: action_id.clone(), kind: "caused" });
            }
            for dep in &planned.depends_on {
                self.edges.push(CausalEdge { from: dep.clone(), to: action_id.clone(), kind: "depends_on" });
            }
        }
    }

    pub fn record_tool_call(&mut self, tc: &ToolCall) {
        self.upsert_node(&tc.tool_call_id, CausalNodeKind::ToolCall { tool_call_id: tc.tool_call_id.clone(), kind: tc.kind.clone() });
        if let Some((action_id, _)) = tc.node_id.split_once(':') {
            self.edges.push(CausalEdge { from: action_id.to_string(), to: tc.tool_call_id.clone(), kind: "triggered" });
        }
    }

    pub fn record_tool_result(&mut self, tr: &ToolResult) {
        self.upsert_node(&tr.tool_result_id, CausalNodeKind::ToolResult { tool_result_id: tr.tool_result_id.clone(), success: tr.success });
        self.edges.push(CausalEdge { from: tr.tool_call_id.clone(), to: tr.tool_result_id.clone(), kind: "resolved" });
    }

    pub fn record_acted(&mut self, acted: &LoopActed) {
        let key = acted.tool_result_id.as_deref().unwrap_or(&acted.capability_request_id).to_string();
        self.upsert_node(&key, CausalNodeKind::LoopActed { action_id: acted.action_id.clone(), success: acted.success });
        if let Some(action_id) = &acted.action_id {
            self.edges.push(CausalEdge { from: action_id.clone(), to: key.clone(), kind: "executed" });
        }
        if let Some(tr) = &acted.tool_result_id {
            self.edges.push(CausalEdge { from: tr.clone(), to: key.clone(), kind: "resulted_in" });
        }
    }

    pub fn record_dispatch(&mut self, _dispatch_id: &str, _from_agent: &str, _to_agent: &str) {
        // REMOVED: RequestDispatch causal node creation
    }

    pub fn record_sub_result(&mut self, dispatch_id: &str, agent_id: &str, success: bool) {
        let key = format!("subresult:{dispatch_id}");
        self.upsert_node(&key, CausalNodeKind::SubTaskResult { dispatch_id: dispatch_id.to_string(), agent_id: agent_id.to_string(), success });
        self.edges.push(CausalEdge { from: dispatch_id.to_string(), to: key, kind: "completed" });
    }
}

pub fn update_causal_graph(cg: &mut CausalGraph, event: &RuntimeEvent) {
    match event {
        RuntimeEvent::LoopPlanned(p) => cg.record_planned(p),
        RuntimeEvent::LoopActed(a) => cg.record_acted(a),
        RuntimeEvent::ToolCall(tc) => cg.record_tool_call(tc),
        RuntimeEvent::ToolResult(tr) => cg.record_tool_result(tr),
        // REMOVED: RequestDispatch handling (synthetic dispatch eliminated)
        RuntimeEvent::SubTaskResult(r) => cg.record_sub_result(&r.dispatch_id, &r.agent_id, r.success),
        _ => {}
    }
}
