use super::event::CanonEvent;
use anyhow::{anyhow, Result};
use crc32fast::Hasher;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

const MAGIC: u32 = 0x544C4F47; // "TLOG"
const VERSION: u16 = 1;
const HEADER_LEN: u16 = 40;
const HEADER_LEN_USIZE: usize = 40;
const SCHEMA_FILE: &str = "schema.bin";

fn fnv1a_32(value: &str) -> u32 {
    let mut hash: u32 = 0x811c9dc5;
    for b in value.as_bytes() {
        hash ^= *b as u32;
        hash = hash.wrapping_mul(0x01000193);
    }
    hash
}

fn read_u16(buf: &[u8]) -> u16 {
    u16::from_le_bytes([buf[0], buf[1]])
}

fn read_u32(buf: &[u8]) -> u32 {
    u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]])
}

fn read_u64(buf: &[u8]) -> u64 {
    u64::from_le_bytes([
        buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
    ])
}

pub struct BinaryTlogWriter {
    path: PathBuf,
    file: Mutex<BufWriter<File>>,
    seq: AtomicU64,
    fsync: bool,
}

impl BinaryTlogWriter {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            file: Mutex::new(BufWriter::new(file)),
            seq: AtomicU64::new(0),
            fsync: false,
        })
    }

    pub fn with_fsync(mut self, enabled: bool) -> Self {
        self.fsync = enabled;
        self
    }

    pub fn write_event(&self, event: &CanonEvent) -> Result<()> {
        let payload = serde_json::to_vec(event)?;
        let payload_len = payload.len() as u32;
        let mut hasher = Hasher::new();
        hasher.update(&payload);
        let crc32 = hasher.finalize();
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        let source_id = fnv1a_32(&event.source);
        let kind_id = fnv1a_32(&event.kind);

        let mut header = Vec::with_capacity(HEADER_LEN as usize);
        header.extend_from_slice(&MAGIC.to_le_bytes());
        header.extend_from_slice(&VERSION.to_le_bytes());
        header.extend_from_slice(&HEADER_LEN.to_le_bytes());
        header.extend_from_slice(&event.ts.to_le_bytes());
        header.extend_from_slice(&source_id.to_le_bytes());
        header.extend_from_slice(&kind_id.to_le_bytes());
        header.extend_from_slice(&seq.to_le_bytes());
        header.extend_from_slice(&payload_len.to_le_bytes());
        header.extend_from_slice(&crc32.to_le_bytes());

        if header.len() != HEADER_LEN as usize {
            return Err(anyhow!("binary header length mismatch"));
        }

        let mut guard = self.file.lock().expect("binary tlog writer poisoned");
        guard.get_ref().lock_exclusive()?;
        guard.write_all(&header)?;
        guard.write_all(&payload)?;
        guard.flush()?;
        if self.fsync {
            guard.get_ref().sync_data()?;
        }
        guard.get_ref().unlock()?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SchemaEntry {
    id: u32,
    source: String,
    kind: String,
}

struct SchemaRegistry {
    map: std::collections::HashMap<(String, String), u32>,
    next_id: u32,
    file: BufWriter<File>,
}

impl SchemaRegistry {
    fn open(dir: &Path) -> Result<Self> {
        let path = dir.join(SCHEMA_FILE);
        if !path.exists() {
            let file = OpenOptions::new().create(true).append(true).open(&path)?;
            return Ok(Self {
                map: std::collections::HashMap::new(),
                next_id: 1,
                file: BufWriter::new(file),
            });
        }
        let mut file = OpenOptions::new().read(true).append(true).open(&path)?;
        let mut map = std::collections::HashMap::new();
        let mut next_id = 1u32;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let mut cursor = 0usize;
        while cursor + 4 <= bytes.len() {
            let len = read_u32(&bytes[cursor..cursor + 4]) as usize;
            cursor += 4;
            if cursor + len > bytes.len() {
                break;
            }
            let entry: SchemaEntry = bincode::deserialize(&bytes[cursor..cursor + len])?;
            map.insert((entry.source.clone(), entry.kind.clone()), entry.id);
            next_id = next_id.max(entry.id.saturating_add(1));
            cursor += len;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            map,
            next_id,
            file: BufWriter::new(file),
        })
    }

    fn id_for(&mut self, source: &str, kind: &str) -> Result<u32> {
        if let Some(id) = self.map.get(&(source.to_string(), kind.to_string())) {
            return Ok(*id);
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let entry = SchemaEntry {
            id,
            source: source.to_string(),
            kind: kind.to_string(),
        };
        let bytes = bincode::serialize(&entry)?;
        let len = bytes.len() as u32;
        self.file.write_all(&len.to_le_bytes())?;
        self.file.write_all(&bytes)?;
        self.file.flush()?;
        self.map
            .insert((entry.source.clone(), entry.kind.clone()), id);
        Ok(id)
    }
}

impl Default for SegmentConfig {
    fn default() -> Self {
        Self {
            max_bytes: 64 * 1024 * 1024,
            index_stride: 256,
            time_bucket_ms: 5_000,
            retain_segments: None,
        }
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
    fsync: bool,
    inner: Mutex<SegmentFiles>,
    registry: Mutex<SchemaRegistry>,
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
        let registry = SchemaRegistry::open(dir)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            config,
            seq: AtomicU64::new(next_seq),
            fsync: false,
            inner: Mutex::new(files),
            registry: Mutex::new(registry),
        })
    }

    pub fn with_fsync(mut self, enabled: bool) -> Self {
        self.fsync = enabled;
        self
    }

    pub fn write_event(&self, event: &CanonEvent) -> Result<()> {
        let kind_id = {
            let mut registry = self.registry.lock().expect("schema registry poisoned");
            registry.id_for(&event.source, &event.kind)?
        };
        let payload = serde_json::to_vec(event)?;
        let payload_len = payload.len() as u32;
        let mut hasher = Hasher::new();
        hasher.update(&payload);
        let crc32 = hasher.finalize();
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        let source_id = fnv1a_32(&event.source);

        let mut header = Vec::with_capacity(HEADER_LEN as usize);
        header.extend_from_slice(&MAGIC.to_le_bytes());
        header.extend_from_slice(&VERSION.to_le_bytes());
        header.extend_from_slice(&HEADER_LEN.to_le_bytes());
        header.extend_from_slice(&event.ts.to_le_bytes());
        header.extend_from_slice(&source_id.to_le_bytes());
        header.extend_from_slice(&kind_id.to_le_bytes());
        header.extend_from_slice(&seq.to_le_bytes());
        header.extend_from_slice(&payload_len.to_le_bytes());
        header.extend_from_slice(&crc32.to_le_bytes());

        if header.len() != HEADER_LEN as usize {
            return Err(anyhow!("binary header length mismatch"));
        }

        let mut guard = self.inner.lock().expect("binary segment writer poisoned");
        guard.log.get_ref().lock_exclusive()?;
        let record_size = (HEADER_LEN as u64) + (payload_len as u64);
        if guard.size + record_size > self.config.max_bytes {
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
        guard.log.write_all(&header)?;
        guard.log.write_all(&payload)?;
        guard.size = guard.size.saturating_add(record_size);
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
}

fn open_new_segment_files(dir: &Path, base_seq: u64) -> Result<SegmentFiles> {
    let name = format!("{:020}", base_seq);
    let log_path = dir.join(format!("{}.log", name));
    let idx_path = dir.join(format!("{}.idx", name));
    let time_path = dir.join(format!("{}.time", name));
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(&log_path)?;
    let idx = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&idx_path)?;
    let time = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&time_path)?;
    let size = log.metadata().map(|m| m.len()).unwrap_or(0);
    Ok(SegmentFiles {
        log: BufWriter::new(log),
        idx: BufWriter::new(idx),
        time: BufWriter::new(time),
        size,
        last_time_bucket: None,
        records: 0,
    })
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

