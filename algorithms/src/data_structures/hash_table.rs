//! Open-addressing hash table with linear probing.
//!
//! Variables:
//!   buckets  : Vec<Option<(K,V)>>  — slot array, length C (capacity)
//!   occupied : usize               — number of live entries
//!   C        : usize               — capacity, always power of 2
//!
//! Equations:
//!   h(k)         = k.hash() mod C                      (home slot)
//!   probe(h, i)  = (h + i) mod C                       (linear probe step i)
//!   load_factor  = occupied / C
//!   resize when load_factor > 0.7: C' = 2*C, rehash all entries
//!
//!   insert(k,v): probe from h(k) until empty slot,  O(1) amortised
//!   get(k):      probe from h(k) until k found or empty, O(1) amortised
//!   remove(k):   tombstone slot (set to None), O(1) amortised

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub struct HashTable<K: Hash + Eq + Clone, V: Clone> {
    buckets: Vec<Option<(K, V)>>,
    occupied: usize,
}

impl<K: Hash + Eq + Clone, V: Clone> HashTable<K, V> {
    pub fn new() -> Self {
        Self { buckets: vec![None; 16], occupied: 0 }
    }

    fn hash_index(&self, key: &K) -> usize {
        let mut h = DefaultHasher::new();
        key.hash(&mut h);
        (h.finish() as usize) & (self.buckets.len() - 1)
    }

    pub fn insert(&mut self, key: K, val: V) {
        if self.occupied * 10 >= self.buckets.len() * 7 {
            self.resize();
        }
        let mut i = self.hash_index(&key);
        loop {
            match &self.buckets[i] {
                None => {
                    self.buckets[i] = Some((key, val));
                    self.occupied += 1;
                    return;
                }
                Some((k, _)) if *k == key => {
                    self.buckets[i] = Some((key, val));
                    return;
                }
                _ => {
                    i = (i + 1) & (self.buckets.len() - 1);
                }
            }
        }
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        let mut i = self.hash_index(key);
        loop {
            match &self.buckets[i] {
                None => return None,
                Some((k, v)) if k == key => return Some(v),
                _ => {
                    i = (i + 1) & (self.buckets.len() - 1);
                }
            }
        }
    }

    pub fn remove(&mut self, key: &K) -> bool {
        let mut i = self.hash_index(key);
        loop {
            match &self.buckets[i] {
                None => return false,
                Some((k, _)) if k == key => {
                    self.buckets[i] = None;
                    self.occupied -= 1;
                    return true;
                }
                _ => {
                    i = (i + 1) & (self.buckets.len() - 1);
                }
            }
        }
    }

    pub fn len(&self) -> usize {
        self.occupied
    }
    pub fn is_empty(&self) -> bool {
        self.occupied == 0
    }

    fn resize(&mut self) {
        let new_cap = self.buckets.len() * 2;
        let mut new_buckets: Vec<Option<(K, V)>> = vec![None; new_cap];
        for slot in self.buckets.drain(..) {
            if let Some((k, v)) = slot {
                let mut h = DefaultHasher::new();
                k.hash(&mut h);
                let mut i = (h.finish() as usize) & (new_cap - 1);
                while new_buckets[i].is_some() {
                    i = (i + 1) & (new_cap - 1);
                }
                new_buckets[i] = Some((k, v));
            }
        }
        self.buckets = new_buckets;
    }
}
