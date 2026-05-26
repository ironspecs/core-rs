//! Root-owned node-store proxy for `RootDom`.
//!
//! This file owns the proxy type that stands in for `RootDom`'s node slotmap.
//! Its purpose is to preserve the existing node-store method surface while
//! attaching one separate metrics tally dedicated only to node-store
//! operations.
//!
//! Invariants and rules:
//!
//! - this proxy owns exactly one `SlotMap<DomNodeSlotKey, DomNode>`
//! - node-store metrics here are distinct from `RootDom` metrics
//! - the proxy surface must stay limited to the existing node-store methods
//!   plus the metrics API
//! - metric counters must stay fixed-size and allocation-free after creation
//! - retained metric history must remain oldest-to-newest

use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::types::dom_node::{DomNode, DomNodeSlotKey};
use collections::metrics::{MetricHistoryEntry, MetricHistoryStore, MetricName, MetricSnapshot};
use parking_lot::Mutex;
use slotmap::SlotMap;

/// Number of retained node-store metric history entries.
pub const ROOT_DOM_NODES_METRIC_HISTORY_CAPACITY: usize = 128;

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

/// Snapshot copy of root node-store metric counter values.
pub type RootDomNodesMetrics =
    MetricSnapshot<RootDomNodesMetricName, { RootDomNodesMetricName::COUNT }>;

/// One retained root node-store metric increment event.
pub type RootDomNodesMetricHistoryEntry = MetricHistoryEntry<RootDomNodesMetricName>;

type RootDomNodesMetricHistoryStore =
    MetricHistoryStore<RootDomNodesMetricName, ROOT_DOM_NODES_METRIC_HISTORY_CAPACITY>;

/// Snapshot-backed iterator over retained root node-store metric history.
#[derive(Clone, Debug)]
pub struct RootDomNodesMetricHistoryIter {
    entries: [Option<RootDomNodesMetricHistoryEntry>; ROOT_DOM_NODES_METRIC_HISTORY_CAPACITY],
    index: usize,
    len: usize,
}

/// Proxy for `RootDom`'s node slot store with dedicated node-store metrics.
#[derive(Debug)]
pub(crate) struct RootDomNodes {
    nodes: SlotMap<DomNodeSlotKey, DomNode>,
    counts: [AtomicU64; RootDomNodesMetricName::COUNT],
    history: Mutex<RootDomNodesMetricHistoryStore>,
}

impl RootDomNodes {
    /// Creates one empty node store with keyed slot allocation enabled.
    #[must_use]
    pub(crate) fn with_key() -> Self {
        Self {
            nodes: SlotMap::with_key(),
            counts: std::array::from_fn(|_| AtomicU64::new(0)),
            history: Mutex::new(RootDomNodesMetricHistoryStore::default()),
        }
    }

    /// Returns whether the store contains one node for the provided key.
    #[must_use]
    pub(crate) fn contains_key(&self, key: DomNodeSlotKey) -> bool {
        self.increment_metric(RootDomNodesMetricName::ContainsKey);
        self.nodes.contains_key(key)
    }

    /// Returns one node by its slot key.
    #[must_use]
    pub(crate) fn get(&self, key: DomNodeSlotKey) -> Option<&DomNode> {
        self.increment_metric(RootDomNodesMetricName::Get);
        self.nodes.get(key)
    }

    /// Returns one mutable node by its slot key.
    #[must_use]
    pub(crate) fn get_mut(&mut self, key: DomNodeSlotKey) -> Option<&mut DomNode> {
        self.increment_metric(RootDomNodesMetricName::GetMut);
        self.nodes.get_mut(key)
    }

    /// Inserts one node and returns its allocated slot key.
    pub(crate) fn insert(&mut self, value: DomNode) -> DomNodeSlotKey {
        self.increment_metric(RootDomNodesMetricName::Insert);
        self.nodes.insert(value)
    }

    /// Inserts one node built from its allocated slot key.
    pub(crate) fn insert_with_key(
        &mut self,
        value: impl FnOnce(DomNodeSlotKey) -> DomNode,
    ) -> DomNodeSlotKey {
        self.increment_metric(RootDomNodesMetricName::InsertWithKey);
        self.nodes.insert_with_key(value)
    }

    /// Returns whether the store currently contains no nodes.
    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn is_empty(&self) -> bool {
        self.increment_metric(RootDomNodesMetricName::IsEmpty);
        self.nodes.is_empty()
    }

