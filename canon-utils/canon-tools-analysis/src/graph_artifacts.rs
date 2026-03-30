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
    pub file_path: Option<String>,
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
    pub file_path: Option<String>,
    pub external_reference_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphImportBinding {
    pub module_path: String,
    pub visible_path: String,
    pub target_path: String,
    pub alias: Option<String>,
    pub is_reexport: bool,
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphResolvedSymbol {
    pub requested_path: String,
    pub canonical_path: String,
    pub file_path: Option<String>,
    pub via_binding: Option<GraphImportBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphProofExpectation {
    Rename { old_symbol: String, new_symbol: String, path: String },
    Move { old_symbol: String, new_symbol: String, new_module_path: String, path: String },
    Import { import_path: String, path: String },
    CreateModule { module_path: String, path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GraphProofReport {
    pub verified: bool,
    pub artifact_id: Option<String>,
    pub summary: String,
    pub failures: Vec<String>,
}

pub fn load_graph_artifact(path: &Path) -> Result<CanonIR> {
    let mut ir = serde_json::from_slice::<CanonIR>(&fs::read(path)?)?;
    ir.restore();
    Ok(ir)
}

pub fn load_latest_workspace_graph_artifact(workspace_root: &Path) -> Result<(GraphArtifactSummary, CanonIR)> {
    let index_path = workspace_root.join("state").join("graph").join("index").join("latest_workspace.json");
    let index = serde_json::from_slice::<GraphArtifactIndex>(&fs::read(index_path)?)?;
    let ir = load_graph_artifact(&index.latest_workspace.artifact_path)?;
    Ok((index.latest_workspace, ir))
}

pub fn verify_graph_expectations(workspace_root: &Path, expectations: &[GraphProofExpectation]) -> Result<GraphProofReport> {
    let (summary, ir) = load_latest_workspace_graph_artifact(workspace_root)?;
    let module_map = module_membership_map(&ir);
    let symbol_map = graph_symbol_paths(&ir, &module_map);
    let module_paths = ir
        .nodes
        .iter()
        .filter_map(|node| match &node.kind {
            CanonNodeKind::Module { path_id, .. } => Some(ir.lookup_path(*path_id).to_string()),
            _ => None,
        })
        .collect::<std::collections::HashSet<_>>();

    let mut failures = Vec::new();
    for expectation in expectations {
        match expectation {
            GraphProofExpectation::Rename { old_symbol, new_symbol, path } => {
                if symbol_map.contains_key(old_symbol) {
                    failures.push(format!("old symbol still present after rename: {old_symbol}"));
                }
                if !symbol_map.contains_key(new_symbol) {
                    failures.push(format!("new symbol missing after rename: {new_symbol}"));
                }
                if let Some((module_path, _)) = split_symbol_path(new_symbol) {
                    verify_module_file_membership(workspace_root, &module_paths, module_path, path, &mut failures);
                }
            }
            GraphProofExpectation::Move { old_symbol, new_symbol, new_module_path, path } => {
                if symbol_map.contains_key(old_symbol) {
                    failures.push(format!("old symbol still present after move: {old_symbol}"));
                }
                if !symbol_map.contains_key(new_symbol) {
                    failures.push(format!("new symbol missing after move: {new_symbol}"));
                }
                verify_module_file_membership(workspace_root, &module_paths, new_module_path, path, &mut failures);
            }
            GraphProofExpectation::Import { import_path, path } => {
                if !symbol_map.contains_key(import_path) {
                    failures.push(format!("import target missing from graph: {import_path}"));
                }
                if let Some(module_path) = module_path_from_relative_file(path) {
                    verify_module_file_membership(workspace_root, &module_paths, &module_path, path, &mut failures);
                }
            }
            GraphProofExpectation::CreateModule { module_path, path } => {
                verify_module_file_membership(workspace_root, &module_paths, module_path, path, &mut failures);
            }
        }
    }

    Ok(GraphProofReport {
        verified: failures.is_empty(),
        artifact_id: Some(summary.artifact_id.clone()),
        summary: if failures.is_empty() { format!("graph proof ok for {} expectation(s)", expectations.len()) } else { format!("graph proof failed for {} expectation(s)", expectations.len()) },
        failures,
    })
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
        grouped.entry(name.clone()).or_default().push((node.id.0, symbol_path, kind.to_string(), module_path));
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
                file_path: None,
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
        let call_count = ir.call_graph.neighbours(NodeId(module_node.id.0)).filter(|(dst, _)| module_map.get(&dst.0).is_some_and(|dst_path| dst_path == &module_path)).count();
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
        hotspots.push(ModuleCohesionHotspot { module_path, module_edge_count, call_edge_count, pressure_score });
    }
    hotspots.sort_by(|a, b| b.pressure_score.cmp(&a.pressure_score).then_with(|| b.module_edge_count.cmp(&a.module_edge_count)));
    hotspots.truncate(limit);
    hotspots
}

pub fn graph_backed_rename_candidates(workspace_root: &Path, limit: usize) -> Result<Vec<GraphRenameCandidate>> {
    let (_, ir) = load_latest_workspace_graph_artifact(workspace_root)?;
    let mut out = duplicate_definition_rename_candidates(&ir, limit);
    for candidate in &mut out {
        candidate.file_path = candidate.module_path.as_deref().and_then(|module_path| workspace_relative_path_for_module(workspace_root, module_path));
    }
    Ok(out)
}

pub fn graph_backed_module_hotspots(workspace_root: &Path, limit: usize) -> Result<Vec<ModuleCohesionHotspot>> {
    let (_, ir) = load_latest_workspace_graph_artifact(workspace_root)?;
    Ok(module_cohesion_hotspots(&ir, limit))
}

pub fn graph_backed_module_moves(workspace_root: &Path, limit: usize) -> Result<Vec<GraphModuleMoveCandidate>> {
    let (_, ir) = load_latest_workspace_graph_artifact(workspace_root)?;
    let mut out = module_move_candidates(&ir, limit);
    for candidate in &mut out {
        candidate.file_path = workspace_relative_path_for_module(workspace_root, &candidate.from_module_path);
    }
    Ok(out)
}

pub fn graph_import_bindings(workspace_root: &Path) -> Result<Vec<GraphImportBinding>> {
    let (_, ir) = load_latest_workspace_graph_artifact(workspace_root)?;
    let module_files = graph_module_files(workspace_root, &ir);
    let mut out = Vec::new();
    for (module_path, file_path) in module_files {
        let source = match fs::read_to_string(workspace_root.join(&file_path)) {
            Ok(source) => source,
            Err(_) => continue,
        };
        let ast = match syn::parse_file(&source) {
            Ok(ast) => ast,
            Err(_) => continue,
        };
        for item in ast.items {
            let syn::Item::Use(item_use) = item else {
                continue;
            };
            collect_use_bindings(workspace_root, &module_path, &file_path, item_use.vis, &item_use.tree, &mut out);
        }
    }
    out.sort_by(|a, b| a.visible_path.cmp(&b.visible_path).then_with(|| a.target_path.cmp(&b.target_path)));
    out.dedup_by(|a, b| a.visible_path == b.visible_path && a.target_path == b.target_path);
    Ok(out)
}

pub fn resolve_graph_symbol_path(workspace_root: &Path, symbol_path: &str) -> Result<Option<GraphResolvedSymbol>> {
    let (_, ir) = load_latest_workspace_graph_artifact(workspace_root)?;
    let module_map = module_membership_map(&ir);
    let symbols = graph_symbol_paths(&ir, &module_map);
    if symbols.contains_key(symbol_path) {
        return Ok(Some(GraphResolvedSymbol {
            requested_path: symbol_path.to_string(),
            canonical_path: symbol_path.to_string(),
            file_path: split_symbol_path(symbol_path).and_then(|(module_path, _)| workspace_relative_path_for_module(workspace_root, module_path)),
            via_binding: None,
        }));
    }

    let bindings = graph_import_bindings(workspace_root)?;
    let binding_map: HashMap<String, GraphImportBinding> = bindings.into_iter().map(|binding| (binding.visible_path.clone(), binding)).collect();
    let mut current = symbol_path.to_string();
    let mut via_binding = None;
    let mut seen = std::collections::HashSet::new();
    while seen.insert(current.clone()) {
        let Some(binding) = binding_map.get(&current).cloned() else {
            return Ok(None);
        };
        if via_binding.is_none() {
            via_binding = Some(binding.clone());
        }
        current = binding.target_path.clone();
        if symbols.contains_key(&current) {
            return Ok(Some(GraphResolvedSymbol {
                requested_path: symbol_path.to_string(),
                canonical_path: current.clone(),
                file_path: split_symbol_path(&current).and_then(|(module_path, _)| workspace_relative_path_for_module(workspace_root, module_path)),
                via_binding,
            }));
        }
    }
    Ok(None)
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

fn graph_module_files(workspace_root: &Path, ir: &CanonIR) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for node in &ir.nodes {
        let CanonNodeKind::Module { path_id, .. } = &node.kind else {
            continue;
        };
        let module_path = ir.lookup_path(*path_id).to_string();
        if let Some(file_path) = workspace_relative_path_for_module(workspace_root, &module_path) {
            out.insert(module_path, file_path);
        }
    }
    out
}

fn graph_symbol_paths(ir: &CanonIR, module_map: &HashMap<u32, String>) -> HashMap<String, &'static str> {
    let mut out = HashMap::new();
    for node in &ir.nodes {
        let Some((name, kind)) = symbol_identity(ir, &node.kind) else {
            continue;
        };
        let module_path = module_map.get(&node.id.0).map(String::as_str);
        out.insert(qualify_symbol_path(module_path, &name), kind);
    }
    out
}

fn module_move_candidates(ir: &CanonIR, limit: usize) -> Vec<GraphModuleMoveCandidate> {
    let module_map = module_membership_map(ir);
    let hotspots = module_cohesion_hotspots(ir, limit.saturating_mul(2).max(4));
    let mut out = Vec::new();
    for hotspot in hotspots {
        let Some((symbol_path, kind, to_module_path, external_reference_count)) = best_move_candidate_for_module(ir, &module_map, &hotspot.module_path) else {
            continue;
        };
        out.push(GraphModuleMoveCandidate { symbol_path, from_module_path: hotspot.module_path, to_module_path, kind, file_path: None, external_reference_count });
        if out.len() >= limit {
            break;
        }
    }
    out
}

fn best_move_candidate_for_module(ir: &CanonIR, module_map: &HashMap<u32, String>, module_path: &str) -> Option<(String, String, String, usize)> {
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
                best = Some((symbol_path, kind.to_string(), external_target.0, external_target.1));
            }
        }
    }
    best
}

