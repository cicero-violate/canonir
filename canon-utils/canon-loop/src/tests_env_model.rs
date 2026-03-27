#[cfg(test)]
mod tests {
    use crate::env_model::{EntrypointKind, WorkspaceModel};
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn inspect_missing_workspace_reports_bootstrap_precondition() {
        let goal = "# Goal\n- Project path: `/tmp/definitely_missing_canon_goal_test`\n";
        let model = WorkspaceModel::inspect(goal, PathBuf::from("/tmp").as_path()).unwrap();
        assert!(!model.path_exists);
        assert_eq!(model.entrypoint_kind, EntrypointKind::None);
        assert!(model
            .planner_lines()
            .iter()
            .any(|line| line.contains("first action must create/init")));
    }

    #[test]
    fn parse_module_gap_from_lib_rs() {
        let temp = std::env::temp_dir().join(format!("canon-env-model-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(temp.join("src")).unwrap();
        fs::write(temp.join("Cargo.toml"), "[package]\nname='x'\nversion='0.1.0'\n").unwrap();
        fs::write(temp.join("src/lib.rs"), "pub mod index;\n").unwrap();
        let goal = format!("# Goal\n- Project path: `{}`\n", temp.display());
        let model = WorkspaceModel::inspect(&goal, PathBuf::from("/tmp").as_path()).unwrap();
        assert_eq!(model.entrypoint_kind, EntrypointKind::Lib);
        assert_eq!(model.crate_name.as_deref(), Some("x"));
        assert!(model.source_files.iter().any(|p| p == &PathBuf::from("src/lib.rs")));
        assert_eq!(model.module_gaps.len(), 1);
        let _ = fs::remove_dir_all(temp);
    }
}
