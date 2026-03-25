use crate::CanonEvent;
use anyhow::Result;
use fs2::FileExt;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

const MAGIC: u32 = 0x544C4F47; // "TLOG"

fn read_u32(buf: &[u8]) -> u32 {
    u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]])
}

pub fn is_binary_tlog(path: &Path) -> bool {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return false,
    };
    let mut magic = [0u8; 4];
    if file.read_exact(&mut magic).is_err() {
        return false;
    }
    u32::from_le_bytes(magic) == MAGIC
}

#[derive(Debug, Clone)]
pub struct SegmentConfig {
    pub max_bytes: u64,
    pub index_stride: u32,
    pub time_bucket_ms: u64,
    pub retain_segments: Option<usize>,
}

impl Default for SegmentConfig {
    fn default() -> Self {
        Self { max_bytes: 64 * 1024 * 1024, index_stride: 256, time_bucket_ms: 5_000, retain_segments: None }
    }
}

impl SegmentConfig {
    pub fn with_env(mut self) -> Self {
        if let Ok(raw) = std::env::var("CANON_TLOG_RETAIN_SEGMENTS") {
            if let Ok(value) = raw.parse::<usize>() {
                if value > 0 {
                    self.retain_segments = Some(value);
                }
            }
        }
        self
    }
}

struct SegmentFiles {
    log: BufWriter<File>,
    idx: BufWriter<File>,
    time: BufWriter<File>,
    size: u64,
    last_time_bucket: Option<u64>,
    records: u32,
}

pub struct BinarySegmentWriter {
    dir: PathBuf,
    config: SegmentConfig,
    seq: AtomicU64,
    last_ts: AtomicU64,
    fsync: bool,
    inner: Mutex<SegmentFiles>,
}

impl BinarySegmentWriter {
    pub fn open(dir: &Path) -> Result<Self> {
        Self::open_with_config(dir, SegmentConfig::default().with_env())
    }

    pub fn open_with_config(dir: &Path, config: SegmentConfig) -> Result<Self> {
        fs::create_dir_all(dir)?;
        let mut base_seq = 0u64;
        if let Some(found) = find_latest_segment(dir)? {
            base_seq = found;
        }
        let (mut files, recovered_seq) = recover_segment(dir, base_seq, &config)?;
        let next_seq = recovered_seq.map(|s| s.saturating_add(1)).unwrap_or(base_seq);
        if files.size >= config.max_bytes {
            files = open_new_segment_files(dir, next_seq)?;
        }
        if let Some(keep) = config.retain_segments {
            apply_retention(dir, keep)?;
        }
        Ok(Self { dir: dir.to_path_buf(), config, seq: AtomicU64::new(next_seq), last_ts: AtomicU64::new(0), fsync: false, inner: Mutex::new(files) })
    }

    pub fn with_fsync(mut self, enabled: bool) -> Self {
        self.fsync = enabled;
        self
    }

    pub fn write_canon_event(&self, event: &CanonEvent) -> Result<()> {
        let prev = self.last_ts.fetch_max(event.ts, Ordering::Relaxed);
        if event.ts < prev {
            return Err(anyhow::anyhow!("non-monotonic timestamp: {} < prev {}", event.ts, prev));
        }
        if event.payload.input.is_null() || event.payload.output.is_null() || event.payload.delta.is_null() {
            return Err(anyhow::anyhow!("CanonPayload input/output/delta must not be null"));
        }
        let mut line = serde_json::to_vec(event)?;
        line.push(b'\n');
        let line_len = line.len() as u64;
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);

        let mut guard = self.inner.lock().expect("binary segment writer poisoned");
        guard.log.get_ref().lock_exclusive()?;

        if guard.size + line_len > self.config.max_bytes {
            guard.log.flush()?;
            guard.idx.flush()?;
            guard.time.flush()?;
            let new_files = open_new_segment_files(&self.dir, seq)?;
            *guard = new_files;
            if let Some(keep) = self.config.retain_segments {
                apply_retention(&self.dir, keep)?;
            }
        }

        let record_pos = guard.size;
        guard.log.write_all(&line)?;
        guard.size = guard.size.saturating_add(line_len);
        guard.records = guard.records.saturating_add(1);

        if guard.records % self.config.index_stride == 0 {
            guard.idx.write_all(&seq.to_le_bytes())?;
            guard.idx.write_all(&record_pos.to_le_bytes())?;
        }

        let bucket = event.ts / self.config.time_bucket_ms;
        if guard.last_time_bucket != Some(bucket) {
            guard.time.write_all(&bucket.to_le_bytes())?;
            guard.time.write_all(&record_pos.to_le_bytes())?;
            guard.last_time_bucket = Some(bucket);
        }

        guard.log.flush()?;
        guard.idx.flush()?;
        guard.time.flush()?;
        if self.fsync {
            guard.log.get_ref().sync_data()?;
            guard.idx.get_ref().sync_data()?;
            guard.time.get_ref().sync_data()?;
        }
        guard.log.get_ref().unlock()?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }
}

