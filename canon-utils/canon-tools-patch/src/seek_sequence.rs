/// Adapted from codex-rs seek_sequence: attempts to find a slice within a list of lines
/// using four passes of increasing leniency.
pub fn seek_sequence(haystack: &[String], needle: &[String], start_idx: usize, must_end_at_eof: bool) -> Option<usize> {
    if needle.is_empty() {
        return Some(haystack.len());
    }
    let passes: &[fn(&str, &str) -> bool] = &[
        |h, n| h == n,
        |h, n| h.trim_end() == n.trim_end(),
        |h, n| h.trim() == n.trim(),
        |h, n| normalize(h) == normalize(n),
    ];

    for &equal in passes {
        if let Some(idx) = seek_with(haystack, needle, start_idx, must_end_at_eof, equal) {
            return Some(idx);
        }
    }
    None
}

fn seek_with(
    haystack: &[String],
    needle: &[String],
    start_idx: usize,
    must_end_at_eof: bool,
    equal: fn(&str, &str) -> bool,
) -> Option<usize> {
    if needle.is_empty() {
        return Some(start_idx);
    }
    'outer: for idx in start_idx..=haystack.len().saturating_sub(needle.len()) {
        for (offset, pat) in needle.iter().enumerate() {
            if !equal(&haystack[idx + offset], pat) {
                continue 'outer;
            }
        }
        if must_end_at_eof && idx + needle.len() != haystack.len() {
            continue;
        }
        return Some(idx);
    }
    None
}

fn normalize(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '“' | '”' | '„' | '‟' => '"',
            '‘' | '’' | '‚' | '‛' => '\'',
            '–' | '—' | '―' | '‐' => '-',
            _ => c,
        })
        .collect()
}
