use crate::core::rustc_session::SpanRange;
use anyhow::{anyhow, Result};

pub fn patch_file(src: &str, spans: &[SpanRange], new_ident: &str) -> Result<String> {
    if spans.is_empty() {
        return Ok(src.to_string());
    }
    let mut sorted = spans.to_vec();
    sorted.sort_by(|a, b| b.lo.cmp(&a.lo));
    let mut updated = src.to_string();
    for span in sorted {
        if span.hi > updated.len() || span.lo > span.hi {
            return Err(anyhow!("invalid span {}..{}", span.lo, span.hi));
        }
        updated.replace_range(span.lo..span.hi, new_ident);
    }
    Ok(updated)
}
