//! One retained refresh-tracked item yielded from most-recent to least-recent order.
//!
//! This file owns the borrowed view of one retained refresh-tracked store item
//! so callers can read its key, value, and refresh metadata without owning any
//! of them.
//!
//! Strict invariants:
//! - all accessors are const and allocation-free
//! - the item borrows the store and does not outlive it

use std::time::Instant;

/// One retained item yielded from most-recent to least-recent order.
#[derive(Clone, Copy, Debug)]
pub struct RefreshTrackedStoreRecencyItem<'a, K, V> {
    pub key: &'a K,
    pub value: &'a V,
    pub last_updated_at: Instant,
    pub last_refresh_requested_at: Instant,
}

impl<'a, K, V> RefreshTrackedStoreRecencyItem<'a, K, V> {
    #[must_use]
    pub const fn key(self) -> &'a K {
        self.key
    }

    #[must_use]
    pub const fn value(self) -> &'a V {
        self.value
    }

    #[must_use]
    pub const fn last_updated_at(self) -> Instant {
        self.last_updated_at
    }

    #[must_use]
    pub const fn last_refresh_requested_at(self) -> Instant {
        self.last_refresh_requested_at
    }
}
