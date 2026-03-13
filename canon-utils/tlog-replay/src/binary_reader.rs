use anyhow::{anyhow, Result};
use canon_tlog_writer::CanonEvent;
use crc32fast::Hasher;
use std::fs;
use std::path::Path;

const MAGIC: u32 = 0x544C4F47; // "TLOG"
const HEADER_LEN: usize = 40;

fn read_u16(buf: &[u8]) -> u16 {
    u16::from_le_bytes([buf[0], buf[1]])
}

fn read_u32(buf: &[u8]) -> u32 {
    u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]])
}

fn read_u64(buf: &[u8]) -> u64 {
    u64::from_le_bytes([buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7]])
}

pub fn read_binary_events(path: &Path) -> Result<Vec<CanonEvent>> {
    let bytes = fs::read(path)?;
    let mut cursor = 0usize;
    let mut events = Vec::new();

    while cursor + HEADER_LEN <= bytes.len() {
        let header = &bytes[cursor..cursor + HEADER_LEN];
        let magic = read_u32(&header[0..4]);
        if magic != MAGIC {
            return Err(anyhow!("invalid tlog magic at offset {}", cursor));
        }
        let _version = read_u16(&header[4..6]);
        let header_len = read_u16(&header[6..8]) as usize;
        if header_len < HEADER_LEN {
            return Err(anyhow!("invalid header_len {}", header_len));
        }
        let _ts = read_u64(&header[8..16]);
        let _source_id = read_u32(&header[16..20]);
        let _kind_id = read_u32(&header[20..24]);
        let _seq = read_u64(&header[24..32]);
        let payload_len = read_u32(&header[32..36]) as usize;
        let crc32 = read_u32(&header[36..40]);

        let payload_start = cursor + header_len;
        let payload_end = payload_start + payload_len;
        if payload_end > bytes.len() {
            return Err(anyhow!("truncated payload at offset {}", cursor));
        }
        let payload = &bytes[payload_start..payload_end];
        let mut hasher = Hasher::new();
        hasher.update(payload);
        let computed = hasher.finalize();
        if computed != crc32 {
            return Err(anyhow!("crc mismatch at offset {}", cursor));
        }
        let event: CanonEvent = serde_json::from_slice(payload)?;
        events.push(event);
        cursor = payload_end;
    }

    Ok(events)
}

pub fn is_binary_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && read_u32(&bytes[0..4]) == MAGIC
}

pub fn read_binary_events_from_segment_with_start_seq(
    log_path: &Path,
    start_seq: u64,
) -> Result<Vec<CanonEvent>> {
    let idx_path = log_path.with_extension("idx");
    let mut start_pos = 0u64;
    if idx_path.exists() {
        let idx_bytes = fs::read(&idx_path)?;
        let mut cursor = 0usize;
        while cursor + 16 <= idx_bytes.len() {
            let seq = read_u64(&idx_bytes[cursor..cursor + 8]);
            let pos = read_u64(&idx_bytes[cursor + 8..cursor + 16]);
            if seq <= start_seq {
                start_pos = pos;
            } else {
                break;
            }
            cursor += 16;
        }
    }

    let bytes = fs::read(log_path)?;
    let mut cursor = start_pos as usize;
    let mut events = Vec::new();

    while cursor + HEADER_LEN <= bytes.len() {
        let header = &bytes[cursor..cursor + HEADER_LEN];
        let magic = read_u32(&header[0..4]);
        if magic != MAGIC {
            break;
        }
        let header_len = read_u16(&header[6..8]) as usize;
        if header_len < HEADER_LEN {
            break;
        }
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
        if seq >= start_seq {
            let event: CanonEvent = serde_json::from_slice(payload)?;
            events.push(event);
        }
        cursor = payload_end;
    }
    Ok(events)
}
