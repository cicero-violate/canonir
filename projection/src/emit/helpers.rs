// CONTRACT:
// - No sorting
// - No graph traversal
// - No mutation
// - Pure string rendering of Plan

/// Shared trait for emitters; all emitters render into a string with indentation
/// supplied by `pad`.
pub(super) trait Emit {
    fn emit(&self, pad: &str) -> String;
}

/// Emit `#[attr]` lines for an attrs list.
pub(super) fn fmt_attrs(attrs: &[String], pad: &str) -> String {
    attrs.iter().map(|a| format!("{}#[{}]\n", pad, a)).collect()
}

/// Emit a `where` clause block if non-empty.
pub(super) fn fmt_where(wc: &[String]) -> String {
    if wc.is_empty() {
        String::new()
    } else {
        format!("\nwhere\n    {}", wc.join(",\n    "))
    }
}
