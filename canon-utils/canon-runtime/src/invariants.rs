use canon_event::{CanonEvent, EventEmitterHandle, RuntimeEvent};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Clone, Copy)]
struct Stat {
    zero_count: u64,
    total: u64,
}

impl Stat {
    fn update(&mut self, delta_is_zero: bool) {
        if delta_is_zero {
            self.zero_count = self.zero_count.saturating_add(1);
        }
        self.total = self.total.saturating_add(1);
    }

    fn confidence(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.zero_count as f64 / self.total as f64
        }
    }
}

#[derive(Clone, Copy)]
struct Feature {
    name: &'static str,
    extractor: fn(&CanonEvent) -> i64,
}

fn len_parent_ids(ev: &CanonEvent) -> i64 {
    ev.parent_ids.len() as i64
}

fn delta_size(ev: &CanonEvent) -> i64 {
    ev.payload.delta.to_string().len() as i64
}

fn kind_hash(ev: &CanonEvent) -> i64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    ev.kind.hash(&mut h);
    h.finish() as i64
}

fn actor_hash(ev: &CanonEvent) -> i64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    ev.actor.hash(&mut h);
    h.finish() as i64
}

#[derive(Clone)]
struct Config {
    theta: f64,
    min_support: u64,
    enforce: bool,
    max_history: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self { theta: 0.98, min_support: 50, enforce: false, max_history: 2048 }
    }
}

impl Config {
    fn from_env() -> Self {
        let mut cfg = Self::default();
        if let Ok(v) = std::env::var("CANON_INVARIANT_THETA") {
            if let Ok(f) = v.parse::<f64>() {
                cfg.theta = f.clamp(0.0, 1.0);
            }
        }
        if let Ok(v) = std::env::var("CANON_INVARIANT_SUPPORT") {
            if let Ok(u) = v.parse::<u64>() {
                cfg.min_support = u;
            }
        }
        if let Ok(v) = std::env::var("CANON_INVARIANT_ENFORCE") {
            let on = matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "on");
            cfg.enforce = on;
        }
        if let Ok(v) = std::env::var("CANON_INVARIANT_HISTORY") {
            if let Ok(u) = v.parse::<usize>() {
                cfg.max_history = u.max(64).min(16_384);
            }
        }
        cfg
    }
}

pub struct InvariantEngine {
    features: Vec<Feature>,
    prev: VecDeque<CanonEvent>,
    stats: HashMap<&'static str, Stat>,
    invariants: HashSet<&'static str>,
    cfg: Config,
}

impl InvariantEngine {
    pub fn new() -> Self {
        let features = vec![
            Feature { name: "parent_count", extractor: len_parent_ids },
            Feature { name: "delta_size", extractor: delta_size },
            Feature { name: "kind_hash", extractor: kind_hash },
            Feature { name: "actor_hash", extractor: actor_hash },
        ];
        Self { features, prev: VecDeque::new(), stats: HashMap::new(), invariants: HashSet::new(), cfg: Config::from_env() }
    }

    fn record(&mut self, feature: &'static str, delta_zero: bool, emitter: &EventEmitterHandle, parent: &canon_event::EventId) -> bool {
        let entry = self.stats.entry(feature).or_insert(Stat { zero_count: 0, total: 0 });
        entry.update(delta_zero);
        let after = entry.confidence();
        if after >= self.cfg.theta && entry.total >= self.cfg.min_support && !self.invariants.contains(feature) {
            self.invariants.insert(feature);
            let event = RuntimeEvent::InvariantDiscovered(canon_event::InvariantDiscovered { feature: feature.to_string(), confidence: after, support: entry.total });
            emitter.emit_child(event, vec![parent.clone()], file!(), line!());
            return true;
        }
        if self.cfg.enforce && self.invariants.contains(feature) && !delta_zero {
            let err = RuntimeEvent::ErrorOccurred(canon_event::new_error_occurred(
                "invariant_violation",
                "invariant-engine",
                format!("invariant {} violated", feature),
                "error",
                serde_json::json!({ "feature": feature, "delta_zero": delta_zero, "support": entry.total }),
                None,
            ));
            emitter.emit_child(err, vec![parent.clone()], file!(), line!());
            return false;
        }
        true
    }

    pub fn observe(&mut self, event: &CanonEvent, emitter: &EventEmitterHandle) -> bool {
        let mut ok = true;
        if let Some(prev) = self.prev.back().cloned() {
            let features = self.features.clone();
            for feature in features {
                let curr = (feature.extractor)(event);
                let before = (feature.extractor)(&prev);
                let delta_zero = curr == before;
                ok = ok && self.record(feature.name, delta_zero, emitter, &event.id);
            }
        }
        self.prev.push_back(event.clone());
        if self.prev.len() > self.cfg.max_history {
            self.prev.pop_front();
        }
        ok
    }

    /// Enforce invariants at write-time (pre-append gate)
    pub fn validate_before_append(
        &self,
        event: &RuntimeEvent,
        parent_ids: &Vec<canon_event::EventId>,
    ) -> Result<(), String> {
        // Invariant 11: Payload must exist (basic structural check via debug repr)
        let kind = canon_event::event_kind_str(event);
        if kind.is_empty() {
            return Err("invalid_event_kind".to_string());
        }

        // Invariant 3: Non-root events must have parents.
        // Lawful cycle-seed root events originate at runtime and therefore start without parents.
        let lawful_root_seed = matches!(event, RuntimeEvent::Tick(_));
        if kind != "root" && !lawful_root_seed && parent_ids.is_empty() {
            return Err("missing_parent_ids".to_string());
        }

        // Invariant 7/19: effect events must have a control parent (approximation)
        if kind == "effect" && parent_ids.is_empty() {
            return Err("orphan_effect_event".to_string());
        }

        // NOTE: learned statistical invariants (self.invariants) are enforced in observe()
        // This function enforces structural + hard invariants required by SPEC

        Ok(())
    }
}
