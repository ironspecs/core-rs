use std::collections::BTreeMap;

use crate::types::dom_attr_parse::{normalize_attribute_value, validate_attribute_value};
use crate::types::dom_node::{
    DomDocumentFragmentNode, DomDocumentNode, DomElementNode, stored_attribute_value,
    write_attribute,
};
use crate::types::{
    DomAttribute, DomAttributeValue, DomAttrs, DomError, parse_authored_value_input,
};

/// Shared attribute API for DOM node types that can carry attributes.
///
/// This trait centralizes the stored-attribute operations shared by elements,
/// documents, and document fragments.
pub trait DomAttributeNode {
    /// Returns one stored attribute value from this node when present.
    fn attr(&self, attribute: &DomAttribute) -> Option<&DomAttributeValue>;

    /// Returns the raw attribute storage for this node.
    fn attrs(&self) -> &DomAttrs;

    /// Returns the parsed `style` attribute declarations for this node.
    fn attribute_style_map(&self) -> &BTreeMap<DomAttribute, DomAttributeValue>;

    /// Returns the parsed class token list for this node.
    fn class_list(&self) -> &[String];

    /// Sets one attribute on this node.
    fn set_attr(
        &mut self,
        attribute: DomAttribute,
        value: Option<DomAttributeValue>,
    ) -> Result<(), DomError>;

    /// Returns whether the `after_attr_write` stage should update intrinsic
    /// state for this attribute-carrying node.
    #[must_use]
    fn after_attr_write_updates_intrinsic(&self, attribute: &DomAttribute) -> bool;

    /// Sets one attribute from string name and value input.
    fn set_attr_str(&mut self, attribute: &str, value: &str) -> Result<(), DomError> {
        let attribute = DomAttribute::try_from(attribute)?;
        let value = parse_authored_value_input(&attribute, value)?;
        self.set_attr(attribute, value)
    }
}

impl DomAttributeNode for DomElementNode {
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
        validate_attribute_value(&attribute, value.as_ref())?;
        write_attribute(
            &mut self.attrs,
            &mut self.attribute_style_map,
            &mut self.class_list,
            &attribute,
            value,
        )?;
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

fn validate_document_like_attr_target(attribute: &DomAttribute) -> Result<(), DomError> {
    if *attribute == DomAttribute::Id {
        return Err(DomError::InvalidAttributeTarget);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::types::dom_node::DomNodeSlotKey;
    use crate::types::{
        DomAttribute, DomAttributeNode, DomAttributeValue, DomError, parse_css_color_rgba,
    };

    use super::{DomDocumentFragmentNode, DomDocumentNode, DomElementNode};

    fn test_owner_document_key() -> DomNodeSlotKey {
        let mut keys = slotmap::SlotMap::with_key();
        keys.insert(())
    }

    fn color(value: &str) -> DomAttributeValue {
        DomAttributeValue::Color(parse_css_color_rgba(value).expect("test color should parse"))
    }

    #[test]
    fn trait_set_attr_str_updates_raw_attrs_and_parsed_views() {
        let mut node = DomDocumentNode::new(test_owner_document_key());

        DomAttributeNode::set_attr_str(
            &mut node,
            "style",
            "width: 12px; color: #ffffff; width: 14px",
        )
        .expect("style should parse");
        DomAttributeNode::set_attr_str(&mut node, "class", "hero primary hero")
            .expect("class should parse");

        assert_eq!(
            DomAttributeNode::attrs(&node)
                .attributes
                .get(&DomAttribute::Style),
            Some(&DomAttributeValue::String(
                "width: 12px; color: #ffffff; width: 14px".to_string()
            ))
        );
        assert_eq!(
            DomAttributeNode::attribute_style_map(&node).get(&DomAttribute::Width),
            Some(&DomAttributeValue::String("14px".to_string()))
        );
        assert_eq!(
            DomAttributeNode::attribute_style_map(&node)
                .get(&DomAttribute::Custom("color".to_string())),
            Some(&color("#ffffff"))
        );
        assert_eq!(
            DomAttributeNode::class_list(&node),
            &[
                "hero".to_string(),
                "primary".to_string(),
                "hero".to_string()
            ]
        );
    }

    #[test]
    fn trait_set_attr_rejects_id_on_document_like_nodes() {
        let mut document = DomDocumentNode::new(test_owner_document_key());
        let mut fragment = DomDocumentFragmentNode::new(test_owner_document_key());

        let document_error = DomAttributeNode::set_attr(
            &mut document,
            DomAttribute::Id,
            Some(DomAttributeValue::String("hero".to_string())),
        )
        .expect_err("document should reject id");
        let fragment_error = DomAttributeNode::set_attr(
            &mut fragment,
            DomAttribute::Id,
            Some(DomAttributeValue::String("hero".to_string())),
        )
        .expect_err("fragment should reject id");

        assert_eq!(document_error, DomError::InvalidAttributeTarget);
        assert_eq!(fragment_error, DomError::InvalidAttributeTarget);
    }

    #[test]
    fn trait_attr_reads_raw_non_style_attributes() {
        let mut node = DomDocumentNode::new(test_owner_document_key());

        DomAttributeNode::set_attr(
            &mut node,
            DomAttribute::Width,
            Some(DomAttributeValue::String("12px".to_string())),
        )
        .expect("width should set");

        assert_eq!(
            DomAttributeNode::attr(&node, &DomAttribute::Width),
            Some(&DomAttributeValue::String("12px".to_string()))
        );
    }

    #[test]
    fn trait_attr_reads_id_and_style_on_element_nodes() {
        let mut node = DomElementNode::new(
            test_owner_document_key(),
            crate::types::dom_tag::DomTag::Div,
        );

        DomAttributeNode::set_attr(
            &mut node,
            DomAttribute::Id,
            Some(DomAttributeValue::String("hero".to_string())),
        )
        .expect("id should set");
        DomAttributeNode::set_attr_str(&mut node, "style", "width: 12px")
            .expect("style should set");

        assert_eq!(
            DomAttributeNode::attr(&node, &DomAttribute::Id),
            Some(&DomAttributeValue::String("hero".to_string()))
        );
        assert_eq!(DomAttributeNode::attrs(&node).id.as_deref(), Some("hero"));
        assert_eq!(
            DomAttributeNode::attribute_style_map(&node).get(&DomAttribute::Width),
            Some(&DomAttributeValue::String("12px".to_string()))
        );
    }

    #[test]
    fn element_trait_reports_intrinsic_update_attrs_from_descriptor_behavior() {
        let img = DomElementNode::new(
            test_owner_document_key(),
            crate::types::dom_tag::DomTag::Img,
        );
        let div = DomElementNode::new(
            test_owner_document_key(),
            crate::types::dom_tag::DomTag::Div,
        );

        assert!(DomAttributeNode::after_attr_write_updates_intrinsic(
            &img,
            &DomAttribute::Src,
        ));
        assert!(DomAttributeNode::after_attr_write_updates_intrinsic(
            &img,
            &DomAttribute::Width,
        ));
        assert!(!DomAttributeNode::after_attr_write_updates_intrinsic(
            &img,
            &DomAttribute::FontSize,
        ));
        assert!(!DomAttributeNode::after_attr_write_updates_intrinsic(
            &div,
            &DomAttribute::Width,
        ));
    }
}
