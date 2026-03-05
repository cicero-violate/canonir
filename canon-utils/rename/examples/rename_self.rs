fn main() -> Result<(), Box<dyn std::error::Error>> {
    let project = "/workspace/ai_sandbox/canon/canon-utils/rename";
    let renames = vec![
        (
            "crate::core::project_editor::determine_source_root".to_string(),
            "crate::core::project_editor::resolve_source_root".to_string(),
        ),
        (
            "crate::core::project_editor::module_path_for_file".to_string(),
            "crate::core::project_editor::module_path_from_file".to_string(),
        ),
    ];

    let report = rename::rename_symbol_pairs(std::path::Path::new(project), &renames);
    println!("status: {}", report.status());
    if let Some(error) = report.error {
        eprintln!("error: {}", error);
        std::process::exit(1);
    }
    Ok(())
}
