//! Dual-transport frame parser — handles both private SSE and calpico frames.
//!
//! Private SSE (/backend-api/f/conversation):
//!   data: {"id":"...","object":"chat.completion.chunk","choices":[{"delta":{"content":"token"}}]}
//!   data: [DONE]
//!
//! Calpico (/backend-api/calpico):
//!   1. {"o": "append", "p": "/message/content/parts/0", "v": "text"}
//!   2. {"v": "token text"}
//!   3. {"o": "patch", "p": "", "v": [{"o":"append","p":"/message/content/parts/0","v":"text"}]}
//!   4. {"type": "message_stream_complete"}  ← done sentinel

use serde_json::Value;

/// Extract content text from one raw calpico WS frame.
///
/// Returns `Some(text)` for frames that carry assistant content tokens.
/// Returns `None` for control frames, structural bootstraps, and the done sentinel.
pub fn extract_sse_delta(raw: &str) -> Option<String> {
    let data = raw.strip_prefix("data: ").unwrap_or(raw).trim();

    if data.is_empty() || data == "[DONE]" {
        return None;
    }

    let v: Value = serde_json::from_str(data).ok()?;
    let obj = v.as_object()?;

    // ── Private SSE shape ────────────────────────────────────────────────────
    // {"id":"...","object":"chat.completion.chunk","choices":[{"delta":{"content":"token"}}]}
    if obj.get("object").and_then(|v| v.as_str()) == Some("chat.completion.chunk") {
        let content = obj
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|c| c.get("delta"))
            .and_then(|d| d.get("content"))
            .and_then(|v| v.as_str());
        return content.map(|s| s.to_string());
    }

    // Frames with an explicit "o" (operation) field — shapes 1 and 3.
    if let Some(op) = obj.get("o").and_then(|o| o.as_str()) {
        match op {
            "append" => {
                // Shape 1: only emit if path targets the content parts array.
                let p = obj.get("p").and_then(|p| p.as_str()).unwrap_or("");
                if p.contains("parts") {
                    return obj.get("v").and_then(|v| v.as_str()).map(|s| s.to_string());
                }
                return None;
            }
            "patch" => {
                // Shape 3: terminal patch — collect all inner appends to parts.
                let arr = obj.get("v").and_then(|v| v.as_array())?;
                let mut out = String::new();
                for item in arr {
                    if item.get("o").and_then(|o| o.as_str()) != Some("append") {
                        continue;
                    }
                    let p = item.get("p").and_then(|p| p.as_str()).unwrap_or("");
                    if p.contains("parts") {
                        if let Some(s) = item.get("v").and_then(|v| v.as_str()) {
                            out.push_str(s);
                        }
                    }
                }
                return if out.is_empty() { None } else { Some(out) };
            }
            _ => return None,
        }
    }

    // Shape 2: bare token frame {"v": "text"}.
    // Guard against structural bootstrap frames that carry "c", "p", or "type".
    if obj.contains_key("type") || obj.contains_key("c") || obj.contains_key("p") {
        return None;
    }
    if let Some(text) = obj.get("v").and_then(|v| v.as_str()) {
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }

    // Shape 4: array-valued bare frame {"v": [{...}, ...]} — no outer "o".
    // Collect all inner appends targeting content parts.
    if let Some(arr) = obj.get("v").and_then(|v| v.as_array()) {
        let mut out = String::new();
        for item in arr {
            if item.get("o").and_then(|o| o.as_str()) != Some("append") {
                continue;
            }
            let p = item.get("p").and_then(|p| p.as_str()).unwrap_or("");
            if p.contains("parts") {
                if let Some(s) = item.get("v").and_then(|v| v.as_str()) {
                    out.push_str(s);
                }
            }
        }
        if !out.is_empty() {
            return Some(out);
        }
    }

    None
}

/// Returns true if this frame signals end of the assistant turn.
pub fn is_done(raw: &str) -> bool {
    let data = raw.strip_prefix("data: ").unwrap_or(raw).trim();
    if data == "[DONE]" {
        return true;
    }
    if let Ok(v) = serde_json::from_str::<Value>(data) {
        if v.get("type").and_then(|t| t.as_str()) == Some("message_stream_complete") {
            return true;
        }
    }
    false
}
