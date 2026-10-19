//! Public DOM types and authored value shapes for `ui-dom`.
//!
//! This file represents the shared type boundary used across the rest of the
//! crate and by external callers. Its purpose is to define stable authored DOM
//! values, errors, supporting structs, and root-owned metric vocabulary
//! without mixing in runtime-domain implementation details. The CONTENT
//! attribute vocabulary (`DomAttribute` — presentation properties live in
//! `PresentationProperty`, PLAN.md approach C) and the combined canonical
//! snapshot (`DomCanonicalSnapshot`) live in their own files and are
//! re-exported here.
//!
//! Invariants and rules:
//!
//! - this file owns cross-cutting value types and errors, not runtime policy
//! - parsing and authored value rules here must stay strict and deterministic
//! - app-local extensions should not broaden the shared type surface casually
//!
//! Advice and code philosophy:
//!
//! - keep shared types low-level and composable
//! - put behavior in the owning modules; keep these shapes literal
mod attribute_invalidation;
pub mod box_model;
pub mod dom_after_attr_write_intrinsic_update;
pub mod dom_attr_node;
mod dom_attr_parse;
pub mod dom_attribute;
pub mod dom_attribute_node;
pub mod dom_box_model;
pub mod dom_canonical_snapshot;
pub mod dom_document_fragment_node;
pub mod dom_document_node;
mod dom_element_descriptor_builder;
pub mod dom_element_node;
pub mod dom_event;
mod dom_event_behavior;
mod dom_event_definition;
mod dom_event_descriptor_builder;
pub mod dom_event_detail;
pub mod dom_event_dispatch_context;
pub mod dom_event_listener;
pub mod dom_event_listener_context;
mod dom_event_listener_owner;
pub mod dom_event_name;
pub mod dom_node;
pub mod dom_node_base;
mod dom_ops;
pub mod dom_paint_state;
pub mod dom_pseudo_state;
pub mod dom_pseudo_state_bits;
mod dom_query_selector;
pub mod dom_style_object;
pub mod dom_tag;
pub mod dom_text_node;
pub mod dom_transition;
pub mod element_behaviors;
mod event_listener_name_bits;
mod layout_scratch;
pub mod paint_config;
pub mod pixel_density;
pub mod primitives;
pub mod root_dom;
pub mod root_dom_context;
pub mod root_dom_context_config;
pub mod root_dom_fragment;
pub mod root_dom_metric_name;
pub mod root_dom_metrics;
pub mod root_dom_nodes;
pub mod root_dom_nodes_metric_history_iter;
pub mod root_dom_nodes_metric_name;
mod simple_selector;
mod white_space;

