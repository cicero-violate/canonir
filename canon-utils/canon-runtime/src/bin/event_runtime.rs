use anyhow::{anyhow, Result};
use canon_event::EVENT_SCHEMA_VERSION;
use canon_event_store::replay_graph_from_tlog;
use canon_event_store::AnyEvent;
use canon_event_store::{extract_rustc_event, read_any_events_from_path, read_any_events_from_path_with_start_seq};
use canon_loop::LoopStageExecutor;
use canon_route::RouteExecutor;
use canon_runtime::bootstrap::{bootstrap_config, new_prompt_registry, prompts_dir, reload_prompt_file};
use canon_prompt_events::runtime_goal_prompt_loaded;
use canon_runtime::consumers::agent_registry::{AgentRegistryConsumer, AgentRegistryHandle};
use canon_runtime::consumers::analyst_consumer::AnalystConsumer;
use canon_runtime::consumers::capability_executor::CapabilityExecutor;
use canon_runtime::consumers::check_consumer::CheckConsumer;
use canon_runtime::consumers::diagnostics_consumer::DiagnosticsConsumer;
use canon_runtime::consumers::dispatch_consumer::DispatchConsumer;
use canon_runtime::consumers::error_logger::ErrorLogger;
use canon_runtime::consumers::goal_gen_consumer::GoalGenConsumer;
use canon_runtime::consumers::goal_graph_consumer::GoalGraphConsumer;
use canon_runtime::consumers::watchdog_consumer::WatchdogConsumer;
use canon_runtime::hooks::{AuditLogHook, CapabilityRateLimitHook, CostCapHook, HookChain};
use canon_runtime::{spawn_kernel_processor, EventRuntime, KernelMsg};
use crossbeam_channel as cc;
use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher};
use signal_hook::consts::signal::SIGHUP;
use signal_hook::iterator::Signals;
use std::collections::HashSet;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

// Goal-gen projects live under a dedicated subdir so cleaning does not wipe user test projects.
const GOALGEN_PROJECTS_DIR: &str = "/workspace/ai_sandbox/canon/test_projects/goalgen";
const AGENT_GOAL_PATH: &str = "/workspace/ai_sandbox/canon/canon-agent-prompts/AGENT_GOAL.md";
const CLEAN_GOALGEN_ON_START: bool = false;

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
    lock_contents.lines().find_map(|line| line.strip_prefix("pid=")).and_then(|value| value.trim().parse::<u32>().ok())
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
    let state = contents[close_paren + 1..].trim_start().chars().next().unwrap_or(' ');
    Ok(state != 'Z')
}

fn acquire_lock(path: &Path) -> Result<Option<LockGuard>> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => {
            let _ = file.write_all(format!("pid={}\n", std::process::id()).as_bytes());
            return Ok(Some(LockGuard { path: path.to_path_buf(), _file: file }));
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(err) => return Err(err.into()),
    }

    let mut contents = String::new();
    if let Ok(mut file) = File::open(path) {
        let _ = file.read_to_string(&mut contents);
    }
    let Some(pid) = parse_pid(&contents) else {
        eprintln!("[event_runtime] another instance is running (lock: {})", path.display());
        return Ok(None);
    };
    let alive = pid_is_alive(pid)?;
    if alive {
        eprintln!("[event_runtime] another instance is running (lock: {})", path.display());
        return Ok(None);
    }

    let _ = fs::remove_file(path);
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let _ = file.write_all(format!("pid={}\n", std::process::id()).as_bytes());
    Ok(Some(LockGuard { path: path.to_path_buf(), _file: file }))
}

fn clean_test_projects() {
    if CLEAN_GOALGEN_ON_START {
        let p = Path::new(GOALGEN_PROJECTS_DIR);
        let _ = fs::remove_dir_all(p);
        let _ = fs::create_dir_all(p);
    }
}

fn clear_agent_goal() {
    let _ = fs::write(AGENT_GOAL_PATH, "# goal-pending\n");
}

// ---------------------------------------------------------------------------
// Queue message type — P → Q_e / Q_c
//
// Producers push EventMsg into Q_e.
// W=1 (the main loop) is the sole receiver of both; it defines commit order and is
// the only writer to L (log/tlog).  Consumers (C ≥ 1) are driven from W
// via bus dispatch and track their own offsets; they never mutate L.
// ---------------------------------------------------------------------------

