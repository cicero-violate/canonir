use rename::core::project_editor::ProjectEditor;
#[cfg(feature = "rustc_frontend")]
use rename::core::rustc_resolver::RustcResolver;
use rename::structured::FieldMutation;

#[cfg(not(feature = "rustc_frontend"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = "/workspace/ai_sandbox/canon/canon-utils/rename/rename_self.json";
    let err = "rustc_frontend feature disabled";
    let json = format!(r#"{{"rustc_args":[],"def_paths":[],"error":{}}}"#, json_string(err));
    std::fs::write(out, json)?;
    println!("status: error");
    println!("json: {}", out);
    println!("read: cat {}", out);
    Ok(())
}

#[cfg(feature = "rustc_frontend")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (project, original, renamed, out) = (
        "/workspace/ai_sandbox/canon/canon-utils/rename",
        "crate::core::project_editor::determine_source_root",
        "crate::core::project_editor::determine_source_root_for_project",
        "/workspace/ai_sandbox/canon/canon-utils/rename/rename_self.json",
    );
    let (mut rustc_args, mut def_paths) = (Vec::<String>::new(), Vec::<String>::new());
    if let Ok(r) = RustcResolver::new(project.as_ref()) {
        rustc_args = r.debug_cargo_rustc_args().unwrap_or_default();
        def_paths = r.debug_def_paths().unwrap_or_default();
    }
    let mut err = String::new();
    let mut editor = ProjectEditor::load_with_rustc(project.as_ref()).map_err(|e| {
        err = format!("{e:?}");
        e
    })?;
    let (sid, new) = if editor.has_symbol(original) { (original, renamed) } else { (renamed, original) };
    let new_ident = new.rsplit("::").next().unwrap_or(new);
    if let Err(e) = editor
        .queue_by_id(sid, FieldMutation::RenameIdent(new_ident.to_string()))
        .and_then(|_| editor.validate().map(|_| ()))
        .and_then(|_| editor.apply().map(|_| ()))
        .and_then(|_| editor.preview().map(|_| ()))
        .and_then(|_| editor.commit().map(|_| ()))
    {
        err = format!("{e:?}");
    }
    let json = format!(r#"{{"rustc_args":{},"def_paths":{},"error":{}}}"#, json_array(&rustc_args), json_array(&def_paths), json_string(&err));
    std::fs::write(out, json)?;
    println!("status: {}", if err.is_empty() { "ok" } else { "error" });
    println!("json: {}", out);
    println!("read: cat {}", out);
    Ok(())
}

fn json_array(values: &[String]) -> String {
    let mut out = String::from("[");
    for (i, v) in values.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&json_string(v));
    }
    out.push(']');
    out
}
fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                use std::fmt::Write;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