fn recover_segment(
    dir: &Path,
    base_seq: u64,
    config: &SegmentConfig,
) -> Result<(SegmentFiles, Option<u64>)> {
    let name = format!("{:020}", base_seq);
    let log_path = dir.join(format!("{}.log", name));
    let idx_path = dir.join(format!("{}.idx", name));
    let time_path = dir.join(format!("{}.time", name));

    if !log_path.exists() {
        let files = open_new_segment_files(dir, base_seq)?;
        return Ok((files, None));
    }

    let bytes = fs::read(&log_path)?;
    let mut cursor = 0usize;
    let mut size = 0u64;
    let mut records: u32 = 0;
    let mut last_seq: Option<u64> = None;
    let mut last_time_bucket: Option<u64> = None;

    let idx = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&idx_path)?;
    let time = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&time_path)?;
    let mut idx_writer = BufWriter::new(idx);
    let mut time_writer = BufWriter::new(time);

    while cursor + HEADER_LEN_USIZE <= bytes.len() {
        let header = &bytes[cursor..cursor + HEADER_LEN_USIZE];
        let magic = read_u32(&header[0..4]);
        if magic != MAGIC {
            break;
        }
        let header_len = read_u16(&header[6..8]) as usize;
        if header_len < HEADER_LEN_USIZE {
            break;
        }
        let ts = read_u64(&header[8..16]);
        let seq = read_u64(&header[24..32]);
        let payload_len = read_u32(&header[32..36]) as usize;
        let crc32 = read_u32(&header[36..40]);
        let payload_start = cursor + header_len;
        let payload_end = payload_start + payload_len;
        if payload_end > bytes.len() {
            break;
        }
        let payload = &bytes[payload_start..payload_end];
        let mut hasher = Hasher::new();
        hasher.update(payload);
        if hasher.finalize() != crc32 {
            break;
        }
        records = records.saturating_add(1);
        size = payload_end as u64;
        last_seq = Some(seq);

        if records % config.index_stride == 0 {
            idx_writer.write_all(&seq.to_le_bytes())?;
            idx_writer.write_all(&(cursor as u64).to_le_bytes())?;
        }
        let bucket = ts / config.time_bucket_ms;
        if last_time_bucket != Some(bucket) {
            time_writer.write_all(&bucket.to_le_bytes())?;
            time_writer.write_all(&(cursor as u64).to_le_bytes())?;
            last_time_bucket = Some(bucket);
        }
        cursor = payload_end;
    }

    idx_writer.flush()?;
    time_writer.flush()?;

    if size < bytes.len() as u64 {
        let file = OpenOptions::new().write(true).open(&log_path)?;
        file.set_len(size)?;
    }

    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(&log_path)?;
    let idx = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(&idx_path)?;
    let time = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(&time_path)?;

    Ok((
        SegmentFiles {
            log: BufWriter::new(log),
            idx: BufWriter::new(idx),
            time: BufWriter::new(time),
            size,
            last_time_bucket,
            records,
        },
        last_seq,
    ))
}
