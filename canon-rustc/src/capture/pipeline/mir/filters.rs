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

pub(crate) fn is_internal_mir_const_repr(s: &str) -> bool {
    s.contains("{alloc") || s.starts_with("alloc") || s.contains("promoted[")
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
