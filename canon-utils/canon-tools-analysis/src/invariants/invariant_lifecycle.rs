use std::collections::HashMap;
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvariantStatus {
    Candidate,
    Promoted,
    Demoted,
    HardBanned,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_promotes_on_support() {
        let mut lc = InvariantLifecycle::new();
        lc.record_support(1);
        lc.tick(1);
        assert!(lc.promoted_invariants().iter().any(|e| e.fingerprint == 1));
    }

    #[test]
    fn promoted_demotes_on_violation() {
        let mut lc = InvariantLifecycle::new();
        lc.record_support(2);
        lc.tick(1);
        lc.record_violation(2);
        lc.tick(2);
        assert!(lc.promoted_invariants().iter().all(|e| e.fingerprint != 2));
    }

    #[test]
    fn hard_ban_after_six_violations() {
        let mut lc = InvariantLifecycle::new();
        lc.record_support(3);
        lc.tick(1);
        for _ in 0..6 { lc.record_violation(3); }
        lc.tick(2);
        let entry = lc.entries().find(|e| e.fingerprint == 3);
        assert!(entry.map(|e| matches!(e.status, InvariantStatus::HardBanned)).unwrap_or(false));
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InvariantEntry {
    pub fingerprint: u64,
    pub description: String,
    pub status: InvariantStatus,
    pub support_samples: usize,
    pub violation_samples: usize,
    pub last_updated_epoch: u64,
}

pub struct InvariantLifecycle {
    pub entries: HashMap<u64, InvariantEntry>,
}

impl InvariantLifecycle {
    pub fn new() -> Self {
        Self { entries: HashMap::new() }
    }

    pub fn record_support(&mut self, fp: u64) {
        let entry = self.entries.entry(fp).or_insert(InvariantEntry {
            fingerprint: fp,
            description: String::new(),
            status: InvariantStatus::Candidate,
            support_samples: 0,
            violation_samples: 0,
            last_updated_epoch: 0,
        });
        entry.support_samples += 1;
    }

    pub fn record_violation(&mut self, fp: u64) {
        if let Some(entry) = self.entries.get_mut(&fp) {
            entry.violation_samples += 1;
        }
    }

    pub fn tick(&mut self, epoch: u64) -> Vec<(u64, InvariantStatus)> {
        let mut changes = Vec::new();

        for (fp, entry) in self.entries.iter_mut() {
            entry.last_updated_epoch = epoch;

            match entry.status {
                InvariantStatus::Candidate => {
                    if entry.violation_samples >= 6 {
                        entry.status = InvariantStatus::HardBanned;
                        changes.push((*fp, InvariantStatus::HardBanned));
                    } else if entry.support_samples > 0 && entry.violation_samples == 0 {
                        entry.status = InvariantStatus::Promoted;
                        changes.push((*fp, InvariantStatus::Promoted));
                    }
                }
                InvariantStatus::Promoted => {
                    if entry.violation_samples >= 6 {
                        entry.status = InvariantStatus::HardBanned;
                        changes.push((*fp, InvariantStatus::HardBanned));
                    } else if entry.violation_samples > 0 {
                        entry.status = InvariantStatus::Demoted;
                        changes.push((*fp, InvariantStatus::Demoted));
                    }
                }
                InvariantStatus::Demoted => {
                    if entry.violation_samples >= 6 {
                        entry.status = InvariantStatus::HardBanned;
                        changes.push((*fp, InvariantStatus::HardBanned));
                    }
                }
                InvariantStatus::HardBanned => {}
            }
        }

        changes
    }

    pub fn promoted_invariants(&self) -> Vec<&InvariantEntry> {
        self.entries.values().filter(|e| matches!(e.status, InvariantStatus::Promoted)).collect()
    }

    pub fn entries(&self) -> impl Iterator<Item = &InvariantEntry> {
        self.entries.values()
    }
}
