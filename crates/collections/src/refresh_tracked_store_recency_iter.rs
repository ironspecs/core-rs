//! Iterator over retained refresh-tracked store items from most-recent to least-recent.
//!
//! This file owns the recency-ordered iterator over a retained refresh-tracked
//! store so callers can read retained items in most-recent-first order without
//! allocating.
//!
//! Strict invariants:
//! - iteration order is most-recent to least-recent
//! - iteration borrows the store and reads keys through its recency markers

use std::collections::btree_set;
use std::hash::Hash;

use crate::refresh_tracked_store::RefreshTrackedStore;
use crate::refresh_tracked_store_recency_item::RefreshTrackedStoreRecencyItem;

/// Iterator over retained items from most-recent to least-recent.
#[derive(Debug)]
pub struct RefreshTrackedStoreRecencyIter<'a, K, V>
where
    K: Clone + Eq + Hash,
{
    pub(crate) store: &'a RefreshTrackedStore<K, V>,
    pub(crate) markers: btree_set::Iter<'a, u64>,
}

impl<'a, K, V> Iterator for RefreshTrackedStoreRecencyIter<'a, K, V>
where
    K: Clone + Eq + Hash,
{
    type Item = RefreshTrackedStoreRecencyItem<'a, K, V>;

    fn next(&mut self) -> Option<Self::Item> {
        let marker = *self.markers.next_back()?;
        let key = self
            .store
            .key_by_marker
            .get(&marker)
            .expect("refresh tracked store recency marker should map to a key");
        let entry = self
            .store
            .entries_by_key
            .get(key)
            .expect("refresh tracked store recency key should map to an entry");

        Some(RefreshTrackedStoreRecencyItem {
            key,
            value: &entry.value,
            last_updated_at: entry.last_updated_at,
            last_refresh_requested_at: entry.last_refresh_requested_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recency_iter_yields_most_recent_first() {
        let mut store = RefreshTrackedStore::<u32, &'static str>::new();
        let _ = store.store(1, "one");
        let _ = store.store(2, "two");
        let _ = store.store(3, "three");

        let keys: Vec<u32> = store.iter_recency().map(|item| *item.key()).collect();
        assert_eq!(keys, vec![3, 2, 1]);

        let empty = RefreshTrackedStore::<u32, &'static str>::new();
        assert!(empty.iter_recency().next().is_none());
    }
}
