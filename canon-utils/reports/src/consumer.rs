use canon_types::{EventDelta, EventMask, KernelEventConsumer, KernelState};
use std::path::PathBuf;

use crate::generate_reports_from_tlog;

#[derive(Debug, Default)]
pub struct ReportConsumer {
    pub last_tick: u64,
    pub event_count: usize,
    last_generated_tick: Option<u64>,
    tlog_path: Option<PathBuf>,
    out_dir: Option<PathBuf>,
}

impl ReportConsumer {
    pub fn new() -> Self {
        let tlog_path = std::env::var("CANON_REPORTS_TLOG").ok().map(PathBuf::from);
        let out_dir = std::env::var("CANON_REPORTS_OUT").ok().map(PathBuf::from);
        Self {
            last_tick: 0,
            event_count: 0,
            last_generated_tick: None,
            tlog_path,
            out_dir,
        }
    }
}

impl KernelEventConsumer for ReportConsumer {
    fn mask(&self) -> EventMask {
        EventMask::ALL
    }

    fn on_event(&mut self, delta: &EventDelta, _state: &KernelState) {
        self.last_tick = delta.tick;
        self.event_count = self.event_count.saturating_add(1);
        let Some(tlog_path) = self.tlog_path.as_ref() else {
            return;
        };
        let Some(out_dir) = self.out_dir.as_ref() else {
            return;
        };
        if self.last_generated_tick == Some(delta.tick) {
            return;
        }
        if let Err(err) = generate_reports_from_tlog(tlog_path, out_dir) {
            eprintln!("canon_reports: failed to generate reports: {err}");
            return;
        }
        self.last_generated_tick = Some(delta.tick);
    }
}
