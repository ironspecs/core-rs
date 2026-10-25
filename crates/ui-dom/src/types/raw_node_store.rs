//! The uninstrumented `NodeStore` view over a raw `SlotMap`.
//!
//! What this file represents:
//!
//! - a thin owned wrapper over `&mut SlotMap<DomNodeSlotKey, DomNode>` that
//!   implements the `NodeStore` trait for the fragment store and the link
//!   machinery — the stores that carry no metric proxy
//!
//! Ownership boundary:
//!
//! - this is the raw half of the observability seam: every method forwards
//!   straight to the `SlotMap` with no instrumentation
//!
//! Invariants and rules:
//!
//! - `get`/`insert`/`append_child_to_parent` are plain `SlotMap`/tree-link
//!   calls, byte-for-byte the access the pre-splice fragment walk performed
//!
//! Extraction note (PR 5, E1 phase 2): structural rule 1 names an impl file
//! after its self type, and `SlotMap` is a foreign type — implementing
//! `NodeStore` for `SlotMap` directly would demand a `slot_map.rs` file. This
//! local wrapper keeps the impl in a honestly-named file instead.
use slotmap::SlotMap;

use crate::types::DomError;
use crate::types::dom_node::{DomNode, DomNodeSlotKey};
use crate::types::node_store::NodeStore;
use crate::types::tree_links::append_child_to_parent;

#[cfg(test)]
use crate::types::dom_tag::DomTag;

/// An uninstrumented node-store view over a raw `SlotMap`.
pub(crate) struct RawNodeStore<'a>(pub(crate) &'a mut SlotMap<DomNodeSlotKey, DomNode>);

impl NodeStore for RawNodeStore<'_> {
    fn get(&self, key: DomNodeSlotKey) -> Option<&DomNode> {
        self.0.get(key)
    }

    fn insert(&mut self, value: DomNode) -> DomNodeSlotKey {
        self.0.insert(value)
    }

    fn append_child_to_parent(
        &mut self,
        parent_key: DomNodeSlotKey,
        child_key: DomNodeSlotKey,
    ) -> Result<DomNodeSlotKey, DomError> {
        append_child_to_parent(self.0, parent_key, child_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_store_routes_get_insert_and_append_through_the_slotmap() {
        let mut nodes = SlotMap::with_key();
        let root = nodes.insert_with_key(DomNode::document);
        let mut store = RawNodeStore(&mut nodes);

        let child = store.insert(DomNode::element(root, DomTag::Div));
        assert!(store.get(child).is_some());

        let appended = store
            .append_child_to_parent(root, child)
            .expect("append should succeed");
        assert_eq!(appended, child);
        assert_eq!(
            store.get(root).and_then(DomNode::first_child_key_for_store),
            Some(child)
        );
    }
}
