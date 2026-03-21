use anyhow::{anyhow, Result};
use canon_event::{CanonEvent, CanonPayload, EventMeta, RustcEvent};
use canon_event_store::{extract_rustc_event, read_any_events_from_path, replay_graph_from_tlog, AnyEvent};
use std::collections::HashSet;
use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 5 {
        eprintln!("usage: verify_tlog_equivalence --json <event.tlog> --binary <event.tlog.d> [--stress]");
        return Err(anyhow!("missing args"));
    }
    let mut json_path: Option<PathBuf> = None;
    let mut bin_path: Option<PathBuf> = None;
    let mut stress = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => {
                i += 1;
                json_path = args.get(i).map(PathBuf::from);
            }
            "--binary" => {
                i += 1;
                bin_path = args.get(i).map(PathBuf::from);
            }
            "--stress" => {
                stress = true;
            }
            _ => {}
        }
        i += 1;
    }
    let json_path = json_path.ok_or_else(|| anyhow!("missing --json"))?;
    let bin_path = bin_path.ok_or_else(|| anyhow!("missing --binary"))?;

    if stress {
        run_stress(&json_path, &bin_path)?;
    }

    let gj = replay_graph_from_tlog(&json_path)?;
    let gb = replay_graph_from_tlog(&bin_path)?;

    let mut diffs = Vec::new();
    if gj.nodes.len() != gb.nodes.len() {
        diffs.push(format!("node_count json={} binary={}", gj.nodes.len(), gb.nodes.len()));
    }
    if gj.edges.len() != gb.edges.len() {
        diffs.push(format!("edge_count json={} binary={}", gj.edges.len(), gb.edges.len()));
    }

    let json_nodes = node_set(&gj.nodes);
    let bin_nodes = node_set(&gb.nodes);
    if json_nodes != bin_nodes {
        diffs.push(format!("node_set mismatch: json_only={} binary_only={}", json_nodes.difference(&bin_nodes).count(), bin_nodes.difference(&json_nodes).count()));
    }

    let json_edges = edge_set(&gj.edges);
    let bin_edges = edge_set(&gb.edges);
    if json_edges != bin_edges {
        diffs.push(format!("edge_set mismatch: json_only={} binary_only={}", json_edges.difference(&bin_edges).count(), bin_edges.difference(&json_edges).count()));
    }

    let json_sessions = count_sessions(&json_path)?;
    let bin_sessions = count_sessions(&bin_path)?;
    if json_sessions != bin_sessions {
        diffs.push(format!("session_count json={} binary={}", json_sessions, bin_sessions));
    }

    if diffs.is_empty() {
        println!("verification: PASS");
        return Ok(());
    }
    println!("verification: FAIL");
    for diff in diffs {
        println!("- {}", diff);
    }
    Err(anyhow!("verification failed"))
}

fn node_set(nodes: &[canon_event_store::CodeGraphNode]) -> HashSet<(u32, String, String, Option<u32>, Option<u32>)> {
    nodes.iter().map(|n| (n.id, n.kind.clone(), n.symbol.clone(), n.file_id, n.line)).collect()
}

fn edge_set(edges: &[canon_event_store::CodeGraphEdge]) -> HashSet<(u32, u32, String)> {
    edges.iter().map(|e| (e.src, e.dst, e.kind.clone())).collect()
}

fn count_sessions(path: &Path) -> Result<u64> {
    let mut count = 0u64;
    for event in read_any_events_from_path(path)? {
        match event {
            AnyEvent::Canon(canon) => {
                if let Some(kernel) = extract_rustc_event(&canon) {
                    if let RustcEvent::SessionStart(_) = kernel {
                        count += 1;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(count)
}

fn run_stress(json_path: &Path, bin_path: &Path) -> Result<()> {
    use canon_event::BinarySegmentWriter;
    use std::fs::File;
    use std::io::BufWriter;

    if let Some(parent) = json_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(parent) = bin_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _ = std::fs::remove_file(json_path);
    if bin_path.is_dir() {
        let _ = std::fs::remove_dir_all(bin_path);
    }
    let mut json_writer = BufWriter::new(File::create(json_path)?);
    let bin_writer = BinarySegmentWriter::open(bin_path)?;

    let session = RustcEvent::SessionStart(canon_event::SessionStart { project: "stress".to_string(), schema: 2, byte_offset: 0 });
    let session_json = serde_json::to_value(&session)?;
    let canon = make_canon("rustc", CanonPayload::RustcEvent(session_json.clone()));
    let line = serde_json::to_string(&canon)?;
    std::io::Write::write_all(&mut json_writer, line.as_bytes())?;
    std::io::Write::write_all(&mut json_writer, b"\n")?;
    let _ = bin_writer.write_canon_event(&make_canon("rustc", CanonPayload::RustcEvent(session_json)));

    for i in 0..10_000u32 {
        let node = RustcEvent::NodeDefined(canon_event::NodeDefined { symbol: format!("node_{i}"), kind: "FUNCTION".to_string(), file: "src/lib.rs".to_string(), line: 1, col: 1, lo: 0, hi: 0 });
        let val = serde_json::to_value(&node)?;
        let canon = make_canon("rustc", CanonPayload::RustcEvent(val.clone()));
        let line = serde_json::to_string(&canon)?;
        std::io::Write::write_all(&mut json_writer, line.as_bytes())?;
        std::io::Write::write_all(&mut json_writer, b"\n")?;
        let _ = bin_writer.write_canon_event(&make_canon("rustc", CanonPayload::RustcEvent(val)));
    }
    for i in 0..50_000u32 {
        let src = format!("node_{}", i % 10_000);
        let dst = format!("node_{}", (i * 7) % 10_000);
        let edge = RustcEvent::EdgeDefined(canon_event::EdgeDefined { src, dst, kind: "CALL".to_string() });
        let val = serde_json::to_value(&edge)?;
        let canon = make_canon("rustc", CanonPayload::RustcEvent(val.clone()));
        let line = serde_json::to_string(&canon)?;
        std::io::Write::write_all(&mut json_writer, line.as_bytes())?;
        std::io::Write::write_all(&mut json_writer, b"\n")?;
        let _ = bin_writer.write_canon_event(&make_canon("rustc", CanonPayload::RustcEvent(val)));
    }
    json_writer.flush()?;
    let _ = 0;
    Ok(())
}

fn make_canon(source: &str, payload: CanonPayload) -> CanonEvent {
    let meta = EventMeta { ts: now_ms(), source: source.to_string(), file: String::new(), line: 0 };
    CanonEvent { event_id: None, meta, payload }
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}
