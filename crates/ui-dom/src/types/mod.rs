//! Public DOM types and authored value shapes for `ui-dom`.
//!
//! This file represents the shared type boundary used across the rest of the
//! crate and by external callers. Its purpose is to define stable authored DOM
//! values, errors, supporting structs, and root-owned metric vocabulary
//! without mixing in runtime-domain implementation details.
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
pub mod box_model;
pub mod dom_attr_node;
mod dom_attr_parse;
pub mod dom_event;
mod dom_event_behavior;
mod dom_event_definition;
mod dom_event_descriptor_builder;
pub mod dom_event_detail;
pub mod dom_event_dispatch_context;
pub mod dom_event_listener;
pub mod dom_event_listener_context;
pub mod dom_event_name;
pub mod dom_node;
mod dom_ops;
pub mod dom_pseudo_state;
mod dom_query_selector;
pub mod dom_tag;
pub mod dom_transition;
pub mod element_behaviors;
mod event_listener_name_bits;
pub mod primitives;
pub mod root_dom;
pub mod root_dom_context;
pub mod root_dom_fragment;
pub mod root_dom_metrics;
pub mod root_dom_nodes;
mod white_space;

pub use box_model::*;
pub use dom_attr_node::*;
pub use dom_attr_parse::parse_css_color_rgba;
pub(crate) use dom_attr_parse::{
    AuthoredLength, parse_authored_length, parse_style_attr_map, resolve_line_height_px,
};
pub use dom_event::*;
pub use dom_event_listener::*;
pub use dom_event_name::*;
pub use dom_node::*;
pub use dom_pseudo_state::*;
pub use dom_tag::*;
pub use dom_transition::*;
pub use element_behaviors::DomElementIntrinsicKind;
pub use primitives::*;
pub use root_dom::*;
pub use root_dom_context::*;
pub use root_dom_fragment::*;
pub use root_dom_metrics::*;
pub use root_dom_nodes::*;
pub(crate) use white_space::{
    DomOverflowWrap, DomTextAlign, DomTextDirection, DomWhiteSpace, DomWordBreak,
    overflow_wrap_from_attr_value, parse_text_align_keyword, parse_text_direction_keyword,
    parse_white_space_keyword, text_align_from_attr_value, text_direction_from_attr_value,
    text_layout_input_and_byte_map_for_white_space, text_layout_input_for_white_space,
    white_space_from_attr_value, word_break_from_attr_value,
};

