pub fn emit_cargo_toml(name: &str, edition: &str, has_binary: bool, dependencies: &[String]) -> String {
    let mut out = format!("[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"{}\"\n\n", name, edition);
    if has_binary {
        out.push_str("[[bin]]\nname = \"app\"\npath = \"src/main.rs\"\n\n");
    }
    out.push_str("[dependencies]\n");
    for dep in dependencies {
        out.push_str(dep);
        out.push('\n');
    }
    // Prevent this emitted crate from inheriting the parent monorepo workspace.
    out.push_str("\n[workspace]\n");
    out
}