pub use box_model::*;
pub use dom_after_attr_write_intrinsic_update::*;
pub use dom_attr_node::*;
pub use dom_attr_parse::parse_css_color_rgba;
pub(crate) use dom_attr_parse::{AuthoredLength, parse_authored_length};
pub use dom_attribute::*;
pub use dom_box_model::*;
pub use dom_canonical_snapshot::*;
pub use dom_document_fragment_node::*;
pub use dom_document_node::*;
pub use dom_element_node::*;
pub use dom_event::*;
pub use dom_event_listener::*;
pub(crate) use dom_event_listener_owner::DomEventListenerOwner;
pub use dom_event_name::*;
pub use dom_node::*;
pub use dom_node_base::*;
pub use dom_paint_state::*;
pub use dom_pseudo_state::*;
pub use dom_pseudo_state_bits::*;
pub use dom_style_object::*;
pub use dom_tag::*;
pub use dom_text_node::*;
pub use dom_transition::*;
pub use element_behaviors::DomElementIntrinsicKind;
pub(crate) use layout_scratch::{LaneRange, LayoutScratch};
pub use paint_config::*;
pub use pixel_density::*;
pub use primitives::*;
pub use root_dom::*;
pub use root_dom_context::*;
pub use root_dom_context_config::*;
pub use root_dom_fragment::*;
pub use root_dom_metric_name::*;
pub use root_dom_metrics::*;
pub use root_dom_nodes::*;
pub use root_dom_nodes_metric_history_iter::*;
pub use root_dom_nodes_metric_name::*;
pub use white_space::{
    DomOverflowWrap, DomTextAlign, DomTextDirection, DomWhiteSpace, DomWordBreak,
};
pub(crate) use white_space::{
    parse_text_direction_keyword, text_layout_input_and_byte_map_for_white_space,
    text_layout_input_for_white_space,
};

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum DomAttributeValue {
    String(String),
    Bool(bool),
    Integer(i64),
    Float(f32),
    /// Normalized `[red, green, blue, alpha]` channels for known color attributes.
    Color([f32; 4]),
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct DomAttrs {
    pub id: Option<String>,
    pub attributes: BTreeMap<DomAttribute, DomAttributeValue>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
/// Public canonical style list for replaying authored style inputs.
///
/// The list is serialized as ordered `(property-name, value)` pairs — the
/// public wire shape for replaying authored declarations — so callers can
/// persist a stable browser-style declaration list without depending on
/// internal node-owned maps or arena-resident declaration keys. Property names
/// are the canonical kebab-case `PresentationProperty::name()` strings; values
/// use `String`/`Color` only.
pub struct DomCanonicalStyleSnapshot {
    pub styles: Vec<(String, DomAttributeValue)>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
/// Public canonical layout variables for replaying browser-style sizing math.
///
/// This snapshot intentionally contains only the well-known layout variables
/// that Gecko- and Blink-style algorithms refer to directly.
pub struct DomCanonicalLayoutSnapshot {
    pub intrinsic: crate::types::DomIntrinsic,
    pub available_width_px: f32,
    pub available_height_px: f32,
    pub used_width_px: f32,
    pub used_height_px: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DomAttrPatch {
    pub id: Option<Option<String>>,
    pub attributes: Option<BTreeMap<DomAttribute, DomAttributeValue>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomError {
    InvalidNodeKey,
    RetainedGpuApplyFailed,
    /// The selector string uses unsupported or malformed selector syntax.
    InvalidSelector,
    InvalidParentTarget,
    InvalidAttributeTarget,
    InvalidPseudoStateTarget,
    InvalidAttributeValue,
    /// a presentation property was written through the attribute API;
    /// presentation is written through `style_mut` (the declaration store) —
    /// the style attribute (`style`) itself remains accepted and routes into
    /// the inline declaration store.
    PresentationRequiresStyleMut,
    UnmeasuredIntrinsic,
    CannotRemoveRoot,
    CycleCreationAttempt,
    OwnerDocumentMismatch,
    ParentMismatch,
    ChildNotFound,
    DuplicateChildInsertion,
    DuplicateElementId,
}

pub(crate) fn parse_authored_value_str(
    attribute: &DomAttribute,
    value: &str,
) -> Result<DomAttributeValue, DomError> {
    if let Some(parsed_value) = parse_presented_options_attr_value(attribute, value)? {
        return Ok(parsed_value);
    }
    if matches_boolean_attribute(attribute) && value == "true" {
        return Ok(DomAttributeValue::Bool(true));
    }
    if matches_boolean_attribute(attribute) && value == "false" {
        return Ok(DomAttributeValue::Bool(false));
    }

    match authored_value_kind(attribute) {
        AuthoredValueKind::PresentedOptionsParsed => {
            unreachable!("presented-options attributes should parse before generic authored values")
        }
        AuthoredValueKind::String => Ok(DomAttributeValue::String(value.to_owned())),
        AuthoredValueKind::Integer => parse_integer_authored_value(value),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AuthoredValueKind {
    PresentedOptionsParsed,
    String,
    Integer,
}

fn authored_value_kind(attribute: &DomAttribute) -> AuthoredValueKind {
    match attribute {
        DomAttribute::PresentedOptionsRole
        | DomAttribute::PresentedOptionsState
        | DomAttribute::PresentedOptionsHighlighted
        | DomAttribute::PresentedOptionsSourceIndex => AuthoredValueKind::PresentedOptionsParsed,
        DomAttribute::Size | DomAttribute::TabIndex => AuthoredValueKind::Integer,
        _ => AuthoredValueKind::String,
    }
}

fn matches_boolean_attribute(attribute: &DomAttribute) -> bool {
    matches!(
        attribute,
        DomAttribute::ReadOnly
            | DomAttribute::Disabled
            | DomAttribute::Required
            | DomAttribute::Checked
            | DomAttribute::Selected
            | DomAttribute::Multiple
    )
}

fn parse_presented_options_attr_value(
    attribute: &DomAttribute,
    value: &str,
) -> Result<Option<DomAttributeValue>, DomError> {
    let parsed_value = match attribute {
        DomAttribute::PresentedOptionsRole | DomAttribute::PresentedOptionsState => {
            DomAttributeValue::String(value.to_owned())
        }
        DomAttribute::PresentedOptionsHighlighted => value
            .parse::<bool>()
            .map(DomAttributeValue::Bool)
            .map_err(|_| DomError::InvalidAttributeValue)?,
        DomAttribute::PresentedOptionsSourceIndex => value
            .parse::<i64>()
            .map(DomAttributeValue::Integer)
            .map_err(|_| DomError::InvalidAttributeValue)?,
        _ => return Ok(None),
    };
    Ok(Some(parsed_value))
}

fn parse_integer_authored_value(value: &str) -> Result<DomAttributeValue, DomError> {
    value
        .parse::<i64>()
        .map(DomAttributeValue::Integer)
        .map_err(|_| DomError::InvalidAttributeValue)
}

pub(crate) fn parse_authored_value_input(
    attribute: &DomAttribute,
    value: &str,
) -> Result<Option<DomAttributeValue>, DomError> {
    if value == "unset" {
        return Ok(None);
    }
    parse_authored_value_str(attribute, value).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_value_input_parses_boolean_form_attributes() {
        assert_eq!(
            parse_authored_value_input(&DomAttribute::Checked, "true")
                .expect("checked true should parse"),
            Some(DomAttributeValue::Bool(true))
        );
        assert_eq!(
            parse_authored_value_input(&DomAttribute::Required, "false")
                .expect("required false should parse"),
            Some(DomAttributeValue::Bool(false))
        );
    }

    #[test]
    fn authored_value_input_preserves_content_values_as_strings() {
        let expected = [
            (DomAttribute::Class, "panel hero"),
            (DomAttribute::Id, "subject"),
            (DomAttribute::Placeholder, "Type here"),
            (DomAttribute::Src, "icon:arrow"),
            (DomAttribute::Custom("data-role".to_string()), "row"),
        ];

        for (attribute, value) in expected {
            assert_eq!(
                parse_authored_value_input(&attribute, value).expect("content value should parse"),
                Some(DomAttributeValue::String(value.to_string()))
            );
        }
    }

    #[test]
    fn canonical_style_and_layout_targets_round_trip_through_serde_json() {
        let attrs = DomAttrs {
            id: Some("subject".to_string()),
            attributes: BTreeMap::from([
                (
                    DomAttribute::Class,
                    DomAttributeValue::String("panel".to_string()),
                ),
                (
                    DomAttribute::Value,
                    DomAttributeValue::String("hello".to_string()),
                ),
                (
                    DomAttribute::Type,
                    DomAttributeValue::String("text".to_string()),
                ),
            ]),
        };
        let serialized_attrs =
            serde_json::to_string(&attrs).expect("canonical style attrs should serialize");
        let deserialized_attrs: DomAttrs = serde_json::from_str(&serialized_attrs)
            .expect("canonical style attrs should deserialize");
        assert_eq!(deserialized_attrs, attrs);

        let layout = DomLayoutState {
            dirty: false,
            layer_kind: DomLayerKind::Normal,
            layer_order: LayerOrder::Raised,
            available_width_px: 320.0,
            available_height_px: 180.0,
            width_px: 120.0,
            height_px: 40.0,
            content_height_px: 40.0,
        };
        let serialized_layout =
            serde_json::to_string(&layout).expect("canonical layout should serialize");
        let deserialized_layout: DomLayoutState =
            serde_json::from_str(&serialized_layout).expect("canonical layout should deserialize");
        assert_eq!(deserialized_layout, layout);

        let intrinsic = DomIntrinsic::Text(TextIntrinsic {
            font_size_px: 16.0,
            min_content_width_px: 48.0,
            max_content_width_px: 96.0,
            line_height_px: 20.0,
            ..Default::default()
        });
        let serialized_intrinsic =
            serde_json::to_string(&intrinsic).expect("canonical intrinsic should serialize");
        let deserialized_intrinsic: DomIntrinsic = serde_json::from_str(&serialized_intrinsic)
            .expect("canonical intrinsic should deserialize");
        assert_eq!(deserialized_intrinsic, intrinsic);
    }
}
