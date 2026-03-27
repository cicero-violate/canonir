use anyhow::{anyhow, Result};
use canon_ir::{CanonIR, CanonNodeKind, NodeId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphRenameCandidate {
    pub symbol_path: String,
    pub suggested_path: String,
    pub kind: String,
    pub module_path: Option<String>,
    pub duplicate_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleCohesionHotspot {
    pub module_path: String,
    pub module_edge_count: usize,
    pub call_edge_count: usize,
    pub pressure_score: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphModuleMoveCandidate {
    pub symbol_path: String,
    pub from_module_path: String,
    pub to_module_path: String,
    pub kind: String,
    pub external_reference_count: usize,
}

pub fn load_graph_artifact(path: &Path) -> Result<CanonIR> {
    let mut ir = serde_json::from_slice::<CanonIR>(&fs::read(path)?)?;
    ir.restore();
    Ok(ir)
}

pub fn load_latest_workspace_graph_artifact(workspace_root: &Path) -> Result<(GraphArtifactSummary, CanonIR)> {
    let index_path = workspace_root
        .join("state")
        .join("graph")
        .join("index")
        .join("latest_workspace.json");
    let index = serde_json::from_slice::<GraphArtifactIndex>(&fs::read(index_path)?)?;
    let ir = load_graph_artifact(&index.latest_workspace.artifact_path)?;
    Ok((index.latest_workspace, ir))
}

pub fn duplicate_definition_rename_candidates(ir: &CanonIR, limit: usize) -> Vec<GraphRenameCandidate> {
    let module_map = module_membership_map(ir);
    let mut grouped: BTreeMap<String, Vec<(u32, String, String, Option<String>)>> = BTreeMap::new();
    for node in &ir.nodes {
        let Some((name, kind)) = symbol_identity(ir, &node.kind) else {
            continue;
        };
        let module_path = module_map.get(&node.id.0).cloned();
        let symbol_path = qualify_symbol_path(module_path.as_deref(), &name);
        grouped
            .entry(name.clone())
            .or_default()
            .push((node.id.0, symbol_path, kind.to_string(), module_path));
    }

    let mut out = Vec::new();
    for (name, entries) in grouped {
        if entries.len() < 2 {
            continue;
        }
        for (idx, (_, symbol_path, kind, module_path)) in entries.iter().enumerate() {
            let new_name = suggested_rename(&name, module_path.as_deref(), idx + 1);
            out.push(GraphRenameCandidate {
                symbol_path: symbol_path.clone(),
                suggested_path: qualify_symbol_path(module_path.as_deref(), &new_name),
                kind: kind.clone(),
                module_path: module_path.clone(),
                duplicate_count: entries.len(),
            });
        }
    }
    out.truncate(limit);
    out
}

pub fn module_cohesion_hotspots(ir: &CanonIR, limit: usize) -> Vec<ModuleCohesionHotspot> {
    let module_map = module_membership_map(ir);
    let mut call_counts: HashMap<String, usize> = HashMap::new();
    for module_node in &ir.nodes {
        let CanonNodeKind::Module { path_id, .. } = &module_node.kind else {
            continue;
        };
        let module_path = ir.lookup_path(*path_id).to_string();
        let call_count = ir
            .call_graph
            .neighbours(NodeId(module_node.id.0))
            .filter(|(dst, _)| module_map.get(&dst.0).is_some_and(|dst_path| dst_path == &module_path))
            .count();
        call_counts.insert(module_path, call_count);
    }

    let mut hotspots = Vec::new();
    for module_node in &ir.nodes {
        let CanonNodeKind::Module { path_id, .. } = &module_node.kind else {
            continue;
        };
        let module_path = ir.lookup_path(*path_id).to_string();
        let module_edge_count = ir.module_graph.neighbours(NodeId(module_node.id.0)).count();
        let call_edge_count = *call_counts.get(&module_path).unwrap_or(&0);
        let pressure_score = module_edge_count as i64 - (call_edge_count as i64 * 2);
        hotspots.push(ModuleCohesionHotspot {
            module_path,
            module_edge_count,
            call_edge_count,
            pressure_score,
        });
    }
    hotspots.sort_by(|a, b| {
        b.pressure_score
            .cmp(&a.pressure_score)
            .then_with(|| b.module_edge_count.cmp(&a.module_edge_count))
    });
    hotspots.truncate(limit);
    hotspots
}

pub fn graph_backed_rename_candidates(workspace_root: &Path, limit: usize) -> Result<Vec<GraphRenameCandidate>> {
    let (_, ir) = load_latest_workspace_graph_artifact(workspace_root)?;
    Ok(duplicate_definition_rename_candidates(&ir, limit))
}

pub fn graph_backed_module_hotspots(workspace_root: &Path, limit: usize) -> Result<Vec<ModuleCohesionHotspot>> {
    let (_, ir) = load_latest_workspace_graph_artifact(workspace_root)?;
    Ok(module_cohesion_hotspots(&ir, limit))
}

pub fn graph_backed_module_moves(workspace_root: &Path, limit: usize) -> Result<Vec<GraphModuleMoveCandidate>> {
    let (_, ir) = load_latest_workspace_graph_artifact(workspace_root)?;
    Ok(module_move_candidates(&ir, limit))
}

fn module_membership_map(ir: &CanonIR) -> HashMap<u32, String> {
    let mut membership = HashMap::new();
    for node in &ir.nodes {
        let CanonNodeKind::Module { path_id, .. } = &node.kind else {
            continue;
        };
        let module_path = ir.lookup_path(*path_id).to_string();
        for (dst, _) in ir.module_graph.neighbours(NodeId(node.id.0)) {
            membership.entry(dst.0).or_insert_with(|| module_path.clone());
        }
    }
    membership
}

fn module_move_candidates(ir: &CanonIR, limit: usize) -> Vec<GraphModuleMoveCandidate> {
    let module_map = module_membership_map(ir);
    let hotspots = module_cohesion_hotspots(ir, limit.saturating_mul(2).max(4));
    let mut out = Vec::new();
    for hotspot in hotspots {
        let Some((symbol_path, kind, to_module_path, external_reference_count)) =
            best_move_candidate_for_module(ir, &module_map, &hotspot.module_path)
        else {
            continue;
        };
        out.push(GraphModuleMoveCandidate {
            symbol_path,
            from_module_path: hotspot.module_path,
            to_module_path,
            kind,
            external_reference_count,
        });
        if out.len() >= limit {
            break;
        }
    }
    out
}

fn best_move_candidate_for_module(
    ir: &CanonIR,
    module_map: &HashMap<u32, String>,
    module_path: &str,
) -> Option<(String, String, String, usize)> {
    let mut best: Option<(String, String, String, usize)> = None;
    for node in &ir.nodes {
        let Some(symbol_module) = module_map.get(&node.id.0) else {
            continue;
        };
        if symbol_module != module_path {
            continue;
        }
        let Some((name, kind)) = symbol_identity(ir, &node.kind) else {
            continue;
        };
        let external_target = dominant_external_target_module(ir, module_map, node.id.0, module_path)?;
        let symbol_path = qualify_symbol_path(Some(module_path), &name);
        match &best {
            Some((_, _, _, best_count)) if *best_count >= external_target.1 => {}
            _ => {
                best = Some((
                    symbol_path,
                    kind.to_string(),
                    external_target.0,
                    external_target.1,
                ));
            }
        }
    }
    best
}

fn dominant_external_target_module(
    ir: &CanonIR,
    module_map: &HashMap<u32, String>,
    node_id: u32,
    current_module: &str,
) -> Option<(String, usize)> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for source in &ir.nodes {
        for (dst, _) in ir.call_graph.neighbours(NodeId(source.id.0)) {
            if dst.0 != node_id {
                continue;
            }
            let Some(module_path) = module_map.get(&source.id.0) else {
                continue;
            };
            if module_path == current_module {
                continue;
            }
            *counts.entry(module_path.clone()).or_insert(0) += 1;
        }
    }
    counts.into_iter().max_by_key(|(_, count)| *count)
}

fn symbol_identity(ir: &CanonIR, kind: &CanonNodeKind) -> Option<(String, &'static str)> {
    match kind {
        CanonNodeKind::Struct { name_id, .. } => Some((ir.lookup_name(*name_id).to_string(), "struct")),
        CanonNodeKind::Enum { name_id, .. } => Some((ir.lookup_name(*name_id).to_string(), "enum")),
        CanonNodeKind::Trait { name_id, .. } => Some((ir.lookup_name(*name_id).to_string(), "trait")),
        CanonNodeKind::AssocType { name_id, .. } => Some((ir.lookup_name(*name_id).to_string(), "assoc_type")),
        CanonNodeKind::AssocConst { name_id, .. } => Some((ir.lookup_name(*name_id).to_string(), "assoc_const")),
        CanonNodeKind::Fn { name_id, .. } => Some((ir.lookup_name(*name_id).to_string(), "fn")),
        _ => None,
    }
}

fn qualify_symbol_path(module_path: Option<&str>, name: &str) -> String {
    match module_path {
        Some(module_path) if !module_path.is_empty() => format!("{module_path}::{name}"),
        _ => format!("crate::{name}"),
    }
}

fn suggested_rename(name: &str, module_path: Option<&str>, ordinal: usize) -> String {
    let module_suffix = module_path
        .and_then(|path| path.rsplit("::").next())
        .map(sanitize_identifier)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("Variant{ordinal}"));
    if name.ends_with(&module_suffix) {
        format!("{name}{ordinal}")
    } else {
        format!("{name}{module_suffix}")
    }
}

fn sanitize_identifier(value: &str) -> String {
    let mut out = String::new();
    let mut capitalize = true;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if capitalize {
                out.extend(ch.to_uppercase());
                capitalize = false;
            } else {
                out.push(ch);
            }
        } else {
            capitalize = true;
        }
    }
    out
}

pub fn latest_graph_artifact_path(workspace_root: &Path) -> Result<PathBuf> {
    let index_path = workspace_root
        .join("state")
        .join("graph")
        .join("index")
        .join("latest_workspace.json");
    let index = serde_json::from_slice::<GraphArtifactIndex>(&fs::read(index_path)?)?;
    if index.latest_workspace.artifact_path.as_os_str().is_empty() {
        return Err(anyhow!("latest graph artifact path is empty"));
    }
    Ok(index.latest_workspace.artifact_path)
}
