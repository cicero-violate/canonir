use anyhow::Result;
use crate::capture::SymbolSpanBundle;
use canon_event::{
    write_shaped_event_auto, CanonPayloadMeta, EventId, EventKind, RustcCaptureCompleted,
    RustcCaptureFailed, RustcCaptureStarted, RustcGraphArtifactWritten,
};
use canon_ir::ir::CanonIR;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMode {
    Sparse,
    Structural,
}

impl CaptureMode {
    pub fn current() -> Self {
        match std::env::var("CANON_RUSTC_CAPTURE_MODE").as_deref() {
            Ok("structural") | Ok("STRUCTURAL") | Ok("full") | Ok("FULL") => Self::Structural,
            _ => Self::Sparse,
        }
    }

    pub fn emits_structural_events(self) -> bool {
        matches!(self, Self::Structural)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sparse => "sparse",
            Self::Structural => "structural",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphArtifactSummary {
    pub artifact_id: String,
    pub artifact_path: PathBuf,
    pub crate_name: String,
    pub node_count: usize,
    pub edge_count: usize,
    pub file_count: usize,
    pub call_edge_count: usize,
    pub module_edge_count: usize,
    pub cfg_edge_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphArtifactIndex {
    pub latest_workspace: GraphArtifactSummary,
}

pub fn write_graph_artifact(
    workspace_root: &Path,
    crate_name: &str,
    ir: &CanonIR,
    span_bundle: Option<&SymbolSpanBundle>,
) -> Result<GraphArtifactSummary> {
    let serialized = serde_json::to_vec(ir)?;
    let artifact_id = format!("{:x}", Sha256::digest(&serialized));
    let graph_dir = workspace_root.join("state").join("graph");
    fs::create_dir_all(&graph_dir)?;
    let artifact_path = graph_dir.join(format!("{artifact_id}.json"));
    if !artifact_path.exists() {
        fs::write(&artifact_path, serialized)?;
    }
    let summary = GraphArtifactSummary {
        artifact_id,
        artifact_path,
        crate_name: crate_name.to_string(),
        node_count: ir.nodes.len(),
        edge_count: total_edge_count(ir),
        file_count: unique_file_count(span_bundle),
        call_edge_count: ir.call_graph.edge_count(),
        module_edge_count: ir.module_graph.edge_count(),
        cfg_edge_count: ir.cfg_graph.edge_count(),
    };
    update_graph_artifact_index(workspace_root, &summary)?;
    Ok(summary)
}

pub fn emit_capture_started(
    tlog_path: &Path,
    crate_name: &str,
    capture_mode: CaptureMode,
) -> Result<EventId> {
    write_shaped_event_auto(
        tlog_path,
        "rustc",
        EventKind::RustcCaptureStarted,
        &RustcCaptureStarted {
            crate_name: crate_name.to_string(),
            capture_mode: capture_mode.as_str().to_string(),
            started: true,
        },
        Vec::new(),
        true,
        CanonPayloadMeta { file: file!().to_string(), line: line!() },
    )
}

pub fn emit_graph_artifact_summary(
    tlog_path: &Path,
    summary: &GraphArtifactSummary,
) -> Result<EventId> {
    emit_graph_artifact_summary_with_parents(tlog_path, summary, Vec::new())
}

pub fn emit_graph_artifact_summary_with_parents(
    tlog_path: &Path,
    summary: &GraphArtifactSummary,
    parent_ids: Vec<EventId>,
) -> Result<EventId> {
    write_shaped_event_auto(
        tlog_path,
        "rustc",
        EventKind::RustcGraphArtifactWritten,
        &RustcGraphArtifactWritten {
            crate_name: summary.crate_name.clone(),
            artifact_id: summary.artifact_id.clone(),
            artifact_path: summary.artifact_path.display().to_string(),
            node_count: summary.node_count as u64,
            edge_count: summary.edge_count as u64,
            file_count: summary.file_count as u64,
            call_edge_count: summary.call_edge_count as u64,
            module_edge_count: summary.module_edge_count as u64,
            cfg_edge_count: summary.cfg_edge_count as u64,
        },
        parent_ids.clone(),
        parent_ids.is_empty(),
        CanonPayloadMeta { file: file!().to_string(), line: line!() },
    )
}

pub fn emit_capture_completed(
    tlog_path: &Path,
    crate_name: &str,
    artifact_id: &str,
    parent_ids: Vec<EventId>,
) -> Result<EventId> {
    write_shaped_event_auto(
        tlog_path,
        "rustc",
        EventKind::RustcCaptureCompleted,
        &RustcCaptureCompleted {
            crate_name: crate_name.to_string(),
            artifact_id: artifact_id.to_string(),
            completed: true,
        },
        parent_ids,
        false,
        CanonPayloadMeta { file: file!().to_string(), line: line!() },
    )
}

pub fn emit_capture_failed(
    tlog_path: &Path,
    crate_name: &str,
    message: &str,
    parent_ids: Vec<EventId>,
) -> Result<EventId> {
    write_shaped_event_auto(
        tlog_path,
        "rustc",
        EventKind::RustcCaptureFailed,
        &RustcCaptureFailed {
            crate_name: crate_name.to_string(),
            message: message.to_string(),
        },
        parent_ids.clone(),
        parent_ids.is_empty(),
        CanonPayloadMeta { file: file!().to_string(), line: line!() },
    )
}

fn total_edge_count(ir: &CanonIR) -> usize {
    ir.name_graph.edge_count()
        + ir.type_graph.edge_count()
        + ir.call_graph.edge_count()
        + ir.module_graph.edge_count()
        + ir.cfg_graph.edge_count()
        + ir.region_graph.edge_count()
        + ir.value_graph.edge_count()
        + ir.macro_graph.edge_count()
}

fn unique_file_count(span_bundle: Option<&SymbolSpanBundle>) -> usize {
    use std::collections::BTreeSet;
    let mut files = BTreeSet::new();
    if let Some(bundle) = span_bundle {
        for spans in bundle.spans_by_symbol.values() {
            for span in spans {
                if !span.file.is_empty() {
                    files.insert(span.file.clone());
                }
            }
        }
    }
    files.len()
}

fn update_graph_artifact_index(workspace_root: &Path, summary: &GraphArtifactSummary) -> Result<()> {
    let index_dir = workspace_root.join("state").join("graph").join("index");
    fs::create_dir_all(index_dir.join("by_crate"))?;
    fs::create_dir_all(index_dir.join("by_hash"))?;

    let latest_workspace = GraphArtifactIndex {
        latest_workspace: summary.clone(),
    };
    fs::write(
        index_dir.join("latest_workspace.json"),
        serde_json::to_vec_pretty(&latest_workspace)?,
    )?;
    fs::write(
        index_dir.join("by_crate").join(format!("{}.json", summary.crate_name)),
        serde_json::to_vec_pretty(summary)?,
    )?;
    fs::write(
        index_dir.join("by_hash").join(format!("{}.json", summary.artifact_id)),
        serde_json::to_vec_pretty(summary)?,
    )?;
    Ok(())
}