enum EventMsg {
    /// New inbound event delivered directly in memory — no filesystem poll.
    /// Produced by the notify-watcher thread (P2) and the bootstrap replayer (P1).
    Event(AnyEvent),
    /// Tlog was truncated/recreated (observed by P2).
    /// W must reset its state and replay the provided events from scratch
    /// to maintain deterministic order (Rule 10).
    Reset(Vec<AnyEvent>),
}


fn is_kernel_canon_event(event: &AnyEvent) -> bool {
    if let AnyEvent::Canon(canon) = event {
        extract_rustc_event(canon).is_some()
    } else {
        false
    }
}

// Routing is now event-driven via canon-route RouteExecutor. Routing state is managed
// internally by that consumer rather than here in the runtime binary.

// Observed-event routing state accumulation is handled inside canon-route.

fn handle_event_msg(
    msg: EventMsg, runtime: &mut EventRuntime, processed: &mut usize, cursor_path: &Path, tlog_path: &Path, start_seq: u64, session_id: &str, last_saved: &mut Instant,
    last_saved_processed: &mut usize,
) -> Result<()> {
    match msg {
        EventMsg::Event(event) => {
            runtime.process_events(std::slice::from_ref(&event))?;
            *processed = processed.saturating_add(1);
            if *processed != *last_saved_processed
                && last_saved.elapsed() >= Duration::from_secs(1)
                && save_cursor(cursor_path, tlog_path, *processed, start_seq, session_id, runtime.next_id()).is_ok()
            {
                *last_saved = Instant::now();
                *last_saved_processed = *processed;
            }
        }
        EventMsg::Reset(events) => {
            runtime.reset();
            runtime.process_events(&events)?;
            *processed = events.len();
        }
    }
    Ok(())
}


// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let mut tlog_path: Option<PathBuf> = None;
    let mut once = false;
    let cursor_path = env::var("CANON_EVENT_RUNTIME_CURSOR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/workspace/ai_sandbox/canon/state/event_runtime.cursor.json"));

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
    let event_execution_enabled = std::env::var("CANON_EVENT_EXECUTION").ok().map(|v| v != "0" && v.to_lowercase() != "false").unwrap_or(true);
    let lock_path = env::var("CANON_EVENT_RUNTIME_LOCK").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/workspace/ai_sandbox/canon/state/event_runtime.lock"));
    let _lock_guard = match acquire_lock(&lock_path)? {
        Some(guard) => guard,
        None => return Ok(()),
    };
    let system_id = load_or_create_system_id();

    if std::env::var("CANON_VERIFY_TLOG_EQUIV").ok().as_deref() == Some("1") {
        let _ = maybe_verify_tlog_equivalence(&tlog_path);
    }

    // Use the cursor's start_seq to skip reading old tlog segments.
    // Consumers rebuild from scratch on each run.
    let mut cursor_state = load_cursor_state(&cursor_path, &tlog_path);
    let latest_tlog_session_id = find_last_runtime_started_session_id(&tlog_path);
    if let (Some(cursor), Some(tlog_session_id)) = (&cursor_state, &latest_tlog_session_id) {
        if let Some(cursor_session_id) = &cursor.session_id {
            if cursor_session_id != tlog_session_id {
                eprintln!("[event_runtime] cursor session_id mismatch; ignoring stale cursor (cursor={} tlog={})", cursor_session_id, tlog_session_id);
                cursor_state = None;
            }
        }
    }
    let start_seq: u64 = cursor_state.as_ref().map(|c| c.start_seq).unwrap_or(0);
    let resumed_next_id: u64 = cursor_state.as_ref().map(|c| c.next_id).unwrap_or(0);
    let session_id = cursor_state.as_ref().and_then(|c| c.session_id.clone()).or(latest_tlog_session_id).unwrap_or_else(|| Uuid::new_v4().to_string());
    if let Some(schema_id) = find_last_runtime_started_schema_id(&tlog_path) {
        if schema_id != EVENT_SCHEMA_VERSION {
            eprintln!("[event_runtime] unsupported schema_id in tlog: {} (runtime supports {})", schema_id, EVENT_SCHEMA_VERSION);
        }
    }

    // --- Build runtime (W owns this exclusively) ---
    let prompt_registry = new_prompt_registry();
    clean_test_projects();
    clear_agent_goal();
    bootstrap_config(&tlog_path, &prompt_registry);

    let workspace = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

    // Hot-reload skills on SIGHUP.
    {
        let mut signals = Signals::new([SIGHUP]).expect("signals");
        std::thread::Builder::new()
            .name("canon_skill_reload".to_string())
            .spawn(move || {
                for _ in signals.forever() {
                    canon_skills::global_registry().invalidate_all();
                    eprintln!("[event_runtime] skill cache invalidated (SIGHUP)");
                }
            })
            .expect("skill reload thread");
    }

    let agent_registry = AgentRegistryHandle::default();
    let harness_mode = false;
    let mut consumers: Vec<Box<dyn canon_event::EventConsumer>> = vec![
        Box::new(AnalystConsumer::new(tlog_path.clone())),
        Box::new(LoopStageExecutor::new(workspace.clone(), tlog_path.clone()).with_agent_id("planner_chatgpt_group".to_string())),
        Box::new(RouteExecutor::new(workspace.clone())),
        Box::new(ErrorLogger::new(None)),
        Box::new(CheckConsumer::new()),
        Box::new(DiagnosticsConsumer::new()),
        Box::new(AgentRegistryConsumer::new(agent_registry.clone())),
        Box::new(DispatchConsumer::new()),
        Box::new(GoalGraphConsumer::new()),
        Box::new(WatchdogConsumer::new()),
    ];
    if !harness_mode {
        consumers.insert(0, Box::new(GoalGenConsumer::new(tlog_path.clone())));
    }
    // Goodness consumer logs metrics and emits GoodnessSnapshot on LoopVerified.
    let goodness_root = tlog_path.parent().map(|p| p.to_path_buf());
    consumers.push(Box::new(canon_goodness::GoodnessConsumer::new(goodness_root)));
    if event_execution_enabled {
        consumers.push(Box::new(CapabilityExecutor::new(workspace.clone())));
    }
    canon_exec::init_llm_worker();
    canon_exec::init_analysis_worker();
    canon_exec::init_bash_worker();
    let mut runtime = EventRuntime::new(consumers);
    // Hooks / middleware chain.
    if let Ok(cfg) = canon_llm::config::CapabilityConfig::snapshot_store_load() {
        let mut hooks = HookChain::new();
        hooks.add_pre(Box::new(CapabilityRateLimitHook::from_config(&cfg)));
        hooks.add_pre(Box::new(CostCapHook::from_config(&cfg)));
        hooks.add_post(Box::new(AuditLogHook::new()));
        runtime.set_hooks(std::sync::Arc::new(hooks));
    }
    runtime.set_execute_capabilities(false);
    // set_tlog_path tells W where to append (L).  Only W calls this; only W writes L.
    runtime.set_tlog_path(tlog_path.clone());
    runtime.set_next_id(resumed_next_id);

    // Emit AgentRegistered for each card in capability_config.toml so that
    // AgentRegistryConsumer and DispatchConsumer see agents before any work arrives.
    for payload in canon_runtime::bootstrap::load_agent_cards() {
        runtime.emit_event(canon_event::RuntimeEvent::AgentRegistered(canon_event::AgentRegistered { payload })).ok();
    }

    // Authoritative prompt loading happens through the runtime bus.
    {
        let goal_content = std::fs::read_to_string(AGENT_GOAL_PATH).unwrap_or_else(|_| "# goal-pending\n".to_string());
        runtime.emit_event(runtime_goal_prompt_loaded(&goal_content)).ok();
    }
    // Read events that already exist in L at startup (in-memory after this point).
    let bootstrap_events: Vec<AnyEvent> = if tlog_path.exists() { read_any_events_from_path_with_start_seq(&tlog_path, start_seq).unwrap_or_default() } else { vec![] };

    // Always start at tail — never replay into consumers.
    let mut processed: usize = bootstrap_events.len();

    // --- Once mode: W processes the current snapshot of L, then exits ---
    if once {
        if !tlog_path.exists() {
            return Err(anyhow!("tlog not found: {}", tlog_path.display()));
        }
        if processed < bootstrap_events.len() {
            runtime.process_events(&bootstrap_events[processed..])?;
        }
        processed = bootstrap_events.len();
        let _ = save_cursor(&cursor_path, &tlog_path, processed, start_seq, &session_id, runtime.next_id());
        canon_exec::shutdown_llm_worker();
        canon_exec::shutdown_analysis_worker();
        canon_exec::shutdown_bash_worker();
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
    // Q_e: event-plane queue (tlog events/replay/reset).
    let (q_event_tx, q_event_rx) = cc::unbounded::<EventMsg>();
    let (q_kernel_tx, q_kernel_rx) = cc::unbounded::<KernelMsg>();
    let event_budget_per_cycle = std::env::var("CANON_EVENT_RUNTIME_EVENT_BUDGET").ok().and_then(|v| v.parse::<usize>().ok()).filter(|v| *v > 0).unwrap_or(256);

    let kernel_emitter = runtime.emitter_handle();
    let _kernel_processor = spawn_kernel_processor(q_kernel_rx, kernel_emitter);

    // --- P1: bootstrap replayer ---
    // Unprocessed events already in memory — push directly into Q, no file re-read.
    for event in bootstrap_events.into_iter().skip(processed) {
        if is_kernel_canon_event(&event) {
            q_kernel_tx.send(KernelMsg::Event(event)).ok();
        } else {
            q_event_tx.send(EventMsg::Event(event)).ok();
        }
    }

    // --- P2: notify watcher ---
    // Uses OS-level inotify/kqueue to detect tlog changes.
    // On notification: reads new entries into memory, delivers each as EventMsg::Event
    // directly into Q.  Zero polling, zero sleep — events arrive in real time.
    // File is L (durable log); Q is the live in-memory delivery pipe.
    {
        let watcher_tlog = tlog_path.clone();
        let watcher_tx = q_event_tx.clone();
        let kernel_tx = q_kernel_tx.clone();
        let mut watcher_start_seq = start_seq;
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
        let watch_target = if watcher_tlog.is_dir() { watcher_tlog.clone() } else { watcher_tlog.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from(".")) };
        if watch_target.exists() {
            fs_watcher.watch(&watch_target, RecursiveMode::NonRecursive)?;
        }

        std::thread::Builder::new().name("canon-p2-watcher".to_string()).spawn(move || {
            let _watcher = fs_watcher; // keep alive for thread lifetime
            while let Ok(res) = fs_rx.recv() {
                if res.is_err() {
                    continue;
                }
                let all = match read_any_events_from_path_with_start_seq(&watcher_tlog, watcher_start_seq) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                if all.len() < watcher_seen {
                    // L was truncated or recreated — W must reset (Rule 10).
                    watcher_seen = 0;
                    if kernel_tx.send(KernelMsg::Reset).is_err() {
                        break;
                    }
                    let mut non_kernel = Vec::new();
                    for event in all {
                        if is_kernel_canon_event(&event) {
                            if kernel_tx.send(KernelMsg::Event(event)).is_err() {
                                break;
                            }
                        } else {
                            non_kernel.push(event);
                        }
                    }
                    if watcher_tx.send(EventMsg::Reset(non_kernel)).is_err() {
                        break;
                    }
                } else {
                    // Deliver only the new suffix: each event enters Q individually
                    // so W can interleave other message types between them.
                    for event in all.into_iter().skip(watcher_seen) {
                        watcher_seen += 1;
                        if is_kernel_canon_event(&event) {
                            if kernel_tx.send(KernelMsg::Event(event)).is_err() {
                                break;
                            }
                        } else if watcher_tx.send(EventMsg::Event(event)).is_err() {
                            break;
                        }
                    }
                    // Advance start_seq to the latest segment so future reads
                    // only scan the current segment instead of the full history.
                    if let Some(latest_seq) = latest_segment_seq(&watcher_tlog) {
                        if latest_seq > watcher_start_seq {
                            let from_latest = read_any_events_from_path_with_start_seq(&watcher_tlog, latest_seq).unwrap_or_default().len();
                            watcher_start_seq = latest_seq;
                            watcher_seen = from_latest;
                        }
                    }
                }
            }
        })?;
    }

    // P3 tick timer removed — system is purely event-driven.
    // Periodic ticks caused every consumer to re-observe on every heartbeat,
    // generating O(N_agents * ticks_per_sec) spurious loop_observed events.

    // --- P4: prompt-directory watcher ---
    // Watches canon-agent-prompts/ for .md file changes. On change: re-reads
    // the file, updates the in-memory PromptRegistry, and emits the
    // authoritative PromptLoaded runtime event through the bus.
    {
        let prompts_path = PathBuf::from(prompts_dir());
        let registry_for_prompts = prompt_registry.clone();
        let prompt_emitter = runtime.emitter_handle();

        if prompts_path.exists() {
            let (prompt_fs_tx, prompt_fs_rx) = cc::unbounded::<notify::Result<notify::Event>>();
            let mut prompt_watcher = RecommendedWatcher::new(
                move |res| {
                    let _ = prompt_fs_tx.send(res);
                },
                NotifyConfig::default(),
            )?;
            prompt_watcher.watch(&prompts_path, RecursiveMode::NonRecursive)?;

            std::thread::Builder::new().name("canon-p4-prompts".to_string()).spawn(move || {
                let _watcher = prompt_watcher;
                let mut last_reload: std::collections::HashMap<PathBuf, Instant> = std::collections::HashMap::new();
                while let Ok(Ok(event)) = prompt_fs_rx.recv() {
                    for path in &event.paths {
                        // Debounce: skip if same file reloaded within 500ms
                        let now = Instant::now();
                        if last_reload.get(path).map_or(false, |t| now.duration_since(*t) < Duration::from_millis(500)) {
                            continue;
                        }
                        last_reload.insert(path.clone(), now);
                        if let Some(prompt) = reload_prompt_file(path, &registry_for_prompts) {
                            prompt_emitter.emit_with_parents(canon_event::RuntimeEvent::PromptLoaded(prompt), vec![], file!(), line!());
                        }
                    }
                }
            })?;
        }
    }

    // Emit runtime_started so watch_log.py and the tlog show when a new
    // process begins. Written after P2 watcher_seen is fixed so P2 delivers it.
    runtime.emit_debug_event(
        "event-runtime".to_string(),
        "runtime_started".to_string(),
        serde_json::json!({
            "pid": std::process::id(),
            "tlog": tlog_path.display().to_string(),
            "event_stream_id": tlog_path.display().to_string(),
            "session_id": session_id.clone(),
            "schema_id": EVENT_SCHEMA_VERSION,
            "build_id": env!("CANON_BUILD_ID"),
            "commit_id": env!("CANON_COMMIT_ID"),
            "system_id": system_id,
        }),
    )?;
    if env!("CANON_COMMIT_ID").starts_with("unknown") {
        eprintln!("[event_runtime] warning: CANON_COMMIT_ID is unknown; build metadata is incomplete");
    }
    // =========================================================================
    // W = 1 — single writer loop (purely event-driven; no ticks)
    //
    // Each iteration:
    // 1. Drain emitter_rx — consumer threads (CapabilityExecutor, etc.) emit
    //    results (CapabilityCompleted, CapabilityFailed) here. Without this,
    //    async results would never reach the tlog or other consumers.
    // 2. Drain q_event_rx up to event_budget_per_cycle.
    // 3. If nothing was available, block with a short timeout so we don't spin,
    //    then loop back to drain emitter_rx again.
    // =========================================================================
    let mut last_saved = Instant::now();
    let mut last_saved_processed = processed;

    loop {
        // Step 1: drain any events emitted by consumer threads (e.g. CapabilityCompleted).
        // These sit in emitter_rx until W processes them; they do NOT arrive via P2/q_event_rx.
        runtime.flush_emitted_events()?;

        // Step 2: drain q_event_rx (tlog-sourced events from P2).
        let mut handled = 0usize;
        while handled < event_budget_per_cycle {
            match q_event_rx.try_recv() {
                Ok(event_msg) => {
                    handle_event_msg(event_msg, &mut runtime, &mut processed, &cursor_path, &tlog_path, start_seq, &session_id, &mut last_saved, &mut last_saved_processed)?;
                    handled = handled.saturating_add(1);
                }
                Err(cc::TryRecvError::Empty) => break,
                Err(cc::TryRecvError::Disconnected) => break,
            }
        }

        if handled > 0 {
            // More tlog events may have arrived; loop immediately.
            continue;
        }

        // Step 3: nothing in q_event_rx — wait briefly then loop back to drain emitter_rx.
        // This is the only "polling" in the system; it is not tick-driven and emits no events.
        match q_event_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(event_msg) => {
                handle_event_msg(event_msg, &mut runtime, &mut processed, &cursor_path, &tlog_path, start_seq, &session_id, &mut last_saved, &mut last_saved_processed)?;
            }
            Err(cc::RecvTimeoutError::Timeout) => {}
            Err(cc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Cursor helpers — track W's read offset into L for crash recovery / replay.
// ---------------------------------------------------------------------------

/// Returns the base sequence number of the latest `.log` segment in `dir`.
/// Used by the P2 watcher to advance `watcher_start_seq` after each batch so
/// that subsequent reads only scan the current segment rather than full history.
fn latest_segment_seq(dir: &Path) -> Option<u64> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("log") {
                return None;
            }
            p.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse::<u64>().ok())
        })
        .max()
}

