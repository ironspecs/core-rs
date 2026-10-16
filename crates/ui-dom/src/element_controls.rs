use crate::types::box_model::DomBoxModel;
use crate::types::dom_node::DomElementNode;
use crate::types::dom_tag::{DomElementEditKind, DomElementLifecycleBehavior, DomTag};
use crate::types::primitives::{DomRect, PointPx};
use crate::types::{DomAttribute, DomAttributeValue};

#[cfg(test)]
use crate::types::box_model::BoxEdgeInsets;
#[cfg(test)]
use crate::types::dom_attr_node::DomAttributeNode;
#[cfg(test)]
use crate::types::{DomNode, DomNodeSlotKey};

pub(crate) use crate::resolved_numeric_control::ResolvedNumericControl;

pub(crate) const DEFAULT_RANGE_MIN_VALUE: f32 = 0.0;
pub(crate) const DEFAULT_RANGE_MAX_VALUE: f32 = 100.0;
const DEFAULT_STEP_VALUE: f32 = 1.0;
const DEFAULT_RANGE_WIDTH_PX: f32 = 120.0;
const DEFAULT_CHECKBOX_SIZE_PX: f32 = 18.0;
const MIN_RANGE_TRACK_HEIGHT_PX: f32 = 4.0;
const MIN_RANGE_THUMB_SIZE_PX: f32 = 14.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ResolvedElementControl {
    TextInput,
    Textarea,
    Checkbox(ResolvedCheckboxControl),
    Number(ResolvedNumberControl),
    Range(ResolvedRangeControl),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedCheckboxControl {
    pub checked: bool,
    pub disabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ResolvedNumberControl {
    pub numeric: ResolvedNumericControl,
    pub readonly: bool,
    pub disabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ResolvedRangeControl {
    pub numeric: ResolvedNumericControl,
    pub disabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResolvedElementControlPart {
    WholeControl,
    NumberDecrementButton,
    NumberEditor,
    NumberIncrementButton,
    RangeTrack,
    RangeThumb,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NumericStepDirection {
    Decrement,
    Increment,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NumericControlKind {
    Number,
    Range,
}

#[must_use]
pub(crate) fn resolved_element_control(element: &DomElementNode) -> Option<ResolvedElementControl> {
    match element.tag {
        DomTag::Textarea => Some(ResolvedElementControl::Textarea),
        DomTag::Input => Some(resolved_input_control(element)),
        _ => None,
    }
}

#[must_use]
pub(crate) fn resolved_text_edit_kind(element: &DomElementNode) -> DomElementEditKind {
    match resolved_element_control(element) {
        Some(ResolvedElementControl::Textarea) => DomElementEditKind::MultiLine,
        Some(ResolvedElementControl::Number(_) | ResolvedElementControl::TextInput) => {
            DomElementEditKind::SingleLine
        }
        Some(ResolvedElementControl::Checkbox(_) | ResolvedElementControl::Range(_)) => {
            DomElementEditKind::None
        }
        None => element.behavior().edit_kind,
    }
}

#[must_use]
pub(crate) fn checkbox_default_preferred_size_px(line_height_px: f32) -> (f32, f32) {
    let edge_px = line_height_px.max(DEFAULT_CHECKBOX_SIZE_PX);
    (edge_px, edge_px)
}

#[must_use]
pub(crate) fn range_default_preferred_size_px(line_height_px: f32) -> (f32, f32) {
    (
        DEFAULT_RANGE_WIDTH_PX,
        line_height_px.max(MIN_RANGE_THUMB_SIZE_PX),
    )
}

#[must_use]
pub(crate) fn number_editor_box_model(box_model: DomBoxModel) -> DomBoxModel {
    let stepper_size_px = number_stepper_size_px(box_model);
    let border_box = DomRect {
        left_px: box_model.border_box.left_px + stepper_size_px,
        top_px: box_model.border_box.top_px,
        width_px: (box_model.border_box.width_px - stepper_size_px * 2.0).max(0.0),
        height_px: box_model.border_box.height_px,
    };
    DomBoxModel {
        border_box,
        ..box_model
    }
}

#[must_use]
pub(crate) fn number_stepper_rects(box_model: DomBoxModel) -> (DomRect, DomRect) {
    let stepper_size_px = number_stepper_size_px(box_model);
    let decrement_rect = DomRect {
        left_px: box_model.border_box.left_px,
        top_px: box_model.border_box.top_px,
        width_px: stepper_size_px,
        height_px: box_model.border_box.height_px.max(0.0),
    };
    let increment_rect = DomRect {
        left_px: box_model.border_box.left_px + (box_model.border_box.width_px - stepper_size_px),
        top_px: box_model.border_box.top_px,
        width_px: stepper_size_px,
        height_px: box_model.border_box.height_px.max(0.0),
    };
    (decrement_rect, increment_rect)
}

#[must_use]
pub(crate) fn checkbox_indicator_rect(box_model: DomBoxModel) -> DomRect {
    let edge_px = box_model
        .border_box
        .width_px
        .min(box_model.border_box.height_px)
        .max(0.0);
    let left_px = box_model.border_box.left_px + ((box_model.border_box.width_px - edge_px) * 0.5);
    let top_px = box_model.border_box.top_px + ((box_model.border_box.height_px - edge_px) * 0.5);
    DomRect {
        left_px,
        top_px,
        width_px: edge_px,
        height_px: edge_px,
    }
}

#[must_use]
pub(crate) fn range_track_rect(box_model: DomBoxModel) -> DomRect {
    let thumb_size_px = range_thumb_size_px(box_model);
    let track_height_px = (box_model.border_box.height_px * 0.25).max(MIN_RANGE_TRACK_HEIGHT_PX);
    DomRect {
        left_px: box_model.border_box.left_px + thumb_size_px * 0.5,
        top_px: box_model.border_box.top_px
            + ((box_model.border_box.height_px - track_height_px) * 0.5),
        width_px: (box_model.border_box.width_px - thumb_size_px).max(0.0),
        height_px: track_height_px,
    }
}

#[must_use]
pub(crate) fn range_thumb_rect(box_model: DomBoxModel, control: ResolvedRangeControl) -> DomRect {
    let track_rect = range_track_rect(box_model);
    let thumb_size_px = range_thumb_size_px(box_model);
    DomRect {
        left_px: track_rect.left_px + track_rect.width_px * control.numeric.value_fraction()
            - thumb_size_px * 0.5,
        top_px: box_model.border_box.top_px
            + ((box_model.border_box.height_px - thumb_size_px) * 0.5),
        width_px: thumb_size_px,
        height_px: thumb_size_px,
    }
}

#[must_use]
pub(crate) fn range_value_string_from_point(
    box_model: DomBoxModel,
    control: ResolvedRangeControl,
    point: PointPx,
) -> String {
    let track_rect = range_track_rect(box_model);
    let fraction = if track_rect.width_px <= f32::EPSILON {
        0.0
    } else {
        ((point.x_px - track_rect.left_px) / track_rect.width_px).clamp(0.0, 1.0)
    };
    control.numeric.value_string_from_fraction(fraction)
}

#[must_use]
pub(crate) fn resolved_control_part_from_point(
    element: &DomElementNode,
    box_model: DomBoxModel,
    point: PointPx,
) -> Option<ResolvedElementControlPart> {
    if element.behavior().hit_test_behavior != DomElementLifecycleBehavior::ElementControls
        || !point_in_rect(point, box_model.border_box)
    {
        return None;
    }
    match resolved_element_control(element)? {
        ResolvedElementControl::Checkbox(_)
        | ResolvedElementControl::TextInput
        | ResolvedElementControl::Textarea => Some(ResolvedElementControlPart::WholeControl),
        ResolvedElementControl::Number(_) => {
            let (decrement_rect, increment_rect) = number_stepper_rects(box_model);
            if point_in_rect(point, decrement_rect) {
                return Some(ResolvedElementControlPart::NumberDecrementButton);
            }
            if point_in_rect(point, increment_rect) {
                return Some(ResolvedElementControlPart::NumberIncrementButton);
            }
            Some(ResolvedElementControlPart::NumberEditor)
        }
        ResolvedElementControl::Range(control) => {
            if point_in_rect(point, range_thumb_rect(box_model, control)) {
                return Some(ResolvedElementControlPart::RangeThumb);
            }
            Some(ResolvedElementControlPart::RangeTrack)
        }
    }
}

#[must_use]
fn resolved_input_control(element: &DomElementNode) -> ResolvedElementControl {
    let input_type = string_attr_value(element, &DomAttribute::Type).unwrap_or("text");
    match input_type {
        "checkbox" => ResolvedElementControl::Checkbox(ResolvedCheckboxControl {
            checked: bool_attr_value(element, &DomAttribute::Checked),
            disabled: bool_attr_value(element, &DomAttribute::Disabled),
        }),
        "number" => ResolvedElementControl::Number(ResolvedNumberControl {
            numeric: ResolvedNumericControl::new(element, NumericControlKind::Number),
            readonly: bool_attr_value(element, &DomAttribute::ReadOnly),
            disabled: bool_attr_value(element, &DomAttribute::Disabled),
        }),
        "range" => ResolvedElementControl::Range(ResolvedRangeControl {
            numeric: ResolvedNumericControl::new(element, NumericControlKind::Range),
            disabled: bool_attr_value(element, &DomAttribute::Disabled),
        }),
        _ => ResolvedElementControl::TextInput,
    }
}

#[must_use]
fn string_attr_value<'a>(element: &'a DomElementNode, attribute: &DomAttribute) -> Option<&'a str> {
    match element.attrs.attributes.get(attribute) {
        Some(DomAttributeValue::String(value)) => Some(value.as_str()),
        _ => None,
    }
}

#[must_use]
fn bool_attr_value(element: &DomElementNode, attribute: &DomAttribute) -> bool {
    matches!(
        element.attrs.attributes.get(attribute),
        Some(DomAttributeValue::Bool(true))
    )
}

#[must_use]
pub(crate) fn parsed_string_attr(
    element: &DomElementNode,
    attribute: &DomAttribute,
) -> Option<f32> {
    string_attr_value(element, attribute).and_then(|value| value.parse::<f32>().ok())
}

#[must_use]
pub(crate) fn parsed_positive_step_attr(element: &DomElementNode) -> f32 {
    parsed_string_attr(element, &DomAttribute::Step)
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(DEFAULT_STEP_VALUE)
}

#[must_use]
pub(crate) fn numeric_value_string(value_px: f32) -> String {
    format!("{value_px}")
}

#[must_use]
fn number_stepper_size_px(box_model: DomBoxModel) -> f32 {
    box_model.border_box.height_px.max(0.0)
}

#[must_use]
fn range_thumb_size_px(box_model: DomBoxModel) -> f32 {
    box_model
        .border_box
        .height_px
        .max(MIN_RANGE_THUMB_SIZE_PX)
        .min(box_model.border_box.width_px.max(0.0))
}

#[must_use]
fn point_in_rect(point: PointPx, rect: DomRect) -> bool {
    point.x_px >= rect.left_px
        && point.x_px <= rect.left_px + rect.width_px
        && point.y_px >= rect.top_px
        && point.y_px <= rect.top_px + rect.height_px
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input_with_type(input_type: &str) -> DomElementNode {
        let DomNode::Element(mut node) = DomNode::element(DomNodeSlotKey::default(), DomTag::Input)
        else {
            panic!("input should create an element node");
        };
        node.set_attr(
            DomAttribute::Type,
            Some(DomAttributeValue::String(input_type.to_string())),
        )
        .expect("type");
        node
    }

    fn assert_f32_eq(left: f32, right: f32) {
        assert!((left - right).abs() <= f32::EPSILON);
    }

    #[test]
    fn resolves_checkbox_number_and_range_controls() {
        assert!(matches!(
            resolved_element_control(&input_with_type("checkbox")),
            Some(ResolvedElementControl::Checkbox(_))
        ));
        assert!(matches!(
            resolved_element_control(&input_with_type("number")),
            Some(ResolvedElementControl::Number(_))
        ));
        assert!(matches!(
            resolved_element_control(&input_with_type("range")),
            Some(ResolvedElementControl::Range(_))
        ));
    }

    #[test]
    fn range_defaults_stay_slider_shaped() {
        let (width_px, height_px) = range_default_preferred_size_px(16.0);
        assert!(width_px > height_px);
    }

    #[test]
    fn checkbox_defaults_stay_square() {
        let (width_px, height_px) = checkbox_default_preferred_size_px(12.0);
        assert_f32_eq(width_px, height_px);
    }

    #[test]
    fn number_hit_testing_distinguishes_stepper_parts() {
        let element = input_with_type("number");
        let box_model = DomBoxModel::new(
            DomRect {
                left_px: 10.0,
                top_px: 20.0,
                width_px: 100.0,
                height_px: 20.0,
            },
            BoxEdgeInsets::default(),
            BoxEdgeInsets::default(),
            BoxEdgeInsets::default(),
        );
        assert_eq!(
            resolved_control_part_from_point(
                &element,
                box_model,
                PointPx {
                    x_px: 15.0,
                    y_px: 25.0,
                },
            ),
            Some(ResolvedElementControlPart::NumberDecrementButton)
        );
    }
}
