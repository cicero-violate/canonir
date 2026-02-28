pub mod bootstrap;
pub mod call;
pub mod capability;
pub mod dispatcher;
pub mod io;
pub mod llm_provider;
pub mod meta;
pub mod observe;
pub mod reward;
pub mod runner;
pub mod agent_config;
pub mod slice;
pub mod sse;
pub mod ws_server;

pub mod ir;
pub mod evolution;
pub mod agent_commands;
pub mod emit_shell;
pub mod executor;

pub mod layout {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, Default)]
    pub struct FileTopology;
}

pub mod runtime {
    pub mod reward {
        // Match actual pipeline call:
        // compute_pipeline_reward(ir, &candidate, 0.0, 0.0)
        pub fn compute_pipeline_reward<T>(
            _a: &T,
            _b: &T,
            _c: f64,
            _d: f64,
        ) -> f64 {
            0.0
        }
    }

    pub mod policy_updater {
        #[derive(Debug)]
        pub struct PolicyUpdateError;

        pub fn update_policy<T>(
            current: &T,
            _aggregate_reward: f64,
        ) -> Result<T, PolicyUpdateError>
        where
            T: Clone,
        {
            Ok(current.clone())
        }
    }
}
pub mod pipelines;