use std::collections::BTreeMap;
use std::convert::TryFrom;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum DomAttribute {
    Id,
    Style,
    Class,
    Transition,
    TransitionProperty,
    TransitionDuration,
    TransitionTimingFunction,
    TransitionDelay,
    ReadOnly,
    Disabled,
    Required,
    Checked,
    Selected,
    Multiple,
    /// Focusability override; non-negative values are focusable in v1.
    TabIndex,
    Display,
    Overflow,
    Transform,
    Isolate,
    /// Controls whether authored DOM positioning semantics should create a layer.
    Position,
    /// Authored flex-direction attribute; parsed later by flex layout work.
    FlexDirection,
    /// Authored flex-wrap attribute; parsed later by flex layout work.
    FlexWrap,
    /// Authored flex-flow attribute; parsed later by flex layout work.
    FlexFlow,
    /// Authored flex-grow attribute; parsed later by flex layout work.
    FlexGrow,
    /// Authored flex-shrink attribute; parsed later by flex layout work.
    FlexShrink,
    /// Authored flex-basis attribute; parsed later by flex layout work.
    FlexBasis,
    /// Authored flex shorthand attribute; parsed later by flex layout work.
    Flex,
    /// Authored order attribute; parsed later by flex layout work.
    Order,
    /// Authored justify-content attribute; parsed later by flex layout work.
    JustifyContent,
    /// Authored align-items attribute; parsed later by flex layout work.
    AlignItems,
    /// Authored align-self attribute; parsed later by flex layout work.
    AlignSelf,
    /// Authored align-content attribute; parsed later by flex layout work.
    AlignContent,
    /// Authored gap attribute; parsed later by flex layout work.
    Gap,
    /// Authored row-gap attribute; parsed later by flex layout work.
    RowGap,
    /// Authored column-gap attribute; parsed later by flex layout work.
    ColumnGap,
    /// Authored place-content attribute; parsed later by flex layout work.
    PlaceContent,
    /// Authored place-items attribute; parsed later by flex layout work.
    PlaceItems,
    /// Authored place-self attribute; parsed later by flex layout work.
    PlaceSelf,
    /// Authored width attribute; parsed later by layout work.
    Width,
    /// Authored height attribute; parsed later by layout work.
    Height,
    /// Authored minimum width attribute; parsed later by layout work.
    MinWidth,
    /// Authored minimum height attribute; parsed later by layout work.
    MinHeight,
    /// Authored maximum width attribute; parsed later by layout work.
    MaxWidth,
    /// Authored maximum height attribute; parsed later by layout work.
    MaxHeight,
    /// Authored top edge attribute; parsed later by layout work.
    Top,
    /// Authored left edge attribute; parsed later by layout work.
    Left,
    /// Authored right edge attribute; parsed later by layout work.
    Right,
    /// Authored bottom edge attribute; parsed later by layout work.
    Bottom,
    /// Authored top margin attribute; parsed later by layout work.
    MarginTop,
    /// Authored left margin attribute; parsed later by layout work.
    MarginLeft,
    /// Authored right margin attribute; parsed later by layout work.
    MarginRight,
    /// Authored bottom margin attribute; parsed later by layout work.
    MarginBottom,
    /// Authored top padding attribute; parsed later by layout work.
    PaddingTop,
    /// Authored left padding attribute; parsed later by layout work.
    PaddingLeft,
    /// Authored right padding attribute; parsed later by layout work.
    PaddingRight,
    /// Authored bottom padding attribute; parsed later by layout work.
    PaddingBottom,
    /// Authored top border width attribute; parsed later by layout work.
    BorderTop,
    /// Authored left border width attribute; parsed later by layout work.
    BorderLeft,
    /// Authored right border width attribute; parsed later by layout work.
    BorderRight,
    /// Authored bottom border width attribute; parsed later by layout work.
    BorderBottom,
    /// Authored font family attribute; parsed later by style and text work.
    FontFamily,
    /// Authored font weight attribute; parsed later by style and text work.
    FontWeight,
    /// Authored font style attribute; parsed later by style and text work.
    FontStyle,
    /// Authored font size attribute; parsed later by style and text work.
    FontSize,
    /// Authored line-height attribute; parsed later by style and text work.
    LineHeight,
    /// Authored text alignment attribute; parsed later by style and text work.
    TextAlign,
    /// Authored white-space attribute; parsed later by style and text work.
    WhiteSpace,
    /// Authored overflow-wrap attribute; parsed later by style and text work.
    OverflowWrap,
    /// Authored word-break attribute; parsed later by style and text work.
    WordBreak,
    /// Authored language hint attribute; parsed later by text work.
    Lang,
    /// Authored direction attribute; parsed later by text work.
    Dir,
    /// Authored CSS direction property; parsed later by text work.
    Direction,
    Src,
    List,
    Label,
    Max,
    MaxLength,
    Min,
    MinLength,
    Size,
    Placeholder,
    PresentedOptionsRole,
    PresentedOptionsState,
    PresentedOptionsHighlighted,
    PresentedOptionsSourceIndex,
    Step,
    Type,
    Value,
    Custom(String),
}

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
/// The list is serialized as ordered `(attribute, value)` pairs so callers can
/// persist a stable browser-style declaration list without depending on
/// internal node-owned maps.
pub struct DomCanonicalStyleSnapshot {
    pub styles: Vec<(DomAttribute, DomAttributeValue)>,
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

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
/// Public canonical snapshot for replaying authored style inputs and layout
/// variables together.
pub struct DomCanonicalSnapshot {
    pub style: DomCanonicalStyleSnapshot,
    pub layout: DomCanonicalLayoutSnapshot,
}

impl DomCanonicalSnapshot {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
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
    UnmeasuredIntrinsic,
    CannotRemoveRoot,
    CycleCreationAttempt,
    OwnerDocumentMismatch,
    ParentMismatch,
    ChildNotFound,
    DuplicateChildInsertion,
    DuplicateElementId,
}

impl TryFrom<&str> for DomAttribute {
    type Error = DomError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(DomError::InvalidAttributeValue);
        }
        Ok(match value {
            "id" => Self::Id,
            "style" => Self::Style,
            "class" => Self::Class,
            "transition" => Self::Transition,
            "transition-property" => Self::TransitionProperty,
            "transition-duration" => Self::TransitionDuration,
            "transition-timing-function" => Self::TransitionTimingFunction,
            "transition-delay" => Self::TransitionDelay,
            "readonly" => Self::ReadOnly,
            "disabled" => Self::Disabled,
            "required" => Self::Required,
            "checked" => Self::Checked,
            "selected" => Self::Selected,
            "multiple" => Self::Multiple,
            "tab-index" => Self::TabIndex,
            "display" => Self::Display,
            "overflow" => Self::Overflow,
            "transform" => Self::Transform,
            "isolate" => Self::Isolate,
            "position" => Self::Position,
            "flex-direction" => Self::FlexDirection,
            "flex-wrap" => Self::FlexWrap,
            "flex-flow" => Self::FlexFlow,
            "flex-grow" => Self::FlexGrow,
            "flex-shrink" => Self::FlexShrink,
            "flex-basis" => Self::FlexBasis,
            "flex" => Self::Flex,
            "order" => Self::Order,
            "justify-content" => Self::JustifyContent,
            "align-items" => Self::AlignItems,
            "align-self" => Self::AlignSelf,
            "align-content" => Self::AlignContent,
            "gap" => Self::Gap,
            "row-gap" => Self::RowGap,
            "column-gap" => Self::ColumnGap,
            "place-content" => Self::PlaceContent,
            "place-items" => Self::PlaceItems,
            "place-self" => Self::PlaceSelf,
            "width" => Self::Width,
            "height" => Self::Height,
            "min-width" => Self::MinWidth,
            "min-height" => Self::MinHeight,
            "max-width" => Self::MaxWidth,
            "max-height" => Self::MaxHeight,
            "top" => Self::Top,
            "left" => Self::Left,
            "right" => Self::Right,
            "bottom" => Self::Bottom,
            "margin-top" => Self::MarginTop,
            "margin-left" => Self::MarginLeft,
            "margin-right" => Self::MarginRight,
            "margin-bottom" => Self::MarginBottom,
            "padding-top" => Self::PaddingTop,
            "padding-left" => Self::PaddingLeft,
            "padding-right" => Self::PaddingRight,
            "padding-bottom" => Self::PaddingBottom,
            "border-top" => Self::BorderTop,
            "border-left" => Self::BorderLeft,
            "border-right" => Self::BorderRight,
            "border-bottom" => Self::BorderBottom,
            "font-family" => Self::FontFamily,
            "font-weight" => Self::FontWeight,
            "font-style" => Self::FontStyle,
            "font-size" => Self::FontSize,
            "line-height" => Self::LineHeight,
            "text-align" => Self::TextAlign,
            "white-space" => Self::WhiteSpace,
            "overflow-wrap" => Self::OverflowWrap,
            "word-break" => Self::WordBreak,
            "lang" => Self::Lang,
            "dir" => Self::Dir,
            "direction" => Self::Direction,
            "src" => Self::Src,
            "list" => Self::List,
            "label" => Self::Label,
            "max" => Self::Max,
            "maxlength" => Self::MaxLength,
            "min" => Self::Min,
            "minlength" => Self::MinLength,
            "size" => Self::Size,
            "placeholder" => Self::Placeholder,
            "presented-options-role" => Self::PresentedOptionsRole,
            "presented-options-state" => Self::PresentedOptionsState,
            "presented-options-highlighted" => Self::PresentedOptionsHighlighted,
            "presented-options-source-index" => Self::PresentedOptionsSourceIndex,
            "step" => Self::Step,
            "type" => Self::Type,
            "value" => Self::Value,
            custom => Self::Custom(custom.to_owned()),
        })
    }
}

