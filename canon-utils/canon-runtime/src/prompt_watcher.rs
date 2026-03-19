use canon_event::canon_emit;
use notify::{Config as NotifyConfig, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

const DEFAULT_PROMPT_PATH: &str = "/workspace/ai_sandbox/canon/canon-agent-prompts/AGENT_GOAL.md";

pub struct PromptWatcher {
    _watcher: RecommendedWatcher,
}

impl PromptWatcher {
    pub fn start(tlog_path: &Path) -> anyhow::Result<Self> {
        let prompt_path = prompt_path_from_env();
        if !prompt_path.exists() {
            return Ok(Self::empty());
        }
        let (tx, rx) = mpsc::channel();
        let mut watcher = RecommendedWatcher::new(
            move |res: notify::Result<Event>| {
                let _ = tx.send(res);
            },
            NotifyConfig::default(),
        )?;
        watcher.watch(&prompt_path, RecursiveMode::NonRecursive)?;

        let tlog = tlog_path.to_path_buf();
        std::thread::spawn(move || {
            let mut last_hash: Option<String> = None;
            let _ = emit_prompt_if_changed(&prompt_path, &tlog, &mut last_hash);
            let _ = event_loop(&rx, &prompt_path, &tlog, &mut last_hash);
        });

        Ok(Self { _watcher: watcher })
    }

    fn empty() -> Self {
        let (_tx, _rx) = mpsc::channel::<notify::Result<Event>>();
        let watcher = RecommendedWatcher::new(|_res| {}, NotifyConfig::default())
            .expect("failed to init watcher");
        Self { _watcher: watcher }
    }
}

fn prompt_path_from_env() -> PathBuf {
    if let Ok(path) = std::env::var("CANON_AGENT_GOAL_PATH") {
        return PathBuf::from(path);
    }
    if let Ok(dir) = std::env::var("CANON_PROMPTS_DIR") {
        return PathBuf::from(dir).join("AGENT_GOAL.md");
    }
    PathBuf::from(DEFAULT_PROMPT_PATH)
}

fn event_loop(
    rx: &Receiver<notify::Result<Event>>,
    prompt_path: &Path,
    tlog_path: &Path,
    last_hash: &mut Option<String>,
) -> anyhow::Result<()> {
    let mut last_emit = Instant::now() - Duration::from_secs(10);
    loop {
        let evt = match rx.recv() {
            Ok(evt) => evt,
            Err(_) => break,
        };
        let Ok(event) = evt else { continue };
        if !is_relevant(&event.kind) {
            continue;
        }
        if last_emit.elapsed() < Duration::from_millis(200) {
            continue;
        }
        if emit_prompt_if_changed(prompt_path, tlog_path, last_hash)? {
            last_emit = Instant::now();
        }
    }
    Ok(())
}

fn is_relevant(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Modify(_)
            | EventKind::Create(_)
            | EventKind::Remove(_)
            | EventKind::Any
    )
}

fn emit_prompt_if_changed(
    prompt_path: &Path,
    tlog_path: &Path,
    last_hash: &mut Option<String>,
) -> anyhow::Result<bool> {
    let content = std::fs::read_to_string(prompt_path)?;
    let hash = content_hash(&content);
    if last_hash.as_deref() == Some(hash.as_str()) {
        return Ok(false);
    }
    *last_hash = Some(hash.clone());
    let filename = prompt_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("AGENT_GOAL.md");
    let payload = serde_json::json!({
        "prompt_id": "AGENT_GOAL",
        "path": filename,
        "hash": hash,
        "content": content,
    });
    let _ = canon_emit!("prompt_watcher", "prompt_loaded", payload, tlog_path);
    Ok(true)
}

fn content_hash(s: &str) -> String {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
