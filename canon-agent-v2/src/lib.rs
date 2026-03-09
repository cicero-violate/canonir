pub mod call;
pub mod capability;
pub mod act;
pub mod capability_cost;
pub mod capability_types;
pub mod config;
pub mod console;
pub mod dag;
pub mod decompose;
pub mod dispatch;
pub mod endpoint_scheduler;
pub mod endpoint_worker;
pub mod engine;
pub mod execution_result;
pub mod executor_dispatch;
pub mod failure_store;
pub mod goal_embedding;
pub mod gpu_scheduler;
pub mod gpu_scheduler_driver;
pub mod gpu_scheduler_kernels;
pub mod gpu_scheduler_layout;
pub mod graph_algo;
pub mod graph_maintenance;
pub mod graph_runtime;
pub mod llm;
pub mod planner_session;
pub mod planner_state;
pub mod planner_update;
pub mod policy;
pub mod policy_engine;
pub mod policy_train;
pub mod response_router;
pub mod scheduler;
pub mod scheduler_scoring;
pub mod scheduler_state;
pub mod state_snapshot;
pub mod tab_management;
pub mod telemetry;
pub mod template_index;
pub mod template_mutation;
pub mod templates;
pub mod agent_loop;
pub mod ir;
pub mod llm_domains;
pub mod llm_provider;
pub mod parsers;
pub mod pipelines;
pub mod runtime;
pub mod ws_server;

pub use capability::{ExecutionDelta, LOG_ROOT, TEMPLATE_ROOT};

pub mod layout {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, Default)]
    pub struct FileTopology;
}
