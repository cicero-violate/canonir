use anyhow::Result;
use crate::capture::SymbolSpanBundle;
use canon_event::{
    write_shaped_event_auto, CanonPayloadMeta, EventId, EventKind, RustcCaptureCompleted,
    RustcCaptureFailed, RustcCaptureStarted, RustcGraphArtifactWritten,
};
use canon_ir::ir::CanonIR;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

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
    file_count_override: Option<usize>,
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
        file_count: file_count_override.unwrap_or_else(|| unique_file_count(span_bundle)),
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
            artifact_written: true,
            artifact_id_out: summary.artifact_id.clone(),
            file_count: summary.file_count as u64,
            node_count: summary.node_count as u64,
            edge_count: summary.edge_count as u64,
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
            failed: true,
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
    let graph_dir = workspace_root.join("state").join("graph");
    let index_dir = graph_dir.join("index");
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
    prune_graph_artifacts(&graph_dir, &index_dir)?;
    Ok(())
}

fn prune_graph_artifacts(graph_dir: &Path, index_dir: &Path) -> Result<()> {
    let retain_limit = retained_graph_artifact_limit();
    if retain_limit == 0 {
        return Ok(());
    }

    let mut retained_ids = referenced_artifact_ids(index_dir)?;
    let mut artifacts = list_graph_artifacts(graph_dir)?;
    artifacts.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
    for (artifact_id, _) in artifacts.iter().take(retain_limit) {
        retained_ids.insert(artifact_id.clone());
    }

    for (artifact_id, _) in &artifacts {
        if retained_ids.contains(artifact_id) {
            continue;
        }
        let artifact_path = graph_dir.join(format!("{artifact_id}.json"));
        if artifact_path.exists() {
            let _ = fs::remove_file(&artifact_path);
        }
        let by_hash_path = index_dir.join("by_hash").join(format!("{artifact_id}.json"));
        if by_hash_path.exists() {
            let _ = fs::remove_file(&by_hash_path);
        }
    }

    for entry in fs::read_dir(index_dir.join("by_hash"))? {
        let entry = entry?;
        let path = entry.path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if !graph_dir.join(format!("{stem}.json")).exists() {
            let _ = fs::remove_file(path);
        }
    }

    Ok(())
}

fn retained_graph_artifact_limit() -> usize {
    std::env::var("CANON_GRAPH_RETAIN_ARTIFACTS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(32)
}

fn referenced_artifact_ids(index_dir: &Path) -> Result<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    let latest_workspace_path = index_dir.join("latest_workspace.json");
    if latest_workspace_path.exists() {
        let index = serde_json::from_slice::<GraphArtifactIndex>(&fs::read(&latest_workspace_path)?)?;
        ids.insert(index.latest_workspace.artifact_id);
    }

    let by_crate_dir = index_dir.join("by_crate");
    if by_crate_dir.exists() {
        for entry in fs::read_dir(by_crate_dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let summary = serde_json::from_slice::<GraphArtifactSummary>(&fs::read(path)?)?;
            ids.insert(summary.artifact_id);
        }
    }
    Ok(ids)
}

fn list_graph_artifacts(graph_dir: &Path) -> Result<Vec<(String, SystemTime)>> {
    let mut artifacts = Vec::new();
    for entry in fs::read_dir(graph_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let modified = entry
            .metadata()?
            .modified()
            .unwrap_or(SystemTime::UNIX_EPOCH);
        artifacts.push((stem.to_string(), modified));
    }
    Ok(artifacts)
}
