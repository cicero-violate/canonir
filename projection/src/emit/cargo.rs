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

pub fn emit_cargo_toml(crate_name: &str, edition: &str, has_binary: bool) -> String {
    let mut out = format!("[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"{}\"\n\n[dependencies]\n", crate_name, edition);

    if has_binary {
        out.push_str(&format!("\n[[bin]]\nname = \"{}\"\npath = \"src/main.rs\"\n", crate_name));
    }

    // Prevent emitted crate from inheriting parent workspace.
    // Adding empty [workspace] makes this Cargo.toml a workspace root.
    out.push_str("\n[workspace]\n");

    out
}
