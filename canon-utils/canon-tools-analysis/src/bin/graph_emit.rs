use anyhow::{anyhow, Context, Result};
use canon_analysis::{GraphArtifactIndex, GraphArtifactSummary};
use canon_ir::{csr_graph::CsrGraph, CanonIR, CanonNodeKind, EdgeKind};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SourceSymbol {
    module_path: String,
    symbol: String,
    kind: String,
    file: PathBuf,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let workspace = arg_value(&args, "--workspace").unwrap_or("/workspace/ai_sandbox/canon".to_string());
    let crate_name = arg_value(&args, "--crate");
    let crate_path = arg_value(&args, "--crate-path");

    let (crate_root, name) = if let Some(path) = crate_path {
        let root = PathBuf::from(path);
        let name = crate_name_from_manifest(&root.join("Cargo.toml"))
            .unwrap_or_else(|| root.file_name().and_then(|s| s.to_str()).unwrap_or("crate").to_string());
        (root, name)
    } else if let Some(name) = crate_name {
        let root = find_crate_root(&workspace, &name)?;
        (root, name)
    } else {
        return Err(anyhow!("usage: graph_emit --crate <name> [--workspace <path>] | --crate-path <path>"));
    };

    let artifact = write_graph_artifact_from_source(&crate_root, Path::new(&workspace), &name)?;
    println!("{}", serde_json::to_string_pretty(&artifact)?);
    Ok(())
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|w| w[0] == name)
        .map(|w| w[1].to_string())
}

fn find_crate_root(workspace: &str, crate_name: &str) -> Result<PathBuf> {
    let output = Command::new("cargo")
        .current_dir(workspace)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .with_context(|| "failed to run cargo metadata")?;
    if !output.status.success() {
        return Err(anyhow!("cargo metadata failed"));
    }
    let value: Value = serde_json::from_slice(&output.stdout)?;
    let packages = value.get("packages").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    for pkg in packages {
        let name = pkg.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name == crate_name {
            if let Some(manifest) = pkg.get("manifest_path").and_then(|v| v.as_str()) {
                let path = PathBuf::from(manifest);
                return Ok(path.parent().unwrap_or(Path::new(workspace)).to_path_buf());
            }
        }
    }
    Err(anyhow!("crate '{crate_name}' not found in cargo metadata"))
}

