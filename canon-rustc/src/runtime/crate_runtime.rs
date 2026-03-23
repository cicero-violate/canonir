pub fn should_capture_crate(crate_name: Option<&str>, crate_types: &[String]) -> bool {
    if !should_analyze_crate(crate_name, crate_types) {
        return false;
    }
    crate_types
        .iter()
        .any(|t| t == "bin" || t == "lib" || t == "rlib")
}

pub fn should_analyze_crate(crate_name: Option<&str>, crate_types: &[String]) -> bool {
    if crate_name.is_none() {
        return false;
    }
    crate_types
        .iter()
        .any(|t| t == "bin" || t == "lib" || t == "rlib")
}

// Analysis engine has been removed from kernel responsibilities.
