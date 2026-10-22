//! Intrinsic-size lifecycle for DOM elements: measure on create, preview on
//! attr write, commit on style recompute.
//!
//! This file owns the three intrinsic-state transitions
//! (`compute_intrinsic_on_create`, `preview_intrinsic_after_attr_write`,
//! `commit_intrinsic_after_style_recompute`) for text nodes, `img` elements,
//! and default-preferred-size element controls, plus the measurement helpers
//! they share. Text measurement itself lives in `domains/paint`; control
//! preferred sizes come from `element_controls`.
//!
//! Invariants and rules:
//!
//! - the change-detection contract is the returned `bool`: a commit reports
//!   whether the retained intrinsic actually changed, and callers mark
//!   layout/paint dirty from that answer alone
//! - preview never mutates retained state; it clones the node and measures
//!   the clone
//! - measurement failures on already-validated state are internal corruption
//!   (`.expect`), not boundary errors
//! - change detection is equality over the measured intrinsic fields; the
//!   removed `generation` counters were write-only and, on the preferred-size
//!   and icon paths, compared unequal every pass — manufacturing phantom
//!   layout/paint dirties. See `domains/visitor/README.md` for the dirty-bit
//!   doctrine
//! - control preferred sizes read the resolved line height from the style pass
//!   (`inherited_line_height_px`), not the authored `line-height` attr (spine
//!   3.3g-2a: presentation is written through `style_mut`, never as an attr)

use crate::domains::paint::{
    measure_text_intrinsic, measure_text_intrinsic_with_font_size,
    measure_text_intrinsic_with_font_size_line_height_white_space_and_wrap_policy,
};
use crate::domains::text_layout::{DEFAULT_TEXT_FONT_SIZE_PX, ResolvedTextStyle};
use crate::element_controls::{
    ResolvedElementControl, checkbox_default_preferred_size_px, range_default_preferred_size_px,
    resolved_element_control,
};
use crate::numeric::usize_to_f32;
use crate::types::dom_node::{
    DomElementNode, DomIntrinsic, DomNode, ElementDefaultPreferredSizeIntrinsic, ImgIntrinsic,
    LoweredImageSourceKind, stored_attribute_value,
};
use crate::types::dom_tag::DomElementLifecycleBehavior;
use crate::types::{
    DomAttribute, DomAttributeValue, DomAttrs, DomError, DomOverflowWrap, DomTag, DomWhiteSpace,
    DomWordBreak,
};

/// Declares which intrinsic-size lifecycle family one element uses.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DomElementIntrinsicKind {
    #[default]
    None,
    Img,
    ElementDefaultPreferredSize,
}

pub(crate) fn compute_intrinsic_on_create(node: &DomNode) -> Result<DomIntrinsic, DomError> {
    match node {
        DomNode::Document(_) | DomNode::DocumentFragment(_) => Ok(DomIntrinsic::None),
        DomNode::Element(element) => compute_element_intrinsic_from_current_state(element),
        DomNode::Text(text) => Ok(DomIntrinsic::Text(measure_text_intrinsic(text.data()))),
    }
}

pub(crate) fn preview_intrinsic_after_attr_write(
    node: &DomNode,
    attribute: &DomAttribute,
    value: Option<&DomAttributeValue>,
) -> Result<Option<DomIntrinsic>, DomError> {
    if !node.after_attr_write_updates_intrinsic(attribute) {
        return Ok(None);
    }
    let mut preview_node = node.clone();
    preview_node.set_attr(attribute.clone(), value.cloned())?;
    match &preview_node {
        DomNode::Element(element) => {
            Ok(Some(compute_element_intrinsic_from_current_state(element)?))
        }
        DomNode::Document(_) | DomNode::DocumentFragment(_) | DomNode::Text(_) => Ok(None),
    }
}

