use std::collections::HashMap;

pub type ModulePath = Vec<String>;

/// Normalize module hierarchy to canonical filesystem layout
pub fn normalize_module_tree(modules: &[ModulePath]) -> Vec<ModulePath> {
    let mut normalized = modules.to_vec();

    normalized.sort();
    normalized.dedup();

    normalized
}

/// Convert module path to filesystem path
pub fn module_to_fs_path(crate_name: &str, module: &ModulePath) -> String {
    if module.is_empty() {
        format!("emit/{}/src/lib.rs", crate_name)
    } else {
        let path = module.join("/");
        format!("emit/{}/src/{}.rs", crate_name, path)
    }
}
