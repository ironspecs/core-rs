//! Cloneable fixed-size enum-to-count tally snapshot.
//!
//! This file owns the copyable snapshot of enum-addressed metric counters so
//! callers can read aggregate counts without holding the live tally that owns
//! them.
//!
//! Strict invariants:
//! - counter indexes come from the `MetricName` contract
//! - snapshot reads stay allocation-free
//! - counters are a fixed array addressed by stable indexes

use std::marker::PhantomData;

use crate::metrics::MetricName;

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

    pub(crate) fn increment(&mut self, name: Name) {
        let count = self
            .counts
            .get_mut(name.index())
            .expect("metric index should resolve to one counter slot");
        *count = count
            .checked_add(1)
            .expect("metric counter should not overflow");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn snapshot_counts_reflect_from_counts_and_increment() {
        let mut snapshot = MetricSnapshot::<TestMetricName, 2>::from_counts([3, 7]);

        assert_eq!(snapshot.count(TestMetricName::Alpha), 3);
        assert_eq!(snapshot.count(TestMetricName::Beta), 7);
        assert_eq!(snapshot.raw_counts(), [3, 7]);
        assert_eq!(snapshot.raw_counts().iter().sum::<u64>(), 10);

        snapshot.increment(TestMetricName::Alpha);

        assert_eq!(snapshot.count(TestMetricName::Alpha), 4);
        assert_eq!(snapshot.raw_counts().iter().sum::<u64>(), 11);
    }
}
