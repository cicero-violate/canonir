use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use canon_event::{new_error_occurred, EventOutcome, RuntimeEvent};
use crossbeam_channel::Sender;

#[derive(Debug, Clone)]
pub enum HookDecision {
    Allow,
    Deny { reason: String },
    Mutate { replacement: RuntimeEvent },
}

pub trait PreHook: Send + Sync {
    fn name(&self) -> &'static str;
    fn on_pre(&self, event: &RuntimeEvent) -> HookDecision;
}

pub trait PostHook: Send + Sync {
    fn name(&self) -> &'static str;
    fn on_post(&self, event: &RuntimeEvent, outcome: &EventOutcome);
}

#[derive(Default)]
pub struct HookChain {
    pre: Vec<Box<dyn PreHook>>,
    post: Vec<Box<dyn PostHook>>,
}

impl HookChain {
    pub fn new() -> Self {
        Self { pre: Vec::new(), post: Vec::new() }
    }
    pub fn add_pre(&mut self, h: Box<dyn PreHook>) {
        self.pre.push(h);
    }
    pub fn add_post(&mut self, h: Box<dyn PostHook>) {
        self.post.push(h);
    }

    pub fn run_pre(&self, event: &RuntimeEvent) -> HookDecision {
        for hook in &self.pre {
            match hook.on_pre(event) {
                HookDecision::Allow => continue,
                other => return other,
            }
        }
        HookDecision::Allow
    }

    pub fn run_post(&self, event: &RuntimeEvent, outcome: &EventOutcome) {
        for hook in &self.post {
            hook.on_post(event, outcome);
        }
    }
}

// ---------------- Rate limit hook ----------------

struct TokenBucket {
    tokens: f64,
    last: Instant,
    rate_per_sec: f64,
    burst: f64,
}

impl TokenBucket {
    fn new(rate: u32) -> Self {
        let now = Instant::now();
        let r = rate.max(1) as f64;
        Self { tokens: r, last: now, rate_per_sec: r, burst: r }
    }
    fn allow(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + elapsed * self.rate_per_sec).min(self.burst);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

pub struct CapabilityRateLimitHook {
    buckets: Mutex<HashMap<String, TokenBucket>>,
    max_per_sec: u32,
}

impl CapabilityRateLimitHook {
    pub fn from_config(_cfg: &canon_llm::config::CapabilityConfig) -> Self {
        Self { buckets: Mutex::new(HashMap::new()), max_per_sec: 100 }
    }
}

impl PreHook for CapabilityRateLimitHook {
    fn name(&self) -> &'static str {
        "capability_rate_limit"
    }
    fn on_pre(&self, event: &RuntimeEvent) -> HookDecision {
        let RuntimeEvent::CapabilityInvoked(cap) = event else {
            return HookDecision::Allow;
        };
        let mut guard = self.buckets.lock().unwrap();
        let bucket = guard.entry(cap.capability.to_string()).or_insert_with(|| TokenBucket::new(self.max_per_sec));
        if bucket.allow() {
            HookDecision::Allow
        } else {
            HookDecision::Deny { reason: format!("rate_limit:{}", cap.capability) }
        }
    }
}

// ---------------- Cost cap hook ----------------

pub struct CostCapHook {
    max_turns: u64,
    used: std::sync::atomic::AtomicU64,
}

impl CostCapHook {
    pub fn from_config(_cfg: &canon_llm::config::CapabilityConfig) -> Self {
        Self { max_turns: 500, used: std::sync::atomic::AtomicU64::new(0) }
    }
}

impl PreHook for CostCapHook {
    fn name(&self) -> &'static str {
        "cost_cap"
    }
    fn on_pre(&self, event: &RuntimeEvent) -> HookDecision {
        if !matches!(event, RuntimeEvent::Llm(_)) {
            return HookDecision::Allow;
        }
        let next = self.used.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        if next > self.max_turns {
            HookDecision::Deny { reason: "llm_cost_cap_reached".to_string() }
        } else {
            HookDecision::Allow
        }
    }
}

// ---------------- Audit log post-hook ----------------

pub struct AuditLogHook {
    tx: Sender<(String, String)>,
}