struct CursorState {
    start_seq: u64,
    session_id: Option<String>,
    next_id: u64,
}

fn load_cursor_state(path: &Path, tlog_path: &Path) -> Option<CursorState> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let stored_path = value.get("tlog_path")?.as_str()?;
    if stored_path != tlog_path.display().to_string() {
        return None;
    }
    Some(CursorState {
        start_seq: value.get("start_seq")?.as_u64()?,
        session_id: value.get("session_id").and_then(|v| v.as_str()).map(|s| s.to_string()),
        next_id: value.get("next_id").and_then(|v| v.as_u64()).unwrap_or(0),
    })
}

fn save_cursor(path: &Path, tlog_path: &Path, state_version: usize, start_seq: u64, session_id: &str, next_id: u64) -> Result<()> {
    let state = serde_json::json!({
        "tlog_path": tlog_path.display().to_string(),
        "state_version": state_version,
        "processed": state_version,
        "start_seq": start_seq,
        "session_id": session_id,
        "next_id": next_id,
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

fn load_or_create_system_id() -> String {
    if let Ok(system_id) = std::env::var("CANON_SYSTEM_ID") {
        if !system_id.trim().is_empty() {
            return system_id;
        }
    }
    let path = system_id_path();
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let system_id = Uuid::new_v4().to_string();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, &system_id);
    system_id
}

fn system_id_path() -> PathBuf {
    if let Ok(path) = std::env::var("CANON_SYSTEM_ID_PATH") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    if let Ok(workspace) = std::env::var("CANON_WORKSPACE") {
        let trimmed = workspace.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("state").join("system_id");
        }
    }
    PathBuf::from("/workspace/ai_sandbox/canon/state/system_id")
}

fn find_last_runtime_started_session_id(tlog_path: &Path) -> Option<String> {
    find_last_runtime_started_payload(tlog_path).and_then(|payload| payload.get("session_id").and_then(|v| v.as_str()).map(|s| s.to_string()))
}

fn find_last_runtime_started_schema_id(tlog_path: &Path) -> Option<String> {
    find_last_runtime_started_payload(tlog_path).and_then(|payload| payload.get("schema_id").and_then(|v| v.as_str()).map(|s| s.to_string()))
}

fn find_last_runtime_started_payload(tlog_path: &Path) -> Option<serde_json::Value> {
    let events = read_any_events_from_path(tlog_path).ok()?;
    events.into_iter().rev().find_map(|event| {
        let AnyEvent::Canon(canon) = event else {
            return None;
        };
        if canon.kind != canon_event::EventKind::RuntimeStarted {
            return None;
        }
        Some(canon.payload.data.clone())
    })
}

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()
}

