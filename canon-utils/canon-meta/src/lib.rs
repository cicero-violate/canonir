use canon_macros::canon_event_struct;

canon_event_struct!(Meta { file: String, crate_name: String, module: String, line: u32 });

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
        let __meta = canon_event::EventMeta {
            ts: {
                use std::time::{SystemTime, UNIX_EPOCH};
                SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
            },
            source: $source.to_string(),
            file: file!().to_string(),
            line: line!(),
        };
        let __payload = canon_event::CanonPayload::from_kind($kind, serde_json::to_value($payload).unwrap_or_default());
        let __wire = canon_event::CanonEvent { event_id: None, meta: __meta, payload: __payload };
        canon_event::write_canon_event_auto($path, &__wire)
    }};
}
