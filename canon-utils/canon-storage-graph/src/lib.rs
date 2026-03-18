pub mod artifacts_loader;
pub mod graph;
pub mod artifacts;
pub mod health;
pub mod ingest;
pub mod consumer;
pub mod goal_graph_projector;

pub use artifacts_loader::{CodeGraph, Node as GraphNode, Edge as GraphEdge, CsrGraph};
pub use canon_event_store::{CodeGraphEdge, CodeGraphNode};
pub use consumer::GraphConsumer;
