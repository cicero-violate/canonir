pub mod binary_reader;
pub mod capability_graph_projector;
pub mod goal_graph_projector;
pub mod graph_types;
pub mod reader;
pub mod replay;
pub mod session_scan;
pub mod snapshot;

// Writer side — re-exported from canon-event (which absorbed tlog-writer)
pub mod writer {
    pub use canon_event::{
        append_event, append_event_json, BinarySegmentWriter, BinaryTlogWriter, CanonEvent,
        TlogWriter,
    };
}

pub mod schema {
    pub use canon_event::CanonEvent;
}

// Flat re-exports for backward compatibility
pub use binary_reader::{is_binary_magic, read_binary_events};
pub use graph_types::{EdgeRow, NodeRow, KernelCodeGraph};
pub use reader::{
    detect_tlog_format, extract_capability_request, extract_edit_event, extract_rustc_event,
    extract_supervisor_event, parse_any_event, parse_capability_request_value,
    parse_edit_event_value, parse_rustc_event_value, read_any_events, read_any_events_from_path,
    read_any_events_from_path_with_start_seq, AnyEvent, TlogFormat,
};
pub use replay::{
    apply_rustc_event_to_graph, rebuild_symbol_index, replay_events_from_offset,
    replay_graph_from_tlog, replay_graph_from_tlog_incremental,
};
pub use session_scan::{
    find_last_graph_session_offset, find_last_session_offset, session_contains_module_nodes,
};
pub use snapshot::{
    load_graph_snapshot, read_snapshot_metadata, save_graph_snapshot, snapshot_into_rows,
    write_snapshot_metadata, SnapshotMeta,
};
pub use goal_graph_projector::{GoalGraphState, ProjectedGoalNode, replay_goal_graph_from_tlog, replay_goal_graph_incremental};
pub use capability_graph_projector::{CapabilityGraphState, CapabilityOpNode, CapabilityOpEdge, replay_capability_graph_from_tlog};
