pub mod binary_reader;
pub mod capability_graph_projector;
pub mod goal_graph_projector;
pub mod graph_types;
pub mod reader;
pub mod replay;
pub mod session_scan;
pub mod snapshot;

// Flat re-exports
pub use binary_reader::{is_binary_magic, read_binary_events};
pub use capability_graph_projector::{replay_capability_graph_from_tlog, CapabilityGraphState, CapabilityOpEdge, CapabilityOpNode};
pub use goal_graph_projector::{replay_goal_graph_from_tlog, replay_goal_graph_incremental, GoalGraphState, GoalNodeState};
pub use graph_types::{CodeGraphEdge, CodeGraphNode, CodeGraphProjection};
pub use reader::{
    detect_tlog_format, extract_edit_event, extract_rustc_event, extract_supervisor_event, parse_any_event, parse_edit_event_value, parse_rustc_event_value, read_any_events,
    read_any_events_from_path, read_any_events_from_path_with_start_seq, AnyEvent, TlogFormat,
};
pub use replay::{apply_rustc_event_to_graph, rebuild_symbol_index, replay_events_from_offset, replay_graph_for_crate, replay_graph_from_tlog, replay_graph_from_tlog_incremental};
pub use session_scan::{find_last_graph_session_offset, find_last_session_offset, session_contains_module_nodes};
pub use snapshot::{load_graph_snapshot, read_snapshot_metadata, save_graph_snapshot, snapshot_into_rows, write_snapshot_metadata, CodeSnapshot, CodeSnapshotEdge, CodeSnapshotNode, SnapshotMeta};
