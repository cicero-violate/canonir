use crate::{invariants, wire::EventClass, CanonEvent, CanonPayload, CanonPayloadMeta, EventId, EventKind};
use anyhow::Result;
use canon_invariant::{invariant_violation_delta, invariant_violation_state};
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

#[derive(Debug, Clone)]
struct PendingState {
    expected: EventKind,
    parent: EventId,
    source_kind: EventKind,
    note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RejectedEdge {
    parent: String,
    kind: String,
    fingerprint: u64,
}

pub struct BinarySegmentWriter {
    dir: PathBuf,
    config: SegmentConfig,
    seq: AtomicU64,
    last_ts: AtomicU64,
    fsync: bool,
    inner: Mutex<SegmentFiles>,
    dedup: Mutex<DedupCache>,
    rejected_edges: Mutex<DedupCacheEdges>,
    last_event: Mutex<Option<CanonEvent>>,
    pending: Mutex<Option<PendingState>>,
}

#[derive(Default)]
struct DedupCache {
    seen: std::collections::HashSet<u64>,
    order: std::collections::VecDeque<u64>,
    cap: usize,
}

#[derive(Default)]
struct DedupCacheEdges {
    seen: std::collections::HashSet<RejectedEdge>,
    order: std::collections::VecDeque<RejectedEdge>,
    cap: usize,
}

impl DedupCacheEdges {
    fn new(cap: usize) -> Self {
        Self { cap, ..Default::default() }
    }

    fn insert_if_new(&mut self, edge: RejectedEdge) -> bool {
        if self.seen.contains(&edge) {
            return false;
        }
        self.seen.insert(edge.clone());
        self.order.push_back(edge);
        if self.order.len() > self.cap {
            if let Some(old) = self.order.pop_front() {
                self.seen.remove(&old);
            }
        }
        true
    }
}

impl DedupCache {
    fn new(cap: usize) -> Self {
        Self { cap, ..Default::default() }
    }

    fn insert_if_new(&mut self, h: u64) -> bool {
        if self.seen.contains(&h) {
            return false;
        }
        self.seen.insert(h);
        self.order.push_back(h);
        if self.order.len() > self.cap {
            if let Some(old) = self.order.pop_front() {
                self.seen.remove(&old);
            }
        }
        true
    }
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
        let (mut files, recovered_seq, last_event) = recover_segment(dir, base_seq, &config)?;
        let pending = last_event.as_ref().and_then(invariants::required_successor).map(|p| PendingState {
            expected: p.expected,
            parent: p.parent,
            source_kind: p.source_kind,
            note: p.note,
        });
        let next_seq = recovered_seq.map(|s| s.saturating_add(1)).unwrap_or(base_seq);
        if files.size >= config.max_bytes {
            files = open_new_segment_files(dir, next_seq)?;
        }
        if let Some(keep) = config.retain_segments {
            apply_retention(dir, keep)?;
        }
        Ok(Self {
            dir: dir.to_path_buf(),
            config,
            seq: AtomicU64::new(next_seq),
            last_ts: AtomicU64::new(0),
            fsync: false,
            inner: Mutex::new(files),
            dedup: Mutex::new(DedupCache::new(1024)),
            rejected_edges: Mutex::new(DedupCacheEdges::new(1024)),
            last_event: Mutex::new(last_event),
            pending: Mutex::new(pending),
        })
    }

    pub fn with_fsync(mut self, enabled: bool) -> Self {
        self.fsync = enabled;
        self
    }

    /// Advance the in-memory pending-required-successor FSM to reflect a control event
    /// that was written by another process and read back from the tlog. Does not write
    /// anything; only updates `pending` so subsequent writes from this process are not
    /// spuriously rejected by the `missing required successor` invariant check.
    pub fn notify_replayed_event(&self, event: &CanonEvent) {
        if event.kind.class() != EventClass::Control {
            return;
        }
        let next_pending = invariants::required_successor(event).map(|p| PendingState {
            expected: p.expected,
            parent: p.parent,
            source_kind: p.source_kind,
            note: p.note,
        });
        eprintln!(
            "[tlog][notify_replayed] kind={} id={} actor={} next_expected={:?}",
            event.kind, event.id, event.actor,
            next_pending.as_ref().map(|p| p.expected.to_string())
        );
        *self.pending.lock().expect("pending poisoned") = next_pending;
    }