    /// Removes one node by slot key.
    #[allow(dead_code)]
    pub(crate) fn remove(&mut self, key: DomNodeSlotKey) -> Option<DomNode> {
        self.increment_metric(RootDomNodesMetricName::Remove);
        self.nodes.remove(key)
    }

    /// Returns the aggregate count for one named node-store metric.
    #[must_use]
    pub fn count(&self, name: RootDomNodesMetricName) -> u64 {
        self.counts[name.index()].load(Ordering::Relaxed)
    }

    /// Returns one snapshot copy of the node-store metrics.
    #[must_use]
    pub fn metrics(&self) -> RootDomNodesMetrics {
        MetricSnapshot::from_counts(std::array::from_fn(|index| {
            self.counts[index].load(Ordering::Relaxed)
        }))
    }

    /// Returns retained node-store metric increment history in oldest-to-newest order.
    #[must_use]
    pub fn history(&self) -> RootDomNodesMetricHistoryIter {
        let history = self.history.lock();
        let mut entries = [None; ROOT_DOM_NODES_METRIC_HISTORY_CAPACITY];
        let mut len = 0;
        for entry in history.history() {
            entries[len] = Some(entry);
            len += 1;
        }
        RootDomNodesMetricHistoryIter {
            entries,
            index: 0,
            len,
        }
    }

    /// Clears node-store metric counters and history.
    pub fn reset_metrics(&mut self) {
        for count in &self.counts {
            count.store(0, Ordering::Relaxed);
        }
        self.history.lock().reset();
    }

    fn increment_metric(&self, name: RootDomNodesMetricName) {
        let count = &self.counts[name.index()];
        let previous_count = count.fetch_add(1, Ordering::Relaxed);
        assert!(
            previous_count < u64::MAX,
            "node-store metric counter should not overflow"
        );
        self.history.lock().record(name);
    }
}

impl Deref for RootDomNodes {
    type Target = SlotMap<DomNodeSlotKey, DomNode>;

    fn deref(&self) -> &Self::Target {
        &self.nodes
    }
}

impl DerefMut for RootDomNodes {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.nodes
    }
}

impl Iterator for RootDomNodesMetricHistoryIter {
    type Item = RootDomNodesMetricHistoryEntry;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.len {
            return None;
        }
        let entry = self.entries[self.index];
        self.index += 1;
        entry
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.len - self.index;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for RootDomNodesMetricHistoryIter {}

#[cfg(test)]
mod tests {
    use super::{ROOT_DOM_NODES_METRIC_NAMES, RootDomNodes, RootDomNodesMetricName};
    use crate::types::{DomNode, DomTag};

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

    #[test]
    fn root_dom_nodes_counts_tracked_operations() {
        let mut nodes = RootDomNodes::with_key();
        let root = nodes.insert_with_key(DomNode::document);
        let child = nodes.insert(DomNode::element(root, DomTag::Div));

        assert!(nodes.contains_key(root));
        assert!(nodes.get(root).is_some());
        assert!(nodes.get_mut(child).is_some());
        assert!(!nodes.is_empty());
        assert!(nodes.remove(child).is_some());

        assert_eq!(nodes.count(RootDomNodesMetricName::InsertWithKey), 1);
        assert_eq!(nodes.count(RootDomNodesMetricName::Insert), 1);
        assert_eq!(nodes.count(RootDomNodesMetricName::ContainsKey), 1);
        assert_eq!(nodes.count(RootDomNodesMetricName::Get), 1);
        assert_eq!(nodes.count(RootDomNodesMetricName::GetMut), 1);
        assert_eq!(nodes.count(RootDomNodesMetricName::IsEmpty), 1);
        assert_eq!(nodes.count(RootDomNodesMetricName::Remove), 1);
    }

    #[test]
    fn root_dom_nodes_reset_clears_counts_and_history() {
        let mut nodes = RootDomNodes::with_key();
        let root = nodes.insert_with_key(DomNode::document);

        let _ = nodes.get(root);
        nodes.reset_metrics();

        assert_eq!(nodes.count(RootDomNodesMetricName::InsertWithKey), 0);
        assert_eq!(nodes.count(RootDomNodesMetricName::Get), 0);
        assert_eq!(nodes.history().len(), 0);
    }
}
