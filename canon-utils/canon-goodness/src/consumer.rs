use canon_event::{canon_emit, EventConsumer, EventEmitterHandle, EventFilter, RuntimeEvent};

use crate::{aggregator::{compute_g, compute_reward}, reducers::AllReducers, MetricsStorage};

pub struct GoodnessConsumer {
    reducers: AllReducers,
    g_prev: f32,
    storage: Option<MetricsStorage>,
    emitter: Option<EventEmitterHandle>,
}

impl GoodnessConsumer {
    pub fn new(storage_root: Option<std::path::PathBuf>) -> Self {
        Self {
            reducers: AllReducers::new(),
            g_prev: 1.0,
            storage: storage_root.map(|p| MetricsStorage::new(&p)),
            emitter: None,
        }
    }

    pub fn latest_g(&self) -> f32 {
        self.g_prev
    }
}

impl EventConsumer for GoodnessConsumer {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.emitter = Some(emitter);
    }

    fn on_event(&mut self, event: &RuntimeEvent) {
        self.reducers.update_all(event);

        if let RuntimeEvent::LoopVerified(v) = event {
            let metrics = self.reducers.snapshot();
            let g_now = compute_g(&metrics);
            let delta = compute_reward(g_now, self.g_prev);
            self.g_prev = g_now;

            if let Some(store) = &self.storage {
                store.append_metrics(v.tick, &metrics);
                store.append_goodness(v.tick, g_now, delta);
            }

            if let Some(emitter) = &self.emitter {
                canon_emit!(emitter; GoodnessSnapshot(canon_event::GoodnessSnapshot {
                    tick: v.tick,
                    g: g_now,
                    delta_g: delta,
                    metrics: serde_json::to_value(&metrics).unwrap_or_default(),
                }));
            }
        }
    }
}