    pub fn write_canon_event(&self, event: &CanonEvent) -> Result<()> {
        if let Some(retry_err) = self.check_invalid_retry(event)? {
            return Err(retry_err);
        }

        // --- Invariant gate ---
        if let Err(err) = invariants::validate_event(event) {
            self.record_rejected_edge(event, &err.to_string(), None)?;
            return Err(err);
        }

        {
            let last = self.last_event.lock().expect("last_event poisoned");
            if let Some(prev) = last.as_ref() {
                if let Err(err) = invariants::validate_transition(prev, event) {
                    self.record_rejected_edge(event, &err.to_string(), Some(prev.id.clone()))?;
                    return Err(err);
                }
            }
        }

        let mut required_successor_override = false;
        {
            let mut pending = self.pending.lock().expect("pending poisoned");
            if let Some(req) = pending.as_ref() {
                if event.kind.class() == EventClass::Effect {
                    // effect events neither discharge nor mutate the control FSM
                } else if event.kind != req.expected {
                    eprintln!(
                        "[tlog][pending_violation] got={} id={} actor={} expected={} after={} parent={} note={}",
                        event.kind, event.id, event.actor,
                        req.expected, req.source_kind, req.parent, req.note
                    );
                    let err = anyhow::anyhow!(
                        "invariant violation: missing required successor after {} id={}; expected={}; got={}; note={}",
                        req.source_kind,
                        req.parent,
                        req.expected,
                        event.kind,
                        req.note
                    );
                    self.record_rejected_edge(event, &err.to_string(), Some(req.parent.clone()))?;
                    *pending = None;
                    return Err(err);
                } else {
                    eprintln!(
                        "[tlog][pending_discharged] kind={} id={} discharged_expected={} after={}",
                        event.kind, event.id, req.expected, req.source_kind
                    );
                    required_successor_override = true;
                    *pending = None;
                }
            }
        }

        if !required_successor_override {
            let content_hash = event_content_hash(event);
            let mut dedup = self.dedup.lock().expect("dedup cache poisoned");
            if !dedup.insert_if_new(content_hash) {
                eprintln!(
                    "[tlog][dedup_reject] kind={} id={} actor={} content_hash_collision",
                    event.kind, event.id, event.actor
                );
                let err = anyhow::anyhow!(
                    "invariant violation: duplicate event within dedup window kind={}; id={}",
                    event.kind,
                    event.id
                );
                self.record_rejected_edge(event, &err.to_string(), None)?;
                return Err(err);
            }
        }

        self.write_canon_event_inner(event)?;
        if event.kind.class() == EventClass::Control {
            let next = invariants::required_successor(event).map(|p| PendingState {
                expected: p.expected,
                parent: p.parent,
                source_kind: p.source_kind,
                note: p.note,
            });
            eprintln!(
                "[tlog][pending_set] after_kind={} after_id={} next_expected={:?}",
                event.kind, event.id,
                next.as_ref().map(|p| p.expected.to_string())
            );
            *self.pending.lock().expect("pending poisoned") = next;
        }
        Ok(())
    }

    fn write_canon_event_inner(&self, event: &CanonEvent) -> Result<()> {

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
        *self.last_event.lock().expect("last_event poisoned") = Some(event.clone());
        Ok(())
    }

    fn check_invalid_retry(&self, event: &CanonEvent) -> Result<Option<anyhow::Error>> {
        if invariants::is_recovery_event(event) || self.is_invariant_violation_event(event) {
            return Ok(None);
        }
        let Some(parent) = event.parent_ids.first() else {
            return Ok(None);
        };
        let edge = RejectedEdge {
            parent: parent.to_string(),
            kind: event.kind.to_string(),
            fingerprint: event_content_hash(event),
        };
        let rejected = self.rejected_edges.lock().expect("rejected_edges poisoned");
        if rejected.seen.contains(&edge) {
            let err = anyhow::anyhow!(
                "invariant violation: invalid_retry parent={}; kind={}; blocked until recovery_event/reset_event/override_event",
                parent,
                event.kind
            );
            drop(rejected);
            self.record_rejected_edge(event, &err.to_string(), Some(parent.clone()))?;
            return Ok(Some(err));
        }
        Ok(None)
    }