pub(crate) fn commit_intrinsic_after_style_recompute(
    node: &mut DomNode,
    inherited_font_size_px: f32,
    inherited_line_height_px: Option<f32>,
    inherited_white_space: DomWhiteSpace,
    inherited_overflow_wrap: DomOverflowWrap,
    inherited_word_break: DomWordBreak,
    inherited_text_style: ResolvedTextStyle,
) -> bool {
    match node {
        DomNode::Element(element)
            if element.behavior().intrinsic_kind
                == DomElementIntrinsicKind::ElementDefaultPreferredSize
                && element
                    .behavior()
                    .after_style_recompute_intrinsic_update_behavior
                    == DomElementLifecycleBehavior::ElementControls =>
        {
            let next_intrinsic = DomIntrinsic::ElementDefaultPreferredSize(
                compute_preferred_size_intrinsic(
                    element,
                    inherited_font_size_px,
                    inherited_line_height_px,
                )
                .expect("text control intrinsic should stay validated"),
            );
            replace_intrinsic_if_changed(node, next_intrinsic)
        }
        DomNode::Element(element)
            if element.behavior().intrinsic_kind == DomElementIntrinsicKind::Img
                && element.tag == DomTag::Img
                && matches!(
                    element.lowered_image_source_kind(),
                    Some(LoweredImageSourceKind::InternalIcon(_))
                ) =>
        {
            let next_intrinsic = icon_img_intrinsic(inherited_font_size_px);
            replace_intrinsic_if_changed(node, next_intrinsic)
        }
        DomNode::Text(text) => {
            let current_intrinsic = match &text.base.intrinsic {
                DomIntrinsic::Text(intrinsic) => Some(intrinsic.clone()),
                DomIntrinsic::None
                | DomIntrinsic::NotMeasured
                | DomIntrinsic::Img(_)
                | DomIntrinsic::ElementDefaultPreferredSize(_) => None,
            };
            let next_intrinsic =
                measure_text_intrinsic_with_font_size_line_height_white_space_and_wrap_policy(
                    text.data(),
                    inherited_font_size_px,
                    inherited_line_height_px,
                    inherited_white_space,
                    inherited_overflow_wrap,
                    inherited_word_break,
                    inherited_text_style,
                );
            if matches!(&current_intrinsic, Some(current) if current == &next_intrinsic) {
                return false;
            }
            text.set_text_run_id_hint(None);
            text.base.intrinsic = DomIntrinsic::Text(next_intrinsic);
            true
        }
        DomNode::Document(_) | DomNode::DocumentFragment(_) | DomNode::Element(_) => false,
    }
}

fn compute_element_intrinsic_from_current_state(
    element: &DomElementNode,
) -> Result<DomIntrinsic, DomError> {
    match element.behavior().intrinsic_kind {
        DomElementIntrinsicKind::None => Ok(DomIntrinsic::None),
        DomElementIntrinsicKind::Img => img_intrinsic_from_current_state(element),
        DomElementIntrinsicKind::ElementDefaultPreferredSize => {
            let next_intrinsic = compute_preferred_size_intrinsic(
                element,
                preferred_size_measurement_font_size_px(&element.base.intrinsic),
                None,
            )?;
            Ok(DomIntrinsic::ElementDefaultPreferredSize(next_intrinsic))
        }
    }
}

