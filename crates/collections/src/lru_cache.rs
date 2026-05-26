//! Fixed-capacity least-recently-used cache support.
//!
//! This file owns one generic bounded LRU cache so multiple crates can reuse
//! the same strict recency and eviction behavior without reimplementing it in
//! domain modules.
//!
//! Strict invariants:
//! - cache capacity must be non-zero
//! - the cache owns recency ordering and eviction; callers must not mirror it
//! - `get` and `preload` both refresh recency for an existing key
//! - eviction removes exactly the least-recently-used entry

use std::collections::HashMap;
use std::hash::Hash;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreloadStatus {
    Warmed,
    Touched,
}

#[derive(Clone, Debug)]
struct LruEntry<K, V> {
    value: V,
    previous: Option<K>,
    next: Option<K>,
}

#[derive(Clone, Debug)]
pub struct LruCache<K, V, const CAPACITY: usize>
where
    K: Clone + Eq + Hash,
{
    entries: HashMap<K, LruEntry<K, V>>,
    most_recent: Option<K>,
    least_recent: Option<K>,
}

impl<K, V, const CAPACITY: usize> Default for LruCache<K, V, CAPACITY>
where
    K: Clone + Eq + Hash,
{
    fn default() -> Self {
        assert!(CAPACITY > 0, "lru cache capacity should be non-zero");
        Self {
            entries: HashMap::with_capacity(CAPACITY),
            most_recent: None,
            least_recent: None,
        }
    }
}

impl<K, V, const CAPACITY: usize> LruCache<K, V, CAPACITY>
where
    K: Clone + Eq + Hash,
{
    #[must_use]
    pub fn contains(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    #[cfg(test)]
    pub fn clear(&mut self) {
        self.entries.clear();
        self.most_recent = None;
        self.least_recent = None;
    }

    pub fn get(&mut self, key: &K) -> Option<&V> {
        self.touch(key);
        self.entries.get(key).map(|entry| &entry.value)
    }

    pub fn insert(&mut self, key: &K, value: V) -> Option<V> {
        self.insert_and_evict(key, value)
            .map(|(_, evicted)| evicted)
    }

    pub fn insert_and_evict(&mut self, key: &K, value: V) -> Option<(K, V)> {
        if let Some(existing) = self.entries.get_mut(key) {
            let replaced = std::mem::replace(&mut existing.value, value);
            self.touch(key);
            return Some((key.clone(), replaced));
        }

        self.entries.insert(
            key.clone(),
            LruEntry {
                value,
                previous: None,
                next: self.most_recent.clone(),
            },
        );
        if let Some(most_recent) = self.most_recent.clone() {
            self.entries
                .get_mut(&most_recent)
                .expect("most recent entry should exist")
                .previous = Some(key.clone());
        }
        self.most_recent = Some(key.clone());
        if self.least_recent.is_none() {
            self.least_recent = Some(key.clone());
        }
        self.enforce_capacity()
    }

    pub fn preload<F>(&mut self, key: &K, build_value: F) -> PreloadStatus
    where
        F: FnOnce() -> V,
    {
        if self.contains(key) {
            self.touch(key);
            return PreloadStatus::Touched;
        }
        self.insert(key, build_value());
        PreloadStatus::Warmed
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        let removed = self.entries.remove(key)?;
        self.detach_key(key, removed.previous.clone(), removed.next.clone());
        Some(removed.value)
    }

    pub fn touch(&mut self, key: &K) {
        if self.most_recent.as_ref() == Some(key) {
            return;
        }
        let Some((previous, next)) = self
            .entries
            .get(key)
            .map(|entry| (entry.previous.clone(), entry.next.clone()))
        else {
            return;
        };
        self.detach_key(key, previous, next);
        if let Some(current_most_recent) = self.most_recent.clone() {
            self.entries
                .get_mut(&current_most_recent)
                .expect("most recent entry should exist")
                .previous = Some(key.clone());
        }
        let entry = self
            .entries
            .get_mut(key)
            .expect("touched entry should still exist");
        entry.previous = None;
        entry.next.clone_from(&self.most_recent);
        self.most_recent = Some(key.clone());
        if self.least_recent.is_none() {
            self.least_recent = Some(key.clone());
        }
    }

    fn detach_key(&mut self, key: &K, previous: Option<K>, next: Option<K>) {
        if let Some(previous_key) = previous.clone() {
            self.entries
                .get_mut(&previous_key)
                .expect("previous entry should exist")
                .next
                .clone_from(&next);
        }
        if let Some(next_key) = next.clone() {
            self.entries
                .get_mut(&next_key)
                .expect("next entry should exist")
                .previous
                .clone_from(&previous);
        }
        if self.most_recent.as_ref() == Some(key) {
            self.most_recent = next;
        }
        if self.least_recent.as_ref() == Some(key) {
            self.least_recent = previous;
        }
    }

    fn enforce_capacity(&mut self) -> Option<(K, V)> {
        if self.entries.len() > CAPACITY {
            let least_recent = self
                .least_recent
                .clone()
                .expect("least recent entry should exist when cache exceeds capacity");
            let evicted = self
                .remove(&least_recent)
                .expect("least recent entry should remove while cache exceeds capacity");
            return Some((least_recent, evicted));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{LruCache, PreloadStatus};

    #[test]
    fn lru_cache_evicts_oldest_entry() {
        let mut cache = LruCache::<u8, &'static str, 2>::default();
        let _ = cache.insert(&1, "one");
        let _ = cache.insert(&2, "two");
        assert_eq!(cache.get(&1), Some(&"one"));
        let _ = cache.insert(&3, "three");

        assert!(cache.contains(&1));
        assert!(!cache.contains(&2));
        assert!(cache.contains(&3));
    }

    #[test]
    fn lru_cache_preload_reports_warm_and_touch() {
        let mut cache = LruCache::<u8, &'static str, 2>::default();

        assert_eq!(cache.preload(&1, || "one"), PreloadStatus::Warmed);
        assert_eq!(cache.preload(&1, || "other"), PreloadStatus::Touched);
        assert_eq!(cache.get(&1), Some(&"one"));
        cache.clear();
        assert!(!cache.contains(&1));
    }
}
