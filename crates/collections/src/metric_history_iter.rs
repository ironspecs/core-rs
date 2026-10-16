//! Non-allocating iterator over retained metric increment history.
//!
//! This file owns the oldest-to-newest iterator over a retained metric history
//! store so callers can read retained increments without allocating.
//!
//! Strict invariants:
//! - iteration order is oldest-to-newest
//! - iteration stays allocation-free
//! - the iterator reports its exact remaining length

use crate::metric_history_store::MetricHistoryStore;
use crate::metrics::MetricHistoryEntry;

/// Non-allocating iterator over retained metric increment history.
#[derive(Clone, Debug)]
pub struct MetricHistoryIter<'a, Name, const HISTORY_CAPACITY: usize> {
    pub(crate) history: &'a MetricHistoryStore<Name, HISTORY_CAPACITY>,
    pub(crate) offset: usize,
}

impl<Name: Copy, const HISTORY_CAPACITY: usize> Iterator
    for MetricHistoryIter<'_, Name, HISTORY_CAPACITY>
{
    type Item = MetricHistoryEntry<Name>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset >= self.history.history_len {
            return None;
        }
        let entry = self.history.history_entry_at_offset(self.offset);
        self.offset += 1;
        Some(entry)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.history.history_len - self.offset;
        (remaining, Some(remaining))
    }
}

impl<Name: Copy, const HISTORY_CAPACITY: usize> ExactSizeIterator
    for MetricHistoryIter<'_, Name, HISTORY_CAPACITY>
{
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum TestMetricName {
        First,
        Second,
        Third,
    }

    #[test]
    fn iter_yields_oldest_to_newest_with_exact_len() {
        let mut store = MetricHistoryStore::<TestMetricName, 4>::default();
        store.record(TestMetricName::First);
        store.record(TestMetricName::Second);
        store.record(TestMetricName::Third);

        let mut iter = store.history();
        assert_eq!(iter.len(), 3);
        assert_eq!(
            iter.next().map(|entry| entry.name),
            Some(TestMetricName::First)
        );
        assert_eq!(
            iter.next().map(|entry| entry.name),
            Some(TestMetricName::Second)
        );
        assert_eq!(
            iter.next().map(|entry| entry.name),
            Some(TestMetricName::Third)
        );
        assert_eq!(iter.next(), None);
        assert_eq!(iter.len(), 0);

        let empty = MetricHistoryStore::<TestMetricName, 4>::default();
        assert_eq!(empty.history().len(), 0);
        assert_eq!(empty.history().next(), None);
    }
}
