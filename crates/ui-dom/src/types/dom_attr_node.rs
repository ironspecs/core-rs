//! Re-export shim for the shared DOM attribute node trait.
//!
//! The `DomAttributeNode` trait relocated to `dom_attribute_node.rs` during
//! the behavior-type-file structural migration. This file re-exports it so
//! existing `crate::types::dom_attr_node::DomAttributeNode` import paths keep
//! resolving unchanged.

pub use crate::types::dom_attribute_node::DomAttributeNode;
