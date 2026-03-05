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
    expr == "std::option::Option::None" || expr == "core::option::Option::None" || expr == "Option::None"
}

pub(crate) fn is_internal_mir_const_repr(s: &str) -> bool {
    s.contains("{alloc") || s.starts_with("alloc") || s.contains("promoted[")
}

pub(crate) fn is_filtered_internal_call_path(path: &str) -> bool {
    let normalized = strip_instance_generics(path);
    let fmt_arguments_ctor = normalized.contains("fmt::Arguments::new");
    let fmt_rt_argument_ctor = normalized.contains("fmt::rt::Argument::new");
    matches!(
        normalized.as_str(),
        "std::hint::must_use"
            | "core::hint::must_use"
            | "std::io::_print"
            | "std::io::_eprint"
            | "core::fmt::Arguments::new"
            | "std::fmt::Arguments::new"
            | "core::fmt::Arguments::new_v1"
            | "std::fmt::Arguments::new_v1"
            | "core::fmt::Arguments::new_v1_formatted"
            | "std::fmt::Arguments::new_v1_formatted"
    ) || fmt_arguments_ctor
        || fmt_rt_argument_ctor
        || normalized.ends_with("::new_display")
        || normalized.ends_with("::branch")
        || normalized.ends_with("::from_residual")
        || normalized.ends_with("::from_output")
        || normalized.ends_with("::from_str")
        || normalized.contains("SizedTypeProperties")
        || normalized.contains("::__iterator_get_unchecked")
        || normalized.ends_with("::is_val_statically_known")
}

pub(crate) fn path_has_unresolved_generic(path: &str) -> bool {
    let bytes = path.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] != b'<' || i + 1 >= bytes.len() {
            continue;
        }
        let c = bytes[i + 1];
        if !c.is_ascii_uppercase() {
            continue;
        }
        if i + 2 >= bytes.len() {
            return true;
        }
        let next = bytes[i + 2];
        if next == b'>' || next == b',' || next.is_ascii_whitespace() {
            return true;
        }
    }
    false
}
