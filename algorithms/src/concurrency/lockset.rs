//! Lockset-based race detection (Eraser-style).

use std::collections::{HashMap, HashSet};

pub type LockId = String;
pub type VarId = String;
pub type ThreadId = String;

#[derive(Default)]
pub struct LocksetState {
    /// thread -> current held locks
    pub held: HashMap<ThreadId, HashSet<LockId>>,
    /// var -> set of (thread, lockset) access records
    pub accesses: HashMap<VarId, Vec<(ThreadId, HashSet<LockId>)>>,
}

impl LocksetState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn acquire(&mut self, thread: &str, lock: LockId) {
        self.held.entry(thread.to_string()).or_default().insert(lock);
    }

    pub fn release(&mut self, thread: &str, lock: &str) {
        if let Some(ls) = self.held.get_mut(thread) {
            ls.remove(lock);
        }
    }

    pub fn record_access(&mut self, thread: &str, var: VarId) {
        let ls = self.held.get(thread).cloned().unwrap_or_default();
        self.accesses.entry(var).or_default().push((thread.to_string(), ls));
    }

    /// Returns variables with potential data races.
    pub fn races(&self) -> Vec<VarId> {
        self.accesses
            .iter()
            .filter(|(_, records)| {
                for i in 0..records.len() {
                    for j in (i + 1)..records.len() {
                        let (t1, ls1) = &records[i];
                        let (t2, ls2) = &records[j];
                        if t1 != t2 && ls1.intersection(ls2).next().is_none() {
                            return true;
                        }
                    }
                }
                false
            })
            .map(|(v, _)| v.clone())
            .collect()
    }
}
