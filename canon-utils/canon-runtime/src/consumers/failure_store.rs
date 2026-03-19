use canon_agent::failure_store::FailureStore;
use canon_event::{CanonEvent, EventConsumer, EventEmitterHandle, EventFilter};
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
