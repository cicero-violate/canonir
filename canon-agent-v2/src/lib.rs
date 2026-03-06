pub mod call;
pub mod ir;
pub mod llm_domains;
pub mod llm_provider;
pub mod parsers;
pub mod pipelines;
pub mod runtime;
pub mod ws_server;

pub mod layout {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, Default)]
    pub struct FileTopology;
}
