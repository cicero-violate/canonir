use crate::extractor::extract_symbols;

use std::path::Path;

use std::path::PathBuf;

use walkdir::WalkDir;

pub struct FileMap {
    pub path: std::PathBuf,
    pub symbols: Vec<Symbol>,
}

pub fn build_repomap(root_dir: &std::Path) -> Vec<FileMap> {
    let mut result = Vec::new();
    
    for entry in WalkDir::new(root_dir).into_iter().filter_map(|e| e.ok()).filter(|e| e.path().extension().map(|x| x == "rs").unwrap_or(false)) {
        let path = entry.path().to_path_buf();
        let src = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => continue,
        };
    
        let symbols = extract_symbols(&src);
        if !symbols.is_empty() {
            result.push(FileMap { path, symbols });
        }
    }
    
    result
}

pub fn estimate_tokens(s: &str) -> usize {
    s.len() / 4
}

pub fn render_repomap(maps: &[FileMap], root_dir: &std::Path) -> String {
    let mut out = String::new();
    
    for fm in maps {
        // Show path relative to root so the LLM sees `src/graph.rs` not abs path
        let rel = fm.path.strip_prefix(root_dir).unwrap_or(&fm.path);
        out.push_str(&format!("{}:\n", rel.display()));
    
        for sym in &fm.symbols {
            out.push_str(&sym.render());
            out.push('\n');
        }
        out.push('\n');
    }
    
    out
}