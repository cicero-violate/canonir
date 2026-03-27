use crate::{
    add_import_paths, create_module_files, move_symbol_pairs, rename_symbol_pairs,
    SymbolIndex,
};
use canon_analysis::{GraphArtifactIndex, GraphArtifactSummary};
use canon_ir::{csr_graph::CsrGraph, CanonIR, CanonNodeKind, EdgeKind};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use tempfile::TempDir;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SourceSymbol {
    module_path: String,
    symbol: String,
    kind: String,
    file: PathBuf,
}

fn temp_project() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    dir
}

fn reports_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn with_local_reports_env<T>(project: &Path, f: impl FnOnce() -> T) -> T {
    let _guard = reports_env_lock().lock().unwrap();
    let reports_root = project.join("state/reports_out/crates/unknown");
    let previous = std::env::var_os("CANON_REPORTS_OUT");
    std::env::set_var("CANON_REPORTS_OUT", &reports_root);
    let result = f();
    match previous {
        Some(value) => std::env::set_var("CANON_REPORTS_OUT", value),
        None => std::env::remove_var("CANON_REPORTS_OUT"),
    }
    result
}

fn write_project_files(project: &Path, files: &[(&str, &str)]) {
    let cargo_toml = r#"[package]
name = "semantic_capability_fixture"
version = "0.1.0"
edition = "2021"

[lib]
path = "src/lib.rs"
"#;
    fs::write(project.join("Cargo.toml"), cargo_toml).unwrap();
    for (path, content) in files {
        let full = project.join(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(full, content).unwrap();
    }
}

fn derive_module_path(project: &Path, file: &Path) -> String {
    let src_root = project.join("src");
    let rel = file.strip_prefix(&src_root).unwrap();
    let mut segments: Vec<String> = rel
        .components()
        .filter_map(|c| c.as_os_str().to_str().map(|s| s.to_string()))
        .collect();
    let filename = segments.pop().unwrap();
    if filename != "lib.rs" && filename != "main.rs" && filename != "mod.rs" {
        segments.push(filename.trim_end_matches(".rs").to_string());
    }
    let mut out = vec!["crate".to_string()];
    out.extend(segments.into_iter().filter(|s| !s.is_empty()));
    out.join("::")
}

fn collect_source_symbols(project: &Path) -> Vec<SourceSymbol> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(project.join("src")).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let content = fs::read_to_string(path).unwrap();
        let ast = syn::parse_file(&content).unwrap();
        let module_path = derive_module_path(project, path);
        collect_items(&ast.items, path, &module_path, &mut out);
    }
    out
}

fn collect_items(items: &[syn::Item], file: &Path, module_path: &str, out: &mut Vec<SourceSymbol>) {
    for item in items {
        match item {
            syn::Item::Fn(item) => out.push(SourceSymbol {
                module_path: module_path.to_string(),
                symbol: item.sig.ident.to_string(),
                kind: "fn".to_string(),
                file: file.to_path_buf(),
            }),
            syn::Item::Struct(item) => out.push(SourceSymbol {
                module_path: module_path.to_string(),
                symbol: item.ident.to_string(),
                kind: "struct".to_string(),
                file: file.to_path_buf(),
            }),
            syn::Item::Enum(item) => out.push(SourceSymbol {
                module_path: module_path.to_string(),
                symbol: item.ident.to_string(),
                kind: "enum".to_string(),
                file: file.to_path_buf(),
            }),
            syn::Item::Trait(item) => out.push(SourceSymbol {
                module_path: module_path.to_string(),
                symbol: item.ident.to_string(),
                kind: "trait".to_string(),
                file: file.to_path_buf(),
            }),
            syn::Item::Mod(item) => {
                out.push(SourceSymbol {
                    module_path: module_path.to_string(),
                    symbol: item.ident.to_string(),
                    kind: "module".to_string(),
                    file: file.to_path_buf(),
                });
                if let Some((_, items)) = &item.content {
                    let nested = format!("{module_path}::{}", item.ident);
                    collect_items(items, file, &nested, out);
                }
            }
            _ => {}
        }
    }
}