impl AuditLogHook {
    pub fn new() -> Self {
        let (tx, rx) = crossbeam_channel::unbounded::<(String, String)>();
        std::thread::Builder::new()
            .name("audit_log_hook".into())
            .spawn(move || {
                let path = std::path::PathBuf::from("/workspace/ai_sandbox/canon/state/audit.log");
                let _ = std::fs::create_dir_all(path.parent().unwrap_or_else(|| std::path::Path::new(".")));
                while let Ok((kind, outcome)) = rx.recv() {
                    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
                    let line = serde_json::json!({ "ts": ts, "event_kind": kind, "outcome": outcome }).to_string();
                    let _ = std::fs::OpenOptions::new().create(true).append(true).open(&path).and_then(|mut f| {
                        use std::io::Write;
                        writeln!(f, "{line}")
                    });
                }
            })
            .expect("audit log thread");
        Self { tx }
    }
}

impl PostHook for AuditLogHook {
    fn name(&self) -> &'static str {
        "audit_log"
    }
    fn on_post(&self, event: &RuntimeEvent, outcome: &EventOutcome) {
        let kind = format!("{event:?}");
        let outcome_kind = match outcome {
            EventOutcome::Emit { .. } => "emit",
            EventOutcome::EmitMany { .. } => "emit_many",
            EventOutcome::NoOp(_) => "noop",
            EventOutcome::Error { .. } => "error",
        };
        let _ = self.tx.send((kind, outcome_kind.to_string()));
    }
}

// ---------------- Watchdog pre-hook ----------------

pub struct WatchdogPreHook {
    last_stage: Mutex<HashMap<&'static str, u64>>,
    current_tick: std::sync::atomic::AtomicU64,
}

const WD_THRESHOLDS: &[(&str, u64)] = &[("observed", 10), ("planned", 15), ("acted", 15), ("verified", 20), ("rewarded", 25)];

impl WatchdogPreHook {
    pub fn new() -> Self {
        let mut map = HashMap::new();
        for (s, _) in WD_THRESHOLDS {
            map.insert(*s, 0u64);
        }
        Self { last_stage: Mutex::new(map), current_tick: std::sync::atomic::AtomicU64::new(0) }
    }
}

impl PreHook for WatchdogPreHook {
    fn name(&self) -> &'static str {
        "watchdog_pre"
    }
    fn on_pre(&self, event: &RuntimeEvent) -> HookDecision {
        match event {
            RuntimeEvent::Tick(t) => {
                self.current_tick.store(t.tick, std::sync::atomic::Ordering::SeqCst);
                let now = t.tick;
                let stalled: Vec<(String, u64)> = {
                    let guard = self.last_stage.lock().unwrap();
                    WD_THRESHOLDS
                        .iter()
                        .filter_map(|(stage, thr)| {
                            let last = guard.get(stage).copied().unwrap_or(0);
                            let idle = now.saturating_sub(last);
                            if idle >= *thr {
                                Some((stage.to_string(), idle))
                            } else {
                                None
                            }
                        })
                        .collect()
                };
                if stalled.is_empty() {
                    HookDecision::Allow
                } else {
                    let msg = stalled.iter().map(|(s, idle)| format!("{s}:{idle}")).collect::<Vec<_>>().join(",");
                    HookDecision::Deny { reason: format!("watchdog_stall:{msg}") }
                }
            }
            RuntimeEvent::LoopObserved(_) => {
                self.last_stage.lock().unwrap().insert("observed", self.current_tick.load(std::sync::atomic::Ordering::SeqCst));
                HookDecision::Allow
            }
            RuntimeEvent::LoopPlanned(_) => {
                self.last_stage.lock().unwrap().insert("planned", self.current_tick.load(std::sync::atomic::Ordering::SeqCst));
                HookDecision::Allow
            }
            RuntimeEvent::LoopActed(_) => {
                self.last_stage.lock().unwrap().insert("acted", self.current_tick.load(std::sync::atomic::Ordering::SeqCst));
                HookDecision::Allow
            }
            RuntimeEvent::LoopVerified(_) => {
                self.last_stage.lock().unwrap().insert("verified", self.current_tick.load(std::sync::atomic::Ordering::SeqCst));
                HookDecision::Allow
            }
            RuntimeEvent::VerifierPolicyUpdated(_) => HookDecision::Allow,
            RuntimeEvent::LoopRewarded(_) => {
                self.last_stage.lock().unwrap().insert("rewarded", self.current_tick.load(std::sync::atomic::Ordering::SeqCst));
                HookDecision::Allow
            }
            _ => HookDecision::Allow,
        }
    }
}

// Helper for ErrorOccurred emission
pub fn hook_denied_event(reason: &str) -> RuntimeEvent {
    RuntimeEvent::ErrorOccurred(new_error_occurred("hook_denied", "hook_chain", reason, "error", serde_json::json!({ "reason": reason }), None))
}
