//! Retained refresh-tracked store with explicit recency ordering.
//!
//! This file owns one generic retained key-value store that tracks refresh
//! metadata and most-recent-to-least-recent iteration order without owning any
//! domain reaction policy. The recency item and recency iterator are
//! re-exported here from their own files.
//!
//! Strict invariants:
//! - entries are retained until the caller removes them
//! - recency ordering is owned here and keyed by internal markers
//! - `last_updated_at` and `last_refresh_requested_at` are coarse monotonic
//!   signals owned by the caller
//! - stale updates never replace fresher retained values

use std::collections::{BTreeSet, HashMap};
use std::hash::Hash;
use std::time::{Duration, Instant};

pub use crate::refresh_tracked_store_recency_item::RefreshTrackedStoreRecencyItem;
pub use crate::refresh_tracked_store_recency_iter::RefreshTrackedStoreRecencyIter;

#[derive(Debug)]
pub(crate) struct RefreshTrackedStoreEntry<V> {
    pub(crate) value: V,
    pub(crate) last_updated_at: Instant,
    pub(crate) last_refresh_requested_at: Instant,
    recency_marker: u64,
}

/// Exact write outcome for one retained store mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefreshTrackedStoreWriteResult {
    Inserted,
    Replaced,
    IgnoredStaleUpdate,
}

/// Retained key-value store with explicit refresh metadata and recency order.
#[derive(Debug)]
pub struct RefreshTrackedStore<K, V>
where
    K: Clone + Eq + Hash,
{
    pub(crate) entries_by_key: HashMap<K, RefreshTrackedStoreEntry<V>>,
    markers_by_recency: BTreeSet<u64>,
    pub(crate) key_by_marker: HashMap<u64, K>,
    next_recency_marker: u64,
}

