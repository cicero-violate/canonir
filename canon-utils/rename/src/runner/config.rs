use std::path::PathBuf;

#[derive(Debug, Clone, Copy)]
pub enum RenameSelfMode {
    Incremental,
    Bulk,
}

impl RenameSelfMode {
    pub(crate) fn from_env() -> Self {
        match std::env::var("RENAME_MODE")
            .unwrap_or_else(|_| "incremental".to_string())
            .to_lowercase()
            .as_str()
        {
            "bulk" => RenameSelfMode::Bulk,
            _ => RenameSelfMode::Incremental,
        }
    }
}

pub struct RenameSelfConfig {
    pub project: PathBuf,
    pub symbols_json: PathBuf,
    pub report_dir: PathBuf,
    pub offset: usize,
    pub limit: usize,
    pub mode: RenameSelfMode,
}

pub(crate) fn project_from_args() -> PathBuf {
    std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/workspace/ai_sandbox/canon/canon-agent-v2"))
}

impl RenameSelfConfig {
    pub(crate) fn from_env() -> Self {
        let project = project_from_args();
        let symbols_json = project.join("analysis").join("symbols.json");
        let report_dir = PathBuf::from("/workspace/ai_sandbox/canon/canon-utils/rename");
        let offset = std::env::var("RENAME_OFFSET")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);
        let limit = std::env::var("RENAME_LIMIT")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(usize::MAX);
        Self {
            project,
            symbols_json,
            report_dir,
            offset,
            limit,
            mode: RenameSelfMode::from_env(),
        }
    }
}

pub struct RenameSelfResult {
    pub report_path: PathBuf,
    pub status: String,
}

pub struct SuggestConfig {
    pub symbols_json: PathBuf,
    pub project: PathBuf,
    pub model: String,
    pub batch_size: usize,
    pub dry_run: bool,
}

impl SuggestConfig {
    pub(crate) fn from_env() -> Self {
        let project = project_from_args();
        let symbols_json = project.join("analysis").join("symbols.json");
        let model = std::env::var("RENAME_MODEL").unwrap_or_else(|_| "claude-opus-4-5".to_string());
        let batch_size = std::env::var("RENAME_BATCH_SIZE")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(20);
        let dry_run = std::env::var("RENAME_DRY_RUN").ok().as_deref() == Some("1");
        Self {
            symbols_json,
            project,
            model,
            batch_size,
            dry_run,
        }
    }
}
