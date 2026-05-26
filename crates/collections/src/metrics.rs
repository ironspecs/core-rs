//! Shared fixed-size metrics storage support.
//!
//! This file owns generic enum-addressed metrics storage and retained-history
//! mechanics so multiple crates can share one canonical implementation without
//! tying metric meaning to a specific runtime domain.
//!
//! Strict invariants:
//! - metric names must resolve to stable counter indexes
//! - reads after construction stay allocation-free
//! - reset clears counts and retained history together
//! - retained history iteration remains oldest-to-newest

use std::marker::PhantomData;
use std::time::{Duration, Instant};

/// Mechanical contract for enum-addressed metrics storage.
pub trait MetricName: Copy {
    /// Returns the fixed counter index for this metric.
    fn index(self) -> usize;
}

/// Cloneable fixed-size enum-to-count tally.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MetricSnapshot<Name, const N: usize> {
    counts: [u64; N],
    marker: PhantomData<Name>,
}

impl<Name, const N: usize> Default for MetricSnapshot<Name, N> {
    fn default() -> Self {
        Self {
            counts: [0; N],
            marker: PhantomData,
        }
    }
}

impl<Name: MetricName, const N: usize> MetricSnapshot<Name, N> {
    #[must_use]
    pub fn from_counts(counts: [u64; N]) -> Self {
        Self {
            counts,
            marker: PhantomData,
        }
    }

    #[must_use]
    pub fn count(&self, name: Name) -> u64 {
        self.counts[name.index()]
    }

    #[must_use]
    pub fn raw_counts(&self) -> [u64; N] {
        self.counts
    }

    fn increment(&mut self, name: Name) {
        let count = self
            .counts
            .get_mut(name.index())
            .expect("metric index should resolve to one counter slot");
        *count = count
            .checked_add(1)
            .expect("metric counter should not overflow");
    }
}

/// One retained metric increment event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MetricHistoryEntry<Name> {
    pub name: Name,
    pub recorded_at: Duration,
}

/// Shared fixed-size retained metric history store.
#[derive(Debug)]
pub struct MetricHistoryStore<Name, const HISTORY_CAPACITY: usize> {
    started_at: Instant,
    history_entries: [Option<MetricHistoryEntry<Name>>; HISTORY_CAPACITY],
    history_next: usize,
    history_len: usize,
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

    fn history_entry_at_offset(&self, offset: usize) -> MetricHistoryEntry<Name> {
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

/// Shared fixed-size metrics owner with retained increment history.
#[derive(Debug)]
pub struct Metrics<Name, const N: usize, const HISTORY_CAPACITY: usize> {
    counts: MetricSnapshot<Name, N>,
    history: MetricHistoryStore<Name, HISTORY_CAPACITY>,
}

impl<Name: Copy, const N: usize, const HISTORY_CAPACITY: usize> Default
    for Metrics<Name, N, HISTORY_CAPACITY>
{
    fn default() -> Self {
        Self {
            counts: MetricSnapshot::default(),
            history: MetricHistoryStore::default(),
        }
    }
}

impl<Name: MetricName, const N: usize, const HISTORY_CAPACITY: usize>
    Metrics<Name, N, HISTORY_CAPACITY>
{
    #[must_use]
    pub fn metrics(&self) -> MetricSnapshot<Name, N> {
        self.counts
    }

    #[must_use]
    pub fn count(&self, name: Name) -> u64 {
        self.counts.count(name)
    }

    #[must_use]
    pub fn history(&self) -> MetricHistoryIter<'_, Name, HISTORY_CAPACITY> {
        MetricHistoryIter {
            history: &self.history,
            offset: 0,
        }
    }

    pub fn increment(&mut self, name: Name) {
        self.counts.increment(name);
        self.history.record(name);
    }

    pub fn reset(&mut self) {
        self.counts = MetricSnapshot::default();
        self.history.reset();
    }
}

/// Non-allocating iterator over retained metric increment history.
#[derive(Clone, Debug)]
pub struct MetricHistoryIter<'a, Name, const HISTORY_CAPACITY: usize> {
    history: &'a MetricHistoryStore<Name, HISTORY_CAPACITY>,
    offset: usize,
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
    use super::{MetricName, Metrics};

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum TestMetricName {
        Alpha,
        Beta,
    }

    impl MetricName for TestMetricName {
        fn index(self) -> usize {
            match self {
                Self::Alpha => 0,
                Self::Beta => 1,
            }
        }
    }

    type TestMetrics = Metrics<TestMetricName, 2, 3>;

    #[test]
    fn metrics_count_named_events() {
        let mut metrics = TestMetrics::default();

        metrics.increment(TestMetricName::Alpha);
        metrics.increment(TestMetricName::Alpha);
        metrics.increment(TestMetricName::Beta);

        assert_eq!(metrics.count(TestMetricName::Alpha), 2);
        assert_eq!(metrics.count(TestMetricName::Beta), 1);
    }

    #[test]
    fn metrics_history_retains_oldest_to_newest_window() {
        let mut metrics = TestMetrics::default();

        metrics.increment(TestMetricName::Alpha);
        metrics.increment(TestMetricName::Beta);
        metrics.increment(TestMetricName::Alpha);
        metrics.increment(TestMetricName::Beta);

        let history: Vec<_> = metrics.history().collect();

        assert_eq!(history.len(), 3);
        assert_eq!(history[0].name, TestMetricName::Beta);
        assert_eq!(history[1].name, TestMetricName::Alpha);
        assert_eq!(history[2].name, TestMetricName::Beta);
    }

    #[test]
    fn metrics_reset_clears_counts_and_history() {
        let mut metrics = TestMetrics::default();

        metrics.increment(TestMetricName::Alpha);
        metrics.increment(TestMetricName::Beta);
        metrics.reset();

        assert_eq!(metrics.count(TestMetricName::Alpha), 0);
        assert_eq!(metrics.count(TestMetricName::Beta), 0);
        assert_eq!(metrics.history().len(), 0);
    }
}