fn dominant_external_target_module(ir: &CanonIR, module_map: &HashMap<u32, String>, node_id: u32, current_module: &str) -> Option<(String, usize)> {
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

fn collect_use_bindings(workspace_root: &Path, current_module_path: &str, file_path: &str, visibility: syn::Visibility, tree: &syn::UseTree, out: &mut Vec<GraphImportBinding>) {
    for binding in flatten_use_tree(current_module_path, Vec::new(), tree) {
        let target_path = resolve_use_segments(current_module_path, &binding.target_segments);
        let visible_path = qualify_symbol_path(Some(current_module_path), &binding.visible_name);
        out.push(GraphImportBinding {
            module_path: current_module_path.to_string(),
            visible_path,
            target_path,
            alias: binding.alias,
            is_reexport: matches!(visibility, syn::Visibility::Public(_)),
            file_path: Some(workspace_root.join(file_path).strip_prefix(workspace_root).unwrap_or_else(|_| Path::new(file_path)).to_string_lossy().replace('\\', "/")),
        });
    }
}

#[derive(Clone)]
struct FlatUseBinding {
    target_segments: Vec<String>,
    visible_name: String,
    alias: Option<String>,
}

fn flatten_use_tree(current_module_path: &str, prefix: Vec<String>, tree: &syn::UseTree) -> Vec<FlatUseBinding> {
    let _ = current_module_path;
    match tree {
        syn::UseTree::Path(path) => {
            let mut next_prefix = prefix;
            next_prefix.push(path.ident.to_string());
            flatten_use_tree(current_module_path, next_prefix, &path.tree)
        }
        syn::UseTree::Name(name) => {
            vec![FlatUseBinding { visible_name: name.ident.to_string(), target_segments: prefix.into_iter().chain(std::iter::once(name.ident.to_string())).collect(), alias: None }]
        }
        syn::UseTree::Rename(rename) => vec![FlatUseBinding {
            visible_name: rename.rename.to_string(),
            target_segments: prefix.into_iter().chain(std::iter::once(rename.ident.to_string())).collect(),
            alias: Some(rename.rename.to_string()),
        }],
        syn::UseTree::Group(group) => group.items.iter().flat_map(|item| flatten_use_tree(current_module_path, prefix.clone(), item)).collect(),
        syn::UseTree::Glob(_) => Vec::new(),
    }
}

fn resolve_use_segments(current_module_path: &str, segments: &[String]) -> String {
    if segments.is_empty() {
        return current_module_path.to_string();
    }
    match segments[0].as_str() {
        "crate" | "self" | "super" => resolve_relative_module_path(current_module_path, segments),
        "std" | "core" | "alloc" => segments.join("::"),
        _ => {
            let mut resolved = vec!["crate".to_string()];
            resolved.extend(segments.iter().cloned());
            resolved.join("::")
        }
    }
}

fn resolve_relative_module_path(current_module_path: &str, segments: &[String]) -> String {
    let mut base = current_module_path.split("::").filter(|segment| !segment.is_empty()).map(ToString::to_string).collect::<Vec<_>>();
    if base.is_empty() {
        base.push("crate".to_string());
    }
    let mut iter = segments.iter();
    while let Some(segment) = iter.next() {
        match segment.as_str() {
            "crate" => {
                base.clear();
                base.push("crate".to_string());
            }
            "self" => {}
            "super" => {
                if base.len() > 1 {
                    base.pop();
                }
            }
            other => base.push(other.to_string()),
        }
    }
    base.join("::")
}

fn suggested_rename(name: &str, module_path: Option<&str>, ordinal: usize) -> String {
    let module_suffix = module_path.and_then(|path| path.rsplit("::").next()).map(sanitize_identifier).filter(|s| !s.is_empty()).unwrap_or_else(|| format!("Variant{ordinal}"));
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

fn workspace_relative_path_for_module(workspace_root: &Path, module_path: &str) -> Option<String> {
    let mut segments = module_path.split("::").filter(|segment| !segment.is_empty() && *segment != "crate").collect::<Vec<_>>();
    let path = if segments.is_empty() {
        workspace_root.join("src/lib.rs")
    } else {
        let leaf = segments.pop()?;
        let mut base = workspace_root.join("src");
        for segment in segments {
            base.push(segment);
        }
        base.join(format!("{leaf}.rs"))
    };
    path.strip_prefix(workspace_root).ok().map(|relative| relative.to_string_lossy().to_string())
}

fn module_path_from_relative_file(path: &str) -> Option<String> {
    let path = Path::new(path);
    let rel = path.strip_prefix("src").ok().unwrap_or(path);
    let mut segments = rel.components().filter_map(|component| component.as_os_str().to_str()).collect::<Vec<_>>();
    let filename = segments.pop()?;
    let mut module_segments = vec!["crate".to_string()];
    for segment in segments {
        if !segment.is_empty() {
            module_segments.push(segment.to_string());
        }
    }
    let stem = filename.strip_suffix(".rs")?;
    if stem != "lib" && stem != "main" && stem != "mod" {
        module_segments.push(stem.to_string());
    }
    Some(module_segments.join("::"))
}

fn split_symbol_path(symbol_path: &str) -> Option<(&str, &str)> {
    symbol_path.rsplit_once("::")
}

fn verify_module_file_membership(workspace_root: &Path, module_paths: &std::collections::HashSet<String>, module_path: &str, expected_path: &str, failures: &mut Vec<String>) {
    if !module_paths.contains(module_path) {
        failures.push(format!("module missing from graph: {module_path}"));
        return;
    }
    let expected_rel = expected_path.replace('\\', "/");
    let actual_rel = workspace_relative_path_for_module(workspace_root, module_path).unwrap_or_else(|| format!("missing-path-for:{module_path}")).replace('\\', "/");
    if actual_rel != expected_rel {
        failures.push(format!("module/file membership mismatch for {module_path}: expected {expected_rel}, got {actual_rel}"));
    }
}

pub fn latest_graph_artifact_path(workspace_root: &Path) -> Result<PathBuf> {
    let index_path = workspace_root.join("state").join("graph").join("index").join("latest_workspace.json");
    let index = serde_json::from_slice::<GraphArtifactIndex>(&fs::read(index_path)?)?;
    if index.latest_workspace.artifact_path.as_os_str().is_empty() {
        return Err(anyhow!("latest graph artifact path is empty"));
    }
    Ok(index.latest_workspace.artifact_path)
}

#[cfg(test)]
mod tests {
    use super::{duplicate_definition_rename_candidates, graph_import_bindings, module_cohesion_hotspots, module_move_candidates, resolve_graph_symbol_path, GraphArtifactIndex, GraphArtifactSummary};
    use canon_ir::{csr_graph::CsrGraph, CanonIR, CanonNodeKind};
    use std::fs;

    fn sample_ir() -> CanonIR {
        let mut ir = CanonIR::new();
        let mod_alpha = ir.intern_path("crate::alpha").unwrap();
        let mod_beta = ir.intern_path("crate::beta").unwrap();
        let foo = ir.intern_name("Foo");
        let bar = ir.intern_name("Bar");
        let alpha_id = ir.push_node(CanonNodeKind::Module { path_id: mod_alpha, flags: 0 });
        let beta_id = ir.push_node(CanonNodeKind::Module { path_id: mod_beta, flags: 0 });
        let foo_alpha = ir.push_node(CanonNodeKind::Struct { name_id: foo, generics: Vec::new(), fields: Vec::new(), derives: Vec::new(), attrs: Vec::new(), flags: 0, struct_kind: 0 });
        let foo_beta = ir.push_node(CanonNodeKind::Struct { name_id: foo, generics: Vec::new(), fields: Vec::new(), derives: Vec::new(), attrs: Vec::new(), flags: 0, struct_kind: 0 });
        let bar_alpha = ir.push_node(CanonNodeKind::Fn { name_id: bar, sig_id: foo_alpha, body: None, attrs: Vec::new(), flags: 0 });
        let node_data = ir.nodes.iter().map(|node| node.id).collect::<Vec<_>>();
        ir.module_graph = CsrGraph::from_edges(
            node_data.clone(),
            vec![(alpha_id.0, foo_alpha.0, canon_ir::EdgeKind::Contains), (alpha_id.0, bar_alpha.0, canon_ir::EdgeKind::Contains), (beta_id.0, foo_beta.0, canon_ir::EdgeKind::Contains)],
        );
        ir.call_graph = CsrGraph::from_edges(node_data, vec![(foo_alpha.0, bar_alpha.0, canon_ir::EdgeKind::Calls), (foo_beta.0, bar_alpha.0, canon_ir::EdgeKind::Calls)]);
        ir
    }

    fn write_sample_workspace(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::create_dir_all(dir.path().join("state/graph/index/by_crate")).unwrap();
        fs::create_dir_all(dir.path().join("state/graph/index/by_hash")).unwrap();
        for (path, content) in files {
            let full = dir.path().join(path);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(full, content).unwrap();
        }
        let ir = sample_ir();
        let artifact_path = dir.path().join("state/graph/sample.json");
        fs::write(&artifact_path, serde_json::to_vec(&ir).unwrap()).unwrap();
        let summary = GraphArtifactSummary {
            artifact_id: "sample".into(),
            artifact_path: artifact_path.clone(),
            crate_name: "fixture".into(),
            node_count: ir.nodes.len(),
            edge_count: ir.module_graph.edge_count(),
            file_count: 2,
            call_edge_count: ir.call_graph.edge_count(),
            module_edge_count: ir.module_graph.edge_count(),
            cfg_edge_count: 0,
        };
        let index = GraphArtifactIndex { latest_workspace: summary.clone() };
        fs::write(dir.path().join("state/graph/index/latest_workspace.json"), serde_json::to_vec(&index).unwrap()).unwrap();
        fs::write(dir.path().join("state/graph/index/by_crate/fixture.json"), serde_json::to_vec(&summary).unwrap()).unwrap();
        fs::write(dir.path().join("state/graph/index/by_hash/sample.json"), serde_json::to_vec(&summary).unwrap()).unwrap();
        dir
    }

    #[test]
    fn duplicate_definition_candidates_are_graph_backed() {
        let ir = sample_ir();
        let candidates = duplicate_definition_rename_candidates(&ir, 8);
        assert!(candidates.iter().any(|candidate| candidate.symbol_path == "crate::alpha::Foo"));
        assert!(candidates.iter().any(|candidate| candidate.symbol_path == "crate::beta::Foo"));
    }

    #[test]
    fn module_hotspots_and_move_candidates_are_derived_from_graph() {
        let ir = sample_ir();
        let hotspots = module_cohesion_hotspots(&ir, 4);
        assert!(hotspots.iter().any(|hotspot| hotspot.module_path == "crate::alpha"));
        let moves = module_move_candidates(&ir, 4);
        assert!(moves.len() <= 4);
        assert!(moves.iter().all(|candidate| !candidate.to_module_path.is_empty()));
    }

    #[test]
    fn graph_import_bindings_capture_alias_and_reexport() {
        let dir = write_sample_workspace(&[("src/alpha.rs", "pub struct Foo;\npub fn Bar() {}\n"), ("src/beta.rs", "pub use crate::alpha::Foo as PublicFoo;\nuse crate::alpha::Bar as LocalBar;\n")]);
        let bindings = graph_import_bindings(dir.path()).unwrap();
        assert!(bindings.iter().any(|binding| { binding.visible_path == "crate::beta::PublicFoo" && binding.target_path == "crate::alpha::Foo" && binding.is_reexport }));
        assert!(bindings.iter().any(|binding| { binding.visible_path == "crate::beta::LocalBar" && binding.target_path == "crate::alpha::Bar" && !binding.is_reexport }));
    }

    #[test]
    fn resolve_graph_symbol_path_follows_alias_binding() {
        let dir = write_sample_workspace(&[("src/alpha.rs", "pub struct Foo;\npub fn Bar() {}\n"), ("src/beta.rs", "use crate::alpha::Foo as PublicFoo;\n")]);
        let resolved = resolve_graph_symbol_path(dir.path(), "crate::beta::PublicFoo").unwrap().unwrap();
        assert_eq!(resolved.canonical_path, "crate::alpha::Foo");
        assert!(resolved.via_binding.is_some());
    }
}
