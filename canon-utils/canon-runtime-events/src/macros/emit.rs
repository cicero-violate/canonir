/// Emit a canonical tlog event to the given path.
///
/// Automatically selects the correct writer:
/// - directory or `.tlog.d` path → `BinarySegmentWriter` (JSONL segments)
/// - file path → `emit_event_json` (single JSONL file)
///
/// Returns `anyhow::Result<()>`.
///
/// # Example
/// ```rust,ignore
/// canon_emit!("event-runtime", "capability_requested", payload, &tlog_path)?;
/// ```
#[macro_export]
macro_rules! canon_emit {
    ($source:expr, $kind:expr, $payload:expr, $path:expr) => {{
        let __event = $crate::TlogEvent::new($source, $kind, $payload);
        $crate::write_event_auto($path, &__event)
    }};
}
