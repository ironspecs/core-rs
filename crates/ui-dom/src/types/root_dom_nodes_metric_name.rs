//! Root node-store metric name vocabulary for `ui-dom`.
//!
//! This file represents the fixed metric vocabulary used to profile retained
//! node-store operations. Metric names are enum variants indexed into fixed
//! counter arrays; the shared storage and history mechanics live in
//! `collections::metrics`.
//!
//! Invariants and rules:
//!
//! - this file owns only the `RootDomNodesMetricName` enum, its companion name
//!   table, and its `impl` blocks
//! - metric names must be enum variants, not strings or public struct fields
//! - the name table must stay a fixed array indexed by `RootDomNodesMetricName`

use collections::metrics::MetricName;

/// Names one root node-store metric counter.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RootDomNodesMetricName {
    ContainsKey,
    Get,
    GetMut,
    Insert,
    InsertWithKey,
    IsEmpty,
    Remove,
}

pub(crate) const ROOT_DOM_NODES_METRIC_NAMES: [RootDomNodesMetricName; 7] = [
    RootDomNodesMetricName::ContainsKey,
    RootDomNodesMetricName::Get,
    RootDomNodesMetricName::GetMut,
    RootDomNodesMetricName::Insert,
    RootDomNodesMetricName::InsertWithKey,
    RootDomNodesMetricName::IsEmpty,
    RootDomNodesMetricName::Remove,
];

impl RootDomNodesMetricName {
    /// Number of defined node-store metric names.
    pub const COUNT: usize = ROOT_DOM_NODES_METRIC_NAMES.len();

    /// Returns the fixed counter index for this metric.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::ContainsKey => 0,
            Self::Get => 1,
            Self::GetMut => 2,
            Self::Insert => 3,
            Self::InsertWithKey => 4,
            Self::IsEmpty => 5,
            Self::Remove => 6,
        }
    }
}

impl MetricName for RootDomNodesMetricName {
    fn index(self) -> usize {
        RootDomNodesMetricName::index(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_dom_nodes_metric_name_indices_are_unique_and_in_bounds() {
        let mut seen_indices = [false; RootDomNodesMetricName::COUNT];

        for name in ROOT_DOM_NODES_METRIC_NAMES {
            let index = name.index();
            assert!(index < RootDomNodesMetricName::COUNT);
            assert_eq!(ROOT_DOM_NODES_METRIC_NAMES[index], name);
            assert!(
                !seen_indices[index],
                "metric index should be assigned to exactly one metric"
            );
            seen_indices[index] = true;
        }

        assert!(seen_indices.into_iter().all(|seen| seen));
    }
}
