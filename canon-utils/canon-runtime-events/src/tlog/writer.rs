use anyhow::Result;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::event::TlogEvent;
use super::rotate::{maybe_rotate, RotateConfig};
use fs2::FileExt;

pub struct TlogWriter {
    path: PathBuf,
    file: Mutex<BufWriter<File>>,
    fsync: bool,
    retries: usize,
    retry_delay_ms: u64,
    rotate: RotateConfig,
}

impl TlogWriter {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).read(true).open(path)?;
        Ok(Self { path: path.to_path_buf(), file: Mutex::new(BufWriter::new(file)), fsync: false, retries: 2, retry_delay_ms: 10, rotate: RotateConfig::default() })
    }

    pub fn with_fsync(mut self, enabled: bool) -> Self {
        self.fsync = enabled;
        self
    }

    pub fn with_retries(mut self, retries: usize, retry_delay_ms: u64) -> Self {
        self.retries = retries;
        self.retry_delay_ms = retry_delay_ms;
        self
    }

    pub fn with_rotation(mut self, rotate: RotateConfig) -> Self {
        self.rotate = rotate;
        self
    }

    pub fn write_event(&self, event: &TlogEvent) -> Result<()> {
        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 0..=self.retries {
            match self.write_once(event) {
                Ok(()) => return Ok(()),
                Err(err) => {
                    last_err = Some(err);
                    if attempt < self.retries {
                        std::thread::sleep(std::time::Duration::from_millis(self.retry_delay_ms));
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("write_event failed")))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn write_once(&self, event: &TlogEvent) -> Result<()> {
        maybe_rotate(&self.path, &self.rotate).ok();
        let mut guard = self.file.lock().expect("tlog writer poisoned");
        guard.get_ref().lock_exclusive()?;
        let line = serde_json::to_string(event)?;
        guard.write_all(line.as_bytes())?;
        guard.write_all(b"\n")?;
        guard.flush()?;
        if self.fsync {
            guard.get_ref().sync_data()?;
        }
        guard.get_ref().unlock()?;
        Ok(())
    }
}

pub(crate) fn emit_event_json(path: &Path, source: impl Into<String>, kind: impl Into<String>, payload: serde_json::Value) -> Result<()> {
    let event = TlogEvent::new(source, kind, payload);
    let writer = TlogWriter::open(path)?;
    writer.write_event(&event)
}