    fn record_rejected_edge(&self, event: &CanonEvent, message: &str, parent: Option<EventId>) -> Result<()> {
        if !self.is_invariant_violation_event(event) {
            let parent_for_violation = parent.unwrap_or_else(|| event.parent_ids.first().cloned().unwrap_or_else(|| EventId::new(crate::new_event_id())));
            let violation = self.build_invariant_violation_event_with_parent(event, message, parent_for_violation)?;
            let edge = RejectedEdge {
                parent: violation.parent_ids.first().map(ToString::to_string).unwrap_or_default(),
                kind: event.kind.to_string(),
                fingerprint: event_content_hash(event),
            };
            let mut rejected = self.rejected_edges.lock().expect("rejected_edges poisoned");
            let is_new = rejected.insert_if_new(edge);
            drop(rejected);
            if is_new {
                self.write_canon_event_inner(&violation)?;
            }
        }
        Ok(())
    }

    fn build_invariant_violation_event_with_parent(&self, rejected: &CanonEvent, message: &str, parent: EventId) -> Result<CanonEvent> {
        let full_message = format!(
            "{}; rejected_kind={}; rejected_id={}; rejected_actor={}; rejected_meta_file={}; rejected_meta_line={}",
            message,
            rejected.kind,
            rejected.id,
            rejected.actor,
            rejected.payload.meta.file,
            rejected.payload.meta.line
        );
        let delta = invariant_violation_delta(full_message);
        let state = invariant_violation_state();
        let payload = CanonPayload {
            input: serde_json::json!({ "tick": delta.tick, "id": delta.id }),
            output: serde_json::to_value(&delta.event)?,
            delta: serde_json::json!({ "graph_version": state.graph_version }),
            meta: CanonPayloadMeta { file: file!().to_string(), line: line!() },
            data: serde_json::json!({
                "rejected": {
                    "id": rejected.id.as_str(),
                    "kind": rejected.kind.as_str(),
                    "actor": rejected.actor,
                    "meta_file": rejected.payload.meta.file,
                    "meta_line": rejected.payload.meta.line,
                },
                "delta": serde_json::to_value(&delta)?,
                "state": serde_json::to_value(&state)?,
            }),
        };
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Ok(CanonEvent::new(
            EventId::new(crate::new_event_id()),
            vec![parent],
            "writer",
            EventKind::Code,
            ts,
            payload,
            true,
        ))
    }

    fn is_invariant_violation_event(&self, event: &CanonEvent) -> bool {
        matches!(
            event.payload.data.get("delta")
                .and_then(|d| d.get("event"))
                .and_then(|e| e.get("InvariantViolation")),
            Some(_)
        )
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }
}

fn event_content_hash(event: &CanonEvent) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    event.kind.hash(&mut h);
    event.payload.data.hash(&mut h);
    event.payload.delta.hash(&mut h);
    h.finish()
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

