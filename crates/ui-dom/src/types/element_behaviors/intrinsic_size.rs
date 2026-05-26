//! Intrinsic-size behavior definitions for DOM elements.

use crate::domains::paint::{
    measure_text_intrinsic, measure_text_intrinsic_with_font_size,
    measure_text_intrinsic_with_font_size_line_height_white_space_and_wrap_policy,
};
use crate::domains::text_layout::DEFAULT_TEXT_FONT_SIZE_PX;
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
    DomWordBreak, resolve_line_height_px,
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
                    next_intrinsic_generation(&element.base.intrinsic),
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
            let next_intrinsic = icon_img_intrinsic_from_attrs(
                &element.attrs,
                inherited_font_size_px,
                next_intrinsic_generation(&element.base.intrinsic),
            )
            .expect("icon img intrinsic attrs should stay validated");
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
            let mut next_intrinsic =
                measure_text_intrinsic_with_font_size_line_height_white_space_and_wrap_policy(
                    text.data(),
                    inherited_font_size_px,
                    inherited_line_height_px,
                    inherited_white_space,
                    inherited_overflow_wrap,
                    inherited_word_break,
                );
            let next_generation = current_intrinsic.as_ref().map_or_else(
                || next_intrinsic_generation(&text.base.intrinsic),
                |intrinsic| intrinsic.generation,
            );
            next_intrinsic.generation = next_generation;
            if matches!(&current_intrinsic, Some(current) if current == &next_intrinsic) {
                return false;
            }
            next_intrinsic.generation = next_intrinsic.generation.wrapping_add(1);
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
                next_intrinsic_generation(&element.base.intrinsic),
            )?;
            Ok(DomIntrinsic::ElementDefaultPreferredSize(next_intrinsic))
        }
    }
}

fn compute_preferred_size_intrinsic(
    element: &DomElementNode,
    font_size_px: f32,
    generation: u64,
) -> Result<ElementDefaultPreferredSizeIntrinsic, DomError> {
    let line_height_px = line_height_px_for_attrs(&element.attrs, font_size_px);
    if element.tag == DomTag::Select {
        return select_intrinsic_from_current_state(
            element,
            font_size_px,
            line_height_px,
            generation,
        );
    }
    match resolved_element_control(element) {
        Some(ResolvedElementControl::TextInput) => Ok(text_input_intrinsic_from_current_state(
            element,
            font_size_px,
            line_height_px,
            generation,
            false,
        )?),
        Some(ResolvedElementControl::Textarea) => Ok(textarea_intrinsic_from_current_state(
            element,
            font_size_px,
            line_height_px,
            generation,
        )),
        Some(ResolvedElementControl::Checkbox(_)) => {
            let (width_px, height_px) = checkbox_default_preferred_size_px(line_height_px);
            Ok(ElementDefaultPreferredSizeIntrinsic {
                font_size_px,
                width_px,
                height_px,
                line_height_px,
                generation,
            })
        }
        Some(ResolvedElementControl::Number(_)) => Ok(text_input_intrinsic_from_current_state(
            element,
            font_size_px,
            line_height_px,
            generation,
            true,
        )?),
        Some(ResolvedElementControl::Range(_)) => {
            let (width_px, height_px) = range_default_preferred_size_px(line_height_px);
            Ok(ElementDefaultPreferredSizeIntrinsic {
                font_size_px,
                width_px,
                height_px,
                line_height_px,
                generation,
            })
        }
        None => unreachable!(
            "default preferred size intrinsic should only measure opted-in form controls"
        ),
    }
}

fn parse_optional_pixel_attr_value(
    attrs: &DomAttrs,
    target_attribute: &DomAttribute,
) -> Result<Option<f32>, DomError> {
    let Some(value) = stored_attribute_value(attrs, target_attribute) else {
        return Ok(None);
    };
    match value {
        DomAttributeValue::String(value) => parse_pixel_value(value).map(Some),
        DomAttributeValue::Bool(_)
        | DomAttributeValue::Integer(_)
        | DomAttributeValue::Float(_)
        | DomAttributeValue::Color(_) => Err(DomError::InvalidAttributeValue),
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
    generation: u64,
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
        generation,
    })
}

fn textarea_intrinsic_from_current_state(
    element: &DomElementNode,
    font_size_px: f32,
    line_height_px: f32,
    generation: u64,
) -> ElementDefaultPreferredSizeIntrinsic {
    let _ = element;
    let width_px = measured_text_width_px(&textarea_default_width_sample(), font_size_px);
    let row_count = DEFAULT_TEXTAREA_ROWS;
    ElementDefaultPreferredSizeIntrinsic {
        font_size_px,
        width_px,
        height_px: line_height_px * usize_to_f32(row_count),
        line_height_px,
        generation,
    }
}

