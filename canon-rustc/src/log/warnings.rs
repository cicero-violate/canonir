// legacy log disabled (event.log removed)
use std::path::Path;

use crate::log::tlog_writer::TlogWriter;
use crate::log::panic_capture::panic_log_root;

pub fn append_rustc_log(_output_dir: &Path, msg: &str) {
    // HARD DISABLE: prevent split logging
    // all logs must go through event.tlog via append_rustc_warning
    let _ = msg;
}

pub fn append_rustc_warning(msg: &str) {
    let Some(root) = panic_log_root() else {
        return;
    };
    let logs_dir = root.join("state").join("event_log");
    let _ = std::fs::create_dir_all(&logs_dir);
    let path = logs_dir.join("event.tlog");
    if let Ok(mut writer) = TlogWriter::open(&path) {
        let _ = writer.write_warning(msg);
    }
}