pub(crate) fn parse_authored_value_str(
    attribute: &DomAttribute,
    value: &str,
) -> Result<DomAttributeValue, DomError> {
    if let Some(parsed_value) = parse_presented_options_attr_value(attribute, value)? {
        return Ok(parsed_value);
    }
    if dom_attr_parse::is_known_color_attribute(attribute) {
        return parse_css_color_rgba(value).map(DomAttributeValue::Color);
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
        DomAttribute::Custom(_)
        | DomAttribute::Id
        | DomAttribute::Style
        | DomAttribute::Class
        | DomAttribute::Transition
        | DomAttribute::TransitionProperty
        | DomAttribute::TransitionDuration
        | DomAttribute::TransitionTimingFunction
        | DomAttribute::TransitionDelay
        | DomAttribute::ReadOnly
        | DomAttribute::Disabled
        | DomAttribute::Required
        | DomAttribute::Checked
        | DomAttribute::Selected
        | DomAttribute::Multiple
        | DomAttribute::Display
        | DomAttribute::Overflow
        | DomAttribute::Transform
        | DomAttribute::Position
        | DomAttribute::FlexDirection
        | DomAttribute::FlexWrap
        | DomAttribute::FlexFlow
        | DomAttribute::FlexGrow
        | DomAttribute::FlexShrink
        | DomAttribute::FlexBasis
        | DomAttribute::Flex
        | DomAttribute::Order
        | DomAttribute::JustifyContent
        | DomAttribute::AlignItems
        | DomAttribute::AlignSelf
        | DomAttribute::AlignContent
        | DomAttribute::Gap
        | DomAttribute::RowGap
        | DomAttribute::ColumnGap
        | DomAttribute::PlaceContent
        | DomAttribute::PlaceItems
        | DomAttribute::PlaceSelf
        | DomAttribute::Width
        | DomAttribute::Height
        | DomAttribute::MinWidth
        | DomAttribute::MinHeight
        | DomAttribute::MaxWidth
        | DomAttribute::MaxHeight
        | DomAttribute::Top
        | DomAttribute::Left
        | DomAttribute::Right
        | DomAttribute::Bottom
        | DomAttribute::MarginTop
        | DomAttribute::MarginLeft
        | DomAttribute::MarginRight
        | DomAttribute::MarginBottom
        | DomAttribute::PaddingTop
        | DomAttribute::PaddingLeft
        | DomAttribute::PaddingRight
        | DomAttribute::PaddingBottom
        | DomAttribute::BorderTop
        | DomAttribute::BorderLeft
        | DomAttribute::BorderRight
        | DomAttribute::BorderBottom
        | DomAttribute::FontFamily
        | DomAttribute::FontWeight
        | DomAttribute::FontStyle
        | DomAttribute::FontSize
        | DomAttribute::LineHeight
        | DomAttribute::TextAlign
        | DomAttribute::WhiteSpace
        | DomAttribute::OverflowWrap
        | DomAttribute::WordBreak
        | DomAttribute::Lang
        | DomAttribute::Dir
        | DomAttribute::Direction
        | DomAttribute::Src
        | DomAttribute::List
        | DomAttribute::Label
        | DomAttribute::Max
        | DomAttribute::MaxLength
        | DomAttribute::Min
        | DomAttribute::MinLength
        | DomAttribute::Placeholder
        | DomAttribute::Step
        | DomAttribute::Type
        | DomAttribute::Value
        | DomAttribute::Isolate => AuthoredValueKind::String,
        DomAttribute::Size | DomAttribute::TabIndex => AuthoredValueKind::Integer,
    }
}

