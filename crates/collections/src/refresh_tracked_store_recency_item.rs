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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn recency_item_accessors_expose_key_value_and_markers() {
        let key = 7u32;
        let value = "retained";
        let updated_at = Instant::now();
        let requested_at = updated_at + Duration::from_secs(1);

        let item = RefreshTrackedStoreRecencyItem {
            key: &key,
            value: &value,
            last_updated_at: updated_at,
            last_refresh_requested_at: requested_at,
        };

        assert_eq!(item.key(), &key);
        assert_eq!(item.value(), &value);
        assert_eq!(item.last_updated_at(), updated_at);
        assert_eq!(item.last_refresh_requested_at(), requested_at);
    }
}