impl<K, V> RefreshTrackedStore<K, V>
where
    K: Clone + Eq + Hash,
{
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries_by_key: HashMap::new(),
            markers_by_recency: BTreeSet::new(),
            key_by_marker: HashMap::new(),
            next_recency_marker: 0,
        }
    }

    #[must_use]
    pub fn contains(&self, key: &K) -> bool {
        self.entries_by_key.contains_key(key)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries_by_key.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries_by_key.is_empty()
    }

    /// Stores a retained value and marks it as most-recently refreshed now.
    pub fn store(&mut self, key: K, value: V) -> RefreshTrackedStoreWriteResult {
        let now = Instant::now();
        self.store_with_time(key, value, now, now)
    }

    /// Records a retained value and monotonic refresh metadata for one key.
    pub fn store_with_time(
        &mut self,
        key: K,
        value: V,
        updated_at: Instant,
        requested_at: Instant,
    ) -> RefreshTrackedStoreWriteResult {
        if self.entries_by_key.contains_key(&key) {
            return self.update_existing_entry_with_time(&key, value, updated_at, requested_at);
        }

        let recency_marker = self.next_recency_marker();
        self.entries_by_key.insert(
            key.clone(),
            RefreshTrackedStoreEntry {
                value,
                last_updated_at: updated_at,
                last_refresh_requested_at: requested_at,
                recency_marker,
            },
        );
        self.insert_marker(recency_marker, key);
        RefreshTrackedStoreWriteResult::Inserted
    }

    /// Returns the retained value without changing recency ordering.
    pub fn peek(&self, key: &K) -> Option<&V> {
        self.entries_by_key.get(key).map(|entry| &entry.value)
    }

    /// Returns the retained value mutably without changing recency ordering.
    pub fn peek_mut(&mut self, key: &K) -> Option<&mut V> {
        self.entries_by_key
            .get_mut(key)
            .map(|entry| &mut entry.value)
    }

    /// Promotes one retained value to most-recent and returns it.
    pub fn touch_and_get(&mut self, key: &K) -> Option<&V> {
        self.promote_key(key)?;
        self.peek(key)
    }

    /// Promotes one retained value to most-recent and returns it mutably.
    pub fn touch_and_get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.promote_key(key)?;
        self.peek_mut(key)
    }

    /// Records that one retained key has been requested for refresh.
    pub fn mark_refresh_requested(&mut self, key: &K) -> bool {
        self.mark_refresh_requested_at(key, Instant::now())
    }

    /// Records that one retained key has been requested for refresh at `at`.
    pub fn mark_refresh_requested_at(&mut self, key: &K, at: Instant) -> bool {
        let Some(entry) = self.entries_by_key.get_mut(key) else {
            return false;
        };
        entry.last_refresh_requested_at = at.max(entry.last_refresh_requested_at);
        true
    }

    /// Records that one retained key received fresher data now.
    pub fn mark_updated(&mut self, key: &K) -> bool {
        self.mark_updated_at(key, Instant::now())
    }

    /// Records that one retained key received fresher data at `at`.
    pub fn mark_updated_at(&mut self, key: &K, at: Instant) -> bool {
        let Some(last_updated_at) = self
            .entries_by_key
            .get(key)
            .map(|entry| entry.last_updated_at)
        else {
            return false;
        };
        if at <= last_updated_at {
            return true;
        }

        if let Some(entry) = self.entries_by_key.get_mut(key) {
            entry.last_updated_at = at;
        } else {
            return false;
        }
        self.promote_key(key).is_some()
    }

    /// Removes one retained value and its refresh metadata.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        let entry = self.entries_by_key.remove(key)?;
        self.remove_marker(entry.recency_marker);
        Some(entry.value)
    }

    /// Returns whether the retained value is missing or older than `max_age`.
    #[must_use]
    pub fn is_stale(&self, key: &K, max_age: Duration) -> bool {
        let Some(entry) = self.entries_by_key.get(key) else {
            return true;
        };
        Instant::now().duration_since(entry.last_updated_at) > max_age
    }

    /// Iterates retained items from most-recent to least-recent.
    #[must_use]
    pub fn iter_recency(&self) -> RefreshTrackedStoreRecencyIter<'_, K, V> {
        RefreshTrackedStoreRecencyIter {
            store: self,
            markers: self.markers_by_recency.iter(),
        }
    }

    /// Returns the least-recent retained key.
    #[must_use]
    pub fn least_recent_key(&self) -> Option<&K> {
        let marker = *self.markers_by_recency.first()?;
        self.key_by_marker.get(&marker)
    }

    fn update_existing_entry_with_time(
        &mut self,
        key: &K,
        value: V,
        updated_at: Instant,
        requested_at: Instant,
    ) -> RefreshTrackedStoreWriteResult {
        let mut should_promote = false;
        let mut replaced = false;

        if let Some(entry) = self.entries_by_key.get_mut(key) {
            if updated_at > entry.last_updated_at {
                entry.value = value;
                entry.last_updated_at = updated_at;
                should_promote = true;
                replaced = true;
            }
            entry.last_refresh_requested_at = requested_at.max(entry.last_refresh_requested_at);
        } else {
            return RefreshTrackedStoreWriteResult::IgnoredStaleUpdate;
        }

        if should_promote {
            let _ = self.promote_key(key);
        }
        if replaced {
            return RefreshTrackedStoreWriteResult::Replaced;
        }
        RefreshTrackedStoreWriteResult::IgnoredStaleUpdate
    }

    fn promote_key(&mut self, key: &K) -> Option<()> {
        let old_marker = self.entries_by_key.get(key)?.recency_marker;
        let new_marker = self.next_recency_marker();

        let entry = self.entries_by_key.get_mut(key)?;
        entry.recency_marker = new_marker;

        self.remove_marker(old_marker);
        self.insert_marker(new_marker, key.clone());
        Some(())
    }

    fn insert_marker(&mut self, marker: u64, key: K) {
        let inserted_marker = self.markers_by_recency.insert(marker);
        assert!(
            inserted_marker,
            "refresh tracked store recency marker should be unique"
        );
        let replaced_key = self.key_by_marker.insert(marker, key);
        assert!(
            replaced_key.is_none(),
            "refresh tracked store marker index should not replace an existing key"
        );
    }

    fn remove_marker(&mut self, marker: u64) {
        let removed_marker = self.markers_by_recency.remove(&marker);
        assert!(
            removed_marker,
            "refresh tracked store marker index should contain every retained marker"
        );
        let removed_key = self.key_by_marker.remove(&marker);
        assert!(
            removed_key.is_some(),
            "refresh tracked store key index should contain every retained marker"
        );
    }

    fn next_recency_marker(&mut self) -> u64 {
        let recency_marker = self.next_recency_marker;
        self.next_recency_marker = self
            .next_recency_marker
            .checked_add(1)
            .expect("refresh tracked store recency marker should not overflow");
        recency_marker
    }
}