fn matches_boolean_attribute(attribute: &DomAttribute) -> bool {
    matches!(
        attribute,
        DomAttribute::Isolate
            | DomAttribute::ReadOnly
            | DomAttribute::Disabled
            | DomAttribute::Required
            | DomAttribute::Checked
            | DomAttribute::Selected
            | DomAttribute::PresentedOptionsHighlighted
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
    fn dom_attribute_recognizes_new_text_and_form_names() {
        assert_eq!(
            DomAttribute::try_from("readonly").expect("readonly should parse"),
            DomAttribute::ReadOnly
        );
        assert_eq!(
            DomAttribute::try_from("font-family").expect("font-family should parse"),
            DomAttribute::FontFamily
        );
        assert_eq!(
            DomAttribute::try_from("font-weight").expect("font-weight should parse"),
            DomAttribute::FontWeight
        );
        assert_eq!(
            DomAttribute::try_from("font-style").expect("font-style should parse"),
            DomAttribute::FontStyle
        );
        assert_eq!(
            DomAttribute::try_from("line-height").expect("line-height should parse"),
            DomAttribute::LineHeight
        );
        assert_eq!(
            DomAttribute::try_from("text-align").expect("text-align should parse"),
            DomAttribute::TextAlign
        );
        assert_eq!(
            DomAttribute::try_from("white-space").expect("white-space should parse"),
            DomAttribute::WhiteSpace
        );
        assert_eq!(
            DomAttribute::try_from("overflow-wrap").expect("overflow-wrap should parse"),
            DomAttribute::OverflowWrap
        );
        assert_eq!(
            DomAttribute::try_from("word-break").expect("word-break should parse"),
            DomAttribute::WordBreak
        );
        assert_eq!(
            DomAttribute::try_from("lang").expect("lang should parse"),
            DomAttribute::Lang
        );
        assert_eq!(
            DomAttribute::try_from("dir").expect("dir should parse"),
            DomAttribute::Dir
        );
        assert_eq!(
            DomAttribute::try_from("direction").expect("direction should parse"),
            DomAttribute::Direction
        );
        assert_eq!(
            DomAttribute::try_from("maxlength").expect("maxlength should parse"),
            DomAttribute::MaxLength
        );
        assert_eq!(
            DomAttribute::try_from("placeholder").expect("placeholder should parse"),
            DomAttribute::Placeholder
        );
    }

    #[test]
    fn dom_attribute_recognizes_flex_surface_names() {
        let expected = [
            ("flex-direction", DomAttribute::FlexDirection),
            ("flex-wrap", DomAttribute::FlexWrap),
            ("flex-flow", DomAttribute::FlexFlow),
            ("flex-grow", DomAttribute::FlexGrow),
            ("flex-shrink", DomAttribute::FlexShrink),
            ("flex-basis", DomAttribute::FlexBasis),
            ("flex", DomAttribute::Flex),
            ("order", DomAttribute::Order),
            ("justify-content", DomAttribute::JustifyContent),
            ("align-items", DomAttribute::AlignItems),
            ("align-self", DomAttribute::AlignSelf),
            ("align-content", DomAttribute::AlignContent),
            ("gap", DomAttribute::Gap),
            ("row-gap", DomAttribute::RowGap),
            ("column-gap", DomAttribute::ColumnGap),
            ("place-content", DomAttribute::PlaceContent),
            ("place-items", DomAttribute::PlaceItems),
            ("place-self", DomAttribute::PlaceSelf),
        ];

        for (name, attribute) in expected {
            assert_eq!(
                DomAttribute::try_from(name).expect("flex surface name should parse"),
                attribute
            );
        }
    }

    #[test]
    fn dom_attribute_recognizes_transition_longhand_names() {
        let expected = [
            ("transition-property", DomAttribute::TransitionProperty),
            ("transition-duration", DomAttribute::TransitionDuration),
            (
                "transition-timing-function",
                DomAttribute::TransitionTimingFunction,
            ),
            ("transition-delay", DomAttribute::TransitionDelay),
        ];

        for (name, attribute) in expected {
            assert_eq!(
                DomAttribute::try_from(name).expect("transition longhand should parse"),
                attribute
            );
        }
    }

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
    fn authored_value_input_preserves_flex_surface_values_as_strings() {
        let expected = [
            (DomAttribute::FlexDirection, "row"),
            (DomAttribute::FlexWrap, "wrap"),
            (DomAttribute::FlexFlow, "column wrap"),
            (DomAttribute::FlexGrow, "2"),
            (DomAttribute::FlexShrink, "3"),
            (DomAttribute::FlexBasis, "20px"),
            (DomAttribute::Flex, "1 0 auto"),
            (DomAttribute::Order, "4"),
            (DomAttribute::JustifyContent, "space-between"),
            (DomAttribute::AlignItems, "stretch"),
            (DomAttribute::AlignSelf, "center"),
            (DomAttribute::AlignContent, "space-around"),
            (DomAttribute::Gap, "12px"),
            (DomAttribute::RowGap, "8px"),
            (DomAttribute::ColumnGap, "10px"),
            (DomAttribute::PlaceContent, "center start"),
            (DomAttribute::PlaceItems, "stretch center"),
            (DomAttribute::PlaceSelf, "end auto"),
        ];

        for (attribute, value) in expected {
            assert_eq!(
                parse_authored_value_input(&attribute, value)
                    .expect("flex surface value should parse"),
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
                    DomAttribute::Width,
                    DomAttributeValue::String("120px".to_string()),
                ),
                (
                    DomAttribute::Height,
                    DomAttributeValue::String("40px".to_string()),
                ),
                (
                    DomAttribute::FontSize,
                    DomAttributeValue::String("16px".to_string()),
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
            generation: 4,
        });
        let serialized_intrinsic =
            serde_json::to_string(&intrinsic).expect("canonical intrinsic should serialize");
        let deserialized_intrinsic: DomIntrinsic = serde_json::from_str(&serialized_intrinsic)
            .expect("canonical intrinsic should deserialize");
        assert_eq!(deserialized_intrinsic, intrinsic);
    }
}
