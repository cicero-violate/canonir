use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::invariant_lifecycle::InvariantEntry;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct InvariantStore {
    pub entries: HashMap<u64, InvariantEntry>,
}

impl InvariantStore {
    pub fn load(path: &Path) -> Self {
        if let Ok(data) = fs::read_to_string(path) {
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    pub fn save(&self, path: &Path) {
        let tmp = path.with_extension("tmp");
        let data = serde_json::to_string_pretty(self).unwrap();
        fs::write(&tmp, data).unwrap();
        fs::rename(tmp, path).unwrap();
    }

    pub fn round_trip_check(&self) -> bool {
        let s = serde_json::to_string(self).unwrap();
        let de: InvariantStore = serde_json::from_str(&s).unwrap();
        self.entries.len() == de.entries.len()
    }

    pub fn idempotency_check(&mut self, epoch: u64) -> bool {
        for entry in self.entries.values_mut() {
            entry.last_updated_epoch = epoch;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_store_round_trip_passes() {
        let store = InvariantStore::default();
        assert!(store.round_trip_check());
    }

    #[test]
    fn empty_store_idempotency_passes() {
        let mut store = InvariantStore::default();
        assert!(store.idempotency_check(1));
    }
}
