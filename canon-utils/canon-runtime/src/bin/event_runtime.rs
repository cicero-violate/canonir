use anyhow::{anyhow, Result};
use canon_runtime::bootstrap::{bootstrap_config, new_prompt_registry};
use canon_runtime::consumers::agent::AgentConsumer;
use canon_runtime::consumers::capability_executor::CapabilityExecutor;
use canon_runtime::consumers::llm_executor::LlmCapabilityHandler;
use canon_runtime::{register_default_capabilities, EventRuntime};
use canon_editor::EditConsumer;
use canon_event_store::read_any_events_from_path_with_start_seq;
use canon_event_store::replay_graph_from_tlog;
use canon_event_store::AnyEvent;
use crossbeam_channel as cc;
use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Lock guard — ensures only one instance runs against a given tlog path.
// ---------------------------------------------------------------------------

struct LockGuard {
    path: PathBuf,
    _file: File,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn parse_pid(lock_contents: &str) -> Option<u32> {
    lock_contents
        .lines()
        .find_map(|line| line.strip_prefix("pid="))
        .and_then(|value| value.trim().parse::<u32>().ok())
}

fn pid_is_alive(pid: u32) -> Result<bool> {
    let stat_path = PathBuf::from("/proc").join(pid.to_string()).join("stat");
    let mut file = match File::open(&stat_path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err.into()),
    };
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let close_paren = match contents.rfind(')') {
        Some(idx) => idx,
        None => return Ok(true),
    };
    let state = contents[close_paren + 1..]
        .trim_start()
        .chars()
        .next()
        .unwrap_or(' ');
    Ok(state != 'Z')
}

fn acquire_lock(path: &Path) -> Result<Option<LockGuard>> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => {
            let _ = file.write_all(format!("pid={}\n", std::process::id()).as_bytes());
            return Ok(Some(LockGuard {
                path: path.to_path_buf(),
                _file: file,
            }));
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(err) => return Err(err.into()),
    }

    let mut contents = String::new();
    if let Ok(mut file) = File::open(path) {
        let _ = file.read_to_string(&mut contents);
    }
    let Some(pid) = parse_pid(&contents) else {
        eprintln!(
            "[event_runtime] another instance is running (lock: {})",
            path.display()
        );
        return Ok(None);
    };
    let alive = pid_is_alive(pid)?;
    if alive {
        eprintln!(
            "[event_runtime] another instance is running (lock: {})",
            path.display()
        );
        return Ok(None);
    }

    let _ = fs::remove_file(path);
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let _ = file.write_all(format!("pid={}\n", std::process::id()).as_bytes());
    Ok(Some(LockGuard {
        path: path.to_path_buf(),
        _file: file,
    }))
}

// ---------------------------------------------------------------------------
// Queue message type — P → Q
//
// Producers push Msg variants into the unbounded MPMC channel Q.
// W=1 (the main loop) is the sole receiver; it defines commit order and is
// the only writer to L (log/tlog).  Consumers (C ≥ 1) are driven from W
// via bus dispatch and track their own offsets; they never mutate L.
// ---------------------------------------------------------------------------

