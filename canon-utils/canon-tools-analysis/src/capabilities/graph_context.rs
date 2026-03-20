use canon_types::ReportLayout;
use std::path::{Path, PathBuf};

pub struct GraphContext {
    pub crate_name: String,
    pub graph_dir: PathBuf,
    pub reports_root: PathBuf,
}

pub fn resolve_graph_context(workspace: &Path, crate_name: &str) -> GraphContext {
    let reports_root = workspace.join("state").join("reports_out").join("crates").join(crate_name);
    let layout = ReportLayout::from_crate_root(reports_root.clone());
    GraphContext { crate_name: crate_name.to_string(), graph_dir: layout.graph_dir(), reports_root }
}