fn select_intrinsic_from_current_state(
    element: &DomElementNode,
    font_size_px: f32,
    line_height_px: f32,
    generation: u64,
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
        generation,
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

fn line_height_px_for_attrs(attrs: &DomAttrs, font_size_px: f32) -> f32 {
    stored_attribute_value(attrs, &DomAttribute::LineHeight)
        .and_then(|value| resolve_line_height_px(value, font_size_px))
        .unwrap_or_else(|| measure_text_intrinsic_with_font_size(" ", font_size_px).line_height_px)
}

const IMG_PLACEHOLDER_WIDTH_PX: f32 = 160.0;
const IMG_PLACEHOLDER_HEIGHT_PX: f32 = 90.0;
const IMG_PLACEHOLDER_ASPECT_RATIO: f32 = IMG_PLACEHOLDER_WIDTH_PX / IMG_PLACEHOLDER_HEIGHT_PX;
const DEFAULT_ICON_INTRINSIC_FONT_SIZE_PX: f32 = 14.0;
const DEFAULT_TEXT_CONTROL_COLUMNS: usize = 20;
const DEFAULT_TEXTAREA_ROWS: usize = 2;
const DEFAULT_NUMBER_INPUT_COLUMNS: usize = 10;

fn img_intrinsic_from_current_state(element: &DomElementNode) -> Result<DomIntrinsic, DomError> {
    let generation = next_intrinsic_generation(&element.base.intrinsic);
    if element.tag != DomTag::Img {
        return Ok(DomIntrinsic::None);
    }
    let Some(src) = stored_attribute_value(&element.attrs, &DomAttribute::Src) else {
        return Ok(DomIntrinsic::NotMeasured);
    };
    validate_img_src(src)?;
    let authored_width_px = parse_optional_pixel_attr_value(&element.attrs, &DomAttribute::Width)?;
    let authored_height_px =
        parse_optional_pixel_attr_value(&element.attrs, &DomAttribute::Height)?;
    if matches!(
        element.lowered_image_source_kind(),
        Some(LoweredImageSourceKind::InternalIcon(_))
    ) {
        return icon_img_intrinsic_from_attrs(
            &element.attrs,
            DEFAULT_ICON_INTRINSIC_FONT_SIZE_PX,
            generation,
        );
    }
    let (width_px, height_px) =
        img_placeholder_dimensions_for_authored_size(authored_width_px, authored_height_px);
    if width_px <= 0.0 || height_px <= 0.0 {
        return Err(DomError::InvalidAttributeValue);
    }
    Ok(DomIntrinsic::Img(ImgIntrinsic {
        width_px,
        height_px,
        aspect_ratio: width_px / height_px,
        generation,
    }))
}

fn icon_img_intrinsic_from_attrs(
    attrs: &DomAttrs,
    inherited_font_size_px: f32,
    generation: u64,
) -> Result<DomIntrinsic, DomError> {
    let authored_width_px = parse_optional_pixel_attr_value(attrs, &DomAttribute::Width)?;
    let authored_height_px = parse_optional_pixel_attr_value(attrs, &DomAttribute::Height)?;
    let (width_px, height_px) = icon_intrinsic_dimensions_for_authored_size(
        authored_width_px,
        authored_height_px,
        inherited_font_size_px,
    );
    Ok(DomIntrinsic::Img(ImgIntrinsic {
        width_px,
        height_px,
        aspect_ratio: 1.0,
        generation,
    }))
}

fn img_placeholder_dimensions_for_authored_size(
    authored_width_px: Option<f32>,
    authored_height_px: Option<f32>,
) -> (f32, f32) {
    match (authored_width_px, authored_height_px) {
        (None, None) => (IMG_PLACEHOLDER_WIDTH_PX, IMG_PLACEHOLDER_HEIGHT_PX),
        (Some(width_px), None) => (width_px, width_px / IMG_PLACEHOLDER_ASPECT_RATIO),
        (None, Some(height_px)) => (height_px * IMG_PLACEHOLDER_ASPECT_RATIO, height_px),
        (Some(width_px), Some(height_px)) => (width_px, height_px),
    }
}

fn icon_intrinsic_dimensions_for_authored_size(
    authored_width_px: Option<f32>,
    authored_height_px: Option<f32>,
    inherited_font_size_px: f32,
) -> (f32, f32) {
    match (authored_width_px, authored_height_px) {
        (None, None) => (inherited_font_size_px, inherited_font_size_px),
        (Some(width_px), None) => (width_px, width_px),
        (None, Some(height_px)) => (height_px, height_px),
        (Some(width_px), Some(height_px)) => (width_px, height_px),
    }
}

fn parse_pixel_value(value: &str) -> Result<f32, DomError> {
    if value == "0" {
        return Ok(0.0);
    }
    let Some(number) = value.strip_suffix("px") else {
        return Err(DomError::InvalidAttributeValue);
    };
    number
        .parse::<f32>()
        .map_err(|_| DomError::InvalidAttributeValue)
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

fn next_intrinsic_generation(current: &DomIntrinsic) -> u64 {
    match current {
        DomIntrinsic::Text(metrics) => metrics.generation.wrapping_add(1),
        DomIntrinsic::Img(metrics) => metrics.generation.wrapping_add(1),
        DomIntrinsic::ElementDefaultPreferredSize(metrics) => metrics.generation.wrapping_add(1),
        DomIntrinsic::None | DomIntrinsic::NotMeasured => 0,
    }
}

fn replace_intrinsic_if_changed(node: &mut DomNode, next_intrinsic: DomIntrinsic) -> bool {
    if node.intrinsic() == &next_intrinsic {
        return false;
    }
    node.set_intrinsic_for_store(next_intrinsic);
    true
}
