use std::collections::HashMap;

pub fn hash_lookup<'a, K: std::hash::Hash + Eq, V>(map: &'a HashMap<K, V>, key: &K) -> Option<&'a V> {
    map.get(key)
}