impl<K, V> Default for RefreshTrackedStore<K, V>
where
    K: Clone + Eq + Hash,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{RefreshTrackedStore, RefreshTrackedStoreWriteResult};
    use std::time::{Duration, Instant};

    #[test]
    fn touch_and_get_promotes_entries_to_most_recent() {
        let base = Instant::now();
        let mut store = RefreshTrackedStore::<u32, &'static str>::new();
        let _ = store.store_with_time(1, "one", base, base);
        let _ = store.store_with_time(
            2,
            "two",
            base + Duration::from_secs(1),
            base + Duration::from_secs(1),
        );
        let _ = store.store_with_time(
            3,
            "three",
            base + Duration::from_secs(2),
            base + Duration::from_secs(2),
        );

        assert_eq!(store.least_recent_key(), Some(&1));
        assert_eq!(store.touch_and_get(&1), Some(&"one"));

        let ordered_keys: Vec<u32> = store.iter_recency().map(|item| *item.key()).collect();
        assert_eq!(ordered_keys, vec![1, 3, 2]);
        assert_eq!(store.least_recent_key(), Some(&2));
    }

    #[test]
    fn peek_does_not_change_recency_order() {
        let base = Instant::now();
        let mut store = RefreshTrackedStore::<u32, &'static str>::new();
        let _ = store.store_with_time(1, "one", base, base);
        let _ = store.store_with_time(
            2,
            "two",
            base + Duration::from_secs(1),
            base + Duration::from_secs(1),
        );

        assert_eq!(store.peek(&1), Some(&"one"));

        let ordered_keys: Vec<u32> = store.iter_recency().map(|item| *item.key()).collect();
        assert_eq!(ordered_keys, vec![2, 1]);
        assert_eq!(store.least_recent_key(), Some(&1));
    }

    #[test]
    fn peek_mut_does_not_change_recency_order() {
        let base = Instant::now();
        let mut store = RefreshTrackedStore::<u32, &'static str>::new();
        let _ = store.store_with_time(1, "one", base, base);
        let _ = store.store_with_time(
            2,
            "two",
            base + Duration::from_secs(1),
            base + Duration::from_secs(1),
        );

        let value = store.peek_mut(&1).expect("retained item should exist");
        *value = "updated";

        let ordered_keys: Vec<u32> = store.iter_recency().map(|item| *item.key()).collect();
        assert_eq!(ordered_keys, vec![2, 1]);
        assert_eq!(store.peek(&1), Some(&"updated"));
        assert_eq!(store.least_recent_key(), Some(&1));
    }

    #[test]
    fn mark_updated_promotes_key_only_for_fresher_timestamp() {
        let base = Instant::now();
        let mut store = RefreshTrackedStore::<u32, &'static str>::new();
        let _ = store.store_with_time(1, "one", base, base);
        let _ = store.store_with_time(
            2,
            "two",
            base + Duration::from_secs(1),
            base + Duration::from_secs(1),
        );

        assert!(store.mark_updated_at(&1, base));
        assert_eq!(
            store
                .iter_recency()
                .map(|item| *item.key())
                .collect::<Vec<_>>(),
            vec![2, 1]
        );

        assert!(store.mark_updated_at(&1, base + Duration::from_secs(2)));
        assert_eq!(
            store
                .iter_recency()
                .map(|item| *item.key())
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(store.least_recent_key(), Some(&2));
    }

    #[test]
    fn newer_store_with_time_replaces_value_and_promotes_key() {
        let base = Instant::now();
        let mut store = RefreshTrackedStore::<u32, &'static str>::new();
        let _ = store.store_with_time(1, "one", base, base);
        let _ = store.store_with_time(
            2,
            "two",
            base + Duration::from_secs(1),
            base + Duration::from_secs(1),
        );

        let write_result = store.store_with_time(
            1,
            "updated",
            base + Duration::from_secs(2),
            base + Duration::from_secs(2),
        );

        assert_eq!(write_result, RefreshTrackedStoreWriteResult::Replaced);
        assert_eq!(store.peek(&1), Some(&"updated"));
        assert_eq!(
            store
                .iter_recency()
                .map(|item| *item.key())
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(store.least_recent_key(), Some(&2));
    }

    #[test]
    fn stale_store_with_time_does_not_replace_newer_value() {
        let base = Instant::now();
        let mut store = RefreshTrackedStore::<u32, &'static str>::new();
        let _ = store.store_with_time(
            1,
            "fresh",
            base + Duration::from_secs(2),
            base + Duration::from_secs(2),
        );

        let write_result = store.store_with_time(
            1,
            "stale",
            base + Duration::from_secs(1),
            base + Duration::from_secs(3),
        );

        assert_eq!(
            write_result,
            RefreshTrackedStoreWriteResult::IgnoredStaleUpdate
        );
        assert_eq!(store.peek(&1), Some(&"fresh"));
        let record = store
            .iter_recency()
            .next()
            .expect("retained item should exist");
        assert_eq!(
            record.last_refresh_requested_at(),
            base + Duration::from_secs(3)
        );
    }

    #[test]
    fn remove_clears_entry_and_recency_marker() {
        let base = Instant::now();
        let mut store = RefreshTrackedStore::<u32, &'static str>::new();
        let _ = store.store_with_time(1, "one", base, base);
        let _ = store.store_with_time(
            2,
            "two",
            base + Duration::from_secs(1),
            base + Duration::from_secs(1),
        );

        assert_eq!(store.remove(&2), Some("two"));
        assert!(!store.contains(&2));
        assert_eq!(
            store
                .iter_recency()
                .map(|item| *item.key())
                .collect::<Vec<_>>(),
            vec![1]
        );
        assert_eq!(store.least_recent_key(), Some(&1));
    }

    #[test]
    fn is_stale_returns_true_for_missing_or_expired_entries() {
        let base = Instant::now();
        let mut store = RefreshTrackedStore::<u32, &'static str>::new();
        let _ = store.store_with_time(
            1,
            "one",
            base.checked_sub(Duration::from_secs(10))
                .expect("test instant should support subtraction"),
            base,
        );

        assert!(store.is_stale(&2, Duration::from_secs(1)));
        assert!(store.is_stale(&1, Duration::from_secs(1)));
        assert!(!store.is_stale(&1, Duration::from_mins(1)));
    }

    #[test]
    fn mark_refresh_requested_keeps_monotonic_requested_time() {
        let base = Instant::now();
        let mut store = RefreshTrackedStore::<u32, &'static str>::new();
        let _ = store.store_with_time(1, "one", base, base + Duration::from_secs(2));

        assert!(store.mark_refresh_requested_at(&1, base + Duration::from_secs(1)));
        assert!(store.mark_refresh_requested_at(&1, base + Duration::from_secs(3)));

        let record = store
            .iter_recency()
            .next()
            .expect("retained item should exist");
        assert_eq!(
            record.last_refresh_requested_at(),
            base + Duration::from_secs(3)
        );
    }

    #[test]
    fn store_reports_inserted_for_new_entry() {
        let mut store = RefreshTrackedStore::<u32, &'static str>::new();

        let write_result = store.store(1, "one");

        assert_eq!(write_result, RefreshTrackedStoreWriteResult::Inserted);
        assert_eq!(store.peek(&1), Some(&"one"));
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn mark_updated_returns_false_for_missing_key() {
        let mut store = RefreshTrackedStore::<u32, &'static str>::new();

        assert!(!store.mark_updated(&1));
        assert!(!store.mark_refresh_requested(&1));
    }

    #[test]
    fn equal_timestamp_update_is_ignored() {
        let base = Instant::now();
        let mut store = RefreshTrackedStore::<u32, &'static str>::new();
        let _ = store.store_with_time(1, "one", base, base);

        let write_result = store.store_with_time(1, "other", base, base);

        assert_eq!(
            write_result,
            RefreshTrackedStoreWriteResult::IgnoredStaleUpdate
        );
        assert_eq!(store.peek(&1), Some(&"one"));
    }
}
