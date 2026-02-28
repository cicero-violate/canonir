//! L3 Capability Graph — stateless LLM call orchestration layer.
//!
//! Each CapabilityNode represents one stateless agent call over a typed
//! slice of CanonicalIr. The CapabilityGraph defines which nodes exist,
//! which fields they may read/write, and how their outputs chain together.
//!
//! State lives exclusively in CanonicalIr and CapabilityGraph edges.
//! Nothing inside a node is stateful.
pub mod bootstrap;
pub mod call;
pub mod capability;
pub mod dispatcher;
pub mod io;
pub mod llm_provider;
pub mod meta;
pub mod observe;
pub mod pipeline;
pub mod refactor;
pub mod reward;
pub mod runner;
pub mod slice;
pub mod sse;
pub mod ws_server;
pub use bootstrap::{seed_capability_graph, seed_refactor_proposal};
pub use call::{
    AgentCallError, AgentCallId, AgentCallInput, AgentCallOutput, AgentCallResult,
};
pub use capability::{CapabilityEdge, AgentGraph, CapabilityKind, AgentNode, IrField};
pub use dispatcher::{AgentScheduler, DEFAULT_TRUST_THRESHOLD};
pub use io::{load_capability_graph, save_capability_graph};
pub use llm_provider::{call_llm, LlmProviderError};
pub use meta::{
    evolve_capability_graph, GraphEvolutionError, GraphEvolutionResult, GraphMutation,
    MAX_ENTROPY_DELTA, MIN_NODES, UNDERPERFORM_THRESHOLD,
};
pub use observe::{analyze_ir, ir_observation_to_json, IrAnalysisReport, IrTotals};
pub use pipeline::record_refactor_reward;
pub use pipeline::{run_refactor_pipeline, RefactorError, RefactorResult, RefactorStage};
pub use refactor::{RefactorKind, RefactorProposal, RefactorTarget};
pub use reward::{NodeRewardEntry, NodeRewardLedger, PipelineNodeOutcome};
pub use runner::{run_agent, RunnerConfig, RunnerError, TickStats};
pub use slice::slice_ir_fields;
pub use ws_server::WsBridge;
