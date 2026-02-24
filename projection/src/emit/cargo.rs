//! Cargo.toml emitter.
//!
//! Variables:
//!   crate_name    : String  — from NodeKind::Crate.name
//!   edition       : String  — from NodeKind::Crate.edition
//!   has_binary    : bool    — true if any Module.file == "src/main.rs"
//!
//! Equation:
//!   Cargo.toml = package_section + lib_section? + bin_section?
//!   package_section = "[package]\nname={name}\nversion=\"0.1.0\"\nedition={edition}\n"
//!   lib_section     = "[[lib]]\npath = \"src/lib.rs\"\n"  (always — lib root present)
//!   bin_section     = "[[bin]]\nname={name}\npath=\"src/main.rs\"\n"  if has_binary

use model::ir::{model_ir::ModelIR, node::NodeKind};

pub fn emit_cargo_toml(ir: &ModelIR) -> Option<String> {
    // Find the Crate node.
    let (crate_name, edition) = ir.nodes.iter().find_map(|n| {
        if let NodeKind::Crate { name, edition } = &n.kind {
            Some((name.clone(), edition.clone()))
        } else {
            None
        }
    })?;

    // Detect whether a binary entry point exists.
    let has_binary = ir.nodes.iter().any(|n| {
        matches!(&n.kind, NodeKind::Module { file, .. } if file.ends_with("main.rs"))
    });

    let mut out = format!(
        "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"{}\"\n\n[dependencies]\n",
        crate_name, edition
    );

    if has_binary {
        out.push_str(&format!(
            "\n[[bin]]\nname = \"{}\"\npath = \"src/main.rs\"\n",
            crate_name
        ));
    }

    Some(out)
}