fn crate_name_from_manifest(manifest: &Path) -> Option<String> {
    let content = fs::read_to_string(manifest).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("name") {
            if let Some(eq) = rest.find('=') {
                let value = rest[eq + 1..].trim().trim_matches('"').trim_matches('\'');
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

fn derive_module_path(project: &Path, file: &Path) -> String {
    let src_root = project.join("src");
    let rel = file.strip_prefix(&src_root).unwrap_or(file);
    let mut segments: Vec<String> = rel
        .components()
        .filter_map(|c| c.as_os_str().to_str().map(|s| s.to_string()))
        .collect();
    let filename = segments.pop().unwrap_or_else(|| "lib.rs".to_string());
    if filename != "lib.rs" && filename != "main.rs" && filename != "mod.rs" {
        segments.push(filename.trim_end_matches(".rs").to_string());
    }
    let mut out = vec!["crate".to_string()];
    out.extend(segments.into_iter().filter(|s| !s.is_empty()));
    out.join("::")
}

fn collect_source_symbols(project: &Path) -> Vec<SourceSymbol> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(project.join("src"))
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let content = fs::read_to_string(path).unwrap_or_default();
        let ast = syn::parse_file(&content).unwrap_or_else(|_| syn::File { shebang: None, attrs: Vec::new(), items: Vec::new() });
        let module_path = derive_module_path(project, path);
        collect_items(&ast.items, path, &module_path, &mut out);
    }
    out
}

fn collect_items(items: &[syn::Item], file: &Path, module_path: &str, out: &mut Vec<SourceSymbol>) {
    for item in items {
        match item {
            syn::Item::Fn(item) => out.push(SourceSymbol { module_path: module_path.to_string(), symbol: item.sig.ident.to_string(), kind: "fn".to_string(), file: file.to_path_buf() }),
            syn::Item::Struct(item) => out.push(SourceSymbol { module_path: module_path.to_string(), symbol: item.ident.to_string(), kind: "struct".to_string(), file: file.to_path_buf() }),
            syn::Item::Enum(item) => out.push(SourceSymbol { module_path: module_path.to_string(), symbol: item.ident.to_string(), kind: "enum".to_string(), file: file.to_path_buf() }),
            syn::Item::Trait(item) => out.push(SourceSymbol { module_path: module_path.to_string(), symbol: item.ident.to_string(), kind: "trait".to_string(), file: file.to_path_buf() }),
            syn::Item::Mod(item) => {
                out.push(SourceSymbol { module_path: module_path.to_string(), symbol: item.ident.to_string(), kind: "module".to_string(), file: file.to_path_buf() });
                if let Some((_, items)) = &item.content {
                    let nested = format!("{module_path}::{}", item.ident);
                    collect_items(items, file, &nested, out);
                }
            }
            _ => {}
        }
    }
}

fn write_graph_artifact_from_source(project: &Path, workspace_root: &Path, crate_name: &str) -> Result<GraphArtifactSummary> {
    let symbols = collect_source_symbols(project);
    let mut modules: BTreeSet<String> = BTreeSet::new();
    modules.insert("crate".to_string());
    for entry in walkdir::WalkDir::new(project.join("src"))
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        modules.insert(derive_module_path(project, path));
    }
    for symbol in &symbols {
        modules.insert(symbol.module_path.clone());
    }

    let mut ir = CanonIR::new();
    let mut module_ids = BTreeMap::new();
    for module in &modules {
        let path_id = ir.intern_path(module).unwrap();
        let node_id = ir.push_node(CanonNodeKind::Module { path_id, flags: 0 });
        module_ids.insert(module.clone(), node_id.0);
    }

    let mut module_edges = Vec::new();
    for symbol in &symbols {
        let name_id = ir.intern_name(&symbol.symbol);
        let node_id = match symbol.kind.as_str() {
            "fn" => ir.push_node(CanonNodeKind::Fn { name_id, sig_id: canon_ir::CanonId(0), body: None, attrs: Vec::new(), flags: 0 }),
            "struct" => ir.push_node(CanonNodeKind::Struct { name_id, generics: Vec::new(), fields: Vec::new(), derives: Vec::new(), attrs: Vec::new(), flags: 0, struct_kind: 0 }),
            "enum" => ir.push_node(CanonNodeKind::Enum { name_id, generics: Vec::new(), variants: Vec::new(), derives: Vec::new(), attrs: Vec::new(), flags: 0 }),
            "trait" => ir.push_node(CanonNodeKind::Trait { name_id, generics: Vec::new(), methods: Vec::new(), attrs: Vec::new(), flags: 0 }),
            _ => continue,
        };
        if let Some(module_id) = module_ids.get(&symbol.module_path) {
            module_edges.push((*module_id, node_id.0, EdgeKind::Contains));
        }
    }

    let node_data = ir.nodes.iter().map(|node| node.id).collect::<Vec<_>>();
    ir.module_graph = CsrGraph::from_edges(node_data.clone(), module_edges);
    ir.call_graph = CsrGraph::from_edges(node_data.clone(), Vec::new());
    ir.cfg_graph = CsrGraph::from_edges(node_data, Vec::new());

    let artifact_id = format!("{}-{}", crate_name, unix_ms());
    let artifact_dir = workspace_root.join("state").join("graph");
    fs::create_dir_all(artifact_dir.join("index").join("by_crate"))?;
    fs::create_dir_all(artifact_dir.join("index").join("by_hash"))?;
    let artifact_path = artifact_dir.join(format!("{artifact_id}.json"));
    fs::write(&artifact_path, serde_json::to_vec(&ir)?)?;
    let summary = GraphArtifactSummary {
        artifact_id: artifact_id.clone(),
        artifact_path: artifact_path.clone(),
        crate_name: crate_name.to_string(),
        node_count: ir.nodes.len(),
        edge_count: ir.module_graph.edge_count() + ir.call_graph.edge_count() + ir.cfg_graph.edge_count(),
        file_count: modules.len(),
        call_edge_count: ir.call_graph.edge_count(),
        module_edge_count: ir.module_graph.edge_count(),
        cfg_edge_count: ir.cfg_graph.edge_count(),
        captured_at_ms: unix_ms(),
    };
    // Always record the by-hash entry.
    fs::write(artifact_dir.join("index").join("by_hash").join(format!("{artifact_id}.json")), serde_json::to_vec(&summary)?)?;

    // Only update latest/by_crate if no richer rustc artifact exists.
    let by_crate_path = artifact_dir.join("index").join("by_crate").join(format!("{crate_name}.json"));
    let mut allow_index_update = true;
    if by_crate_path.exists() {
        if let Ok(existing_raw) = fs::read_to_string(&by_crate_path) {
            if let Ok(existing) = serde_json::from_str::<GraphArtifactSummary>(&existing_raw) {
                let existing_richer = existing.call_edge_count > 0 || existing.cfg_edge_count > 0;
                let new_richer = summary.call_edge_count > 0 || summary.cfg_edge_count > 0;
                if existing_richer && !new_richer {
                    allow_index_update = false;
                }
            }
        }
    }
    if allow_index_update {
        let index = GraphArtifactIndex { latest_workspace: summary.clone() };
        fs::write(artifact_dir.join("index").join("latest_workspace.json"), serde_json::to_vec(&index)?)?;
        fs::write(&by_crate_path, serde_json::to_vec(&summary)?)?;
    }
    Ok(summary)
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
