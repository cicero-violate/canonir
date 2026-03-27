use std::env;

pub fn harness_repair_mode_enabled() -> bool {
    env::var("CANON_HARNESS_REPAIR_CRATE")
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
        || env::var("CANON_HARNESS_REPAIR_TEST")
            .ok()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
}
