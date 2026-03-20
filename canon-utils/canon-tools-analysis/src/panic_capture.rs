use serde::{Deserialize, Serialize};
use std::backtrace::Backtrace;
use std::fs::OpenOptions;
use std::io::Write;

#[derive(Serialize, Deserialize)]
pub struct PanicRecord {
    pub message: String,
    pub backtrace: String,
}

pub fn install_panic_hook(log_path: &str) {
    let path = log_path.to_string();
    std::panic::set_hook(Box::new(move |info| {
        let bt = Backtrace::force_capture();

        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            *s
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.as_str()
        } else {
            "panic"
        };

        let record = PanicRecord { message: msg.to_string(), backtrace: format!("{:?}", bt) };

        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            if let Ok(json) = serde_json::to_string(&record) {
                let _ = writeln!(file, "{json}");
            }
        }
    }));
}
