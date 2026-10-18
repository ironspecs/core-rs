//! Authored element node record inside a root-owned DOM store.
//!
//! This file owns the `DomElementNode` struct and its `impl` blocks. Its
//! purpose is to keep element-authored attributes, parsed style and class
//! views, the lowered image source kind, and pseudo-state flags in one honest
//! record without owning the document store or tree mutation policy.
//!
//! Invariants and rules:
//!
//! - this file owns only the `DomElementNode` struct and its `impl` blocks
//! - every `DomElementNode` is owned by exactly one document root key
//! - the lowered image source kind is refreshed from the authored `src`
//!   attribute on every attribute write, never left stale
//! - pseudo-state flags are mutated only through the root-owned pseudo-state
//!   helpers, not by callers holding a bare element reference

use crate::types::dom_attr_node::DomAttributeNode;
use crate::types::dom_attr_parse::validate_attribute_value;
use crate::types::dom_node::{
    DomNodeSlotKey, DomStyleObject, LoweredImageSourceKind, lowered_image_source_kind_from_attrs,
    stored_attribute_value, write_attribute,
};
use crate::types::dom_node_base::DomNodeBase;
use crate::types::dom_pseudo_state::{DomPseudoState, DomPseudoStateBits};
use crate::types::dom_tag::{DomElementBehavior, DomTag};
use crate::types::{DomAttribute, DomAttributeValue, DomAttrs, DomError};

use crate::domains::style::computed::{ComputedGroups, DisplayKw};
use crate::domains::style::{DeclValue, Declaration, PresentationProperty};

#[cfg(test)]
use crate::types::dom_node::{DomIntrinsic, DomNode};
#[cfg(test)]
use crate::types::dom_tag::DomElementEditKind;
#[cfg(test)]
use crate::types::element_behaviors::DomElementIntrinsicKind;

#[derive(Clone, Debug, PartialEq)]
pub struct DomElementNode {
    pub base: DomNodeBase,
    pub tag: DomTag,
    pub attrs: DomAttrs,
    pub(crate) class_list: Vec<String>,
    lowered_image_source_kind: Option<LoweredImageSourceKind>,
    pseudo_state_bits: DomPseudoStateBits,
}

impl DomElementNode {
    /// Returns the document root key that owns this element.
    ///
    /// This is the stable ownership anchor used to prevent nodes from being
    /// attached into the wrong document tree. Callers should treat it as a
    /// document-local identity, not a global DOM id.
    #[must_use]
    pub fn owner_document_key(&self) -> DomNodeSlotKey {
        self.base.owner_document_key
    }

    #[must_use]
    pub(crate) fn new(owner_document_key: DomNodeSlotKey, tag: DomTag) -> Self {
        let descriptor = tag.descriptor();
        let mut base = DomNodeBase::new(owner_document_key);
        base.style
            .set_state_style_declarations(descriptor.state_style_declarations.clone());
        base.style
            .set_base_declarations(descriptor.base_style_declarations.clone());
        let mut attrs = descriptor.default_attrs;
        apply_base_declarations_to_initial_groups(
            &descriptor.base_style_declarations,
            &mut attrs,
            &mut base.presentation.computed,
        );
        Self {
            base,
            tag,
            attrs,
            class_list: Vec::new(),
            lowered_image_source_kind: None,
            pseudo_state_bits: DomPseudoStateBits::default(),
        }
    }

    #[must_use]
    pub(crate) const fn lowered_image_source_kind(&self) -> Option<LoweredImageSourceKind> {
        self.lowered_image_source_kind
    }

    pub(crate) fn refresh_lowered_image_source_kind(&mut self) -> Result<(), DomError> {
        if self.tag != DomTag::Img {
            self.lowered_image_source_kind = None;
            return Ok(());
        }
        self.lowered_image_source_kind = lowered_image_source_kind_from_attrs(&self.attrs)?;
        Ok(())
    }

