//! Root node-store metric history iterator for `ui-dom`.
//!
//! This file represents the snapshot-backed iterator over retained root
//! node-store metric increment history. Its purpose is to expose retained
//! history oldest-to-newest without allocating.
//!
//! Invariants and rules:
//!
//! - this file owns only the `RootDomNodesMetricHistoryIter` struct and its
//!   `impl` blocks
//! - the iterator must walk a snapshot copy, never the live history store
//! - iteration must remain oldest-to-newest and allocation-free

use crate::types::root_dom_nodes::{
    ROOT_DOM_NODES_METRIC_HISTORY_CAPACITY, RootDomNodesMetricHistoryEntry,
};

#[cfg(test)]
use crate::types::dom_node::DomNode;
#[cfg(test)]
use crate::types::dom_tag::DomTag;
#[cfg(test)]
use crate::types::root_dom_nodes::{RootDomNodes, RootDomNodesMetricName};

/// Snapshot-backed iterator over retained root node-store metric history.
#[derive(Clone, Debug)]
pub struct RootDomNodesMetricHistoryIter {
    pub(crate) entries:
        [Option<RootDomNodesMetricHistoryEntry>; ROOT_DOM_NODES_METRIC_HISTORY_CAPACITY],
    pub(crate) index: usize,
    pub(crate) len: usize,
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
    use super::*;

    #[test]
    fn history_iter_yields_oldest_to_newest_with_exact_len() {
        let mut nodes = RootDomNodes::with_key();
        let root = nodes.insert_with_key(DomNode::document);
        let child = nodes.insert(DomNode::element(root, DomTag::Div));
        assert!(nodes.get(root).is_some());
        assert!(nodes.contains_key(child));

        let history = nodes.history();
        assert_eq!(history.len(), 4);

        let names: Vec<_> = history.map(|entry| entry.name).collect();
        assert_eq!(
            names,
            vec![
                RootDomNodesMetricName::InsertWithKey,
                RootDomNodesMetricName::Insert,
                RootDomNodesMetricName::Get,
                RootDomNodesMetricName::ContainsKey,
            ]
        );
    }
}
