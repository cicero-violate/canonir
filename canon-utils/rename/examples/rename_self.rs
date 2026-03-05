use rename::core::project_editor::ProjectEditor;
use rename::structured::FieldMutation;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let project_path = Path::new("/workspace/ai_sandbox/canon/canon-utils/rename");
    let mut editor = ProjectEditor::load_with_rustc(project_path)?;

    let original = "crate::core::symbol_id::normalize_symbol_id";
    let renamed = "crate::core::symbol_id::normalize_symbol_id_local";
    let (symbol_id, new_name) = if editor.has_symbol(original) {
        (original, "normalize_symbol_id_local")
    } else {
        (renamed, "normalize_symbol_id")
    };
    editor.queue_by_id(symbol_id, FieldMutation::RenameIdent(new_name.to_string()))?;

    let conflicts = editor.validate()?;
    println!("conflicts: {conflicts:?}");

    let report = editor.apply()?;
    println!("touched: {:?}", report.touched_files);

    let preview = editor.preview()?;
    println!("preview:\n{preview}");

    if std::env::args().any(|a| a == "--commit") {
        let written = editor.commit()?;
        println!("written: {:?}", written);
    } else {
        println!("(dry-run — pass --commit to apply)");
    }

    Ok(())
}
