//! String intern tables for CanonIR.
//!
//! Variables:
//!   vec : Vec<String>          — index → string  (serialized)
//!   map : HashMap<String, u32> — string → index  (runtime only, not serialized)
//!
//! Equations:
//!   intern(s) -> u32   where vec[u32] == s  (dedup: same string → same index)
//!   lookup(u32) -> &str
//!   O(1) intern, O(1) lookup

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Generic string interner.
/// `vec` is serialized; `map` is rebuilt via `restore_index()` after deserialize.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interner {
    /// Index → string.  This is the source of truth.
    pub vec: Vec<String>,
    /// String → index.  Not serialized — rebuilt from `vec`.
    #[serde(skip)]
    map: HashMap<String, u32>,
}

impl Default for Interner {
    fn default() -> Self {
        Self { vec: Vec::new(), map: HashMap::new() }
    }
}

impl Interner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Intern a string, returning its stable index.
    /// Identical strings always return the same index.
    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(&idx) = self.map.get(s) {
            return idx;
        }
        let idx = self.vec.len() as u32;
        self.vec.push(s.to_owned());
        self.map.insert(s.to_owned(), idx);
        idx
    }

    /// Lookup by index.  Panics on out-of-range (same contract as arena indexing).
    #[inline]
    pub fn lookup(&self, idx: u32) -> &str {
        self.vec[idx as usize].as_str()
    }

    /// Rebuild the reverse map after deserialization.
    /// Call this once after loading CanonIR from JSON.
    pub fn restore_index(&mut self) {
        self.map.clear();
        for (i, s) in self.vec.iter().enumerate() {
            self.map.insert(s.clone(), i as u32);
        }
    }

    pub fn len(&self) -> usize {
        self.vec.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vec.is_empty()
    }
}

// NameId and PathId are defined in node.rs to avoid circular deps.
// Re-export them here so intern module users can import from one place.
pub use crate::node::{NameId, PathId};
