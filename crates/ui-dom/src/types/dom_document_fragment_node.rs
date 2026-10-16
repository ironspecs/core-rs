//! Authored document fragment node record inside a root-owned DOM store.
//!
//! This file owns the `DomDocumentFragmentNode` struct and its `impl` blocks.
//! Its purpose is to keep fragment-container-authored attributes and the parsed
//! style and class views in one honest record without owning the document
//! store or tree mutation policy.
//!
//! Invariants and rules:
//!
//! - this file owns only the `DomDocumentFragmentNode` struct and its `impl`
//!   blocks
//! - every `DomDocumentFragmentNode` is owned by exactly one document root key
//! - `DocumentFragment` is the first node kind in its detached tree
//! - fragment nodes reject the `id` attribute as an invalid attribute target

use std::collections::BTreeMap;

use crate::types::dom_attr_node::DomAttributeNode;
use crate::types::dom_attr_parse::{normalize_attribute_value, validate_attribute_value};
use crate::types::dom_node::{
    DomNodeSlotKey, stored_attribute_value, validate_document_like_attr_target, write_attribute,
};
use crate::types::dom_node_base::DomNodeBase;
use crate::types::{DomAttribute, DomAttributeValue, DomAttrs, DomError};

#[derive(Clone, Debug, PartialEq)]
pub struct DomDocumentFragmentNode {
    /// Shared node links, caches, transitions, and owner identity.
    pub base: DomNodeBase,
    /// Authored fragment-container attrs used while staging subtree content.
    pub attrs: DomAttrs,
    pub(crate) attribute_style_map: BTreeMap<DomAttribute, DomAttributeValue>,
    pub(crate) class_list: Vec<String>,
}

impl DomDocumentFragmentNode {
    /// Returns the document root key that owns this fragment node.
    ///
    /// This exposes the fragment-local owner anchor used while detached content
    /// is being staged. It exists so fragment nodes can still enforce same-root
    /// attachment rules before import.
    #[must_use]
    pub fn owner_document_key(&self) -> DomNodeSlotKey {
        self.base.owner_document_key
    }

    #[must_use]
    pub(crate) fn new(owner_document_key: DomNodeSlotKey) -> Self {
        Self {
            base: DomNodeBase::new(owner_document_key),
            attrs: DomAttrs::default(),
            attribute_style_map: BTreeMap::new(),
            class_list: Vec::new(),
        }
    }
}

impl DomAttributeNode for DomDocumentFragmentNode {
    fn attr(&self, attribute: &DomAttribute) -> Option<&DomAttributeValue> {
        stored_attribute_value(&self.attrs, attribute)
    }

    fn attrs(&self) -> &DomAttrs {
        &self.attrs
    }

    fn attribute_style_map(&self) -> &BTreeMap<DomAttribute, DomAttributeValue> {
        &self.attribute_style_map
    }

    fn class_list(&self) -> &[String] {
        &self.class_list
    }

    fn set_attr(
        &mut self,
        attribute: DomAttribute,
        value: Option<DomAttributeValue>,
    ) -> Result<(), DomError> {
        let value = normalize_attribute_value(&attribute, value)?;
        validate_document_like_attr_target(&attribute)?;
        validate_attribute_value(&attribute, value.as_ref())?;
        write_attribute(
            &mut self.attrs,
            &mut self.attribute_style_map,
            &mut self.class_list,
            &attribute,
            value,
        )
    }

    fn after_attr_write_updates_intrinsic(&self, _attribute: &DomAttribute) -> bool {
        false
    }
}