fn write_graph_artifact_from_source(project: &Path) {
    let symbols = collect_source_symbols(project);
    let mut modules: BTreeSet<String> = BTreeSet::new();
    modules.insert("crate".to_string());
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
            "fn" => ir.push_node(CanonNodeKind::Fn {
                name_id,
                sig_id: canon_ir::CanonId(0),
                body: None,
                attrs: Vec::new(),
                flags: 0,
            }),
            "struct" => ir.push_node(CanonNodeKind::Struct {
                name_id,
                generics: Vec::new(),
                fields: Vec::new(),
                derives: Vec::new(),
                attrs: Vec::new(),
                flags: 0,
                struct_kind: 0,
            }),
            "enum" => ir.push_node(CanonNodeKind::Enum {
                name_id,
                generics: Vec::new(),
                variants: Vec::new(),
                derives: Vec::new(),
                attrs: Vec::new(),
                flags: 0,
            }),
            "trait" => ir.push_node(CanonNodeKind::Trait {
                name_id,
                generics: Vec::new(),
                methods: Vec::new(),
                attrs: Vec::new(),
                flags: 0,
            }),
            "module" => continue,
            _ => continue,
        };
        let module_id = *module_ids.get(&symbol.module_path).unwrap();
        module_edges.push((module_id, node_id.0, EdgeKind::Contains));
    }

    let node_data = ir.nodes.iter().map(|node| node.id).collect::<Vec<_>>();
    ir.module_graph = CsrGraph::from_edges(node_data.clone(), module_edges);
    ir.call_graph = CsrGraph::from_edges(node_data.clone(), Vec::new());
    ir.cfg_graph = CsrGraph::from_edges(node_data, Vec::new());

    let graph_dir = project.join("state/graph");
    fs::create_dir_all(graph_dir.join("index/by_crate")).unwrap();
    fs::create_dir_all(graph_dir.join("index/by_hash")).unwrap();
    let artifact_id = format!("artifact-{}", symbols.len());
    let artifact_path = graph_dir.join(format!("{artifact_id}.json"));
    fs::write(&artifact_path, serde_json::to_vec(&ir).unwrap()).unwrap();

    let summary = GraphArtifactSummary {
        artifact_id: artifact_id.clone(),
        artifact_path: artifact_path.clone(),
        crate_name: "semantic_capability_fixture".to_string(),
        node_count: ir.nodes.len(),
        edge_count: ir.module_graph.edge_count(),
        file_count: walkdir::WalkDir::new(project.join("src"))
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("rs"))
            .count(),
        call_edge_count: 0,
        module_edge_count: ir.module_graph.edge_count(),
        cfg_edge_count: 0,
    };
    let index = GraphArtifactIndex {
        latest_workspace: summary.clone(),
    };
    fs::write(graph_dir.join("index/latest_workspace.json"), serde_json::to_vec(&index).unwrap()).unwrap();
    fs::write(
        graph_dir.join("index/by_crate/semantic_capability_fixture.json"),
        serde_json::to_vec(&summary).unwrap(),
    )
    .unwrap();
    fs::write(
        graph_dir.join("index/by_hash").join(format!("{artifact_id}.json")),
        serde_json::to_vec(&summary).unwrap(),
    )
    .unwrap();
}

fn write_report_spans_from_source(project: &Path) {
    let out_dir = project.join("state/reports_out/crates/unknown/graph");
    fs::create_dir_all(&out_dir).unwrap();
    let symbols = collect_source_symbols(project);
    let mut kinds = serde_json::Map::new();
    let mut lines = Vec::new();
    let files: Vec<PathBuf> = walkdir::WalkDir::new(project.join("src"))
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("rs"))
        .map(|entry| entry.path().to_path_buf())
        .collect();
    for symbol in symbols {
        if symbol.kind == "module" {
            continue;
        }
        let symbol_id = format!("{}::{}", symbol.module_path, symbol.symbol);
        kinds.insert(symbol_id.clone(), serde_json::Value::String(symbol.kind.clone()));
        for file in &files {
            let content = fs::read_to_string(file).unwrap();
            let mut offset = 0usize;
            while let Some(found) = content[offset..].find(&symbol.symbol) {
                let lo = offset + found;
                let hi = lo + symbol.symbol.len();
                let left_ok = lo == 0
                    || !content[..lo]
                        .chars()
                        .next_back()
                        .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_');
                let right_ok = hi == content.len()
                    || !content[hi..]
                        .chars()
                        .next()
                        .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_');
                if left_ok && right_ok {
                    lines.push(serde_json::json!({
                        "symbol_id": symbol_id,
                        "file": file.display().to_string(),
                        "lo": lo,
                        "hi": hi,
                    }));
                }
                offset = hi;
            }
        }
    }
    fs::write(out_dir.join("symbols.json"), serde_json::to_vec(&kinds).unwrap()).unwrap();
    let mut data = String::new();
    for line in lines {
        data.push_str(&serde_json::to_string(&line).unwrap());
        data.push('\n');
    }
    fs::write(out_dir.join("symbol_spans.jsonl"), data).unwrap();
}

