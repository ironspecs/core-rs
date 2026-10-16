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

const DEFAULT_RANGE_MIN_VALUE: f32 = 0.0;
const DEFAULT_RANGE_MAX_VALUE: f32 = 100.0;
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ResolvedNumericControl {
    kind: NumericControlKind,
    current_value: Option<f32>,
    min_value: Option<f32>,
    max_value: Option<f32>,
    step_value: f32,
}

impl ResolvedNumericControl {
    #[must_use]
    pub(crate) fn new(element: &DomElementNode, kind: NumericControlKind) -> Self {
        Self {
            kind,
            current_value: parsed_string_attr(element, &DomAttribute::Value),
            min_value: parsed_string_attr(element, &DomAttribute::Min),
            max_value: parsed_string_attr(element, &DomAttribute::Max),
            step_value: parsed_positive_step_attr(element),
        }
    }

    #[must_use]
    pub(crate) fn display_value_px(&self) -> f32 {
        self.clamped_value_px()
    }

    #[must_use]
    pub(crate) fn stepped_value_string(&self, direction: NumericStepDirection) -> String {
        let next_value = match direction {
            NumericStepDirection::Decrement => self.clamped_value_px() - self.step_value,
            NumericStepDirection::Increment => self.clamped_value_px() + self.step_value,
        };
        numeric_value_string(self.snap_and_clamp_value_px(next_value))
    }

    #[must_use]
    pub(crate) fn keyboard_step_value_string(&self, key: &str) -> Option<String> {
        match key {
            "ArrowDown" | "ArrowLeft" => {
                Some(self.stepped_value_string(NumericStepDirection::Decrement))
            }
            "ArrowUp" | "ArrowRight" => {
                Some(self.stepped_value_string(NumericStepDirection::Increment))
            }
            "Home" if self.kind == NumericControlKind::Range => {
                Some(numeric_value_string(self.minimum_value_px()))
            }
            "End" if self.kind == NumericControlKind::Range => {
                Some(numeric_value_string(self.maximum_value_px()))
            }
            "PageDown" if self.kind == NumericControlKind::Range => Some(numeric_value_string(
                self.snap_and_clamp_value_px(self.clamped_value_px() - self.step_value * 10.0),
            )),
            "PageUp" if self.kind == NumericControlKind::Range => Some(numeric_value_string(
                self.snap_and_clamp_value_px(self.clamped_value_px() + self.step_value * 10.0),
            )),
            _ => None,
        }
    }

    #[must_use]
    pub(crate) fn value_string_from_fraction(&self, fraction: f32) -> String {
        let minimum_value_px = self.minimum_value_px();
        let maximum_value_px = self.maximum_value_px();
        let raw_value = minimum_value_px + (maximum_value_px - minimum_value_px) * fraction;
        numeric_value_string(self.snap_and_clamp_value_px(raw_value))
    }

    #[must_use]
    pub(crate) fn value_fraction(&self) -> f32 {
        let minimum_value_px = self.minimum_value_px();
        let maximum_value_px = self.maximum_value_px();
        let span_px = maximum_value_px - minimum_value_px;
        if span_px <= f32::EPSILON {
            return 0.0;
        }
        ((self.clamped_value_px() - minimum_value_px) / span_px).clamp(0.0, 1.0)
    }

    #[must_use]
    fn clamped_value_px(&self) -> f32 {
        self.snap_and_clamp_value_px(
            self.current_value
                .unwrap_or_else(|| self.default_value_px()),
        )
    }

    #[must_use]
    fn default_value_px(&self) -> f32 {
        match self.kind {
            NumericControlKind::Number => self.min_value.unwrap_or(0.0),
            NumericControlKind::Range => {
                let minimum_value_px = self.minimum_value_px();
                minimum_value_px + (self.maximum_value_px() - minimum_value_px) * 0.5
            }
        }
    }

    #[must_use]
    fn minimum_value_px(&self) -> f32 {
        match self.kind {
            NumericControlKind::Number => self.min_value.unwrap_or(f32::NEG_INFINITY),
            NumericControlKind::Range => self.min_value.unwrap_or(DEFAULT_RANGE_MIN_VALUE),
        }
    }

    #[must_use]
    fn maximum_value_px(&self) -> f32 {
        let minimum_value_px = self.minimum_value_px();
        let fallback_maximum_value_px = match self.kind {
            NumericControlKind::Number => f32::INFINITY,
            NumericControlKind::Range => DEFAULT_RANGE_MAX_VALUE,
        };
        self.max_value
            .unwrap_or(fallback_maximum_value_px)
            .max(minimum_value_px)
    }

    #[must_use]
    fn snap_and_clamp_value_px(&self, value_px: f32) -> f32 {
        let minimum_value_px = self.minimum_value_px();
        let maximum_value_px = self.maximum_value_px();
        let step_base_px = self.min_value.unwrap_or(0.0);
        let snapped_value_px =
            ((value_px - step_base_px) / self.step_value).round() * self.step_value + step_base_px;
        snapped_value_px.clamp(minimum_value_px, maximum_value_px)
    }
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
fn parsed_string_attr(element: &DomElementNode, attribute: &DomAttribute) -> Option<f32> {
    string_attr_value(element, attribute).and_then(|value| value.parse::<f32>().ok())
}

#[must_use]
fn parsed_positive_step_attr(element: &DomElementNode) -> f32 {
    parsed_string_attr(element, &DomAttribute::Step)
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(DEFAULT_STEP_VALUE)
}

#[must_use]
fn numeric_value_string(value_px: f32) -> String {
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
    fn number_step_math_uses_default_step() {
        let node = input_with_type("number");
        let control = ResolvedNumericControl::new(&node, NumericControlKind::Number);
        assert_eq!(
            control.stepped_value_string(NumericStepDirection::Increment),
            "1"
        );
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
