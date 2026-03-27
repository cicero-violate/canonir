use anyhow::Result;
use crate::capture::SymbolSpanBundle;
use canon_event::{CanonEvent, CanonPayload, CanonPayloadMeta, EventId, EventKind};
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
    Ok(GraphArtifactSummary {
        artifact_id,
        artifact_path,
        crate_name: crate_name.to_string(),
        node_count: ir.nodes.len(),
        edge_count: total_edge_count(ir),
        file_count: unique_file_count(span_bundle),
        call_edge_count: ir.call_graph.edge_count(),
        module_edge_count: ir.module_graph.edge_count(),
        cfg_edge_count: ir.cfg_graph.edge_count(),
    })
}

pub fn emit_graph_artifact_summary(
    tlog_path: &Path,
    summary: &GraphArtifactSummary,
) -> Result<()> {
    let payload = CanonPayload::from_data(
        serde_json::json!({ "crate_name": summary.crate_name }),
        serde_json::json!({
            "artifact_id": summary.artifact_id,
            "nodes": summary.node_count,
            "edges": summary.edge_count,
            "files": summary.file_count,
            "call_edges": summary.call_edge_count,
            "module_edges": summary.module_edge_count,
            "cfg_edges": summary.cfg_edge_count,
        }),
        serde_json::json!({ "artifact_id": summary.artifact_id }),
        CanonPayloadMeta { file: file!().to_string(), line: line!() },
        serde_json::to_value(summary)?,
    );
    let event = CanonEvent::new(
        EventId::new(canon_event::new_event_id()),
        Vec::new(),
        "rustc".to_string(),
        EventKind::Debug,
        canon_event::now_millis(),
        payload,
        true,
    );
    canon_event::write_canon_event_auto(tlog_path, &event)
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
