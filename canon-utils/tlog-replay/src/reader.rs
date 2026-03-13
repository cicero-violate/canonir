use canon_types::TlogEvent;

pub fn parse_tlog_event(line: &str) -> Option<TlogEvent> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    serde_json::from_str(trimmed).ok()
}
