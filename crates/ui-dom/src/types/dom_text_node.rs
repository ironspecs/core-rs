//! Authored text node record inside a root-owned DOM store.
//!
//! This file owns the `DomTextNode` struct and its `impl` blocks. Its purpose
//! is to keep authored text content, the owner document key, and the retained
//! paint-time text-run id hint in one honest record without owning the
//! document store or tree mutation policy.
//!
//! Invariants and rules:
//!
//! - this file owns only the `DomTextNode` struct and its `impl` blocks
//! - every `DomTextNode` is owned by exactly one document root key
//! - text nodes reject element-style attributes (`id`, `class`, `style`)
//! - the text-run id hint is a paint-time cache and is cleared whenever text
//!   content changes

use std::sync::Arc;

use crate::domains::layer_ops::TextRunId;
use crate::types::dom_node::DomNodeSlotKey;
use crate::types::dom_node_base::DomNodeBase;

#[derive(Clone, Debug, PartialEq)]
pub struct DomTextNode {
    pub base: DomNodeBase,
    text_run_id_hint: Option<TextRunId>,
    data: Arc<str>,
}

impl DomTextNode {
    /// Returns the document root key that owns this text node.
    ///
    /// This is the same ownership anchor used by element and document nodes.
    /// Text nodes expose it directly so ownership checks can stay uniform
    /// across node kinds.
    #[must_use]
    pub fn owner_document_key(&self) -> DomNodeSlotKey {
        self.base.owner_document_key
    }

    #[must_use]
    pub fn data(&self) -> &str {
        self.data.as_ref()
    }

    #[must_use]
    pub(crate) fn data_arc(&self) -> Arc<str> {
        self.data.clone()
    }

    #[must_use]
    pub(crate) fn text_run_id_hint(&self) -> Option<TextRunId> {
        self.text_run_id_hint
    }

    #[must_use]
    pub(crate) fn new(owner_document_key: DomNodeSlotKey, data: Arc<str>) -> Self {
        Self {
            base: DomNodeBase::new(owner_document_key),
            text_run_id_hint: None,
            data,
        }
    }

    pub(crate) fn set_data(&mut self, data: Arc<str>) {
        self.data = data;
    }

    pub(crate) fn set_text_run_id_hint(&mut self, text_run_id_hint: Option<TextRunId>) {
        self.text_run_id_hint = text_run_id_hint;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_owner_document_key() -> DomNodeSlotKey {
        let mut keys = slotmap::SlotMap::with_key();
        keys.insert(())
    }

    fn test_text_run_id() -> TextRunId {
        let mut ids = slotmap::SlotMap::with_key();
        ids.insert(())
    }

    #[test]
    fn text_node_replaces_data_and_preserves_run_hint() {
        let mut node = DomTextNode::new(test_owner_document_key(), "hello".into());

        assert_eq!(node.data(), "hello");
        assert_eq!(node.data_arc().as_ref(), "hello");
        assert!(node.text_run_id_hint().is_none());

        let hint = test_text_run_id();
        node.set_text_run_id_hint(Some(hint));
        assert_eq!(node.text_run_id_hint(), Some(hint));

        // `set_data` replaces the text but does NOT clear the hint: clearing
        // is the root-owned protocol at call sites, not this record's job.
        node.set_data("world".into());
        assert_eq!(node.data(), "world");
        assert_eq!(node.text_run_id_hint(), Some(hint));
    }
}