fn open_new_segment_files(dir: &Path, base_seq: u64) -> Result<SegmentFiles> {
    let name = format!("{:020}", base_seq);
    let log_path = dir.join(format!("{}.log", name));
    let idx_path = dir.join(format!("{}.idx", name));
    let time_path = dir.join(format!("{}.time", name));
    let log = OpenOptions::new().create(true).append(true).read(true).open(&log_path)?;
    let idx = OpenOptions::new().create(true).write(true).truncate(true).open(&idx_path)?;
    let time = OpenOptions::new().create(true).write(true).truncate(true).open(&time_path)?;
    let size = log.metadata().map(|m| m.len()).unwrap_or(0);
    Ok(SegmentFiles { log: BufWriter::new(log), idx: BufWriter::new(idx), time: BufWriter::new(time), size, last_time_bucket: None, records: 0 })
}

fn find_latest_segment(dir: &Path) -> Result<Option<u64>> {
    let mut max_seq: Option<u64> = None;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("log") {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            if let Ok(seq) = stem.parse::<u64>() {
                max_seq = Some(max_seq.map(|v| v.max(seq)).unwrap_or(seq));
            }
        }
    }
    Ok(max_seq)
}

fn apply_retention(dir: &Path, keep_segments: usize) -> Result<()> {
    if keep_segments == 0 {
        return Ok(());
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("log") {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            if let Ok(seq) = stem.parse::<u64>() {
                entries.push((seq, path));
            }
        }
    }
    entries.sort_by_key(|(seq, _)| *seq);
    if entries.len() <= keep_segments {
        return Ok(());
    }
    let remove_count = entries.len() - keep_segments;
    for (seq, log_path) in entries.into_iter().take(remove_count) {
        let stem = format!("{:020}", seq);
        let idx_path = dir.join(format!("{}.idx", stem));
        let time_path = dir.join(format!("{}.time", stem));
        let _ = fs::remove_file(log_path);
        let _ = fs::remove_file(idx_path);
        let _ = fs::remove_file(time_path);
    }
    Ok(())
}

fn recover_segment(dir: &Path, base_seq: u64, config: &SegmentConfig) -> Result<(SegmentFiles, Option<u64>)> {
    let name = format!("{:020}", base_seq);
    let log_path = dir.join(format!("{}.log", name));
    let idx_path = dir.join(format!("{}.idx", name));
    let time_path = dir.join(format!("{}.time", name));

    if !log_path.exists() {
        return Ok((open_new_segment_files(dir, base_seq)?, None));
    }

    let bytes = fs::read(&log_path)?;

    // Detect legacy binary segment (starts with TLOG magic) — discard and start fresh.
    if bytes.len() >= 4 && read_u32(&bytes[0..4]) == MAGIC {
        OpenOptions::new().write(true).truncate(true).open(&log_path)?;
        let _ = OpenOptions::new().create(true).write(true).truncate(true).open(&idx_path);
        let _ = OpenOptions::new().create(true).write(true).truncate(true).open(&time_path);
        return Ok((open_new_segment_files(dir, base_seq)?, None));
    }

    // Parse JSONL lines, rebuild index files.
    let idx = OpenOptions::new().create(true).write(true).truncate(true).open(&idx_path)?;
    let time_f = OpenOptions::new().create(true).write(true).truncate(true).open(&time_path)?;
    let mut idx_writer = BufWriter::new(idx);
    let mut time_writer = BufWriter::new(time_f);

    let mut size = 0u64;
    let mut records: u32 = 0;
    let mut last_seq: Option<u64> = None;
    let mut last_time_bucket: Option<u64> = None;
    let mut seq = base_seq;
    let mut byte_pos: u64 = 0;

    let content = std::str::from_utf8(&bytes).unwrap_or("");
    for raw_line in content.split('\n') {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            byte_pos = byte_pos.saturating_add(raw_line.len() as u64 + 1);
            continue;
        }
        let event: CanonEvent = match serde_json::from_str(trimmed) {
            Ok(e) => e,
            Err(_) => break,
        };
        let line_end = byte_pos.saturating_add(raw_line.len() as u64 + 1);
        size = line_end;
        records = records.saturating_add(1);
        last_seq = Some(seq);

        if records % config.index_stride == 0 {
            idx_writer.write_all(&seq.to_le_bytes())?;
            idx_writer.write_all(&byte_pos.to_le_bytes())?;
        }
        let bucket = event.ts / config.time_bucket_ms;
        if last_time_bucket != Some(bucket) {
            time_writer.write_all(&bucket.to_le_bytes())?;
            time_writer.write_all(&byte_pos.to_le_bytes())?;
            last_time_bucket = Some(bucket);
        }
        seq = seq.saturating_add(1);
        byte_pos = line_end;
    }

    idx_writer.flush()?;
    time_writer.flush()?;

    // Truncate to last valid record if file has trailing garbage.
    if size < bytes.len() as u64 {
        let file = OpenOptions::new().write(true).open(&log_path)?;
        file.set_len(size)?;
    }

    let log = OpenOptions::new().create(true).append(true).read(true).open(&log_path)?;
    let idx = OpenOptions::new().create(true).append(true).read(true).open(&idx_path)?;
    let time_f = OpenOptions::new().create(true).append(true).read(true).open(&time_path)?;

    Ok((SegmentFiles { log: BufWriter::new(log), idx: BufWriter::new(idx), time: BufWriter::new(time_f), size, last_time_bucket, records }, last_seq))
}
