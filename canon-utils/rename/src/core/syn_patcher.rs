use crate::core::rustc_session::SpanRange;
use anyhow::{anyhow, Result};

pub fn patch_file(src: &str, spans: &[SpanRange], new_ident: &str) -> Result<String> {
    if spans.is_empty() {
        return Ok(src.to_string());
    }
    let mut sorted = spans.to_vec();
    sorted.sort_by(|a, b| b.lo.cmp(&a.lo));
    let src_bytes = src.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(src.len());
    let mut cursor = 0usize;
    let mut sorted_asc = sorted;
    sorted_asc.sort_by_key(|s| s.lo);
    for span in &sorted_asc {
        if span.lo > span.hi || span.hi > src_bytes.len() {
            return Err(anyhow!("invalid span {}..{}", span.lo, span.hi));
        }
        if span.lo < cursor {
            return Err(anyhow!("overlapping spans at {}", span.lo));
        }
        out.extend_from_slice(&src_bytes[cursor..span.lo]);
        out.extend_from_slice(new_ident.as_bytes());
        cursor = span.hi;
    }
    out.extend_from_slice(&src_bytes[cursor..]);
    String::from_utf8(out).map_err(|e| anyhow!("utf8 error after patch: {e}"))
}