enum Msg {
    /// New inbound event delivered directly in memory — no filesystem poll.
    /// Produced by the notify-watcher thread (P2) and the bootstrap replayer (P1).
    Event(AnyEvent),
    /// Tlog was truncated/recreated (observed by P2).
    /// W must reset its state and replay the provided events from scratch
    /// to maintain deterministic order (Rule 10).
    Reset(Vec<AnyEvent>),
    /// Periodic housekeeping tick from the timer producer (P3).
    Tick,
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let mut tlog_path: Option<PathBuf> = None;
    let mut once = false;
    let start_at_tail = env::var("CANON_EVENT_RUNTIME_START_AT_TAIL")
        .ok()
        .map(|v| v != "0" && v.to_lowercase() != "false")
        .unwrap_or(false);
    let cursor_path = env::var("CANON_EVENT_RUNTIME_CURSOR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(
                "/workspace/ai_sandbox/canon/state/event_runtime.cursor.json",
            )
        });

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--tlog" => {
                i += 1;
                tlog_path = args.get(i).map(PathBuf::from);
            }
            "--once" => once = true,
            _ => {}
        }
        i += 1;
    }

    let tlog_path = tlog_path.ok_or_else(|| anyhow!("missing --tlog"))?;
    let event_execution_enabled = std::env::var("CANON_EVENT_EXECUTION")
        .ok()
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false);
    let lock_path = env::var("CANON_EVENT_RUNTIME_LOCK")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from("/workspace/ai_sandbox/canon/state/event_runtime.lock")
        });
    let _lock_guard = match acquire_lock(&lock_path)? {
        Some(guard) => guard,
        None => return Ok(()),
    };

    if std::env::var("CANON_VERIFY_TLOG_EQUIV").ok().as_deref() == Some("1") {
        let _ = maybe_verify_tlog_equivalence(&tlog_path);
    }

    // --- Build runtime (W owns this exclusively) ---
    let registry = std::sync::Arc::new(std::sync::Mutex::new(
        canon_capability::CapabilityRegistry::new(),
    ));
    let prompt_registry = new_prompt_registry();
    bootstrap_config(&tlog_path, &prompt_registry);

    let consumers: Vec<Box<dyn canon_event::EventConsumer>> = vec![
        Box::new(AgentConsumer::new()),
        Box::new(CapabilityExecutor::new(
            registry.clone(),
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
        )),
        Box::new(EditConsumer::new()),
    ];
    let mut runtime = EventRuntime::new_with_registry(consumers, registry.clone());
    {
        let mut reg = registry.lock().expect("capability registry lock");
        register_default_capabilities(&mut reg);
        reg.register(Arc::new(LlmCapabilityHandler::new(prompt_registry.clone())));
    }
    runtime.set_execute_capabilities(event_execution_enabled);
    // set_tlog_path tells W where to append (L).  Only W calls this; only W writes L.
    runtime.set_tlog_path(tlog_path.clone());

    // --- Determine start offset from persisted cursor ---
    let cursor_loaded = load_cursor(&cursor_path, &tlog_path).is_some();
    let start_seq: u64 = load_cursor_seq(&cursor_path, &tlog_path).unwrap_or(0);
    let mut processed: usize = load_cursor(&cursor_path, &tlog_path).unwrap_or(0);

    // Read events that already exist in L at startup (in-memory after this point).
    let bootstrap_events: Vec<AnyEvent> = if tlog_path.exists() {
        read_any_events_from_path_with_start_seq(&tlog_path, start_seq).unwrap_or_default()
    } else {
        vec![]
    };

    // On fresh boot (no cursor) or start-at-tail: skip existing events to avoid
    // re-dispatching stale capability_requested entries to consumers.
    if !cursor_loaded || (start_at_tail && processed == 0 && !bootstrap_events.is_empty()) {
        processed = bootstrap_events.len();
    }

    // --- Once mode: W processes the current snapshot of L, then exits ---
    if once {
        if !tlog_path.exists() {
            return Err(anyhow!("tlog not found: {}", tlog_path.display()));
        }
        if processed < bootstrap_events.len() {
            runtime.process_events(&bootstrap_events[processed..])?;
        }
        processed = bootstrap_events.len();
        let _ = save_cursor(&cursor_path, &tlog_path, processed, start_seq);
        return Ok(());
    }

    // =========================================================================
    // P → Q → W=1 → L
    //
    // Q  is an unbounded MPMC crossbeam channel.
    //    Multiple producers push concurrently without blocking each other.
    //
    // W=1  is the loop below.  It is the sole receiver of Q, the sole caller
    //    of process_events/emit_tick, and the sole appender to L.
    //    Order = arrival at W (Rule 5).  Determinism = single commit path (Rule 9).
    //
    // C ≥ 1  are the EventRuntime bus consumers.  They receive events dispatched
    //    by W, track their own state (offsets), and never write L (Rule 7).
    // =========================================================================
    let (q_tx, q_rx) = cc::unbounded::<Msg>();

    // --- P1: bootstrap replayer ---
    // Unprocessed events already in memory — push directly into Q, no file re-read.
    for event in bootstrap_events.into_iter().skip(processed) {
        q_tx.send(Msg::Event(event)).ok();
    }

    // --- P2: notify watcher ---
    // Uses OS-level inotify/kqueue to detect tlog changes.
    // On notification: reads new entries into memory, delivers each as Msg::Event
    // directly into Q.  Zero polling, zero sleep — events arrive in real time.
    // File is L (durable log); Q is the live in-memory delivery pipe.
    {
        let watcher_tlog = tlog_path.clone();
        let watcher_tx = q_tx.clone();
        let watcher_start_seq = start_seq;
        // watcher_seen tracks how many events from L this producer has already forwarded.
        let mut watcher_seen: usize = processed;

        let (fs_tx, fs_rx) = cc::unbounded::<notify::Result<notify::Event>>();
        let mut fs_watcher = RecommendedWatcher::new(
            move |res| {
                let _ = fs_tx.send(res);
            },
            NotifyConfig::default(),
        )?;

        // For a segmented binary tlog (dir) watch the dir itself; otherwise watch
        // the parent so we also catch the file being created for the first time.
        let watch_target = if watcher_tlog.is_dir() {
            watcher_tlog.clone()
        } else {
            watcher_tlog
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."))
        };
        if watch_target.exists() {
            fs_watcher.watch(&watch_target, RecursiveMode::NonRecursive)?;
        }

        std::thread::spawn(move || {
            let _watcher = fs_watcher; // keep alive for thread lifetime
            while let Ok(res) = fs_rx.recv() {
                if res.is_err() {
                    continue;
                }
                let all = match read_any_events_from_path_with_start_seq(
                    &watcher_tlog,
                    watcher_start_seq,
                ) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                if all.len() < watcher_seen {
                    // L was truncated or recreated — W must reset (Rule 10).
                    watcher_seen = 0;
                    if watcher_tx.send(Msg::Reset(all)).is_err() {
                        break;
                    }
                } else {
                    // Deliver only the new suffix: each event enters Q individually
                    // so W can interleave other message types between them.
                    for event in all.into_iter().skip(watcher_seen) {
                        watcher_seen += 1;
                        if watcher_tx.send(Msg::Event(event)).is_err() {
                            break;
                        }
                    }
                }
            }
        });
    }

    // --- P3: tick timer ---
    // A lightweight background producer that sends a housekeeping Tick into Q
    // every second.  W dispatches emit_tick(); consumers never see this as a
    // log entry (Tick is not appended to L).
    {
        let tick_tx = q_tx.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(1));
            if tick_tx.send(Msg::Tick).is_err() {
                break;
            }
        });
    }

    // =========================================================================
    // W = 1 — single writer loop
    //
    // Sole receiver of Q.  Order is defined by arrival here (Rule 5).
    // All appends to L happen inside runtime.process_events / emit_tick via
    // append_runtime_event — never from any producer thread (Rule 4, 8).
    // =========================================================================
    let mut last_saved = Instant::now();
    let mut last_saved_processed = processed;

    loop {
        match q_rx.recv()? {
            Msg::Event(event) => {
                // W processes and commits; consumers (C) receive via bus dispatch.
                runtime.process_events(std::slice::from_ref(&event))?;
                processed += 1;
                // Persist cursor periodically so replay can resume from offset.
                if processed != last_saved_processed
                    && last_saved.elapsed() >= Duration::from_secs(1)
                {
                    if save_cursor(&cursor_path, &tlog_path, processed, start_seq).is_ok() {
                        last_saved = Instant::now();
                        last_saved_processed = processed;
                    }
                }
            }
            Msg::Reset(events) => {
                // L was recreated — W resets state and replays from the beginning
                // to ensure deterministic order (Rule 10).
                runtime.reset();
                runtime.process_events(&events)?;
                processed = events.len();
            }
            Msg::Tick => {
                runtime.emit_tick()?;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Cursor helpers — track W's read offset into L for crash recovery / replay.
// ---------------------------------------------------------------------------

fn load_cursor(path: &Path, tlog_path: &Path) -> Option<usize> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let stored_path = value.get("tlog_path")?.as_str()?;
    if stored_path != tlog_path.display().to_string() {
        return None;
    }
    value.get("processed")?.as_u64().map(|v| v as usize)
}

fn load_cursor_seq(path: &Path, tlog_path: &Path) -> Option<u64> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let stored_path = value.get("tlog_path")?.as_str()?;
    if stored_path != tlog_path.display().to_string() {
        return None;
    }
    value.get("start_seq")?.as_u64()
}

fn save_cursor(path: &Path, tlog_path: &Path, processed: usize, start_seq: u64) -> Result<()> {
    let state = serde_json::json!({
        "tlog_path": tlog_path.display().to_string(),
        "processed": processed,
        "start_seq": start_seq,
        "updated_ms": now_ms(),
    });
    let tmp_path = path.with_extension("tmp");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&tmp_path, serde_json::to_string(&state)?)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

// ---------------------------------------------------------------------------
// Tlog equivalence verification (optional, debug mode only)
// ---------------------------------------------------------------------------

fn maybe_verify_tlog_equivalence(tlog_path: &Path) -> Result<()> {
    let (json_path, bin_path) = if tlog_path.is_dir() {
        (tlog_path.with_extension("tlog"), tlog_path.to_path_buf())
    } else {
        (tlog_path.to_path_buf(), tlog_path.with_extension("tlog.d"))
    };
    if !json_path.exists() || !bin_path.exists() {
        return Ok(());
    }
    let _diffs = verify_tlog_equivalence(json_path.as_path(), bin_path.as_path())?;
    Ok(())
}

fn verify_tlog_equivalence(json_path: &Path, bin_path: &Path) -> Result<Vec<String>> {
    let json_graph = replay_graph_from_tlog(json_path)?;
    let bin_graph = replay_graph_from_tlog(bin_path)?;
    let mut diffs = Vec::new();
    if json_graph.nodes.len() != bin_graph.nodes.len() {
        diffs.push(format!(
            "node_count json={} binary={}",
            json_graph.nodes.len(),
            bin_graph.nodes.len()
        ));
    }
    if json_graph.edges.len() != bin_graph.edges.len() {
        diffs.push(format!(
            "edge_count json={} binary={}",
            json_graph.edges.len(),
            bin_graph.edges.len()
        ));
    }
    let json_nodes: HashSet<(u32, String, String, Option<u32>, Option<u32>)> = json_graph
        .nodes
        .iter()
        .map(|n| (n.id, n.kind.clone(), n.symbol.clone(), n.file_id, n.line))
        .collect();
    let bin_nodes: HashSet<(u32, String, String, Option<u32>, Option<u32>)> = bin_graph
        .nodes
        .iter()
        .map(|n| (n.id, n.kind.clone(), n.symbol.clone(), n.file_id, n.line))
        .collect();
    if json_nodes != bin_nodes {
        diffs.push(format!(
            "node_set mismatch: json_only={} binary_only={}",
            json_nodes.difference(&bin_nodes).count(),
            bin_nodes.difference(&json_nodes).count()
        ));
    }
    let json_edges: HashSet<(u32, u32, String)> = json_graph
        .edges
        .iter()
        .map(|e| (e.src, e.dst, e.kind.clone()))
        .collect();
    let bin_edges: HashSet<(u32, u32, String)> = bin_graph
        .edges
        .iter()
        .map(|e| (e.src, e.dst, e.kind.clone()))
        .collect();
    if json_edges != bin_edges {
        diffs.push(format!(
            "edge_set mismatch: json_only={} binary_only={}",
            json_edges.difference(&bin_edges).count(),
            bin_edges.difference(&json_edges).count()
        ));
    }
    Ok(diffs)
}