    /// Returns the built-in behavior associated with this element's tag.
    ///
    /// This keeps tag-level behavior decisions centralized on the descriptor
    /// model instead of forcing callers to hard-code special cases. The result
    /// is intended for shared UI behavior, not app-local policy.
    #[must_use]
    pub fn behavior(&self) -> DomElementBehavior {
        self.tag.descriptor().behavior
    }

    #[must_use]
    pub(crate) fn style(&self) -> &DomStyleObject {
        &self.base.style
    }

    /// Returns the current pseudo-state flags for this element.
    #[must_use]
    pub fn pseudo_state_bits(&self) -> DomPseudoStateBits {
        self.pseudo_state_bits
    }

    /// Returns whether this element currently carries the provided pseudo-state.
    #[must_use]
    pub fn has_pseudo_state(&self, pseudo_state: DomPseudoState) -> bool {
        self.pseudo_state_bits.contains(pseudo_state)
    }

    pub(crate) fn set_pseudo_state(&mut self, pseudo_state: DomPseudoState) {
        self.pseudo_state_bits.insert(pseudo_state);
    }

    pub(crate) fn clear_pseudo_state(&mut self, pseudo_state: DomPseudoState) {
        self.pseudo_state_bits.remove(pseudo_state);
    }
}

/// Applies the element's unconditional base behavior declarations at creation
/// (spine 3.3g): today the only base declaration is the built-in `display`,
/// which must be present from creation so pre-style-pass readers (layer
/// membership, layout measurement) see the tag default in the computed groups.
/// It is the lowest cascade tier: the winners fold keeps it as the winner only
/// when no author rule or inline declaration overrode it.
fn apply_base_declarations_to_initial_groups(
    declarations: &[Declaration],
    _attrs: &mut DomAttrs,
    groups: &mut ComputedGroups,
) {
    for declaration in declarations {
        if declaration.property == PresentationProperty::Display
            && let DeclValue::Keyword(keyword) = declaration.value
            && let Some(display) = DisplayKw::from_keyword(keyword)
        {
            groups.box_.display = display;
        }
    }
}

impl DomAttributeNode for DomElementNode {
    fn attr(&self, attribute: &DomAttribute) -> Option<&DomAttributeValue> {
        stored_attribute_value(&self.attrs, attribute)
    }

    fn attrs(&self) -> &DomAttrs {
        &self.attrs
    }

    fn class_list(&self) -> &[String] {
        &self.class_list
    }

    fn set_attr(
        &mut self,
        attribute: DomAttribute,
        value: Option<DomAttributeValue>,
    ) -> Result<(), DomError> {
        validate_attribute_value(&attribute, value.as_ref())?;
        write_attribute(&mut self.attrs, &mut self.class_list, &attribute, value);
        self.refresh_lowered_image_source_kind()
    }

    fn after_attr_write_updates_intrinsic(&self, attribute: &DomAttribute) -> bool {
        self.tag
            .descriptor()
            .behavior
            .after_attr_write_intrinsic_update
            .matches(attribute)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_owner_document_key() -> DomNodeSlotKey {
        let mut keys = slotmap::SlotMap::with_key();
        keys.insert(())
    }

    #[test]
    fn input_nodes_start_with_input_defaults() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Input);
        node.after_create_intrinsic_update()
            .expect("input create intrinsic should measure");
        let element = node.as_element().expect("element node");

        assert_eq!(
            element.attrs.attributes.get(&DomAttribute::Type),
            Some(&DomAttributeValue::String("text".to_string()))
        );
        // The built-in `display` base declaration no longer lives in the attrs
        // map (spine 3.3g-2a): it seeds the computed box group at creation.
        assert_eq!(node.computed_box().display, DisplayKw::InlineBlock);
        assert!(element.behavior().focusable);
        assert_eq!(element.behavior().edit_kind, DomElementEditKind::SingleLine);
        assert_eq!(
            element.behavior().intrinsic_kind,
            DomElementIntrinsicKind::ElementDefaultPreferredSize
        );
        assert!(matches!(
            node.intrinsic(),
            DomIntrinsic::ElementDefaultPreferredSize(_)
        ));
    }
}
