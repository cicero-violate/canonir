//! AST-based capture (mirrored from capture crate)

use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

use model::model_ir::*;

pub fn capture_ast(root: &Path) -> Result<Model> {
    let mut modules = Vec::new();
    let mut structs = Vec::new();
    let mut traits = Vec::new();
    let mut impls = Vec::new();
    let mut functions = Vec::new();

    let mut order = 0usize;

    for file in collect_rs_files(root)? {
        let content = fs::read_to_string(&file)?;
        let ast = syn::parse_file(&content)?;

        let module_id = format!("mod:{}", file.display());

        modules.push(Module { id: module_id.clone(), path: module_id.clone(), file: file.display().to_string(), content: content.clone(), parent: None, children: vec![], order });
        order += 1;

        for item in ast.items {
            match item {
                syn::Item::Struct(s) => {
                    structs.push(Struct { id: format!("struct:{}", s.ident), name: s.ident.to_string(), module: module_id.clone(), visibility: visibility(&s.vis), order });
                    order += 1;
                }
                syn::Item::Trait(t) => {
                    traits.push(Trait { id: format!("trait:{}", t.ident), name: t.ident.to_string(), module: module_id.clone(), visibility: visibility(&t.vis), order, methods: vec![] });
                    order += 1;
                }
                syn::Item::Fn(f) => {
                    functions.push(Function {
                        id: format!("fn:{}", f.sig.ident),
                        name: f.sig.ident.to_string(),
                        module: module_id.clone(),
                        visibility: visibility(&f.vis),
                        order,
                        params: vec![],
                        return_type: "unknown".into(),
                        body_ir: BodyIR::Todo,
                        effects: Effects::default(),
                    });
                    order += 1;
                }
                syn::Item::Impl(_) => {
                    impls.push(Impl { id: format!("impl:{}", order), struct_: "unknown".into(), trait_: None, module: module_id.clone(), kind: "inherent".into(), order, methods: vec![] });
                    order += 1;
                }
                _ => {}
            }
        }
    }

    Ok(Model {
        version: "0.1".into(),
        crate_: Crate { name: "unknown".into(), root: root.display().to_string(), edition: "2021".into() },
        modules,
        structs,
        traits,
        impls,
        functions,
        hir_items: Vec::new(),
        hir_exprs: Vec::new(),
        hir_paths: Vec::new(),
        hir_types: Vec::new(),
        mir_bodies: Vec::new(),
        rules: Rules { pairing_scope: "module".into(), impl_placement: "same_module".into(), ordering: "source".into() },
        limits: Limits { max_structs_per_module: 1000, max_traits_per_module: 1000, max_impls_per_module: 1000 },
        ordering: Ordering { modules: vec![], structs: vec![], traits: vec![], impls: vec![] },
    })
}

fn collect_rs_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root).into_iter().filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        name != "target" && name != ".git"
    }) {
        let e = entry?;
        if e.path().extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(e.path().to_path_buf());
        }
    }
    Ok(out)
}

fn visibility(v: &syn::Visibility) -> String {
    match v {
        syn::Visibility::Public(_) => "public".into(),
        _ => "private".into(),
    }
}
