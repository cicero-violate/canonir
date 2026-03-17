// Cluster A: pure goal/planning types — re-exported from canon-goal
pub use canon_goal::capability_types;
pub use canon_goal::decompose;
pub use canon_goal::goal;
pub use canon_goal::goal_embedding;
pub use canon_goal::task_graph;
pub use canon_goal::task_graph_patch;

// Cluster B: LLM runtime, network, process
pub mod config;
pub mod endpoint_worker;
pub mod failure_store;
pub mod gpu_scheduler_kernels;
pub mod gpu_scheduler_layout;
pub mod graph_algo;
pub mod llm;
pub mod llm_domains;
pub mod objectives;
pub mod parsers;
pub mod response_router;
pub mod tab_management;
pub mod telemetry;
pub mod ws_server;
