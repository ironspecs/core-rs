//! Shared fixed-size metrics storage support.
//!
//! This file owns the generic enum-addressed metric owner (`Metrics`) and the
//! retained-history entry shape (`MetricHistoryEntry`) so multiple crates can
//! share one canonical tally without tying metric meaning to a specific
//! runtime domain. The metric-name contract (`MetricName`), counter snapshot,
//! history store, and history iterator are re-exported here from their own
//! files.
//!
//! Strict invariants:
//! - metric names must resolve to stable counter indexes
//! - reads after construction stay allocation-free
//! - reset clears counts and retained history together
//! - retained history iteration remains oldest-to-newest

use std::time::Duration;

pub use crate::metric_history_iter::MetricHistoryIter;
pub use crate::metric_history_store::MetricHistoryStore;
pub use crate::metric_name::MetricName;
pub use crate::metric_snapshot::MetricSnapshot;

/// One retained metric increment event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MetricHistoryEntry<Name> {
    pub name: Name,
    pub recorded_at: Duration,
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
