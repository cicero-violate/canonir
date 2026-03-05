pub fn valid_module_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn valid_symbol_name(name: &str) -> bool {
    !name.is_empty() && name.chars().next().unwrap().is_ascii_alphabetic()
}
