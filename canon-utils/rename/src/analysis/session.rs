use anyhow::{anyhow, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub struct AnalysisSession {
    pub module_files: HashMap<String, PathBuf>,
    pub file_modules: HashMap<PathBuf, Vec<String>>,
    pub files: HashSet<PathBuf>,
    pub uses_crate_prefix: bool,
}

impl AnalysisSession {
    pub fn load(project_root: &Path) -> Result<Self> {
        let analysis_dir = project_root.join("analysis");
        let nodes_path = analysis_dir.join("nodes.csv");
        let files_path = analysis_dir.join("files.txt");
        if !nodes_path.exists() || !files_path.exists() {
            return Err(anyhow!(
                "missing analysis outputs; expected nodes.csv and files.txt in {}",
                analysis_dir.display()
            ));
        }
        let (module_files, file_modules, files) = load_module_files_from_analysis(&nodes_path, &files_path)?;
        if module_files.is_empty() {
            return Err(anyhow!(
                "analysis has no module mapping in {}",
                analysis_dir.display()
            ));
        }
        let uses_crate_prefix = module_files.keys().any(|k| k.starts_with("crate::"));
        Ok(Self {
            module_files,
            file_modules,
            files,
            uses_crate_prefix,
        })
    }
}

fn load_module_files_from_analysis(
    nodes_path: &Path,
    files_path: &Path,
) -> Result<(HashMap<String, PathBuf>, HashMap<PathBuf, Vec<String>>, HashSet<PathBuf>)> {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut file_set: HashSet<PathBuf> = HashSet::new();
    let files_content = std::fs::read_to_string(files_path)?;
    for (idx, line) in files_content.lines().enumerate() {
        if idx == 0 {
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.splitn(2, ',');
        let _id = parts.next();
        let Some(path) = parts.next() else {
            continue;
        };
        let pb = PathBuf::from(path);
        file_set.insert(pb.clone());
        files.push(pb);
    }
    let nodes_content = std::fs::read_to_string(nodes_path)?;
    let mut module_files: HashMap<String, PathBuf> = HashMap::new();
    let mut file_modules: HashMap<PathBuf, Vec<String>> = HashMap::new();
    for (idx, line) in nodes_content.lines().enumerate() {
        if idx == 0 {
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.splitn(7, ',');
        let _node_id = parts.next();
        let kind = parts.next().unwrap_or_default();
        let symbol = parts.next().unwrap_or_default();
        let file_id_raw = parts.next().unwrap_or_default();
        if kind != "MODULE" {
            continue;
        }
        let file_id: usize = match file_id_raw.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let Some(file_path) = files.get(file_id) else {
            continue;
        };
        if symbol.is_empty() {
            module_files.entry(String::new()).or_insert_with(|| file_path.clone());
            file_modules.entry(file_path.clone()).or_default().push(String::new());
            continue;
        }
        module_files.insert(symbol.to_string(), file_path.clone());
        file_modules.entry(file_path.clone()).or_default().push(symbol.to_string());
    }
    Ok((module_files, file_modules, file_set))
}