fn compute_preferred_size_intrinsic(
    element: &DomElementNode,
    font_size_px: f32,
    inherited_line_height_px: Option<f32>,
) -> Result<ElementDefaultPreferredSizeIntrinsic, DomError> {
    // The style pass supplies the resolved line height (spine 3.3g-2a: the
    // authored `line-height` is a presentation property, no longer an attr);
    // the create path falls back to the font-derived default.
    let line_height_px = inherited_line_height_px
        .unwrap_or_else(|| measure_text_intrinsic_with_font_size(" ", font_size_px).line_height_px);
    if element.tag == DomTag::Select {
        return select_intrinsic_from_current_state(element, font_size_px, line_height_px);
    }
    match resolved_element_control(element) {
        Some(ResolvedElementControl::TextInput) => Ok(text_input_intrinsic_from_current_state(
            element,
            font_size_px,
            line_height_px,
            false,
        )?),
        Some(ResolvedElementControl::Textarea) => Ok(textarea_intrinsic_from_current_state(
            element,
            font_size_px,
            line_height_px,
        )),
        Some(ResolvedElementControl::Checkbox(_)) => {
            let (width_px, height_px) = checkbox_default_preferred_size_px(line_height_px);
            Ok(ElementDefaultPreferredSizeIntrinsic {
                font_size_px,
                width_px,
                height_px,
                line_height_px,
            })
        }
        Some(ResolvedElementControl::Number(_)) => Ok(text_input_intrinsic_from_current_state(
            element,
            font_size_px,
            line_height_px,
            true,
        )?),
        Some(ResolvedElementControl::Range(_)) => {
            let (width_px, height_px) = range_default_preferred_size_px(line_height_px);
            Ok(ElementDefaultPreferredSizeIntrinsic {
                font_size_px,
                width_px,
                height_px,
                line_height_px,
            })
        }
        None => unreachable!(
            "default preferred size intrinsic should only measure opted-in form controls"
        ),
    }
}

fn preferred_size_measurement_font_size_px(current_intrinsic: &DomIntrinsic) -> f32 {
    match current_intrinsic {
        DomIntrinsic::ElementDefaultPreferredSize(intrinsic) => intrinsic.font_size_px,
        DomIntrinsic::None
        | DomIntrinsic::NotMeasured
        | DomIntrinsic::Text(_)
        | DomIntrinsic::Img(_) => DEFAULT_TEXT_FONT_SIZE_PX,
    }
}

fn text_input_intrinsic_from_current_state(
    element: &DomElementNode,
    font_size_px: f32,
    line_height_px: f32,
    include_stepper_buttons: bool,
) -> Result<ElementDefaultPreferredSizeIntrinsic, DomError> {
    let input_type = string_attr_value(&element.attrs, &DomAttribute::Type)?.unwrap_or("text");
    let width_px = measured_text_width_px(&input_default_width_sample(input_type), font_size_px)
        + if include_stepper_buttons {
            line_height_px * 2.0
        } else {
            0.0
        };
    Ok(ElementDefaultPreferredSizeIntrinsic {
        font_size_px,
        width_px,
        height_px: line_height_px,
        line_height_px,
    })
}

fn textarea_intrinsic_from_current_state(
    element: &DomElementNode,
    font_size_px: f32,
    line_height_px: f32,
) -> ElementDefaultPreferredSizeIntrinsic {
    let _ = element;
    let width_px = measured_text_width_px(&textarea_default_width_sample(), font_size_px);
    let row_count = DEFAULT_TEXTAREA_ROWS;
    ElementDefaultPreferredSizeIntrinsic {
        font_size_px,
        width_px,
        height_px: line_height_px * usize_to_f32(row_count),
        line_height_px,
    }
}

fn select_intrinsic_from_current_state(
    element: &DomElementNode,
    font_size_px: f32,
    line_height_px: f32,
) -> Result<ElementDefaultPreferredSizeIntrinsic, DomError> {
    let value =
        single_line_text_for_measurement(string_attr_value(&element.attrs, &DomAttribute::Value)?);
    let width_px = [
        measured_text_width_px(value, font_size_px),
        measured_text_width_px(&input_default_width_sample("text"), font_size_px),
    ]
    .into_iter()
    .fold(0.0f32, f32::max);
    Ok(ElementDefaultPreferredSizeIntrinsic {
        font_size_px,
        width_px,
        height_px: line_height_px,
        line_height_px,
    })
}

fn string_attr_value<'a>(
    attrs: &'a DomAttrs,
    target_attribute: &DomAttribute,
) -> Result<Option<&'a str>, DomError> {
    let Some(value) = stored_attribute_value(attrs, target_attribute) else {
        return Ok(None);
    };
    match value {
        DomAttributeValue::String(value) => Ok(Some(value.as_str())),
        DomAttributeValue::Bool(_)
        | DomAttributeValue::Integer(_)
        | DomAttributeValue::Float(_)
        | DomAttributeValue::Color(_) => Err(DomError::InvalidAttributeValue),
    }
}