// ---------------------------------------------------------------------------
// Tlog equivalence verification (optional, debug mode only)
// ---------------------------------------------------------------------------

fn maybe_verify_tlog_equivalence(tlog_path: &Path) -> Result<()> {
    let (json_path, bin_path) = if tlog_path.is_dir() { (tlog_path.with_extension("tlog"), tlog_path.to_path_buf()) } else { (tlog_path.to_path_buf(), tlog_path.with_extension("tlog.d")) };
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
        diffs.push(format!("node_count json={} binary={}", json_graph.nodes.len(), bin_graph.nodes.len()));
    }
    if json_graph.edges.len() != bin_graph.edges.len() {
        diffs.push(format!("edge_count json={} binary={}", json_graph.edges.len(), bin_graph.edges.len()));
    }
    let json_nodes: HashSet<(u32, String, String, Option<u32>, Option<u32>)> = json_graph.nodes.iter().map(|n| (n.id, n.kind.clone(), n.symbol.clone(), n.file_id, n.line)).collect();
    let bin_nodes: HashSet<(u32, String, String, Option<u32>, Option<u32>)> = bin_graph.nodes.iter().map(|n| (n.id, n.kind.clone(), n.symbol.clone(), n.file_id, n.line)).collect();
    if json_nodes != bin_nodes {
        diffs.push(format!("node_set mismatch: json_only={} binary_only={}", json_nodes.difference(&bin_nodes).count(), bin_nodes.difference(&json_nodes).count()));
    }
    let json_edges: HashSet<(u32, u32, String)> = json_graph.edges.iter().map(|e| (e.src, e.dst, e.kind.clone())).collect();
    let bin_edges: HashSet<(u32, u32, String)> = bin_graph.edges.iter().map(|e| (e.src, e.dst, e.kind.clone())).collect();
    if json_edges != bin_edges {
        diffs.push(format!("edge_set mismatch: json_only={} binary_only={}", json_edges.difference(&bin_edges).count(), bin_edges.difference(&json_edges).count()));
    }
    Ok(diffs)
}
