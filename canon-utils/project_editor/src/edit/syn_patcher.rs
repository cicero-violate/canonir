use canon_types::SpanRange;
use anyhow::{anyhow, Result};

#[derive(Clone, Debug)]
pub struct SpanReplacement {
    pub span: SpanRange,
    pub replacement: String,
}

pub fn patch_file(src: &str, spans: &[SpanReplacement]) -> Result<String> {
    if spans.is_empty() {
        return Ok(src.to_string());
    }
    let mut sorted = spans.to_vec();
    sorted.sort_by(|a, b| b.span.lo.cmp(&a.span.lo));
    let src_bytes = src.as_bytes();
    let mut chunks: Vec<Vec<u8>> = Vec::with_capacity(sorted.len() * 2 + 1);
    let mut cursor = src_bytes.len();
    for span in &sorted {
        let lo = span.span.lo as usize;
        let hi = span.span.hi as usize;
        if lo > hi || hi > src_bytes.len() {
            return Err(anyhow!("invalid span {}..{}", lo, hi));
        }
        if hi > cursor {
            return Err(anyhow!(
                "overlapping spans: span {}..{} conflicts with already-applied region ending at {}",
                lo,
                hi,
                cursor
            ));
        }
        chunks.push(src_bytes[hi..cursor].to_vec());
        chunks.push(span.replacement.as_bytes().to_vec());
        cursor = lo;
    }
    chunks.push(src_bytes[0..cursor].to_vec());
    chunks.reverse();
    let total_len: usize = chunks.iter().map(|c| c.len()).sum();
    let mut out = Vec::with_capacity(total_len);
    for chunk in chunks {
        out.extend_from_slice(&chunk);
    }
    String::from_utf8(out).map_err(|e| anyhow!("utf8 error after patch: {e}"))
}