fn recover_segment(dir: &Path, base_seq: u64, config: &SegmentConfig) -> Result<(SegmentFiles, Option<u64>, Option<CanonEvent>)> {
    let name = format!("{:020}", base_seq);
    let log_path = dir.join(format!("{}.log", name));
    let idx_path = dir.join(format!("{}.idx", name));
    let time_path = dir.join(format!("{}.time", name));

    if !log_path.exists() {
        return Ok((open_new_segment_files(dir, base_seq)?, None, None));
    }

    let bytes = fs::read(&log_path)?;

    // Detect legacy binary segment (starts with TLOG magic) — discard and start fresh.
    if bytes.len() >= 4 && read_u32(&bytes[0..4]) == MAGIC {
        OpenOptions::new().write(true).truncate(true).open(&log_path)?;
        let _ = OpenOptions::new().create(true).write(true).truncate(true).open(&idx_path);
        let _ = OpenOptions::new().create(true).write(true).truncate(true).open(&time_path);
        return Ok((open_new_segment_files(dir, base_seq)?, None, None));
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
    let mut last_event: Option<CanonEvent> = None;
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
        last_event = Some(event.clone());

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

    Ok((SegmentFiles { log: BufWriter::new(log), idx: BufWriter::new(idx), time: BufWriter::new(time_f), size, last_time_bucket, records }, last_seq, last_event))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CanonPayload, CanonPayloadMeta};
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("canon_runtime_events_{name}_{nanos}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn payload(data: serde_json::Value, delta: serde_json::Value) -> CanonPayload {
        CanonPayload {
            input: json!({"x": 1}),
            output: json!({"y": 1}),
            delta,
            meta: CanonPayloadMeta { file: "test".to_string(), line: 1 },
            data,
        }
    }

    fn event(id: &str, kind: EventKind, ts: u64, data: serde_json::Value, delta: serde_json::Value) -> CanonEvent {
        CanonEvent::new(EventId::new(id.to_string()), Vec::new(), "test", kind, ts, payload(data, delta), true)
    }

    #[test]
    fn effect_events_do_not_discharge_pending_control_obligation() {
        let dir = temp_dir("effect_neutral");
        let writer = BinarySegmentWriter::open(&dir).unwrap();

        let route = event(
            "route-1",
            EventKind::RouteSelected,
            1,
            json!({"approved_route":"observe","suggested_route":"observe"}),
            json!({"gate_changed": false}),
        );
        writer.write_canon_event(&route).unwrap();

        let debug = event(
            "debug-1",
            EventKind::Debug,
            2,
            json!({"kind":"note"}),
            json!({"payload":"side-effect"}),
        );
        writer.write_canon_event(&debug).unwrap();

        let observed = event(
            "obs-1",
            EventKind::LoopObserved,
            3,
            json!({"goal_text":"g"}),
            json!({"compiler_errors":[]}),
        );
        writer.write_canon_event(&observed).unwrap();
    }

    #[test]
    fn wrong_next_control_event_is_rejected_immediately() {
        let dir = temp_dir("strict_obligation");
        let writer = BinarySegmentWriter::open(&dir).unwrap();

        let route = event(
            "route-1",
            EventKind::RouteSelected,
            1,
            json!({"approved_route":"observe","suggested_route":"observe"}),
            json!({"gate_changed": false}),
        );
        writer.write_canon_event(&route).unwrap();

        let acted = event(
            "act-1",
            EventKind::LoopActed,
            2,
            json!({"action_kind":"noop"}),
            json!({"success": true}),
        );
        let err = writer.write_canon_event(&acted).unwrap_err().to_string();
        assert!(err.contains("missing required successor"));
        assert!(err.contains("expected=loop_observed"));
        assert!(err.contains("got=loop_acted"));
    }

    #[test]
    fn route_selected_plan_waits_for_loop_planned_while_capability_effects_are_ignored() {
        let dir = temp_dir("chain_progression");
        let writer = BinarySegmentWriter::open(&dir).unwrap();

        let route_plan = event(
            "route-1",
            EventKind::RouteSelected,
            1,
            json!({"approved_route":"plan","suggested_route":"plan"}),
            json!({"gate_changed": false}),
        );
        writer.write_canon_event(&route_plan).unwrap();

        let capability_done = event(
            "cap-1",
            EventKind::CapabilityCompleted,
            2,
            json!({"request_id":"planner-1","capability":"llm.call"}),
            json!({"result":{"Llm":{"success":true}}}),
        );
        writer.write_canon_event(&capability_done).unwrap();

        let loop_planned = event(
            "planned-1",
            EventKind::LoopPlanned,
            3,
            json!({"action_kind":"noop"}),
            json!({"signals": {}}),
        );
        writer.write_canon_event(&loop_planned).unwrap();
    }
}
