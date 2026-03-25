#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Meta {
    pub file: String,
    pub crate_name: String,
    pub module: String,
    pub line: u32,
}

/// Capture source-location metadata at the call site.
#[macro_export]
macro_rules! capture_meta {
    () => {
        canon_meta::Meta { file: file!().to_string(), crate_name: env!("CARGO_PKG_NAME").to_string(), module: module_path!().to_string(), line: line!() }
    };
}

/// Emit an event with automatic metadata wrapping.
#[macro_export]
macro_rules! canon_emit_meta {
    // Typed variant form — delegate to canon_emit! (no payload wrapping)
    ($emitter:expr; $variant:ident($inner:expr)) => {{
        canon_event::canon_emit!($emitter; $variant($inner))
    }};
    ($emitter:expr; $source:expr, $kind:expr, $payload:expr) => {{
        let __meta = canon_meta::capture_meta!();
        let __wrapped = serde_json::json!({
            "meta": __meta,
            "data": $payload,
        });
        canon_event::canon_emit!($emitter; $source, $kind, __wrapped)
    }};
    ($source:expr, $kind:expr, $payload:expr, $path:expr) => {{
        let __wrapped = serde_json::to_value($payload).unwrap_or_else(|_| serde_json::Value::Null);
        canon_event::canon_emit!(root; $source, $kind, __wrapped, $path)
    }};
}
