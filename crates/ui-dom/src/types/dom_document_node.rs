//! Authored document node record inside a root-owned DOM store.
//!
//! This file owns the `DomDocumentNode` struct and its `impl` blocks. Its
//! purpose is to keep document-root-authored attributes and the parsed style
//! and class views in one honest record without owning the document store or
//! tree mutation policy.
//!
//! Invariants and rules:
//!
//! - this file owns only the `DomDocumentNode` struct and its `impl` blocks
//! - every `DomDocumentNode` is owned by exactly one document root key
//! - `Document` is the first node kind in its tree
//! - document nodes reject the `id` attribute as an invalid attribute target

use std::collections::BTreeMap;

use crate::types::dom_attr_node::DomAttributeNode;
use crate::types::dom_attr_parse::{normalize_attribute_value, validate_attribute_value};
use crate::types::dom_node::{
    DomNodeSlotKey, stored_attribute_value, validate_document_like_attr_target, write_attribute,
};
use crate::types::dom_node_base::DomNodeBase;
use crate::types::{DomAttribute, DomAttributeValue, DomAttrs, DomError};

#[derive(Clone, Debug, PartialEq)]
pub struct DomDocumentNode {
    /// Shared node links, caches, transitions, and owner identity.
    pub base: DomNodeBase,
    /// Authored root-level attrs that apply to the whole document tree.
    pub attrs: DomAttrs,
    pub(crate) attribute_style_map: BTreeMap<DomAttribute, DomAttributeValue>,
    pub(crate) class_list: Vec<String>,
}

impl DomDocumentNode {
    /// Returns the document root key that owns this document node.
    ///
    /// This lets callers confirm ownership integrity when they are holding a
    /// raw document node reference. The returned key is the same owner anchor
    /// stamped onto every node created by the same root document.
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

impl DomAttributeNode for DomDocumentNode {
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
