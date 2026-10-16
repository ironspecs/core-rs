//! Shared fixed-size retained metric history store.
//!
//! This file owns the retained metric increment history ring so multiple
//! crates can share one canonical history implementation without tying metric
//! meaning to a specific runtime domain.
//!
//! Strict invariants:
//! - history is a fixed-capacity ring that overwrites the oldest entries once full
//! - reads after construction stay allocation-free
//! - reset clears retained history
//! - retained history iteration remains oldest-to-newest

use std::time::Instant;

use crate::metric_history_iter::MetricHistoryIter;
use crate::metrics::MetricHistoryEntry;

/// Shared fixed-size retained metric history store.
#[derive(Debug)]
pub struct MetricHistoryStore<Name, const HISTORY_CAPACITY: usize> {
    started_at: Instant,
    history_entries: [Option<MetricHistoryEntry<Name>>; HISTORY_CAPACITY],
    history_next: usize,
    pub(crate) history_len: usize,
}

impl<Name: Copy, const HISTORY_CAPACITY: usize> Default
    for MetricHistoryStore<Name, HISTORY_CAPACITY>
{
    fn default() -> Self {
        Self {
            started_at: Instant::now(),
            history_entries: [None; HISTORY_CAPACITY],
            history_next: 0,
            history_len: 0,
        }
    }
}

impl<Name: Copy, const HISTORY_CAPACITY: usize> MetricHistoryStore<Name, HISTORY_CAPACITY> {
    pub fn record(&mut self, name: Name) {
        self.push_history_entry(MetricHistoryEntry {
            name,
            recorded_at: self.started_at.elapsed(),
        });
    }

    pub fn reset(&mut self) {
        self.started_at = Instant::now();
        self.history_entries = [None; HISTORY_CAPACITY];
        self.history_next = 0;
        self.history_len = 0;
    }

    #[must_use]
    pub fn history(&self) -> MetricHistoryIter<'_, Name, HISTORY_CAPACITY> {
        MetricHistoryIter {
            history: self,
            offset: 0,
        }
    }

    fn push_history_entry(&mut self, entry: MetricHistoryEntry<Name>) {
        self.history_entries[self.history_next] = Some(entry);
        self.history_next = history_next_index::<HISTORY_CAPACITY>(self.history_next);
        self.history_len = (self.history_len + 1).min(HISTORY_CAPACITY);
    }

    pub(crate) fn history_entry_at_offset(&self, offset: usize) -> MetricHistoryEntry<Name> {
        assert!(
            offset < self.history_len,
            "history offset should be within retained entry count"
        );
        let oldest_index = self.oldest_history_index();
        let index = (oldest_index + offset) % HISTORY_CAPACITY;
        self.history_entries[index].expect("retained history slot should contain an entry")
    }

    fn oldest_history_index(&self) -> usize {
        if self.history_len < HISTORY_CAPACITY {
            return 0;
        }
        self.history_next
    }
}

fn history_next_index<const HISTORY_CAPACITY: usize>(current: usize) -> usize {
    assert!(
        current < HISTORY_CAPACITY,
        "history cursor should point at one retained ring slot"
    );
    if current + 1 == HISTORY_CAPACITY {
        return 0;
    }
    current + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum TestMetricName {
        First,
        Second,
        Third,
        Fourth,
    }

    #[test]
    fn history_ring_preserves_oldest_to_newest_and_overwrites_oldest_at_capacity() {
        const HISTORY_CAPACITY: usize = 3;
        let mut store = MetricHistoryStore::<TestMetricName, HISTORY_CAPACITY>::default();

        store.record(TestMetricName::First);
        store.record(TestMetricName::Second);
        store.record(TestMetricName::Third);
        store.record(TestMetricName::Fourth);

        assert_eq!(store.history().len(), HISTORY_CAPACITY);

        let names: Vec<TestMetricName> = store.history().map(|entry| entry.name).collect();
        assert_eq!(
            names,
            vec![
                TestMetricName::Second,
                TestMetricName::Third,
                TestMetricName::Fourth,
            ]
        );
    }
}
