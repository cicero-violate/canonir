pub(crate) fn strip_instance_generics(raw: &str) -> String {
    if !raw.contains("::<") {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len());
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        if i + 2 < chars.len() && chars[i] == ':' && chars[i + 1] == ':' && chars[i + 2] == '<' {
            i += 3;
            let mut depth = 1usize;
            while i < chars.len() && depth > 0 {
                match chars[i] {
                    '<' => depth += 1,
                    '>' => depth -= 1,
                    _ => {}
                }
                i += 1;
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

pub(crate) fn is_zero_arg_enum_ctor_expr_str(expr: &str) -> bool {
    let expr = strip_instance_generics(expr);
    expr == "std::option::Option::None"
        || expr == "core::option::Option::None"
        || expr == "Option::None"
}

pub(crate) fn is_internal_mir_const_repr(s: &str) -> bool {
    s.contains("{alloc") || s.starts_with("alloc") || s.contains("promoted[")
}

pub(crate) fn is_filtered_internal_call_path(path: &str) -> bool {
    matches!(
        path,
        "std::hint::must_use"
            | "core::hint::must_use"
            | "std::io::_print"
            | "std::io::_eprint"
            | "core::fmt::Arguments::new_v1"
            | "std::fmt::Arguments::new_v1"
            | "core::fmt::Arguments::new_v1_formatted"
            | "std::fmt::Arguments::new_v1_formatted"
    ) || path.ends_with("::new_display")
        || path.ends_with("::branch")
        || path.ends_with("::from_residual")
        || path.ends_with("::from_output")
        || path.ends_with("::from_str")
        || path.contains("SizedTypeProperties")
        || path.contains("::__iterator_get_unchecked")
        || path.ends_with("::is_val_statically_known")
}

pub(crate) fn path_has_unresolved_generic(path: &str) -> bool {
    let bytes = path.as_bytes();
    let mut i = 0usize;
    while i + 2 < bytes.len() {
        if bytes[i] == b'<' && bytes[i + 2] == b'>' && bytes[i + 1].is_ascii_uppercase() {
            return true;
        }
        i += 1;
    }
    false
}