fn cargo_check(project: &Path) {
    let output = Command::new("cargo")
        .arg("check")
        .arg("--quiet")
        .current_dir(project)
        .env("CARGO_TARGET_DIR", project.join("target"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cargo check failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_graph_and_invariants(project: &Path, expected: &[&str], absent: &[&str]) {
    write_graph_artifact_from_source(project);
    let index = with_local_reports_env(project, || SymbolIndex::build(project).unwrap());
    let symbols: BTreeSet<String> = collect_source_symbols(project)
        .into_iter()
        .filter(|symbol| symbol.kind != "module")
        .map(|symbol| format!("{}::{}", symbol.module_path, symbol.symbol))
        .collect();
    let catalog: BTreeSet<String> = index
        .symbol_catalog()
        .into_iter()
        .filter(|(_, kind)| kind != "module")
        .map(|(id, _)| id)
        .collect();
    assert_eq!(catalog, symbols, "graph catalog diverged from source graph");
    for wanted in expected {
        assert!(catalog.contains(*wanted), "missing expected symbol {wanted}");
    }
    for forbidden in absent {
        assert!(!catalog.contains(*forbidden), "unexpected symbol {forbidden}");
    }
    let source_symbols = collect_source_symbols(project);
    let mut dedup = BTreeSet::new();
    for symbol in &source_symbols {
        if symbol.kind == "module" {
            continue;
        }
        let key = format!("{}::{}", symbol.module_path, symbol.symbol);
        assert!(dedup.insert(key), "duplicate definition remained in source graph");
        assert!(symbol.file.exists(), "graph points at missing file {}", symbol.file.display());
    }
    cargo_check(project);
}

#[test]
fn rename_symbol_simple_case() {
    let dir = temp_project();
    write_project_files(
        dir.path(),
        &[(
            "src/lib.rs",
            "pub fn run() -> usize { helper() }\n\nfn helper() -> usize { 1 }\n",
        )],
    );
    write_graph_artifact_from_source(dir.path());
    write_report_spans_from_source(dir.path());
    let report = with_local_reports_env(dir.path(), || {
        let index = SymbolIndex::build(dir.path()).unwrap();
        assert!(index.spans_for("crate::helper").is_some(), "missing helper spans");
        rename_symbol_pairs(dir.path(), &[("crate::helper".into(), "crate::compute".into())])
    });
    assert!(report.error.is_none(), "{:?}", report.error);
    let code = fs::read_to_string(dir.path().join("src/lib.rs")).unwrap();
    assert!(code.contains("fn compute()"));
    assert!(code.contains("compute()"));
    assert_graph_and_invariants(dir.path(), &["crate::compute", "crate::run"], &["crate::helper"]);
}

#[test]
fn rename_symbol_cross_module_references() {
    let dir = temp_project();
    write_project_files(
        dir.path(),
        &[
            ("src/lib.rs", "pub mod alpha;\npub mod beta;\n"),
            ("src/alpha.rs", "pub struct Foo;\n"),
            (
                "src/beta.rs",
                "use crate::alpha::Foo;\n\npub fn make() -> Foo { Foo }\n",
            ),
        ],
    );
    write_graph_artifact_from_source(dir.path());
    write_report_spans_from_source(dir.path());
    let report = with_local_reports_env(dir.path(), || {
        let index = SymbolIndex::build(dir.path()).unwrap();
        assert!(index.spans_for("crate::alpha::Foo").is_some(), "missing Foo spans");
        rename_symbol_pairs(
            dir.path(),
            &[("crate::alpha::Foo".into(), "crate::alpha::Bar".into())],
        )
    });
    assert!(report.error.is_none(), "{:?}", report.error);
    let beta = fs::read_to_string(dir.path().join("src/beta.rs")).unwrap();
    assert!(beta.contains("use crate::alpha::Bar;"));
    assert!(beta.contains("-> Bar"));
    assert_graph_and_invariants(
        dir.path(),
        &["crate::alpha::Bar", "crate::beta::make"],
        &["crate::alpha::Foo"],
    );
}

#[test]
fn rename_symbol_duplicate_symbol_is_rejected() {
    let dir = temp_project();
    write_project_files(
        dir.path(),
        &[
            ("src/lib.rs", "pub mod alpha;\npub mod beta;\n"),
            ("src/alpha.rs", "pub struct Foo;\n"),
            (
                "src/beta.rs",
                "pub struct Foo;\npub struct FooBeta;\npub fn make() -> Foo { Foo }\n",
            ),
        ],
    );
    write_graph_artifact_from_source(dir.path());
    write_report_spans_from_source(dir.path());
    let report = with_local_reports_env(dir.path(), || {
        rename_symbol_pairs(
            dir.path(),
            &[("crate::beta::Foo".into(), "crate::beta::FooBeta".into())],
        )
    });
    assert!(report.error.is_some());
}

#[test]
fn rename_symbol_trait_impl_interaction() {
    let dir = temp_project();
    write_project_files(
        dir.path(),
        &[
            ("src/lib.rs", "pub mod alpha;\npub mod beta;\n"),
            (
                "src/alpha.rs",
                "pub trait Worker { fn work(&self) -> usize; }\n\npub struct Job;\n\nimpl Worker for Job { fn work(&self) -> usize { 1 } }\n",
            ),
            (
                "src/beta.rs",
                "use crate::alpha::{Job, Worker};\n\npub fn run() -> usize { Job.work() }\n",
            ),
        ],
    );
    write_graph_artifact_from_source(dir.path());
    write_report_spans_from_source(dir.path());
    let report = with_local_reports_env(dir.path(), || {
        let index = SymbolIndex::build(dir.path()).unwrap();
        assert!(index.spans_for("crate::alpha::Worker").is_some(), "missing Worker spans");
        rename_symbol_pairs(
            dir.path(),
            &[("crate::alpha::Worker".into(), "crate::alpha::Runnable".into())],
        )
    });
    assert!(report.error.is_none(), "{:?}", report.error);
    let alpha = fs::read_to_string(dir.path().join("src/alpha.rs")).unwrap();
    let beta = fs::read_to_string(dir.path().join("src/beta.rs")).unwrap();
    assert!(alpha.contains("trait Runnable"));
    assert!(alpha.contains("impl Runnable for Job"));
    assert!(beta.contains("use crate::alpha::{Job, Runnable};"));
    assert_graph_and_invariants(
        dir.path(),
        &["crate::alpha::Runnable", "crate::alpha::Job"],
        &["crate::alpha::Worker"],
    );
}

#[test]
fn rename_symbol_invalid_missing_symbol() {
    let dir = temp_project();
    write_project_files(dir.path(), &[("src/lib.rs", "pub fn run() {}\n")]);
    write_graph_artifact_from_source(dir.path());
    let report = with_local_reports_env(dir.path(), || {
        rename_symbol_pairs(
            dir.path(),
            &[("crate::missing::Nope".into(), "crate::missing::Nope2".into())],
        )
    });
    assert!(report.error.is_some());
}

#[test]
fn move_symbol_simple_case() {
    let dir = temp_project();
    write_project_files(
        dir.path(),
        &[
            ("src/lib.rs", "pub mod alpha;\npub mod beta;\n"),
            ("src/alpha.rs", "pub struct Worker;\n"),
            ("src/beta.rs", "\n"),
        ],
    );
    write_graph_artifact_from_source(dir.path());
    let report = move_symbol_pairs(
        dir.path(),
        &[("crate::alpha::Worker".into(), "crate::beta".into())],
    );
    assert!(report.error.is_none(), "{:?}", report.error);
    let alpha = fs::read_to_string(dir.path().join("src/alpha.rs")).unwrap();
    let beta = fs::read_to_string(dir.path().join("src/beta.rs")).unwrap();
    assert!(!alpha.contains("Worker"));
    assert!(beta.contains("Worker"));
    assert_graph_and_invariants(
        dir.path(),
        &["crate::beta::Worker"],
        &["crate::alpha::Worker"],
    );
}

#[test]
fn move_symbol_cross_module_reference_updates() {
    let dir = temp_project();
    write_project_files(
        dir.path(),
        &[
            ("src/lib.rs", "pub mod alpha;\npub mod beta;\npub mod gamma;\n"),
            ("src/alpha.rs", "pub struct Worker;\n"),
            ("src/beta.rs", "\n"),
            (
                "src/gamma.rs",
                "use crate::alpha::Worker;\n\npub fn make() -> Worker { Worker }\n",
            ),
        ],
    );
    write_graph_artifact_from_source(dir.path());
    let report = move_symbol_pairs(
        dir.path(),
        &[("crate::alpha::Worker".into(), "crate::beta".into())],
    );
    assert!(report.error.is_none(), "{:?}", report.error);
    let gamma = fs::read_to_string(dir.path().join("src/gamma.rs")).unwrap();
    assert!(gamma.contains("use crate::beta::Worker;"));
    assert_graph_and_invariants(
        dir.path(),
        &["crate::beta::Worker", "crate::gamma::make"],
        &["crate::alpha::Worker"],
    );
}

#[test]
fn move_symbol_invalid_missing_symbol() {
    let dir = temp_project();
    write_project_files(dir.path(), &[("src/lib.rs", "pub mod alpha;\n"), ("src/alpha.rs", "\n")]);
    write_graph_artifact_from_source(dir.path());
    let report = move_symbol_pairs(
        dir.path(),
        &[("crate::alpha::Missing".into(), "crate::beta".into())],
    );
    assert!(report.error.is_some());
}

#[test]
fn move_symbol_invalid_missing_target_module() {
    let dir = temp_project();
    write_project_files(
        dir.path(),
        &[
            ("src/lib.rs", "pub mod alpha;\n"),
            ("src/alpha.rs", "pub struct Worker;\n"),
        ],
    );
    write_graph_artifact_from_source(dir.path());
    let report = move_symbol_pairs(
        dir.path(),
        &[("crate::alpha::Worker".into(), "crate::beta".into())],
    );
    assert!(report.error.is_some());
}

#[test]
fn import_resolution_simple_case() {
    let dir = temp_project();
    write_project_files(
        dir.path(),
        &[
            ("src/lib.rs", "pub mod alpha;\npub mod beta;\n"),
            ("src/alpha.rs", "pub struct Foo;\n"),
            ("src/beta.rs", "pub fn make() -> Foo { Foo }\n"),
        ],
    );
    let report = add_import_paths(dir.path(), &[("src/beta.rs".into(), "crate::alpha::Foo".into())]);
    assert!(report.error.is_none(), "{:?}", report.error);
    let beta = fs::read_to_string(dir.path().join("src/beta.rs")).unwrap();
    assert!(beta.contains("use crate::alpha::Foo;"));
    assert_graph_and_invariants(dir.path(), &["crate::beta::make"], &[]);
}

#[test]
fn import_resolution_cross_module_alias_case() {
    let dir = temp_project();
    write_project_files(
        dir.path(),
        &[
            ("src/lib.rs", "pub mod alpha;\npub mod beta;\n"),
            ("src/alpha.rs", "pub struct Foo;\n"),
            (
                "src/beta.rs",
                "pub struct Foo;\n\npub fn wrap(value: AlphaFoo) -> AlphaFoo { value }\n",
            ),
        ],
    );
    let report = add_import_paths(
        dir.path(),
        &[("src/beta.rs".into(), "crate::alpha::Foo as AlphaFoo".into())],
    );
    assert!(report.error.is_none(), "{:?}", report.error);
    let beta = fs::read_to_string(dir.path().join("src/beta.rs")).unwrap();
    assert!(beta.contains("use crate::alpha::Foo as AlphaFoo;"));
    assert_graph_and_invariants(dir.path(), &["crate::beta::wrap", "crate::beta::Foo"], &[]);
}

#[test]
fn import_resolution_trait_interaction() {
    let dir = temp_project();
    write_project_files(
        dir.path(),
        &[
            ("src/lib.rs", "pub mod alpha;\npub mod beta;\n"),
            (
                "src/alpha.rs",
                "pub trait Greeter { fn greet(&self) -> &'static str; }\n\npub struct Person;\n\nimpl Greeter for Person { fn greet(&self) -> &'static str { \"hi\" } }\n",
            ),
            (
                "src/beta.rs",
                "use crate::alpha::Person;\n\npub fn run() -> &'static str { Person.greet() }\n",
            ),
        ],
    );
    let report = add_import_paths(dir.path(), &[("src/beta.rs".into(), "crate::alpha::Greeter".into())]);
    assert!(report.error.is_none(), "{:?}", report.error);
    let beta = fs::read_to_string(dir.path().join("src/beta.rs")).unwrap();
    assert!(beta.contains("use crate::alpha::Greeter;"));
    assert_graph_and_invariants(dir.path(), &["crate::beta::run"], &[]);
}

#[test]
fn import_resolution_invalid_missing_file() {
    let dir = temp_project();
    write_project_files(dir.path(), &[("src/lib.rs", "pub fn run() {}\n")]);
    let report = add_import_paths(dir.path(), &[("src/missing.rs".into(), "crate::foo::Bar".into())]);
    assert!(report.error.is_some());
}

#[test]
fn import_resolution_invalid_non_canonical_path() {
    let dir = temp_project();
    write_project_files(dir.path(), &[("src/lib.rs", "pub fn run() {}\n")]);
    let report = add_import_paths(
        dir.path(),
        &[("src/lib.rs".into(), "foo::Bar".into())],
    );
    assert!(report.error.is_some());
}

#[test]
fn module_creation_simple_case() {
    let dir = temp_project();
    write_project_files(dir.path(), &[("src/lib.rs", "pub mod merge;\n")]);
    let report = create_module_files(dir.path(), &[("src/merge.rs".into(), Some("merge".into()))]);
    assert!(report.error.is_none(), "{:?}", report.error);
    let merge = fs::read_to_string(dir.path().join("src/merge.rs")).unwrap();
    assert!(merge.contains("module: merge"));
    assert_graph_and_invariants(dir.path(), &[], &[]);
}

#[test]
fn module_creation_nested_case() {
    let dir = temp_project();
    write_project_files(
        dir.path(),
        &[
            ("src/lib.rs", "pub mod feature;\n"),
            ("src/feature.rs", "pub mod inner;\n"),
        ],
    );
    let report = create_module_files(
        dir.path(),
        &[("src/feature/inner.rs".into(), Some("inner".into()))],
    );
    assert!(report.error.is_none(), "{:?}", report.error);
    assert!(dir.path().join("src/feature/inner.rs").exists());
    assert_graph_and_invariants(dir.path(), &[], &[]);
}

#[test]
fn module_creation_existing_file_is_stable() {
    let dir = temp_project();
    write_project_files(
        dir.path(),
        &[
            ("src/lib.rs", "pub mod merge;\n"),
            ("src/merge.rs", "pub struct Merge;\n"),
        ],
    );
    let report = create_module_files(dir.path(), &[("src/merge.rs".into(), Some("merge".into()))]);
    assert!(report.error.is_none(), "{:?}", report.error);
    let merge = fs::read_to_string(dir.path().join("src/merge.rs")).unwrap();
    assert!(merge.contains("pub struct Merge;"));
    assert_graph_and_invariants(dir.path(), &["crate::merge::Merge"], &[]);
}

#[test]
fn module_creation_invalid_non_rust_path() {
    let dir = temp_project();
    write_project_files(dir.path(), &[("src/lib.rs", "pub fn run() {}\n")]);
    let report = create_module_files(
        dir.path(),
        &[("src/merge.txt".into(), Some("merge".into()))],
    );
    assert!(report.error.is_some());
}
