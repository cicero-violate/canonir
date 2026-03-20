use canon_event::{CanonEvent, EventConsumer, EventEmitterHandle, EventFilter, RustcEvent};
use serde::{Deserialize, Serialize};
use std::fs::{create_dir_all, File};
use std::io::{Seek, Write};
use std::path::PathBuf;
use std::sync::Mutex;

pub struct FailureStoreConsumer {
    store: FailureStore,
    file: Mutex<File>,
}

impl FailureStoreConsumer {
    pub fn new(path: Option<PathBuf>) -> Self {
        let path = path.unwrap_or_else(resolve_failure_store_path);
        if let Some(parent) = path.parent() {
            let _ = create_dir_all(parent);
        }
        let file = File::create(&path).expect("failure store open");
        Self {
            store: FailureStore::new(),
            file: Mutex::new(file),
        }
    }

    fn persist(&self) {
        if let Ok(mut guard) = self.file.lock() {
            if let Ok(payload) = serde_json::to_string_pretty(self.store.stats()) {
                let _ = guard.rewind();
                let _ = guard.set_len(0);
                let _ = guard.write_all(payload.as_bytes());
                let _ = guard.write_all(b"\n");
                let _ = guard.sync_data();
            }
        }
    }
}

impl EventConsumer for FailureStoreConsumer {
    fn filter(&self) -> EventFilter {
        EventFilter::ErrorOnly
    }

    fn on_event(&mut self, event: &CanonEvent) {
        self.store.record_event(event);
        self.persist();
    }

    fn set_emitter(&mut self, _emitter: EventEmitterHandle) {}
}

fn resolve_failure_store_path() -> PathBuf {
    std::env::var("CANON_FAILURE_STORE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/workspace/ai_sandbox/canon/state/failure_store.json"))
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct FailureStoreFailureStats {
    total: usize,
    cycle: usize,
    deadlock: usize,
    failure_pattern_rate: f64,
    cycle_frequency: f64,
    deadlock_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct FailureStore {
    stats: FailureStoreFailureStats,
}

impl FailureStore {
    fn new() -> Self {
        Self::default()
    }

    fn record_event(&mut self, event: &CanonEvent) {
        match event {
            CanonEvent::ErrorOccurred(payload) => {
                self.record_message(&payload.kind, &payload.message);
            }
            CanonEvent::CapabilityFailed(payload) => {
                self.record_message("capability_failed", &payload.error);
            }
            CanonEvent::NodeFailed(payload) => {
                let message = payload.error.as_deref().unwrap_or("node_failed");
                self.record_message("node_failed", message);
            }
            CanonEvent::LoopActed(payload) if !payload.success => {
                self.record_message("loop_acted_failed", &payload.stderr);
            }
            CanonEvent::LoopVerified(payload) if !payload.passed => {
                let msg = payload.diagnostics.join("; ");
                self.record_message("loop_verified_failed", &msg);
            }
            CanonEvent::LoopRewarded(payload) if payload.halt => {
                self.record_message("loop_rewarded_halt", "stagnant:halt");
            }
            CanonEvent::Code(code) => match &code.delta.event {
                RustcEvent::PanicCaptured(payload) => {
                    self.record_message("panic_captured", &payload.message);
                }
                RustcEvent::InvariantViolation(payload) => {
                    self.record_message("invariant_violation", &payload.message);
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn stats(&self) -> &FailureStoreFailureStats {
        &self.stats
    }

    fn record_message(&mut self, kind: &str, message: &str) {
        self.stats.total += 1;
        let is_cycle = kind.contains("cycle") || message.to_lowercase().contains("cycle");
        let is_deadlock =
            kind.contains("deadlock") || message.to_lowercase().contains("deadlock");
        if is_cycle {
            self.stats.cycle += 1;
        }
        if is_deadlock {
            self.stats.deadlock += 1;
        }
        let total = self.stats.total.max(1) as f64;
        let cycle = self.stats.cycle as f64;
        let deadlock = self.stats.deadlock as f64;
        self.stats.failure_pattern_rate = (cycle + deadlock) / total;
        self.stats.cycle_frequency = cycle / total;
        self.stats.deadlock_rate = deadlock / total;
    }
}
