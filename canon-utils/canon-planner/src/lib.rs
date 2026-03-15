// Canon Planner - Unified planning, graph building, and analysis
//
// This crate provides a unified interface to the planning subsystem by re-exporting
// from three specialized crates:
// - canon-agent-v3: Planning, goal decomposition, DAG building, mutation engine
// - canon-graph: Graph building, node/edge management, artifacts
// - canon-analysis: Scoring, invariants, SMT, repair

//! # Canon Planner
//!
//! Unified planner providing:
//! - **Graph Building** (`graph` module)
//! - **Planning & Mutation** (`planner` module)
//! - **Analysis & Scoring** (`analysis` module)
//!
//! This implements the equation: **State → Plan → Capability**

// === Graph Module ===
/// Graph building and management
pub mod graph {
    pub use canon_graph::*;
}

// === Planner Module ===
/// Agent planning, goal decomposition, DAG building, mutation engine
pub mod planner {
    pub use canon_agent_v3::*;
}

// === Analysis Module ===
/// Scoring, invariants, SMT, repair
pub mod analysis {
    pub use canon_analysis::*;
}

// Re-export commonly used types at the top level for convenience
pub use graph::{GraphConsumer, KernelGraph, GraphNode, GraphEdge, CsrGraph};
pub use analysis::{
    run_invariant_pipeline,
    generate_reports,
    generate_reports_from_tlog,
    ReportEventConsumer,
    CapabilityEventConsumer,
    SmtConsumer,
    register_analysis_capabilities,
};