fn single_line_text_for_measurement(text: Option<&str>) -> &str {
    text.unwrap_or("").lines().next().unwrap_or("")
}

pub(crate) fn measured_text_width_px(text: &str, font_size_px: f32) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    if font_size_px == 0.0 {
        return 0.0;
    }
    measure_text_intrinsic_with_font_size(text, font_size_px).max_content_width_px
}

pub(crate) fn input_default_width_sample(input_type: &str) -> String {
    let columns = if input_type == "number" {
        DEFAULT_NUMBER_INPUT_COLUMNS
    } else {
        DEFAULT_TEXT_CONTROL_COLUMNS
    };
    "0".repeat(columns)
}

fn textarea_default_width_sample() -> String {
    "0".repeat(DEFAULT_TEXT_CONTROL_COLUMNS)
}

const IMG_PLACEHOLDER_WIDTH_PX: f32 = 160.0;
const IMG_PLACEHOLDER_HEIGHT_PX: f32 = 90.0;
const DEFAULT_ICON_INTRINSIC_FONT_SIZE_PX: f32 = 14.0;
const DEFAULT_TEXT_CONTROL_COLUMNS: usize = 20;
const DEFAULT_TEXTAREA_ROWS: usize = 2;
const DEFAULT_NUMBER_INPUT_COLUMNS: usize = 10;

fn img_intrinsic_from_current_state(element: &DomElementNode) -> Result<DomIntrinsic, DomError> {
    if element.tag != DomTag::Img {
        return Ok(DomIntrinsic::None);
    }
    let Some(src) = stored_attribute_value(&element.attrs, &DomAttribute::Src) else {
        return Ok(DomIntrinsic::NotMeasured);
    };
    validate_img_src(src)?;
    if matches!(
        element.lowered_image_source_kind(),
        Some(LoweredImageSourceKind::InternalIcon(_))
    ) {
        return Ok(icon_img_intrinsic(DEFAULT_ICON_INTRINSIC_FONT_SIZE_PX));
    }
    // `width`/`height` are presentation properties (spine 3.4): the image's
    // authored dimensions are written through `style_mut` and resolved by
    // layout, so the intrinsic falls back to the fixed placeholder size.
    let width_px = IMG_PLACEHOLDER_WIDTH_PX;
    let height_px = IMG_PLACEHOLDER_HEIGHT_PX;
    Ok(DomIntrinsic::Img(ImgIntrinsic {
        width_px,
        height_px,
        aspect_ratio: width_px / height_px,
    }))
}

fn icon_img_intrinsic(inherited_font_size_px: f32) -> DomIntrinsic {
    DomIntrinsic::Img(ImgIntrinsic {
        width_px: inherited_font_size_px,
        height_px: inherited_font_size_px,
        aspect_ratio: 1.0,
    })
}

fn validate_img_src(value: &DomAttributeValue) -> Result<(), DomError> {
    match value {
        DomAttributeValue::String(src) if !src.is_empty() => Ok(()),
        DomAttributeValue::String(_)
        | DomAttributeValue::Bool(_)
        | DomAttributeValue::Integer(_)
        | DomAttributeValue::Float(_)
        | DomAttributeValue::Color(_) => Err(DomError::InvalidAttributeValue),
    }
}

fn replace_intrinsic_if_changed(node: &mut DomNode, next_intrinsic: DomIntrinsic) -> bool {
    if node.intrinsic() == &next_intrinsic {
        return false;
    }
    node.set_intrinsic_for_store(next_intrinsic);
    true
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn zero_font_size_measures_zero_width() {
        assert_eq!(measured_text_width_px("abc", 0.0), 0.0);
        assert!(measured_text_width_px("abc", 12.0) > 0.0);
    }
}
