use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::emit_kernels::{
    compute_emit_plan,
    normalize_module_tree,
    validate_emitted_rust_structure,
    IrItem,
};

fn module_to_file(crate_name: &str, module: &str) -> PathBuf {
    let mut path = PathBuf::from("emit");
    path.push(crate_name);
    path.push("src");

    if module.trim().is_empty() || module == "root" {
        path.push("lib.rs");
        return path;
    }

    let parts: Vec<&str> = module.split("::").collect();

    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            path.push(format!("{}.rs", part));
        } else {
            path.push(part);
        }
    }

    path
}

fn parent_modules(module: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut parts: Vec<&str> = module.split("::").collect();

    while parts.len() > 1 {
        parts.pop();
        out.push(parts.join("::"));
    }

    out
}

fn write_module_file(path: &Path, blocks: &[String]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let mut content = String::new();

    for block in blocks {
        content.push_str(block);
        if !block.ends_with('\n') {
            content.push('\n');
        }
        content.push('\n');
    }

    fs::write(path, content).map_err(|e| e.to_string())
}

fn ensure_mod_declarations(modules: &BTreeSet<String>, crate_name: &str) -> Result<(), String> {
    for module in modules {
        if module.trim().is_empty() || module == "root" {
            continue;
        }

        let parts: Vec<&str> = module.split("::").collect();

        // Determine parent module (or root/lib.rs)
        let parent = if parts.len() == 1 {
            "root".to_string()
        } else {
            parts[..parts.len() - 1].join("::")
        };

        let parent_path = module_to_file(crate_name, &parent);

        if !parent_path.exists() {
            write_module_file(&parent_path, &[])?;
        }

        let child = parts.last().unwrap();

        if child.trim().is_empty() {
            continue;
        }

        let decl = format!("pub mod {};", child);

        let mut content = fs::read_to_string(&parent_path).unwrap_or_default();

        if !content.lines().any(|l| l.trim() == decl) {
            if !content.ends_with('\n') && !content.is_empty() {
                content.push('\n');
            }
            content.push_str(&decl);
            content.push('\n');
            fs::write(&parent_path, content).map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}

pub fn emit_rust_crate(crate_name: &str, items: Vec<IrItem>) -> Result<(), String> {
    let validation_errors = validate_emitted_rust_structure(&items);

    if !validation_errors.is_empty() {
        return Err(format!(
            "IR validation failed: {}",
            validation_errors.join("; ")
        ));
    }

    let modules = normalize_module_tree(&items);

    if modules.is_empty() {
        return Err("No modules discovered during normalization".to_string());
    }

    let plan = compute_emit_plan(&items);

    if plan.is_empty() {
        return Err("Emission plan produced no modules".to_string());
    }

    let mut emitted: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut module_set = BTreeSet::new();

    for (module, blocks) in plan {
        let path = module_to_file(crate_name, &module);
        write_module_file(&path, &blocks)?;

        module_set.insert(module.clone());
        emitted.insert(module, path);
    }

    if emitted.is_empty() {
        return Err("No files emitted during projection".to_string());
    }

    ensure_mod_declarations(&module_set, crate_name)?;

    Ok(())
}
